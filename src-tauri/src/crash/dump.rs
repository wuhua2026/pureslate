//! minidump 落盘（R24 · SPEC §6.6）。零依赖：dbghelp `MiniDumpWriteDump` 直调。
//!
//! 双通道：
//! 1. **SEH 未处理异常**（访问冲突等）→ `SetUnhandledExceptionFilter` 顶级过滤器；
//! 2. **Rust panic**（unwind 前 hook）→ `panic::set_hook`，无异常上下文（仅模块/线程面）。
//!
//! SEH 通道实现要点（本机实测结论，LESSONS §①）：在**故障线程**的过滤器里直接调
//! `MiniDumpWriteDump`（带异常参数）稳定失败（GetLastError=998 ERROR_NOACCESS）——
//! 例外分发期该线程无法被正常挂起取上下文。标准解法：过滤器把异常上下文指针
//! 交给**干净辅助线程**写 dump（可正常挂起故障线程读取 CONTEXT），完成后过滤器
//! 才返回（指针目标=过滤器线程栈，join 期间存活）；辅助线程失败则退化为
//! 无异常流 dump（有总比没有强）。
//!
//! 已知边界（决策记录，LESSONS §②）：进程内处理器无法覆盖 ① 堆损坏（分配器已坏时
//! 写 dump 自身可能失败）② `std::process::abort`（fastfail 设计上不可捕获）。
//! v1 接受该盲区（覆盖绝大多数可复现崩溃）。过滤器/辅助线程内不 unwrap、不做
//! 复杂操作（崩溃路径纪律）。

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

static DUMP_DIR: OnceLock<PathBuf> = OnceLock::new();
/// dump 进行中标记：防处理器内再崩溃导致的递归 dump。
static IN_PROGRESS: AtomicBool = AtomicBool::new(false);
/// SEH 过滤器深拷贝的异常上下文快照（EXCEPTION_RECORD + CONTEXT 原始字节）。
/// 堆上 Box，过滤器 join 期间稳定存活；辅助线程用自有副本调 MiniDumpWriteDump
/// ——绕开「OS 交来的指针在故障线程栈上」的地址空间语义坑（本机实测 998）。
static EX_COPY: Mutex<Option<Box<ExCopy>>> = Mutex::new(None);

/// x64（Win64）尺寸：EXCEPTION_RECORD=152、CONTEXT=1232（WinNT.h）。
/// 仅作字节快照容器，不解引用字段（ExceptionCode=首 u32 除外，布局 MSVC/x64 稳定）；
/// 非 x64 目标不走异常流路径。
#[cfg(target_arch = "x86_64")]
const EXCEPTION_RECORD_SIZE: usize = 152;
#[cfg(target_arch = "x86_64")]
const CONTEXT_SIZE: usize = 1232;

#[cfg(target_arch = "x86_64")]
struct ExCopy {
    record: [u8; EXCEPTION_RECORD_SIZE],
    context: [u8; CONTEXT_SIZE],
    /// EXCEPTION_RECORD.ExceptionCode（record 首 u32）。MiniDumpWriteDump 异常流
    /// 在本机环境不可用（恒 998，见 LESSONS），异常代码经 sidecar JSON 携带。
    exception_code: u32,
}

/// 安装双通道处理器。`dir` 为 dump 落盘目录（生产 = data_root\crash；测试/模拟器可注入）。
pub fn install_handlers(dir: PathBuf) {
    let _ = DUMP_DIR.set(dir);
    #[cfg(windows)]
    // unsafe：安装 SEH 顶级过滤器。kernel32 默认链接（同 DeleteFileW 先例）。
    unsafe {
        SetUnhandledExceptionFilter(Some(top_level_filter));
    }
    let prior = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        write_dump(std::ptr::null_mut());
        prior(info);
    }));
}

/// SEH 顶级过滤器：经辅助线程写 dump 后静默收场（不弹 WER，进程由内核终止）。
#[cfg(windows)]
unsafe extern "system" fn top_level_filter(info: *mut EXCEPTION_POINTERS) -> i32 {
    write_dump_from_filter(info);
    // EXCEPTION_EXECUTE_HANDLER(1)：dump 已自行落盘。
    EXCEPTION_EXECUTE_HANDLER
}
#[cfg(windows)]
const EXCEPTION_EXECUTE_HANDLER: i32 = 1;

/// panic 通道写 dump（无异常流）。可重入保护；失败返回 None（不向上传播错误）。
pub fn write_dump(exception_pointers: *mut c_void) -> Option<PathBuf> {
    if IN_PROGRESS.swap(true, Ordering::SeqCst) {
        return None;
    }
    let out = write_dump_core(exception_pointers);
    IN_PROGRESS.store(false, Ordering::SeqCst);
    out
}

