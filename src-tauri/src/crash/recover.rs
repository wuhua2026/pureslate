//! 孤儿 journal 启动恢复（R24 · SAFETY §3 规则 2，T-6 协议实现）。
//!
//! **T-6 恢复协议（安全审计「journal 路径信任」防 confused deputy，先定死后实现）**：
//! 1. journal 内容**只用于决定"查哪条 manifest 记录"**——绝不直接对 journal 声称的
//!    path 执行任何删除/移动/写入；不可逆去向（direct/recycle）一律只标记不动作
//!    （SAFETY §3：🟢 类可重建，损害可控，UI 醒目提示即可）；
//! 2. quarantine 孤儿的一切文件移动复用 `restore_one`（T-2 四重校验：隔离区根内 /
//!    sha256 / 白名单禁区 / 父链 reparse）——**manifest 是唯一信任锚**；
//! 3. 崩溃窗口「move 已发生、manifest 未写」（store 顺序 intent→move→manifest→result
//!    的中段）：按隔离区命名约定 `<sha256 前 8>_<原名>` 反查**未被任何 manifest 引用**
//!    的候选，唯一命中才**补登记**进 manifest（grade=🟡、category=crash.recovered）；
//!    **不自动还原**——内容身份无法再对已不存在的原文件证明，交用户在隔离区页自行还原；
//! 4. 恢复动作逐条审计（op=recover）；完成后向对应 tx journal **追加** result 行闭环
//!    （只追加不重写），下次启动不再重复处理。
//!
//! 决策表（quarantine 孤儿，按序判定）：
//! | manifest 有 Quarantined 态记录 | → restore_one（四重校验）自动还原 |
//! | manifest 有终态记录(restored/purged) | → 无需处理（已闭环） |
//! | manifest 无记录 且 原路径仍存在 | → 崩溃发生在移动前，文件未受影响 |
//! | manifest 无记录 且 原路径缺失 | → 崩溃窗口 3：反查补登记（唯一候选）/否则记失败 |

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::cleaner::journal::{detect_orphans, Journal, OrphanIntent};
use crate::contract::{CrashRecoveryReport, Disposition, Grade, LogEntry};
use crate::quarantine::{
    add_manifest_entry, load_manifest, restore_one, sha256_file, ManifestEntry, ManifestState,
    RestoreOutcome,
};

/// 对真实全部固定盘隔离区根执行恢复（启动钩子调用；保留期取当前设置）。
pub fn recover_globally() -> CrashRecoveryReport {
    let settings = crate::storage::load_settings();
    let now = crate::quarantine::now_ms();
    recover_with(
        &crate::quarantine::all_quarantine_roots(),
        settings.quarantine_retention_days,
        now,
    )
}

/// 恢复核心（roots/保留期/时钟注入，沙箱测试用）。
/// journal 目录取 `data_root()`（测试经 `set_data_root_override` 注入）。
pub fn recover_with(roots: &[PathBuf], retention_days: u32, now: i64) -> CrashRecoveryReport {
    let mut report = CrashRecoveryReport {
        ran_at: now,
        ..Default::default()
    };
    let orphans = detect_orphans();
    report.orphans_found = orphans.len() as u64;
    for o in orphans {
        match o.disposition {
            Disposition::Quarantine => {
                recover_quarantine_orphan(&o, roots, retention_days, now, &mut report)
            }
            Disposition::Direct | Disposition::Recycle => {
                // SAFETY §3：不可逆去向无法回滚——只标记 orphan + 审计 + UI 醒目提示。
                let detail = "崩溃中断：不可逆去向（direct/recycle）无法回滚，已标记 orphan";
                close_intent(&o, false, detail);
                report.irreversible += 1;
                audit_recover(&o, "fail", detail, now);
            }
        }
    }
    report
}

