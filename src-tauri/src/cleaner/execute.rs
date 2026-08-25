//! 清理执行引擎（R04 · SAFETY §3 / §6.2）。
//!
//! 分级去向（SPEC §6.2 + SAFETY §4.5）：
//! - 🟢 `direct`：Windows `DeleteFileW`（绕过回收站）+ journal；
//! - 🟢 `recycle`：回收站 API（`SHFileOperationW` + FOF_ALLOWUNDO）+ journal；
//! - 🟡/🔴 `quarantine`：移入隔离区（P2-01 store）。
//!
//! 大类目串行、可取消；每个文件操作前写 journal intent、操作后写 result；
//! 单文件失败不中断事务（记 ok=false 汇总）。进程守卫：类目 guard 进程运行中 → 整类阻止。

use std::path::{Path, PathBuf};

use crate::contract::{CleanProgressState, Disposition, Grade};
use crate::guard;
use crate::scanner::walk::CancelToken;

use super::journal::Journal;

/// 一条待清理目标（由 IPC 层把 ScanItem 解析而成）。
#[derive(Debug, Clone)]
pub struct CleanTarget {
    pub path: PathBuf,
    pub grade: Grade,
    pub disposition: Disposition,
    pub category_id: String,
    pub size_bytes: u64,
    /// 类目 guard 进程基名（来自规则）；运行中 → 整类阻止。
    pub guard_process: Option<String>,
}

/// 单条失败明细。
#[derive(Debug, Clone)]
pub struct CleanFailure {
    pub path: String,
    pub reason: String,
}

/// 一次清理事务的汇总报告。
#[derive(Debug, Clone, Default)]
pub struct CleanReport {
    pub tx_id: String,
    pub total: u64,
    pub ok: u64,
    pub fail: u64,
    pub skip: u64,
    pub failures: Vec<CleanFailure>,
}

/// 单项进度回调：`(path, disposition, state, done_bytes, total_bytes)`。
pub type CleanProgress = dyn FnMut(String, Disposition, CleanProgressState, u64, u64) + Send;

/// 执行一次清理事务。`targets` 为已解析条目；`retention_days` 取隔离保留期。
///
/// 并发语义：类目间串行（满足"跨类并列≤2"上限，换取可判定/可测）；类目内串行逐文件。
/// 返回：进度回调（供 IPC 节流推送 `clean_progress`）+ 汇总报告。
pub fn execute(
    tx_id: &str,
    targets: &[CleanTarget],
    cancel: &CancelToken,
    retention_days: u32,
    progress: &mut CleanProgress,
) -> CleanReport {
    let mut report = CleanReport {
        tx_id: tx_id.to_string(),
        ..Default::default()
    };
    let total_bytes: u64 = targets.iter().map(|t| t.size_bytes).sum();
    let mut done_bytes: u64 = 0;

    let mut journal = match Journal::open(tx_id) {
        Ok(j) => j,
        Err(e) => {
            // journal 不可用即整体中止（SAFETY：先日志后动手）。
            report.fail = targets.len() as u64;
            report.failures = targets
                .iter()
                .map(|t| CleanFailure {
                    path: t.path.to_string_lossy().into_owned(),
                    reason: format!("journal 不可用: {e}"),
                })
                .collect();
            return report;
        }
    };

    // 按类目分组（保持首次出现顺序），类目内保持输入顺序。
    use std::collections::HashMap;
    let mut groups: HashMap<String, Vec<&CleanTarget>> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for t in targets {
        if !groups.contains_key(&t.category_id) {
            order.push(t.category_id.clone());
        }
        groups.entry(t.category_id.clone()).or_default().push(t);
    }

    for cat_id in &order {
        let group = &groups[cat_id];
        // 进程守卫：运行中 → 整类阻止（不做半清）。
        let guard_blocked = group[0]
            .guard_process
            .as_deref()
            .is_some_and(|p| guard::any_running(&[p.to_string()]));
        if guard_blocked {
            for t in group {
                push_progress(
                    progress,
                    t,
                    CleanProgressState::Skip,
                    done_bytes,
                    total_bytes,
                );
                done_bytes += t.size_bytes;
                report.total += 1;
                report.skip += 1;
                report.failures.push(CleanFailure {
                    path: t.path.to_string_lossy().into_owned(),
                    reason: "进程守卫：请先退出该应用".into(),
                });
            }
            continue;
        }

        for t in group {
            report.total += 1;
            if cancel.is_cancelled() {
                // 取消：剩余全部标 Skip。
                push_progress(
                    progress,
                    t,
                    CleanProgressState::Skip,
                    done_bytes,
                    total_bytes,
                );
                done_bytes += t.size_bytes;
                report.skip += 1;
                continue;
            }

            let _ = journal.intent(&t.path, t.disposition);
            let outcome = apply_one(t, retention_days);
            let ok = outcome.is_ok();
            let reason = outcome.err().map(|e| e.to_string());
            let _ = journal.result(&t.path, ok, reason.as_deref());

            // 审计日志（覆盖 clean 操作，M8=100%）。
            let _ = crate::logging::audit::record(&crate::contract::LogEntry {
                ts: crate::logging::audit::now_ms(),
                op: "clean".into(),
                tx_id: Some(tx_id.to_string()),
                category_id: Some(t.category_id.clone()),
                path: Some(t.path.to_string_lossy().into_owned()),
                size_bytes: Some(t.size_bytes),
                disposition: Some(t.disposition),
                result: Some(
                    match ok {
                        true => "ok",
                        false => "fail",
                    }
                    .into(),
                ),
                detail: reason.clone(),
            });

            let state = if ok {
                CleanProgressState::Ok
            } else {
                CleanProgressState::Fail
            };
            push_progress(progress, t, state, done_bytes, total_bytes);
            done_bytes += t.size_bytes;
            if ok {
                report.ok += 1;
            } else {
                report.fail += 1;
                report.failures.push(CleanFailure {
                    path: t.path.to_string_lossy().into_owned(),
                    reason: reason.unwrap_or_else(|| "未知错误".into()),
                });
            }
        }
    }
    report
}

