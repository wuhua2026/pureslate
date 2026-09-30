//! 隔离区生命周期（R25 · SAFETY §4.4，P3-06 [DESTRUCTIVE]）。
//!
//! - **到期自动清除**：`expires_at` 已过 → 硬删隔离区文件 + manifest `state→purged`
//!   + 审计 `op=auto_purge`。manifest 行（含 sha256/原路径）保留为持久事务记录，
//!     删除前记录已存在（红线 #2 可追溯面）。
//! - **到期前 3 天提醒**：收集剩余 ≤3 天的条目 id，由 IPC 层发 `quarantine_expiry_warning`。
//! - **restored 行清理**：还原后保留 30 天（SPEC §4.3），到期由本模块删 manifest 行（不动文件）。
//! - **容量上限**：`min(5GB, 所在盘剩余 10%)`；超限**绝不自动删除**，返回"最早批次"id 集
//!   供 UI 显式确认释放（SAFETY §4.4 不得静默丢弃）。
//! - **手动清空/按 id 硬删**（token 由 IPC 层校验后调用 `purge_globally`）。
//!
//! 时间与剩余空间均由参数注入（`run_pass_at_root`），单测不依赖真实时钟/磁盘。

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use crate::contract::{PurgeReport, RestoreFailure};

use super::manifest::{
    load_manifest, prune_restored_before, update_entry_state, ManifestEntry, ManifestState,
};

/// 到期提醒窗口：3 天（SAFETY §4.4）。
pub const EXPIRY_WARNING_MS: i64 = 3 * 86_400_000;
/// restored 行保留期：30 天（SPEC §4.3）。
pub const RESTORED_ROW_RETENTION_MS: i64 = 30 * 86_400_000;
/// 容量上限硬顶：5GB。
pub const QUOTA_CAP_BYTES: u64 = 5 * 1024 * 1024 * 1024;

/// 一遍生命周期的产出（跨根聚合用同一结构）。
#[derive(Debug, Default, Clone)]
pub struct LifecycleReport {
    pub purged: u64,
    pub purged_bytes: u64,
    /// 剩余 ≤3 天的条目 id（供 quarantine_expiry_warning 事件）。
    pub warning_ids: Vec<String>,
    /// 提醒条目的最小剩余天数（无提醒为 0）。
    pub days_left_min: i64,
    pub used_bytes: u64,
    pub quota_bytes: u64,
    pub over_quota: bool,
    /// 超限时"释放最早批次"建议 id（足以回到上限内）。
    pub earliest_batch_ids: Vec<String>,
}

/// 单盘容量上限 = min(5GB, 剩余空间 10%)。
fn quota_bytes_for(free_bytes: u64) -> u64 {
    QUOTA_CAP_BYTES.min(free_bytes / 10)
}

