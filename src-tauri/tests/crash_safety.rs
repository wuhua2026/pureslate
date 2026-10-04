//! R24 崩溃安全集成测试（P4-03 · SAFETY §6.3 边界 7「操作中途 kill 进程」）。
//!
//! 借助 `src/bin/crash-sim.rs`（CARGO_BIN_EXE 注入路径）在**独立进程**里真实执行
//! journal/隔离区移动并中途 kill，父进程扮演"下一次启动"执行恢复：
//! - kill 中断 → manifest 背书的孤儿 → T-6 协议还原回原路径；
//! - dump/panic 通道：minidump 落盘且可解析；
//! - SEH 通道：真实硬件访问违规 → 过滤器落盘（异常代码 0xC0000005）。
//!
//! 沙箱：父进程 `set_data_root_override` + 子进程 `PURESLATE_DATA_ROOT` 环境变量
//! 指向同一数据根；隔离区根显式传沙箱路径（不触碰真实卷根）。

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use pureslate_lib::quarantine::{load_manifest, ManifestState};

const SIM: &str = env!("CARGO_BIN_EXE_crash-sim");

/// 本文件内 DATA_ROOT_OVERRIDE 串行锁（lib 的 TEST_DATA_ROOT_LOCK 是 #[cfg(test)]，
/// 集成测试进程不可见；口径同 tests/compat.rs）。
static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 沙箱根（持本文件锁贯穿测试体——共享全局 DATA_ROOT_OVERRIDE，LESSONS ① 教训；
/// Drop 时清覆盖并删目录）。
struct Sandbox {
    _lock: std::sync::MutexGuard<'static, ()>,
    root: PathBuf,
    data_root: PathBuf,
}