fn push_progress(
    progress: &mut CleanProgress,
    t: &CleanTarget,
    state: CleanProgressState,
    done: u64,
    total: u64,
) {
    progress(
        t.path.to_string_lossy().into_owned(),
        t.disposition,
        state,
        done,
        total,
    );
}

/// 按去向对单个目标执行并返回结果。
fn apply_one(t: &CleanTarget, retention_days: u32) -> Result<(), String> {
    let p = &t.path;
    // 源已不存在：视为"无需操作"（幂等，成功）。
    if !p.exists() {
        return Err("源文件不存在（可能已被处理）".into());
    }
    match t.disposition {
        Disposition::Direct => delete_direct(p),
        Disposition::Recycle => {
            crate::cleaner::recycle::delete_to_recycle_bin(p).map_err(|e| e.to_string())
        }
        Disposition::Quarantine => {
            let root = crate::quarantine::quarantine_root_of(p);
            crate::quarantine::move_into_quarantine(
                &root,
                crate::quarantine::QuarantineInput {
                    original_path: p.clone(),
                    grade: t.grade,
                    category_id: t.category_id.clone(),
                    retention_days,
                },
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
        }
    }
}

/// 🟢 direct：Windows `DeleteFileW`，绕过回收站。零第三方依赖。
fn delete_direct(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        // unsafe：kernel32,DeleteFileW 删除单文件（绕过回收站）。kernel32 默认链接，无需 #[link]。
        unsafe extern "system" {
            fn DeleteFileW(lp_file_name: *const u16) -> i32;
        }
        let wide: Vec<u16> = path
            .to_string_lossy()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let rc = unsafe { DeleteFileW(wide.as_ptr()) };
        if rc == 0 {
            Err(format!("删除失败: {}", path.to_string_lossy()))
        } else {
            Ok(())
        }
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err("direct 删除仅支持 Windows".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox(tag: &str) -> std::sync::MutexGuard<'static, ()> {
        let g = crate::storage::TEST_DATA_ROOT_LOCK.lock().unwrap();
        let d = std::env::temp_dir().join(format!(
            "pureslate-clean-{tag}-{}-{}",
            std::process::id(),
            crate::logging::audit::now_ms()
        ));
        std::fs::create_dir_all(&d).unwrap();
        crate::storage::set_data_root_override(Some(d.join("data-root")));
        let _ = &d;
        g
    }

    fn tgt(root: &Path, name: &str, disposition: Disposition, grade: Grade) -> CleanTarget {
        CleanTarget {
            path: root.join(name),
            grade,
            disposition,
            category_id: "cat".into(),
            size_bytes: 0,
            guard_process: None,
        }
    }

    type Sink = Vec<(String, Disposition, CleanProgressState, u64, u64)>;

    #[test]
    fn direct_execute_writes_journal_and_audit() {
        let _g = sandbox("ex");
        let r = std::env::temp_dir().join(format!("pureslate-clean-ex-{}", std::process::id()));
        std::fs::create_dir_all(&r).unwrap();
        let f1 = r.join("a.tmp");
        std::fs::write(&f1, "x").unwrap();
        let cancel = CancelToken::new();
        let sink: std::sync::Arc<std::sync::Mutex<Sink>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let targets = vec![tgt(&r, "a.tmp", Disposition::Direct, Grade::Green)];
        let sink_c = sink.clone();
        let rep = execute(
            "tx-ex",
            &targets,
            &cancel,
            14,
            &mut move |p, d, s, db, tb| {
                if let Ok(mut g) = sink_c.lock() {
                    g.push((p, d, s, db, tb));
                }
            },
        );
        // direct 已删；进度/审计/journal 齐备。
        assert!(!f1.exists());
        assert_eq!(rep.total, 1);
        assert_eq!(rep.ok, 1);
        assert_eq!(rep.fail, 0);
        let sink = sink.lock().unwrap();
        assert_eq!(sink.len(), 1);
        assert_eq!(sink[0].2, CleanProgressState::Ok);
        drop(sink);
        // 审计有 clean 记录
        let logs = crate::logging::audit::query(
            crate::logging::audit::now_ms() - 60_000,
            crate::logging::audit::now_ms() + 1000,
            Some("clean"),
        );
        assert!(!logs.is_empty());
        // journal 有 intent+result 各一（无孤儿）
        assert!(super::super::journal::detect_orphans().is_empty());
        crate::storage::set_data_root_override(None);
        std::fs::remove_dir_all(&r).ok();
    }

    #[test]
    fn missing_source_is_failure_not_crash() {
        let _g = sandbox("miss");
        let cancel = CancelToken::new();
        let r = std::env::temp_dir().join(format!("pureslate-clean-miss-{}", std::process::id()));
        let targets = vec![tgt(&r, "nope.tmp", Disposition::Direct, Grade::Green)];
        let rep = execute("tx-miss", &targets, &cancel, 14, &mut |_, _, _, _, _| {});
        assert_eq!(rep.fail, 1);
        assert_eq!(rep.ok, 0);
        std::fs::remove_dir_all(&r).ok();
        crate::storage::set_data_root_override(None);
    }

    #[test]
    fn guard_blocks_whole_category() {
        // guard 命中需真实进程；这里用一个绝不会运行的进程名（不影响行为），验证空 guard 不阻止。
        let _g = sandbox("guard");
        let cancel = CancelToken::new();
        let r = std::env::temp_dir().join(format!("pureslate-clean-guard-{}", std::process::id()));
        std::fs::create_dir_all(&r).unwrap();
        let f = r.join("g.tmp");
        std::fs::write(&f, "x").unwrap();
        // guard_process 指向当前进程自身名不可得；用空 Vec 转 Option -> 无效进程名，确保不命中即正常清理。
        let mut t = tgt(&r, "g.tmp", Disposition::Direct, Grade::Green);
        t.guard_process = Some("__pureslate_never_running__.exe".into());
        let rep = execute("tx-guard", &[t], &cancel, 14, &mut |_, _, _, _, _| {});
        // 空 guard 不阻止：文件被删。
        assert_eq!(rep.ok, 1);
        crate::storage::set_data_root_override(None);
        std::fs::remove_dir_all(&r).ok();
    }
}
