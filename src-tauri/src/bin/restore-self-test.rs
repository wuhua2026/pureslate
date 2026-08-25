//! P2-02 还原自测二进制（tools/test-restore.ps1 调用）。
//!
//! 循环 N 次：造样本 → 移入隔离区 → 还原 → sha256 比对原路径。
//! 统计还原成功计数；比率 < 阈值（默认 0.95）时以非零退出码结束（M2 还原率门禁）。
//!
//! 纯沙箱运行：样本/quarantine 均在系统临时目录下，`data_root` 覆盖到沙箱，
//! 绝不触碰真实 `%LOCALAPPDATA%` 或用户文件（只读红线守规矩：仅操作自建样本）。

use std::path::PathBuf;
use std::process::ExitCode;

use pureslate_lib::contract::Grade;
use pureslate_lib::quarantine::{
    move_into_quarantine, restore_one, sha256_file, QuarantineInput, RestoreOutcome, QUARANTINE_DIR,
};
use pureslate_lib::storage::set_data_root_override;

fn main() -> ExitCode {
    let cycles: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(100);
    let threshold = 0.95f64;

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
    let mut fail = 0u64;
    for i in 0..cycles {
        let body = format!("sample-{i}-0123456789abcdefghijklmnopqrstuvwsyz0123456789");
        let orig = sandbox.join(format!("sample-{i}.tmp"));
        if std::fs::write(&orig, &body).is_err() {
            fail += 1;
            continue;
        }
        let hash = match sha256_file(&orig) {
            Ok(h) => h,
            Err(_) => {
                fail += 1;
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
            Err(_) => {
                fail += 1;
                continue;
            }
        };
        // 还原
        match restore_one(&qroot, &entry) {
            RestoreOutcome::Restored => {
                let back_ok = orig.is_file()
                    && sha256_file(&orig).map(|h| h == hash).unwrap_or(false)
                    && !PathBuf::from(&entry.quarantine_path).exists();
                if back_ok {
                    ok += 1;
                } else {
                    eprintln!("[restore-self-test] 迭代 {i}: 还原内容校验失败");
                    fail += 1;
                }
            }
            other => {
                eprintln!("[restore-self-test] 迭代 {i}: 还原未成功: {other:?}");
                fail += 1;
            }
        }
    }

    let total = ok + fail;
    let ratio = if total == 0 {
        0.0
    } else {
        ok as f64 / total as f64
    };
    println!("restore-self-test: passed={ok}/total={total} ratio={ratio:.4}");
    let _ = std::fs::remove_dir_all(&sandbox);
    set_data_root_override(None);

    if total == 0 || ratio < threshold {
        eprintln!("[restore-self-test] 还原率 < {threshold}, 未达 M2 门禁");
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