impl Sandbox {
    fn new(tag: &str) -> Self {
        let lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let root = std::env::temp_dir().join(format!(
            "pureslate-crashit-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let data_root = root.join("data-root");
        std::fs::create_dir_all(&data_root).unwrap();
        pureslate_lib::storage::set_data_root_override(Some(data_root.clone()));
        Self {
            _lock: lock,
            root,
            data_root,
        }
    }

    fn sim(&self) -> Command {
        let mut c = Command::new(SIM);
        c.env("PURESLATE_DATA_ROOT", &self.data_root)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        c
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        pureslate_lib::storage::set_data_root_override(None);
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn dumps_in(crash_dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(crash_dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "dmp").unwrap_or(false))
        .collect();
    v.sort();
    v
}

/// 核心场景（SAFETY §6.3 边界 7）：清理事务中途 kill → 下一次启动恢复。
/// crash-sim 工作目录全部派生自数据根（CLI 不收路径）：`<data_root>\sim\{src,q,marker}`。
#[test]
fn kill_mid_transaction_recovery_restores_manifest_backed_orphan() {
    let sb = Sandbox::new("kill");
    let sim_base = sb.data_root.join("sim");
    let src = sim_base.join("src");
    std::fs::create_dir_all(&src).unwrap();
    for i in 0..8 {
        std::fs::write(src.join(format!("sample-{i:02}.tmp")), format!("body-{i}")).unwrap();
    }
    let qroot = sim_base.join("q");
    let marker = sim_base.join("marker");

    let mut child = sb.sim().args(["clean", "tx-kill"]).spawn().unwrap();

    // 轮询 marker：第 3 个文件已移入隔离区（intent+manifest 已写、result 未写）。
    let deadline = Instant::now() + Duration::from_secs(30);
    while !marker.exists() {
        assert!(Instant::now() < deadline, "sim 未在时限内到达 kill 窗口");
        assert!(
            child.try_wait().unwrap().is_none(),
            "sim 在 kill 前提前退出"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    child.kill().unwrap();
    let _ = child.wait();

    // 前置：marker 语义 = 第 3 个文件（sample-02）原路径已空。
    assert!(
        !src.join("sample-02.tmp").exists(),
        "marker 时刻该文件应已移入隔离区"
    );

    // 下一次启动：恢复引擎对账。
    let report =
        pureslate_lib::crash::recover::recover_with(std::slice::from_ref(&qroot), 14, now_ms());
    assert_eq!(
        report.orphans_found, 1,
        "恰有一个 in-flight 孤儿: {report:?}"
    );
    assert_eq!(report.restored, 1, "应经 manifest 信任锚还原: {report:?}");
    assert!(report.failures.is_empty(), "{report:?}");

    // in-flight 文件已还原回原路径。
    let third = src.join("sample-02.tmp");
    assert!(third.exists(), "in-flight 文件应被还原回原路径");

    // manifest 状态迁移（restored）。
    let (entries, _) = load_manifest(&qroot).unwrap();
    let e = entries
        .iter()
        .find(|e| e.original_path == third.to_string_lossy())
        .expect("manifest 应有 sample-02 记录");
    assert_eq!(e.state, ManifestState::Restored);

    // 已完成的文件留在隔离区（清理语义未回滚），未开始的文件原样。
    assert!(!src.join("sample-00.tmp").exists());
    assert!(!src.join("sample-01.tmp").exists());
    let quarantined = entries
        .iter()
        .filter(|e| e.state == ManifestState::Quarantined)
        .count();
    assert_eq!(quarantined, 2, "前两个已完成的文件应留在隔离区");
    for i in 3..8 {
        assert!(
            src.join(format!("sample-{i:02}.tmp")).exists(),
            "未开始文件应原样"
        );
    }

    // journal 全部闭环。
    assert!(pureslate_lib::cleaner::journal::detect_orphans().is_empty());

    // 恢复动作已审计（op=recover）。
    let logs =
        pureslate_lib::logging::audit::query(now_ms() - 60_000, now_ms() + 1_000, Some("recover"));
    assert!(!logs.is_empty(), "恢复应有审计记录");
}

/// dump 通道（panic-hook 显式写盘）：产物可被解析，模块列表非空。
#[test]
fn dump_writes_parseable_minidump() {
    let sb = Sandbox::new("dump");
    let out = sb.sim().arg("dump").output().unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let dump_dir = sb.data_root.join("crash");
    let dumps = dumps_in(&dump_dir);
    assert_eq!(dumps.len(), 1, "应恰好落盘一份 dump");
    let bytes = std::fs::read(&dumps[0]).unwrap();
    let s = pureslate_lib::crash::parse::parse_summary(&bytes).unwrap();
    assert!(s.module_count >= 1, "模块表应非空");
    assert!(
        s.modules
            .iter()
            .any(|m| m.contains("crash-sim") || m.contains("pureslate")),
        "模块列表应含自身进程: {:?}",
        s.modules
    );
    assert_eq!(s.exception_code, None, "显式写盘无异常流");
}

/// SEH 通道：真实硬件访问违规 → 顶级过滤器快照上下文 → 辅助线程落盘 → 内核终止进程。
/// 异常代码经 sidecar JSON 携带（MiniDumpWriteDump 异常流本机不可用，LESSONS §①）。
#[test]
fn seh_crash_produces_dump_with_access_violation_code() {
    let sb = Sandbox::new("seh");
    let out = sb.sim().arg("seh").output().unwrap();
    assert!(!out.status.success(), "访问违规应导致进程终止");
    let dumps = dumps_in(&sb.data_root.join("crash"));
    assert_eq!(dumps.len(), 1, "SEH 过滤器应落盘一份 dump");
    let s = pureslate_lib::crash::parse::parse_summary(&std::fs::read(&dumps[0]).unwrap()).unwrap();
    assert!(s.module_count >= 1, "异常通道 dump 仍应含模块表");
    // sidecar 异常代码（dump 流内异常段不可用，经过滤器快照写入 sidecar）。
    let code = pureslate_lib::crash::read_sidecar_code(&dumps[0]);
    assert_eq!(code, Some(0xC000_0005), "sidecar 应含访问违例异常代码");
}

/// panic 通道：真实 panic → hook 落盘（无异常流、无 sidecar），进程以 101 退出。
#[test]
fn panic_channel_produces_dump_without_exception_stream() {
    let sb = Sandbox::new("panic");
    let out = sb.sim().arg("panic").output().unwrap();
    assert!(!out.status.success(), "panic 应导致非零退出");
    let dumps = dumps_in(&sb.data_root.join("crash"));
    assert_eq!(dumps.len(), 1, "panic hook 应落盘一份 dump");
    let s = pureslate_lib::crash::parse::parse_summary(&std::fs::read(&dumps[0]).unwrap()).unwrap();
    assert!(s.module_count >= 1);
    assert_eq!(s.exception_code, None);
    assert_eq!(
        pureslate_lib::crash::read_sidecar_code(&dumps[0]),
        None,
        "panic 通道不应有 sidecar"
    );
}
