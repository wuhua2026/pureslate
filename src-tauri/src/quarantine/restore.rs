//! 隔离区还原引擎（R25 · SAFETY §4.2 / §6.3 M12）。
//!
//! - 按 `ManifestEntry.original_path` 还原：隔离文件移回原路径（同卷 `rename` / 跨卷 copy+校验+删源）；
//! - 原路径被占用/已存在 → 落到 `<data_root>\restore-conflict\<yyyyMMdd>\`（`storage::restore_conflict_path`），
//!   计 `conflict` 分母，绝不静默丢弃；
//! - 成功 → manifest 行 `state=restored`；
//! - 还原时回写隔离前的 `original_mtime_ms`（保取证信息，SAFETY §4.2）；
//! - 非 `Quarantined` 态条目直接跳过（Skipped）。

use std::fs;
use std::path::{Path, PathBuf};

use crate::contract::{QuarantineEntry, QuarantineState, RestoreFailure, RestoreReport};

use super::manifest::{load_manifest, update_entry_state, ManifestEntry, ManifestState};
use super::store::{copy_verify_delete, same_volume};

/// 单条还原结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreOutcome {
    /// 成功还原回原路径。
    Restored,
    /// 原路径被占用，已落 restore-conflict 兜底目录。
    Conflict,
    /// 非可还原状态，跳过。
    Skipped(String),
    /// 还原失败。
    Failed(String),
}

/// 还原单条隔离项。
///
/// `root` 为该条所属的隔离区根（测试可注入沙箱根）。
/// 幂等：已 restored/purged 的条目返回 `Skipped`。
pub fn restore_one(root: &Path, entry: &ManifestEntry) -> RestoreOutcome {
    if entry.state != ManifestState::Quarantined {
        return RestoreOutcome::Skipped("该条目不是可还原的已隔离状态".into());
    }
    let src = PathBuf::from(&entry.quarantine_path);
    if !src.is_file() {
        return RestoreOutcome::Failed(format!("隔离区源文件缺失: {}", src.to_string_lossy()));
    }
    let orig = PathBuf::from(&entry.original_path);

    let is_conflict = orig.exists();
    let target = if is_conflict {
        crate::storage::restore_conflict_path(&orig)
    } else {
        orig.clone()
    };

    // 目标父目录必须可建（还原到原路径 = 重建原目录；冲突 = 建 restore-conflict 子目录）。
    if let Some(parent) = target.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            return RestoreOutcome::Failed(format!("还原目标目录不可用: {e}"));
        }
    }

    let moved = if same_volume(&src, &target) {
        fs::rename(&src, &target).map_err(|e| e.to_string())
    } else {
        copy_verify_delete(&src, &target).map_err(|e| e.to_string())
    };
    if let Err(e) = moved {
        return RestoreOutcome::Failed(format!("还原移动失败: {e}"));
    }

    // 还原 mtime（保取证信息；尽力而为，失败不阻断还原主体）。
    if let Some(ms) = entry.original_mtime_ms {
        let _ = set_mtime(&target, ms);
    }

    // 状态迁移到 restored。
    if let Err(e) = update_entry_state(root, &entry.id, ManifestState::Restored) {
        // 文件已还原，manifest 状态迁移失败仅告警（数据已安全；不可静默，记日志）。
        eprintln!("[restore] manifest 状态迁移失败(id={}): {e}", entry.id);
    }

    if is_conflict {
        RestoreOutcome::Conflict
    } else {
        RestoreOutcome::Restored
    }
}

/// 对**单个卷**隔离区根执行还原并汇总。`ids` 为空 → 还原全部可还原项。
fn restore_to_root(root: &Path, ids: &[String], report: &mut RestoreReport) {
    let Ok((entries, _)) = load_manifest(root) else {
        return;
    };
    for e in entries {
        if e.state != ManifestState::Quarantined {
            continue;
        }
        if !ids.is_empty() && !ids.contains(&e.id) {
            continue;
        }
        report.requested += 1;
        match restore_one(root, &e) {
            RestoreOutcome::Restored => report.restored += 1,
            RestoreOutcome::Conflict => report.conflict += 1,
            RestoreOutcome::Skipped(r) | RestoreOutcome::Failed(r) => {
                report.failures.push(RestoreFailure {
                    id: e.id,
                    reason: r,
                });
            }
        }
    }
}