/// 对单个隔离区根执行一遍生命周期。`now`/`free_bytes` 注入（测试可控）。
pub fn run_pass_at_root(root: &Path, now: i64, free_bytes: u64) -> LifecycleReport {
    let mut report = LifecycleReport::default();
    let Ok((entries, _)) = load_manifest(root) else {
        return report;
    };

    // 1) 到期自动清除：硬删文件 + state→purged + 审计。文件缺失也收敛状态（幂等）。
    for e in &entries {
        if e.state != ManifestState::Quarantined || e.expires_at > now {
            continue;
        }
        let qpath = Path::new(&e.quarantine_path);
        let removed = if qpath.is_file() {
            fs::remove_file(qpath).is_ok()
        } else {
            true
        };
        match update_entry_state(root, &e.id, ManifestState::Purged) {
            Err(err) => audit(
                e,
                "auto_purge",
                "fail",
                Some(format!("manifest 状态迁移失败: {err}")),
            ),
            Ok(false) => audit(e, "auto_purge", "fail", Some("manifest 条目未找到".into())),
            Ok(true) => {
                if removed {
                    report.purged += 1;
                    report.purged_bytes += e.size_bytes;
                    audit(e, "auto_purge", "ok", None);
                } else {
                    audit(e, "auto_purge", "fail", Some("隔离区文件删除失败".into()));
                }
            }
        }
    }

    // 2) restored 行 30 天清理（只删行不动文件）。
    let _ = prune_restored_before(root, now - RESTORED_ROW_RETENTION_MS);

    // 3) 重载（状态已迁移）→ 到期提醒 + 容量核算。
    let Ok((entries, _)) = load_manifest(root) else {
        return report;
    };
    let mut quarantined: Vec<ManifestEntry> = entries
        .into_iter()
        .filter(|e| e.state == ManifestState::Quarantined)
        .collect();

    for e in &quarantined {
        let left = e.expires_at - now;
        if left > 0 && left <= EXPIRY_WARNING_MS {
            report.warning_ids.push(e.id.clone());
            let days = (left + 86_399_999) / 86_400_000;
            if report.days_left_min == 0 || days < report.days_left_min {
                report.days_left_min = days;
            }
        }
    }

    report.used_bytes = quarantined.iter().map(|e| e.size_bytes).sum();
    report.quota_bytes = quota_bytes_for(free_bytes);
    if report.used_bytes > report.quota_bytes {
        report.over_quota = true;
        // 超限不自动删除：给出最早批次建议（moved_at 升序，累计到足以回到上限内）。
        quarantined.sort_by(|a, b| a.moved_at.cmp(&b.moved_at).then(a.id.cmp(&b.id)));
        let excess = report.used_bytes - report.quota_bytes;
        let mut freed = 0u64;
        for e in quarantined {
            if freed >= excess {
                break;
            }
            freed += e.size_bytes;
            report.earliest_batch_ids.push(e.id.clone());
        }
    }
    report
}

