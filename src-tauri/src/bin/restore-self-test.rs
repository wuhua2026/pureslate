//! P4-05 M12 千次还原自测（tools/test-restore.ps1 调用）。
//!
//! 循环 N 次：造样本 → 移入隔离区 → 还原 → sha256 比对原路径。
//! stdout 输出 JSON 报告（落 docs/verify/restore-1000.json）；stderr 输出人读
//! 进度与结论。门禁：成功率 ≥ 阈值（M12 = 1000 次 ≥ 99.9%，即 ≤1 失败）；
//! 每个失败项按阶段给出兜底指引（SAFETY §6.2）。
//!
//! 纯沙箱运行：样本/隔离区/数据根均在系统临时目录下（data_root 覆盖注入），
//! 绝不触碰真实 `%LOCALAPPDATA%` 或用户文件。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use pureslate_lib::contract::Grade;
use pureslate_lib::quarantine::{
    move_into_quarantine, restore_one, sha256_file, QuarantineInput, RestoreOutcome, QUARANTINE_DIR,
};
use pureslate_lib::storage::set_data_root_override;

use serde::Serialize;

/// 失败阶段 → 兜底指引（SAFETY §6.2：失败项出兜底指引）。
const GUIDANCE: [(&str, &str); 5] = [
    (
        "write",
        "样本创建失败：检查临时目录剩余空间与写权限后重跑该轮（样本为自建文件，无数据风险）。",
    ),
    (
        "hash",
        "sha256 计算失败：文件可能被杀软/索引器等外部进程短暂锁定，重跑该轮。",
    ),
    (
        "quarantine",
        "移入隔离区失败：检查目标卷剩余空间与写权限；查 manifest.jsonl 与 journal 是否残留（state=quarantined 的行可经 quarantine_restore 还原，数据不丢）。",
    ),
    (
        "restore",
        "还原未完成：原路径被占用/已存在时隔离文件不丢失——已自动落入 <数据根>\\restore-conflict\\<日期>\\，可人工取回；确认占用进程后重试。",
    ),
    (
        "verify",
        "最严重：还原后内容与原始 sha256 不一致——导出审计日志与隔离区 manifest（含 sha256/原路径）定位差异，连同复现步骤提 issue。",
    ),
];