/// 跨全部固定盘隔离区根执行还原。`ids` 为空 → 还原所有可还原项。
pub fn restore_globally(ids: &[String]) -> RestoreReport {
    let mut report = RestoreReport {
        requested: 0,
        restored: 0,
        conflict: 0,
        failures: vec![],
    };
    for root in super::all_quarantine_roots() {
        restore_to_root(&root, ids, &mut report);
    }
    report
}

/// 把一条 manifest 记录映射为 IPC 契约 `QuarantineEntry`（含 `daysLeft`）。
pub fn manifest_to_contract(e: &ManifestEntry) -> QuarantineEntry {
    let now = super::now_ms();
    let days_left = ((e.expires_at - now).max(1) + 86_399_999) / 86_400_000;
    QuarantineEntry {
        id: e.id.clone(),
        original_path: e.original_path.clone(),
        size_bytes: e.size_bytes,
        grade: e.grade,
        category_id: e.category_id.clone(),
        moved_at: e.moved_at,
        expires_at: e.expires_at,
        days_left,
        state: match e.state {
            ManifestState::Quarantined => QuarantineState::Quarantined,
            ManifestState::Restored => QuarantineState::Restored,
            ManifestState::Purged => QuarantineState::Purged,
        },
    }
}

/// 列出全部固定盘上**待管理**（已隔离）条目。历史 restored/purged 不进默认列表。
pub fn list_quarantined_globally() -> Vec<QuarantineEntry> {
    let mut out = Vec::new();
    for root in super::all_quarantine_roots() {
        if let Ok((entries, _)) = load_manifest(&root) {
            out.extend(
                entries
                    .into_iter()
                    .filter(|e| e.state == ManifestState::Quarantined)
                    .map(|e| manifest_to_contract(&e)),
            );
        }
    }
    out
}