fn recover_quarantine_orphan(
    o: &OrphanIntent,
    roots: &[PathBuf],
    retention_days: u32,
    now: i64,
    report: &mut CrashRecoveryReport,
) {
    // ① manifest 有 Quarantined 态记录 → 经 T-2 四重校验还原。
    if let Some((root, entry)) = find_by_original(roots, &o.path, Some(ManifestState::Quarantined))
    {
        match restore_one(&root, &entry) {
            RestoreOutcome::Restored => {
                report.restored += 1;
                let detail = "启动恢复：已还原至原路径";
                close_intent(o, true, detail);
                audit_recover(o, "ok", detail, now);
            }
            RestoreOutcome::Conflict => {
                report.conflict_restored += 1;
                let detail = "启动恢复：原路径被占用，已落 restore-conflict 兜底目录";
                close_intent(o, true, detail);
                audit_recover(o, "ok", detail, now);
            }
            RestoreOutcome::Skipped(r) | RestoreOutcome::Failed(r) => {
                report.failures.push(format!("{}: {r}", o.path));
                let detail = format!("启动恢复失败：{r}");
                close_intent(o, false, &detail);
                audit_recover(o, "fail", &detail, now);
            }
        }
        return;
    }

    // ② manifest 已有终态记录（此前已还原/已清除）→ 无需处理。
    if find_by_original(roots, &o.path, None).is_some() {
        let detail = "启动恢复：已有终态隔离记录（restored/purged），无需处理";
        close_intent(o, true, detail);
        report.untouched += 1;
        audit_recover(o, "ok", detail, now);
        return;
    }

    // ③ 崩溃发生在移动前 → 文件未受影响。
    if Path::new(&o.path).exists() {
        let detail = "启动恢复：崩溃发生在移动前，文件未受影响";
        close_intent(o, true, detail);
        report.untouched += 1;
        audit_recover(o, "ok", detail, now);
        return;
    }

    // ④ 崩溃窗口「move 已发生、manifest 未写」→ 按命名约定反查唯一候选，补登记。
    let Some(orig_name) = Path::new(&o.path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
    else {
        let detail = "启动恢复失败：journal 路径无文件名，无法定位";
        report.failures.push(format!("{}: 路径无文件名", o.path));
        close_intent(o, false, detail);
        audit_recover(o, "fail", detail, now);
        return;
    };
    match find_unclaimed_candidate(roots, &orig_name) {
        Ok(Some((root, cand))) => match adopt_into_manifest(&root, &cand, o, retention_days, now) {
            Ok(()) => {
                let detail = "启动恢复：崩溃中已移动但未登记，已补登记至隔离区（未自动还原，请到隔离区页操作）";
                close_intent(o, true, detail);
                report.adopted += 1;
                audit_recover(o, "ok", detail, now);
            }
            Err(e) => {
                report.failures.push(format!("{}: {e}", o.path));
                close_intent(o, false, &format!("启动恢复失败：{e}"));
                audit_recover(o, "fail", &e, now);
            }
        },
        Ok(None) => {
            let detail = "启动恢复失败：无法定位崩溃中已移动的文件（隔离区无未登记匹配项）";
            report.failures.push(format!("{}: 无匹配候选", o.path));
            close_intent(o, false, detail);
            audit_recover(o, "fail", detail, now);
        }
        Err(ambiguous) => {
            report.failures.push(format!("{}: {ambiguous}", o.path));
            close_intent(o, false, &format!("启动恢复失败：{ambiguous}"));
            audit_recover(o, "fail", &ambiguous, now);
        }
    }
}

/// 按 original_path 查 manifest。`want_state=Some(s)` 只匹配该态；None 匹配任意态。
/// 命中多条（同路径多次隔离）时取 moved_at 最新。
fn find_by_original(
    roots: &[PathBuf],
    original: &str,
    want_state: Option<ManifestState>,
) -> Option<(PathBuf, ManifestEntry)> {
    let norm = original.to_lowercase().replace('/', "\\");
    let mut best: Option<(PathBuf, ManifestEntry)> = None;
    for root in roots {
        let Ok((entries, _)) = load_manifest(root) else {
            continue;
        };
        for e in entries {
            if let Some(s) = want_state {
                if e.state != s {
                    continue;
                }
            }
            if e.original_path.to_lowercase().replace('/', "\\") == norm
                && best.as_ref().is_none_or(|(_, b)| e.moved_at >= b.moved_at)
            {
                best = Some((root.clone(), e));
            }
        }
    }
    best
}

/// 在隔离区根内反查「未登记」候选：文件名形如 `<8 位十六进制>_<原名>`（store 命名约定），
/// 深度 ≤3（yyyyMM/batchId/file），且其路径未被任何 manifest 行引用。
/// 返回 Err = 候选不唯一（拒绝自动采纳）；Ok(None) = 无候选。
fn find_unclaimed_candidate(
    roots: &[PathBuf],
    orig_name: &str,
) -> Result<Option<(PathBuf, PathBuf)>, String> {
    let mut claimed: HashSet<String> = HashSet::new();
    for root in roots {
        if let Ok((entries, _)) = load_manifest(root) {
            for e in entries {
                claimed.insert(e.quarantine_path.to_lowercase());
            }
        }
    }
    let mut hits: Vec<(PathBuf, PathBuf)> = Vec::new();
    for root in roots {
        for entry in walkdir::WalkDir::new(root).max_depth(3).follow_links(false) {
            let Ok(e) = entry else {
                continue;
            };
            let p = e.path();
            if !e.file_type().is_file() {
                continue;
            }
            let Some(name) = p.file_name().map(|n| n.to_string_lossy().into_owned()) else {
                continue;
            };
            let Some((hash, rest)) = name.split_once('_') else {
                continue;
            };
            if hash.len() != 8 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
                continue;
            }
            if rest != orig_name {
                continue;
            }
            if claimed.contains(&p.to_string_lossy().to_lowercase()) {
                continue;
            }
            hits.push((root.clone(), p.to_path_buf()));
        }
    }
    match hits.len() {
        0 => Ok(None),
        1 => Ok(hits.pop()),
        n => Err(format!("隔离区匹配候选不唯一（{n} 处），拒绝自动采纳")),
    }
}