/// SEH 通道：过滤器内快照异常上下文 → 辅助线程写 dump（带异常流；失败退化为无异常流）。
#[cfg(windows)]
fn write_dump_from_filter(info: *mut EXCEPTION_POINTERS) {
    if IN_PROGRESS.swap(true, Ordering::SeqCst) {
        return;
    }
    let tid = unsafe { GetCurrentThreadId() };
    snapshot_exception_context(info);
    // unsafe 边界：EX_COPY 中的副本由本线程在上一步从 OS 交来的指针处
    // `copy_nonoverlapping` 读入（同线程普通内存读）；Box 存于静态，
    // 本线程在 join 处阻塞，调用期间指针稳定。
    let helper = std::thread::Builder::new()
        .name("pureslate-crash-dump".into())
        .spawn(move || {
            let code = sidecar_code();
            let mut done = None;
            if let Some((tid2, rec, ctx)) = ex_copy_ptrs(tid) {
                done = write_dump_ex(tid2, rec, ctx, code);
            }
            done.or_else(|| {
                write_dump_core(std::ptr::null_mut()).inspect(|p| write_sidecar(p, code))
            })
        });
    // 必须等待：返回 1 后进程即被终止，快照随之失效。
    if let Ok(h) = helper {
        let _ = h.join();
    }
    if let Ok(mut g) = EX_COPY.lock() {
        *g = None;
    }
    IN_PROGRESS.store(false, Ordering::SeqCst);
}

/// 从 OS 交来的 EXCEPTION_POINTERS 处深拷贝 EXCEPTION_RECORD + CONTEXT 到静态堆快照。
#[cfg(all(windows, target_arch = "x86_64"))]
fn snapshot_exception_context(info: *mut EXCEPTION_POINTERS) {
    if info.is_null() {
        return;
    }
    // unsafe：仅按已知尺寸逐字节快照 OS 交来的合法结构，不解引用字段、不写回
    // （ExceptionCode 取 record 首 u32，MSVC/x64 布局首字段稳定）。
    if let Ok(mut g) = EX_COPY.lock() {
        let mut buf = Box::new(ExCopy {
            record: [0; EXCEPTION_RECORD_SIZE],
            context: [0; CONTEXT_SIZE],
            exception_code: 0,
        });
        unsafe {
            std::ptr::copy_nonoverlapping(
                (*info).exception_record.cast::<u8>(),
                buf.record.as_mut_ptr(),
                EXCEPTION_RECORD_SIZE,
            );
            std::ptr::copy_nonoverlapping(
                (*info).context_record.cast::<u8>(),
                buf.context.as_mut_ptr(),
                CONTEXT_SIZE,
            );
        }
        buf.exception_code =
            u32::from_le_bytes([buf.record[0], buf.record[1], buf.record[2], buf.record[3]]);
        *g = Some(buf);
    }
}

#[cfg(all(windows, not(target_arch = "x86_64")))]
fn snapshot_exception_context(_info: *mut EXCEPTION_POINTERS) {}

/// 快照中的异常代码（无快照/非 x64 → None）。
#[cfg(windows)]
fn sidecar_code() -> Option<u32> {
    let g = EX_COPY.lock().ok()?;
    let b = g.as_ref()?;
    Some(b.exception_code).filter(|c| *c != 0)
}

/// 取快照指针（(故障线程 id, record, context)）；无快照返回 None。
#[cfg(windows)]
fn ex_copy_ptrs(fallback_tid: u32) -> Option<(u32, *mut c_void, *mut c_void)> {
    let g = EX_COPY.lock().ok()?;
    let b = g.as_ref()?;
    Some((
        fallback_tid,
        b.record.as_ptr() as *mut c_void,
        b.context.as_ptr() as *mut c_void,
    ))
}