/// 设置文件修改时间（epoch ms）。cross-platform（std `FileTimes`）。跨测试/CI 可用。
fn set_mtime(path: &Path, ms: i64) -> std::io::Result<()> {
    if ms < 0 {
        return Ok(());
    }
    let ft = std::fs::FileTimes::new()
        .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_millis(ms as u64));
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)?
        .set_times(ft)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quarantine::store::sha256_file;
    use crate::quarantine::store::{move_into_quarantine, QuarantineInput};
    use crate::quarantine::QUARANTINE_DIR;

    /// 测试沙箱句柄：持 `TEST_DATA_ROOT_LOCK` 贯穿整个测试体，退出作用域（Drop）时
    /// 自动清覆盖并删沙箱。Deref 到根路径，可用 `root.join(...)` 取子路径。
    struct SandboxGuard {
        _lock: std::sync::MutexGuard<'static, ()>,
        root: std::path::PathBuf,
    }
    impl std::ops::Deref for SandboxGuard {
        type Target = std::path::Path;
        fn deref(&self) -> &std::path::Path {
            &self.root
        }
    }
    impl Drop for SandboxGuard {
        fn drop(&mut self) {
            crate::storage::set_data_root_override(None);
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn sandbox(tag: &str) -> SandboxGuard {
        let _lock = crate::storage::TEST_DATA_ROOT_LOCK.lock().unwrap();
        let d = std::env::temp_dir().join(format!(
            "pureslate-qrestore-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(d.join("src")).unwrap();
        std::fs::create_dir_all(d.join("orig")).unwrap();
        // 隔离冲突目录注入沙箱，避免写真实 %LOCALAPPDATA%
        crate::storage::set_data_root_override(Some(d.join("data-root")));
        SandboxGuard { _lock, root: d }
    }

    /// 造样本 → 移入隔离区 → 返回 (entry, 源原始路径, 原始内容 hash)。
    fn seal_sample(root: &Path, name: &str, body: &str) -> (ManifestEntry, PathBuf, String) {
        let orig = root.join("orig").join(name);
        std::fs::write(&orig, body).unwrap();
        let hash = sha256_file(&orig).unwrap();
        let qroot = root.join(QUARANTINE_DIR);
        let entry = move_into_quarantine(
            &qroot,
            QuarantineInput {
                original_path: orig.clone(),
                grade: crate::contract::Grade::Yellow,
                category_id: "cache.test".into(),
                retention_days: 14,
            },
        )
        .unwrap();
        (entry, orig, hash)
    }

    #[test]
    fn restore_success_moves_back_and_marks_state() {
        let root = sandbox("ok");
        let qroot = root.join(QUARANTINE_DIR);
        std::fs::create_dir_all(&qroot).unwrap();
        let (entry, orig, hash) = seal_sample(&root, "a.tmp", "content-a");
        assert!(!orig.exists());

        let outcome = restore_one(&qroot, &entry);
        assert_eq!(outcome, RestoreOutcome::Restored);
        assert!(orig.is_file());
        assert_eq!(sha256_file(&orig).unwrap(), hash);
        // 隔离区源文件已不在
        assert!(!Path::new(&entry.quarantine_path).exists());
        // manifest 状态迁移
        let (entries, _) = crate::quarantine::manifest::load_manifest(&qroot).unwrap();
        let e = entries.iter().find(|e| e.id == entry.id).unwrap();
        assert_eq!(e.state, ManifestState::Restored);
        // root 为 SandboxGuard，作用域结束（Drop）自动清覆盖并删沙箱。
    }

    #[test]
    fn mtime_round_trip_preserved() {
        let root = sandbox("mtime");
        let qroot = root.join(QUARANTINE_DIR);
        std::fs::create_dir_all(&qroot).unwrap();
        let (entry, orig, _) = seal_sample(&root, "m.tmp", "mtime-body");
        let stored_mtime = entry.original_mtime_ms.expect("move 应捕获原 mtime");

        let outcome = restore_one(&qroot, &entry);
        assert_eq!(outcome, RestoreOutcome::Restored);
        let restored_mtime = fs::metadata(&orig)
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        // 允许毫秒舍入差
        assert!((restored_mtime - stored_mtime).abs() <= 1000);
        // root 为 SandboxGuard，作用域结束（Drop）自动清覆盖并删沙箱。
    }

    #[test]
    fn conflict_drops_to_restore_conflict_dir() {
        let root = sandbox("conf");
        let qroot = root.join(QUARANTINE_DIR);
        std::fs::create_dir_all(&qroot).unwrap();
        let (entry, orig, hash) = seal_sample(&root, "c.tmp", "conflict-body");
        // 移入后重建原路径占位 → 触发冲突
        std::fs::write(&orig, "occupier").unwrap();

        let outcome = restore_one(&qroot, &entry);
        assert_eq!(outcome, RestoreOutcome::Conflict);
        // 原占位文件仍在，且内容未被覆盖
        assert_eq!(std::fs::read_to_string(&orig).unwrap(), "occupier");
        // 隔离文件被挪到 restore-conflict 下，且内容 hash 一致
        let conflict_base = crate::storage::data_root().join("restore-conflict");
        let relocated = find_in(&conflict_base, &hash);
        assert!(
            relocated.is_some(),
            "应能在 restore-conflict 找到被转移的隔离文件"
        );
        // 隔离区源文件已不在
        assert!(!Path::new(&entry.quarantine_path).exists());
        // root 为 SandboxGuard，作用域结束（Drop）自动清覆盖并删沙箱。
    }

    #[test]
    fn skips_non_quarantined_state() {
        let root = sandbox("skip");
        let qroot = root.join(QUARANTINE_DIR);
        std::fs::create_dir_all(&qroot).unwrap();
        let (mut entry, _, _) = seal_sample(&root, "s.tmp", "skip-body");
        entry.state = ManifestState::Restored; // 模拟已还原状态
        let outcome = restore_one(&qroot, &entry);
        assert!(matches!(outcome, RestoreOutcome::Skipped(_)));
        // root 为 SandboxGuard，作用域结束（Drop）自动清覆盖并删沙箱。
    }

    #[test]
    fn manifest_to_contract_computes_days_left() {
        let now = crate::quarantine::now_ms();
        let e = ManifestEntry {
            id: "id1".into(),
            original_path: r"C:\o\t.txt".into(),
            quarantine_path: r"C:\.pureslate-quarantine\x".into(),
            size_bytes: 100,
            sha256: "h".into(),
            grade: crate::contract::Grade::Yellow,
            category_id: "c".into(),
            moved_at: now,
            expires_at: now + 3 * 86_400_000,
            original_mtime_ms: Some(now),
            restored_at: None,
            state: ManifestState::Quarantined,
        };
        let c = manifest_to_contract(&e);
        assert_eq!(c.days_left, 3);
        assert_eq!(c.state, QuarantineState::Quarantined);
    }

    fn find_in(dir: &Path, hash: &str) -> Option<PathBuf> {
        if !dir.is_dir() {
            return None;
        }
        for sub in std::fs::read_dir(dir).ok()? {
            let sub = sub.ok()?.path();
            if sub.is_dir() {
                if let Some(f) = find_in(&sub, hash) {
                    return Some(f);
                }
            } else if let Ok(c) = sha256_file(&sub) {
                if c == hash {
                    return Some(sub);
                }
            }
        }
        None
    }
}