/// 把崩溃窗口中"已移入未登记"的文件补登记进 manifest（不移动文件本体）。
fn adopt_into_manifest(
    root: &Path,
    candidate: &Path,
    o: &OrphanIntent,
    retention_days: u32,
    now: i64,
) -> Result<(), String> {
    let sha = sha256_file(candidate).map_err(|e| format!("读取隔离文件失败: {e}"))?;
    let meta = std::fs::metadata(candidate).map_err(|e| format!("读取隔离文件元数据失败: {e}"))?;
    let original_mtime_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64);
    let entry = ManifestEntry {
        id: uuid::Uuid::new_v4().to_string(),
        original_path: o.path.clone(),
        quarantine_path: candidate.to_string_lossy().into_owned(),
        size_bytes: meta.len(),
        sha256: sha,
        // journal 不记等级；归 🟡（保持可还原语义，🔴 会使还原入口灰禁）。
        grade: Grade::Yellow,
        category_id: "crash.recovered".into(),
        moved_at: now,
        expires_at: now + (retention_days as i64) * 86_400_000,
        original_mtime_ms,
        restored_at: None,
        state: ManifestState::Quarantined,
    };
    add_manifest_entry(root, &entry).map_err(|e| format!("补登记写入 manifest 失败: {e}"))
}

/// 向孤儿所在 tx journal 追加 result 行闭环（只追加，SAFETY §3）。
fn close_intent(o: &OrphanIntent, ok: bool, detail: &str) {
    if let Ok(mut j) = Journal::open(&o.tx_id) {
        let _ = j.result(Path::new(&o.path), ok, Some(detail));
    }
}