/// 用异常上下文副本写 dump（在辅助线程调用；失败返回 None 由调用方兜底）。
#[cfg(windows)]
fn write_dump_ex(
    thread_id: u32,
    record: *mut c_void,
    context: *mut c_void,
    code: Option<u32>,
) -> Option<PathBuf> {
    use std::os::windows::io::AsRawHandle as _;

    let dir = DUMP_DIR.get()?;
    std::fs::create_dir_all(dir).ok()?;
    let name = format!(
        "pureslate-crash-{}-{}.dmp",
        timestamp_utc_compact(),
        std::process::id()
    );
    let path = dir.join(&name);
    let file = std::fs::File::create(&path).ok()?;

    // EXCEPTION_POINTERS 指向静态快照（进程自身内存），client_pointers=0。
    let mut local = EXCEPTION_POINTERS {
        exception_record: record,
        context_record: context,
    };
    let exc = MinidumpExceptionInformation {
        thread_id,
        exception_pointers: &mut local as *mut EXCEPTION_POINTERS as *mut c_void,
        client_pointers: 0,
    };
    // unsafe：崩溃路径上的 FFI；句柄/指针均来自上方有效值。
    let ok = unsafe {
        MiniDumpWriteDump(
            GetCurrentProcess(),
            GetCurrentProcessId(),
            file.as_raw_handle(),
            MINIDUMP_NORMAL,
            &exc,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if ok == 0 {
        let _ = std::fs::remove_file(&path); // 半成品不留
        eprintln!("[crash] 带异常流写 dump 失败（GetLastError={}）", unsafe {
            GetLastError()
        });
        return None;
    }
    write_sidecar(&path, code);
    eprintln!("[crash] minidump 已落盘: {}", path.to_string_lossy());
    Some(path)
}

/// dump 落盘成功后写 sidecar JSON（`<dump>.json`，含异常代码）。
/// MiniDumpWriteDump 异常流在本机环境不可用（恒 998，LESSONS §①），
/// 异常代码经此携带；无异常上下文（panic 通道）不写。
#[cfg(windows)]
fn write_sidecar(dump_path: &Path, code: Option<u32>) {
    let Some(code) = code else {
        return;
    };
    let mut s = dump_path.as_os_str().to_os_string();
    s.push(".json");
    let sidecar = PathBuf::from(s);
    let body = serde_json::json!({
        "exceptionCode": code,
        "ts": crate::logging::audit::now_ms(),
    });
    if let Err(e) = std::fs::write(&sidecar, body.to_string()) {
        eprintln!("[crash] sidecar 写入失败: {e}");
    }
}

#[cfg(not(windows))]
fn write_sidecar(_dump_path: &std::path::Path, _code: Option<u32>) {}

/// 写一份 minidump（当前线程视角；`exi` 非空时带异常流——panic 通道恒为空）。
#[cfg(windows)]
fn write_dump_core(exi: *mut c_void) -> Option<PathBuf> {
    use std::os::windows::io::AsRawHandle as _;

    let dir = DUMP_DIR.get()?;
    std::fs::create_dir_all(dir).ok()?;
    let name = format!(
        "pureslate-crash-{}-{}.dmp",
        timestamp_utc_compact(),
        std::process::id()
    );
    let path = dir.join(&name);
    let file = std::fs::File::create(&path).ok()?;

    let mut exc = MinidumpExceptionInformation {
        thread_id: unsafe { GetCurrentThreadId() },
        exception_pointers: exi,
        client_pointers: 0,
    };
    // unsafe：崩溃路径上的 FFI；句柄/指针均来自上方有效值。
    let ok = unsafe {
        MiniDumpWriteDump(
            GetCurrentProcess(),
            GetCurrentProcessId(),
            file.as_raw_handle(),
            MINIDUMP_NORMAL,
            if exi.is_null() {
                std::ptr::null()
            } else {
                &mut exc as *mut MinidumpExceptionInformation as *const MinidumpExceptionInformation
            },
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if ok == 0 {
        let _ = std::fs::remove_file(&path); // 半成品不留
        eprintln!(
            "[crash] MiniDumpWriteDump 失败（GetLastError={}）",
            unsafe { GetLastError() }
        );
        return None;
    }
    eprintln!("[crash] minidump 已落盘: {}", path.to_string_lossy());
    Some(path)
}

#[cfg(not(windows))]
fn write_dump_core(_exi: *mut c_void) -> Option<PathBuf> {
    None // 非 Windows（测试/CI 兜底）无 dbghelp；不落盘
}

/// `yyyyMMdd-HHmmss`（UTC；仅文件命名，无需时区精度）。
fn timestamp_utc_compact() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}{m:02}{d:02}-{h:02}{mi:02}{s:02}",
        h = rem / 3600,
        mi = (rem % 3600) / 60,
        s = rem % 60
    )
}

/// 天数 → (年, 月, 日)。proleptic Gregorian（Howard Hinnant civil，同 storage.rs）。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
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

// ---- Windows FFI（dbghelp 显式链接；kernel32 默认链接） ----

#[cfg(windows)]
#[repr(C)]
struct MinidumpExceptionInformation {
    thread_id: u32,
    exception_pointers: *mut c_void,
    client_pointers: i32,
}

#[cfg(windows)]
const MINIDUMP_NORMAL: u32 = 0;

#[cfg(windows)]
#[link(name = "dbghelp")]
unsafe extern "system" {
    fn MiniDumpWriteDump(
        hprocess: *mut c_void,
        processid: u32,
        hfile: *mut c_void,
        dumptype: u32,
        exceptionparam: *const MinidumpExceptionInformation,
        userstreamparam: *const c_void,
        callbackparam: *const c_void,
    ) -> i32;
}

/// EXCEPTION_POINTERS（仅作指针类型占位，过滤器内不解引用）。
#[cfg(windows)]
#[repr(C)]
struct EXCEPTION_POINTERS {
    exception_record: *mut c_void,
    context_record: *mut c_void,
}

#[cfg(windows)]
unsafe extern "system" {
    fn SetUnhandledExceptionFilter(
        filter: Option<unsafe extern "system" fn(*mut EXCEPTION_POINTERS) -> i32>,
    ) -> usize;
    fn GetCurrentProcess() -> *mut c_void;
    fn GetCurrentProcessId() -> u32;
    fn GetCurrentThreadId() -> u32;
    fn GetLastError() -> u32;
}
