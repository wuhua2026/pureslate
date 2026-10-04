//! crash-sim（R24 kill 测试辅助二进制，仅测试/真机演练用，非产品功能）。
//!
//! 用法：
//! - `crash-sim clean <txId>`——模拟清理事务被 kill：逐文件 intent → 移入隔离区
//!   （写 manifest）→ **停顿 300ms** → result；第 3 个文件移入后写 marker
//!   （父进程轮询到 marker 即 kill，此时该文件「intent+manifest 已写、result 未写」
//!   = 真实 kill 窗口形态）。
//!   工作目录全部派生自数据根（**CLI 不接收任何路径**，收敛 traversal 面）：
//!   `<data_root>\sim\src`（输入样本）｜`<data_root>\sim\q`（隔离区根）｜
//!   `<data_root>\sim\marker`（kill 信号）；journal/审计落 `<data_root>`。
//! - `crash-sim dump`——走 panic-hook 通道写一份 minidump 后正常退出（0）；
//! - `crash-sim seh`——触发硬件访问违规 → SEH 顶级过滤器写 dump → 内核终止进程；
//! - `crash-sim panic`——触发 Rust panic → hook 写 dump（无异常流）→ 退出码 101。
//!
//! 数据根经环境变量 `PURESLATE_DATA_ROOT` 注入，与父测试进程共享沙箱
//! （等价于既有 LOCALAPPDATA 信任面）。

use std::path::PathBuf;
use std::time::Duration;

use pureslate_lib::cleaner::journal::Journal;
use pureslate_lib::contract::{Disposition, Grade};
use pureslate_lib::crash;
use pureslate_lib::quarantine::{self, QuarantineInput};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("clean") => clean(args.get(2).map(String::as_str).unwrap_or("sim-tx")),
        Some("dump") => dump(),
        Some("seh") => seh(),
        Some("panic") => {
            install_crash_dir();
            panic!("crash-sim: 模拟 panic（R24 hook 通道验证）");
        }
        other => {
            eprintln!("用法: crash-sim <clean|dump|seh|panic> [txId]（实际: {other:?}）");
            std::process::exit(2);
        }
    }
}

/// txId 白名单：仅作 journal 文件名组件，杜绝路径穿越（生产 tx 形如 `clean-<hex>`）。
fn valid_tx_id(tx: &str) -> bool {
    !tx.is_empty() && tx.len() <= 64 && tx.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// 模拟清理事务中途被 kill（父进程在 marker 出现后 kill 本进程）。
fn clean(tx: &str) {
    if !valid_tx_id(tx) {
        eprintln!("拒绝：非法 txId（仅 [A-Za-z0-9-]，≤64 字符）");
        std::process::exit(2);
    }
    let base = pureslate_lib::storage::data_root().join("sim");
    let src = base.join("src");
    let qroot = base.join("q");
    let marker = base.join("marker");

    let mut files: Vec<PathBuf> = std::fs::read_dir(&src)
        .expect("sim/src 可读（由父测试进程准备）")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    files.sort();

    let mut j = Journal::open(tx).expect("journal 可开");
    for (i, f) in files.iter().enumerate() {
        j.intent(f, Disposition::Quarantine).expect("intent");
        let moved = quarantine::move_into_quarantine(
            &qroot,
            QuarantineInput {
                original_path: f.clone(),
                grade: Grade::Yellow,
                category_id: "sim.kill".into(),
                retention_days: 14,
            },
        )
        .expect("移入隔离区");
        if i == 2 {
            std::fs::write(&marker, b"kill-now").expect("marker");
        }
        // 真实 kill 窗口：intent+manifest 已落、result 未写。停顿给父进程 kill 时间。
        std::thread::sleep(Duration::from_millis(300));
        j.result(f, true, None).expect("result");
        let _ = moved;
    }
}

fn install_crash_dir() {
    crash::install_handlers(pureslate_lib::storage::data_root().join("crash"));
}

/// panic-hook 通道：显式写一份 dump（无异常上下文）后正常退出。
fn dump() {
    install_crash_dir();
    match crash::dump::write_dump(std::ptr::null_mut()) {
        Some(p) => println!("{}", p.display()),
        None => {
            eprintln!("dump 写入失败");
            std::process::exit(1);
        }
    }
}

/// SEH 通道：硬件访问违规 → 顶级过滤器写 dump → 进程被内核终止（不再返回）。
fn seh() {
    install_crash_dir();
    // unsafe：故意的空指针 volatile 写入，仅在测试二进制中执行（触发 SEH 过滤器）。
    unsafe {
        let p: *mut u8 = std::ptr::null_mut();
        p.write_volatile(0xAB);
    }
    eprintln!("不应到达（SEH 未生效）");
}