fn audit_recover(o: &OrphanIntent, result: &str, detail: &str, now: i64) {
    let _ = crate::logging::audit::record(&LogEntry {
        ts: now,
        op: "recover".into(),
        tx_id: Some(o.tx_id.clone()),
        category_id: None,
        path: Some(o.path.clone()),
        size_bytes: None,
        disposition: Some(o.disposition),
        result: Some(result.into()),
        detail: Some(detail.into()),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 与 lib 单测共享全局 DATA_ROOT_OVERRIDE：持锁整个测试体（LESSONS ① 教训）。
    fn sandbox(tag: &str) -> (PathBuf, std::sync::MutexGuard<'static, ()>) {
        // 抗中毒（LESSONS ②）：单测首败不级联污染同批用例。
        let g = crate::storage::TEST_DATA_ROOT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let d = std::env::temp_dir().join(format!(
            "pureslate-recover-{tag}-{}-{}",
            std::process::id(),
            crate::logging::audit::now_ms()
        ));
        std::fs::create_dir_all(d.join("data-root")).unwrap();
        std::fs::create_dir_all(d.join("orig")).unwrap();
        crate::storage::set_data_root_override(Some(d.join("data-root")));
        (d, g)
    }

    /// 写一条孤儿 intent（tx 未写 result）。
    fn orphan_intent(tx: &str, path: &Path, disp: Disposition) {
        let mut j = Journal::open(tx).unwrap();
        j.intent(path, disp).unwrap();
    }

    /// 造一个"已移入 + 已登记"的隔离条目，返回 (qroot, entry)。
    fn sealed(root: &Path, name: &str) -> (PathBuf, ManifestEntry) {
        let orig = root.join("orig").join(name);
        std::fs::write(&orig, "recover-body").unwrap();
        let qroot = root.join("q");
        std::fs::create_dir_all(&qroot).unwrap();
        let entry = crate::quarantine::move_into_quarantine(
            &qroot,
            crate::quarantine::QuarantineInput {
                original_path: orig,
                grade: Grade::Yellow,
                category_id: "cat.test".into(),
                retention_days: 14,
            },
        )
        .unwrap();
        (qroot, entry)
    }

    #[test]
    fn manifest_backed_orphan_is_restored_and_closed() {
        let (root, _g) = sandbox("restore");
        let (qroot, entry) = sealed(&root, "a.tmp");
        let orig = PathBuf::from(&entry.original_path);
        orphan_intent("tx-r1", &orig, Disposition::Quarantine);
        assert!(!orig.exists());

        let report = recover_with(
            std::slice::from_ref(&qroot),
            14,
            crate::quarantine::now_ms(),
        );
        assert_eq!(report.orphans_found, 1);
        assert_eq!(report.restored, 1);
        assert!(orig.is_file());
        // journal 已闭环
        assert!(detect_orphans().is_empty());
        // manifest 已迁移
        let (entries, _) = load_manifest(&qroot).unwrap();
        assert_eq!(entries[0].state, ManifestState::Restored);
    }

    #[test]
    fn pre_move_crash_is_untouched() {
        let (root, _g) = sandbox("premove");
        let orig = root.join("orig").join("b.tmp");
        std::fs::write(&orig, "still-here").unwrap();
        orphan_intent("tx-r2", &orig, Disposition::Quarantine);

        let report = recover_with(&[root.join("q")], 14, crate::quarantine::now_ms());
        assert_eq!(report.untouched, 1);
        assert!(orig.is_file());
        assert!(detect_orphans().is_empty());
    }

    #[test]
    fn direct_orphan_is_irreversible_marker_only() {
        let (root, _g) = sandbox("direct");
        let orig = root.join("orig").join("c.tmp");
        std::fs::write(&orig, "green").unwrap();
        orphan_intent("tx-r3", &orig, Disposition::Direct);

        let report = recover_with(&[], 14, crate::quarantine::now_ms());
        assert_eq!(report.irreversible, 1);
        // 不可逆去向：只标记，不动文件
        assert!(orig.is_file());
        assert!(detect_orphans().is_empty());
    }

    #[test]
    fn move_without_manifest_is_adopted_not_restored() {
        let (root, _g) = sandbox("adopt");
        let orig = root.join("orig").join("被清样本 🎮.tmp");
        std::fs::write(&orig, "adopt-body").unwrap();
        orphan_intent("tx-r4", &orig, Disposition::Quarantine);
        // 模拟崩溃窗口：手工按命名约定移入（manifest 未写）
        let qroot = root.join("q");
        let batch = qroot.join("202610").join("batch-1");
        std::fs::create_dir_all(&batch).unwrap();
        let cand = batch.join("deadbeef_被清样本 🎮.tmp");
        std::fs::rename(&orig, &cand).unwrap();

        let report = recover_with(
            std::slice::from_ref(&qroot),
            14,
            crate::quarantine::now_ms(),
        );
        assert_eq!(report.adopted, 1);
        // 补登记但未自动还原：原路径仍缺失、文件仍在隔离区
        assert!(!orig.exists());
        assert!(cand.is_file());
        let (entries, _) = load_manifest(&qroot).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].state, ManifestState::Quarantined);
        assert_eq!(entries[0].category_id, "crash.recovered");
        assert_eq!(entries[0].original_path, orig.to_string_lossy());
        assert!(detect_orphans().is_empty());
    }

    #[test]
    fn ambiguous_candidates_are_rejected() {
        let (root, _g) = sandbox("ambig");
        let orig = root.join("orig").join("d.tmp");
        std::fs::write(&orig, "x").unwrap();
        orphan_intent("tx-r5", &orig, Disposition::Quarantine);
        // 模拟崩溃窗口：移动已发生（原路径清空），但两个批次各留同名未登记候选。
        std::fs::remove_file(&orig).unwrap();
        let qroot = root.join("q");
        for b in ["batch-1", "batch-2"] {
            let batch = qroot.join("202610").join(b);
            std::fs::create_dir_all(&batch).unwrap();
            std::fs::write(batch.join("deadbeef_d.tmp"), "candidate").unwrap();
        }

        let report = recover_with(
            std::slice::from_ref(&qroot),
            14,
            crate::quarantine::now_ms(),
        );
        assert_eq!(report.adopted, 0);
        assert_eq!(report.failures.len(), 1);
        assert!(report.failures[0].contains("不唯一"));
        let (entries, _) = load_manifest(&qroot).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn terminal_state_entry_resolves_without_action() {
        let (root, _g) = sandbox("terminal");
        let (qroot, entry) = sealed(&root, "e.tmp");
        // 模拟：孤儿 journal + 该条目已被用户手动还原（终态）
        crate::quarantine::update_entry_state(&qroot, &entry.id, ManifestState::Restored).unwrap();
        orphan_intent(
            "tx-r6",
            &PathBuf::from(&entry.original_path),
            Disposition::Quarantine,
        );

        let report = recover_with(&[qroot], 14, crate::quarantine::now_ms());
        assert_eq!(report.untouched, 1);
        assert_eq!(report.restored, 0);
        assert!(detect_orphans().is_empty());
    }

    #[test]
    fn tampered_manifest_recovery_is_refused_by_t2() {
        // T-6 语义验证：journal+manifest 组合也不能把伪造还原目标写进白名单禁区——
        // restore_one 的 T-2 ③ 校验必须仍然生效。
        let (root, _g) = sandbox("t2");
        let (qroot, _) = sealed(&root, "f.tmp");
        if let Ok(win) = std::env::var("SystemRoot") {
            let evil = format!("{win}\\system32\\evil.dll");
            // 直接改写 manifest 行的 originalPath 模拟篡改（manifest 对用户可写）。
            let (entries, _) = load_manifest(&qroot).unwrap();
            let mut tampered = entries;
            tampered[0].original_path = evil.clone();
            let mut out = String::new();
            for e in &tampered {
                out.push_str(&serde_json::to_string(e).unwrap());
                out.push('\n');
            }
            std::fs::write(qroot.join("manifest.jsonl"), out).unwrap();

            orphan_intent("tx-r7", Path::new(&evil), Disposition::Quarantine);
            let report = recover_with(
                std::slice::from_ref(&qroot),
                14,
                crate::quarantine::now_ms(),
            );
            assert_eq!(report.restored, 0);
            assert_eq!(report.failures.len(), 1);
            assert!(report.failures[0].contains("白名单"));
            assert!(!Path::new(&evil).exists());
        }
    }
}