fn guidance_of(stage: &str) -> &'static str {
    GUIDANCE
        .iter()
        .find(|(s, _)| *s == stage)
        .map(|(_, g)| *g)
        .unwrap_or("重跑该轮；若复现请导出审计日志提 issue。")
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct FailureRecord {
    iteration: u64,
    stage: &'static str,
    reason: String,
    guidance: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    ts: String,
    build_profile: &'static str,
    cycles_requested: u64,
    threshold: f64,
    total: u64,
    passed: u64,
    failed: u64,
    ratio: f64,
    gate_pass: bool,
    duration_ms: u64,
    stage_counts: BTreeMap<String, u64>,
    /// 失败明细（上限 50 条防报告膨胀；全量计数见 stage_counts）。
    failures: Vec<FailureRecord>,
    failures_truncated: bool,
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let cycles: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(100);
    let threshold: f64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(0.95);

    let started = Instant::now();
    let sandbox = std::env::temp_dir().join(format!(
        "pureslate-restore-self-test-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&sandbox);
    if std::fs::create_dir_all(&sandbox).is_err() {
        eprintln!("[restore-self-test] 无法创建沙箱 {sandbox:?}");
        return ExitCode::FAILURE;
    }
    set_data_root_override(Some(sandbox.join("data-root")));
    let qroot = sandbox.join(QUARANTINE_DIR);

    let mut ok = 0u64;
    let mut stage_counts: BTreeMap<String, u64> = BTreeMap::new();
    let mut failures: Vec<FailureRecord> = Vec::new();
    let record_fail = |i: u64,
                       stage: &'static str,
                       reason: String,
                       stage_counts: &mut BTreeMap<String, u64>,
                       failures: &mut Vec<FailureRecord>| {
        *stage_counts.entry(stage.to_string()).or_insert(0) += 1;
        if failures.len() < 50 {
            failures.push(FailureRecord {
                iteration: i,
                stage,
                reason,
                guidance: guidance_of(stage),
            });
        }
    };

    for i in 0..cycles {
        if i > 0 && i % 100 == 0 {
            eprintln!(
                "[restore-self-test] 进度 {i}/{cycles}（passed={ok} failed={}）",
                failures.len()
            );
        }
        let body = format!("sample-{i}-0123456789abcdefghijklmnopqrstuvwsyz0123456789");
        let orig = sandbox.join(format!("sample-{i}.tmp"));
        if let Err(e) = std::fs::write(&orig, &body) {
            record_fail(
                i,
                "write",
                format!("样本写入失败: {e}"),
                &mut stage_counts,
                &mut failures,
            );
            continue;
        }
        let hash = match sha256_file(&orig) {
            Ok(h) => h,
            Err(e) => {
                record_fail(
                    i,
                    "hash",
                    format!("sha256 计算失败: {e}"),
                    &mut stage_counts,
                    &mut failures,
                );
                continue;
            }
        };
        let entry = match move_into_quarantine(
            &qroot,
            QuarantineInput {
                original_path: orig.clone(),
                grade: Grade::Green,
                category_id: "self.test".into(),
                retention_days: 14,
            },
        ) {
            Ok(e) => e,
            Err(e) => {
                record_fail(
                    i,
                    "quarantine",
                    format!("移入隔离区失败: {e}"),
                    &mut stage_counts,
                    &mut failures,
                );
                continue;
            }
        };
        // 还原 + 内容校验
        match restore_one(&qroot, &entry) {
            RestoreOutcome::Restored => {
                let back_ok = orig.is_file()
                    && sha256_file(&orig).map(|h| h == hash).unwrap_or(false)
                    && !PathBuf::from(&entry.quarantine_path).exists();
                if back_ok {
                    ok += 1;
                } else {
                    record_fail(
                        i,
                        "verify",
                        "还原后原路径内容校验失败（缺失或 sha256 不符）".into(),
                        &mut stage_counts,
                        &mut failures,
                    );
                }
            }
            other => {
                record_fail(
                    i,
                    "restore",
                    format!("还原未成功: {other:?}"),
                    &mut stage_counts,
                    &mut failures,
                );
            }
        }
    }

    let total = ok + failures.len() as u64;
    let ratio = if total == 0 {
        0.0
    } else {
        ok as f64 / total as f64
    };
    let gate_pass = total > 0 && ratio >= threshold;
    let report = Report {
        ts: iso_now(),
        build_profile: if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        cycles_requested: cycles,
        threshold,
        total,
        passed: ok,
        failed: failures.len() as u64,
        ratio,
        gate_pass,
        duration_ms: started.elapsed().as_millis() as u64,
        stage_counts: stage_counts.clone(),
        failures: failures.clone(),
        failures_truncated: stage_counts.values().sum::<u64>() > failures.len() as u64,
    };

    // stdout：JSON 报告（重定向落 docs/verify/restore-1000.json）；stderr：人读结论。
    println!(
        "{}",
        serde_json::to_string_pretty(&report).unwrap_or_default()
    );
    eprintln!(
        "restore-self-test: passed={ok}/total={total} ratio={ratio:.4} threshold={threshold} gate={}",
        if gate_pass { "PASS" } else { "FAIL" }
    );
    for (stage, n) in &stage_counts {
        eprintln!("  stage[{stage}] failed={n}");
    }
    let _ = std::fs::remove_dir_all(&sandbox);
    set_data_root_override(None);

    if !gate_pass {
        eprintln!(
            "[restore-self-test] 还原率 {ratio:.4} < 阈值 {threshold}，未达 M12 门禁（失败项兜底指引见报告 failures[].guidance）"
        );
        ExitCode::FAILURE
    } else {
        eprintln!("[restore-self-test] M12 门禁通过。");
        ExitCode::SUCCESS
    }
}

fn iso_now() -> String {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (y, mo, d) = civil_from_days((n / 86400) as i64);
    let s = n % 86400;
    format!(
        "{y:04}-{mo:02}-{d:02}T{:02}:{:02}:{:02}Z",
        s / 3600,
        (s % 3600) / 60,
        s % 60
    )
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as u32, d as u32)
}