/// 对全部固定盘隔离区根执行一遍生命周期（真实时钟 + 真实剩余空间）。
pub fn run_pass_globally(now: i64) -> LifecycleReport {
    let mut total = LifecycleReport::default();
    for root in super::all_quarantine_roots() {
        let drive = root.parent().unwrap_or_else(|| Path::new(r"C:\"));
        let r = run_pass_at_root(&root, now, free_bytes_of(drive));
        total.purged += r.purged;
        total.purged_bytes += r.purged_bytes;
        total.used_bytes += r.used_bytes;
        total.quota_bytes += r.quota_bytes;
        total.over_quota |= r.over_quota;
        if r.days_left_min > 0
            && (total.days_left_min == 0 || r.days_left_min < total.days_left_min)
        {
            total.days_left_min = r.days_left_min;
        }
        total.warning_ids.extend(r.warning_ids);
        total.earliest_batch_ids.extend(r.earliest_batch_ids);
    }
    total
}

/// 手动按 id 硬删（"确认清空"/"释放最早批次"）：全部固定盘。token 缺失 → 全部拒绝（🔴 语义）。
pub fn purge_globally(ids: &[String], confirm_token: &str) -> PurgeReport {
    purge_across(&super::all_quarantine_roots(), ids, confirm_token)
}

/// `purge_globally` 的 roots 可注入核心（测试注入沙箱根）。
pub fn purge_across(
    roots: &[std::path::PathBuf],
    ids: &[String],
    confirm_token: &str,
) -> PurgeReport {
    let mut report = PurgeReport {
        requested: ids.len() as u64,
        purged: 0,
        failures: vec![],
    };
    if confirm_token.trim().is_empty() {
        report.failures = ids
            .iter()
            .map(|id| RestoreFailure {
                id: id.clone(),
                reason: "缺少确认令牌（需二次确认）".into(),
            })
            .collect();
        return report;
    }
    let mut found: HashSet<String> = HashSet::new();
    for root in roots {
        purge_at_root(root, ids, &mut found, &mut report);
    }
    for id in ids {
        if !found.contains(id) {
            report.failures.push(RestoreFailure {
                id: id.clone(),
                reason: "未找到该条目".into(),
            });
        }
    }
    report
}

/// 对单个根按 id 硬删：文件删除 + state→purged + 审计（op=purge）。
fn purge_at_root(
    root: &Path,
    ids: &[String],
    found: &mut HashSet<String>,
    report: &mut PurgeReport,
) {
    let Ok((entries, _)) = load_manifest(root) else {
        return;
    };
    for e in entries {
        if !ids.contains(&e.id) {
            continue;
        }
        found.insert(e.id.clone());
        if e.state != ManifestState::Quarantined {
            report.failures.push(RestoreFailure {
                id: e.id.clone(),
                reason: "该条目已处理（非已隔离状态）".into(),
            });
            continue;
        }
        let qpath = Path::new(&e.quarantine_path);
        let removed = if qpath.is_file() {
            fs::remove_file(qpath).is_ok()
        } else {
            true
        };
        match update_entry_state(root, &e.id, ManifestState::Purged) {
            Err(err) => report.failures.push(RestoreFailure {
                id: e.id.clone(),
                reason: format!("manifest 状态迁移失败: {err}"),
            }),
            Ok(false) => report.failures.push(RestoreFailure {
                id: e.id.clone(),
                reason: "manifest 条目未找到".into(),
            }),
            Ok(true) => {
                if removed {
                    report.purged += 1;
                    audit(&e, "purge", "ok", None);
                } else {
                    report.failures.push(RestoreFailure {
                        id: e.id.clone(),
                        reason: "隔离区文件删除失败".into(),
                    });
                    audit(&e, "purge", "fail", Some("隔离区文件删除失败".into()));
                }
            }
        }
    }
}

/// 磁盘剩余空间（字节）。取失败返回 u64::MAX（容量上限回落到 5GB 硬顶，避免误报超限）。
pub fn free_bytes_of(drive: &Path) -> u64 {
    #[cfg(windows)]
    {
        let mut wide: Vec<u16> = drive.to_string_lossy().encode_utf16().collect();
        wide.push(0);
        let mut avail: u64 = 0;
        let mut total: u64 = 0;
        let mut free: u64 = 0;
        // 安全性：三个输出指针均为有效栈地址；返回 0 表示失败。
        let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut avail, &mut total, &mut free) };
        if ok != 0 {
            free
        } else {
            u64::MAX
        }
    }
    #[cfg(not(windows))]
    {
        let _ = drive;
        u64::MAX
    }
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetDiskFreeSpaceExW(
        lpdirectoryname: *const u16,
        lpfreebytesavailable: *mut u64,
        lptotalnumberofbytes: *mut u64,
        lptotalnumberoffreebytes: *mut u64,
    ) -> i32;
}

/// 审计一条隔离区终态操作（auto_purge / purge）。
fn audit(e: &ManifestEntry, op: &str, result: &str, detail: Option<String>) {
    let entry = crate::contract::LogEntry {
        ts: crate::logging::audit::now_ms(),
        op: op.into(),
        tx_id: None,
        category_id: Some(e.category_id.clone()),
        path: Some(e.original_path.clone()),
        size_bytes: Some(e.size_bytes),
        disposition: Some(crate::contract::Disposition::Quarantine),
        result: Some(result.into()),
        detail,
    };
    if let Err(err) = crate::logging::audit::record(&entry) {
        eprintln!("[lifecycle] 审计写入失败: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quarantine::manifest::{add_manifest_entry, load_manifest};
    use std::path::PathBuf;

    /// 沙箱 + 全程持锁（audit 落 data_root，共享全局须串行）。
    struct Sandbox {
        root: PathBuf,
        _guard: std::sync::MutexGuard<'static, ()>,
    }

    impl Sandbox {
        fn new(tag: &str) -> Self {
            let guard = crate::storage::TEST_DATA_ROOT_LOCK.lock().unwrap();
            let base = std::env::temp_dir().join(format!(
                "pureslate-lifecycle-{tag}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let root = base.join("qroot");
            let data_root = base.join("data-root");
            fs::create_dir_all(&root).unwrap();
            fs::create_dir_all(&data_root).unwrap();
            crate::storage::set_data_root_override(Some(data_root));
            Sandbox {
                root,
                _guard: guard,
            }
        }

        /// 构造一条 manifest 记录（不写文件、不落 manifest；调用方自行追加）。
        fn entry(&self, id: &str, size: u64, moved_at: i64, expires_at: i64) -> ManifestEntry {
            let qpath = self.root.join(format!("{id}.bin"));
            ManifestEntry {
                id: id.into(),
                original_path: format!(r"C:\Users\u\orig-{id}.tmp"),
                quarantine_path: qpath.to_string_lossy().into_owned(),
                size_bytes: size,
                sha256: "abc".into(),
                grade: crate::contract::Grade::Yellow,
                category_id: "cache.wechat".into(),
                moved_at,
                expires_at,
                original_mtime_ms: None,
                restored_at: None,
                state: ManifestState::Quarantined,
            }
        }

        /// 造一条 manifest 记录 + 对应隔离区文件并落 manifest。
        fn seed(&self, id: &str, size: u64, moved_at: i64, expires_at: i64) -> ManifestEntry {
            let e = self.entry(id, size, moved_at, expires_at);
            fs::write(&e.quarantine_path, vec![0u8; size as usize]).unwrap();
            add_manifest_entry(&self.root, &e).unwrap();
            e
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            crate::storage::set_data_root_override(None);
            let _ = fs::remove_dir_all(self.root.parent().unwrap());
        }
    }

    const DAY: i64 = 86_400_000;
    const NOW: i64 = 1_800_000_000_000; // 固定时钟（注入）

    #[test]
    fn expired_entry_auto_purged_with_audit() {
        let sb = Sandbox::new("expired");
        let e = sb.seed("e1", 100, NOW - 20 * DAY, NOW - DAY); // 已过期
        sb.seed("keep", 50, NOW - DAY, NOW + 10 * DAY); // 未到期

        let report = run_pass_at_root(&sb.root, NOW, u64::MAX / 10);

        assert_eq!(report.purged, 1);
        assert_eq!(report.purged_bytes, 100);
        assert!(!Path::new(&e.quarantine_path).exists(), "过期文件须硬删");
        let (entries, _) = load_manifest(&sb.root).unwrap();
        let e1 = entries.iter().find(|x| x.id == "e1").unwrap();
        assert_eq!(e1.state, ManifestState::Purged);
        let keep = entries.iter().find(|x| x.id == "keep").unwrap();
        assert_eq!(keep.state, ManifestState::Quarantined, "未到期不动");

        // 审计 ts 为真实时钟（注入时钟只作用于生命周期判定），按当前时间窗查询。
        let now = crate::logging::audit::now_ms();
        let logs = crate::logging::audit::query(now - 60_000, now + 60_000, Some("auto_purge"));
        assert_eq!(logs.len(), 1, "auto_purge 审计恰好一条");
        assert_eq!(logs[0].path.as_deref(), Some(r"C:\Users\u\orig-e1.tmp"));
    }

    #[test]
    fn expiring_soon_collects_warning_ids() {
        let sb = Sandbox::new("warn");
        sb.seed("w1", 10, NOW - 12 * DAY, NOW + 2 * DAY); // 剩 2 天 → 提醒
        sb.seed("w2", 10, NOW - 13 * DAY, NOW + DAY / 2); // 剩半天 → 提醒，days=1
        sb.seed("far", 10, NOW - DAY, NOW + 10 * DAY); // 不提醒

        let report = run_pass_at_root(&sb.root, NOW, u64::MAX / 10);

        assert!(report.warning_ids.contains(&"w1".to_string()));
        assert!(report.warning_ids.contains(&"w2".to_string()));
        assert!(!report.warning_ids.contains(&"far".to_string()));
        assert_eq!(report.days_left_min, 1, "取最小剩余天数");
        assert_eq!(report.purged, 0, "未到期不清除");
        let (entries, _) = load_manifest(&sb.root).unwrap();
        assert!(entries
            .iter()
            .all(|e| e.state == ManifestState::Quarantined));
    }

    #[test]
    fn over_quota_never_auto_deletes_but_reports_earliest_batch() {
        let sb = Sandbox::new("quota");
        // 两条共 300B；free=500B → quota=min(5GB, 50B)=50B → 超限。
        let old = sb.seed("old", 100, NOW - 13 * DAY, NOW + 5 * DAY);
        let newer = sb.seed("new", 200, NOW - 2 * DAY, NOW + 12 * DAY);

        let report = run_pass_at_root(&sb.root, NOW, 500);

        assert!(report.over_quota);
        assert_eq!(report.quota_bytes, 50);
        assert_eq!(report.used_bytes, 300);
        assert_eq!(
            report.earliest_batch_ids,
            vec!["old".to_string(), "new".to_string()]
        );
        // 超限绝不自动删除：两条文件原样。
        assert!(Path::new(&old.quarantine_path).exists());
        assert!(Path::new(&newer.quarantine_path).exists());
        assert_eq!(report.purged, 0);
    }

    #[test]
    fn restored_rows_pruned_after_30_days() {
        let sb = Sandbox::new("prune");
        // 两条 restored 行（不走 seed：restored 行的文件已还原回原路径，隔离区无文件）。
        let mut e_old = sb.entry("r-old", 10, NOW - 60 * DAY, NOW - 30 * DAY);
        e_old.state = ManifestState::Restored;
        e_old.restored_at = Some(NOW - 31 * DAY); // 还原于 31 天前 → 行清理
        add_manifest_entry(&sb.root, &e_old).unwrap();
        let mut e_new = sb.entry("r-new", 10, NOW - 40 * DAY, NOW - 20 * DAY);
        e_new.state = ManifestState::Restored;
        e_new.restored_at = Some(NOW - DAY); // 昨天还原 → 保留
        add_manifest_entry(&sb.root, &e_new).unwrap();

        let _ = run_pass_at_root(&sb.root, NOW, u64::MAX / 10);

        let (entries, _) = load_manifest(&sb.root).unwrap();
        let ids: Vec<&str> = entries.iter().map(|e| e.id.as_str()).collect();
        assert!(!ids.contains(&"r-old"), "31 天前还原的行须清理: {ids:?}");
        assert!(ids.contains(&"r-new"), "近期还原的行保留");
    }

    #[test]
    fn purge_requires_token() {
        let sb = Sandbox::new("token");
        sb.seed("p1", 10, NOW - DAY, NOW + 13 * DAY);

        let report = purge_across(std::slice::from_ref(&sb.root), &["p1".to_string()], "  ");
        assert_eq!(report.requested, 1);
        assert_eq!(report.purged, 0);
        assert!(report.failures[0].reason.contains("确认令牌"));
        let (entries, _) = load_manifest(&sb.root).unwrap();
        assert_eq!(
            entries[0].state,
            ManifestState::Quarantined,
            "无 token 不得删除"
        );
    }

    #[test]
    fn purge_with_token_hard_deletes_and_audits() {
        let sb = Sandbox::new("purge");
        let e = sb.seed("p1", 10, NOW - DAY, NOW + 13 * DAY);
        sb.seed("p2", 10, NOW - DAY, NOW + 13 * DAY);

        let report = purge_across(
            std::slice::from_ref(&sb.root),
            &["p1".to_string(), "no-such".to_string()],
            "PS-1234",
        );

        assert_eq!(report.requested, 2);
        assert_eq!(report.purged, 1);
        assert!(!Path::new(&e.quarantine_path).exists(), "硬删隔离区文件");
        let (entries, _) = load_manifest(&sb.root).unwrap();
        let p1 = entries.iter().find(|x| x.id == "p1").unwrap();
        assert_eq!(p1.state, ManifestState::Purged);
        assert_eq!(
            entries.iter().find(|x| x.id == "p2").unwrap().state,
            ManifestState::Quarantined,
            "未选中项不动"
        );
        assert!(
            report.failures.iter().any(|f| f.id == "no-such"),
            "未知 id 记失败"
        );
        let logs = crate::logging::audit::query(
            crate::logging::audit::now_ms() - 60_000,
            crate::logging::audit::now_ms() + 60_000,
            Some("purge"),
        );
        assert!(!logs.is_empty(), "purge 须写审计");
    }
}
