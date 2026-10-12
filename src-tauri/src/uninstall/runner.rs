//! 卸载执行器（R27-2 · SPEC §6.7）。
//!
//! 职责边界：只**编排官方卸载器**——拉起、等待其进程树退出、事后判定。
//! 永不强杀（取消 = 停止等待，不 TerminateProcess）、永不自行删除文件
//! （残留由 R27-3 走隔离区管线）。全部 Win32 调用 `#[link]` 直调（零第三方依赖）。
//!
//! 分层：纯函数（命令行拆分/进程树闭包/结果分类/待重启匹配）可单测；
//! FFI 薄层（CreateProcessW/Toolhelp/ShellExecuteExW/注册表）不单测。

use std::ffi::c_void;
use std::path::Path;

use crate::startup::winreg::{enum_values_view, Hive, KEY_WOW64_64KEY};

const WAIT_OBJECT_0: u32 = 0;
const ERROR_ELEVATION_REQUIRED: i32 = 740;
const SEE_MASK_NOCLOSEPROCESS: u32 = 0x0000_0040;
const SW_SHOWNORMAL: i32 = 1;
const STARTF_USESHOWWINDOW: u32 = 0x0000_0001;
const TH32CS_SNAPPROCESS: u32 = 0x0000_0002;
const INVALID_HANDLE: isize = -1;
const REG_MULTI_SZ: u32 = 7;
/// PendingFileRenameOperations 所在键。
const SESSION_MANAGER_KEY: &str = r"SYSTEM\CurrentControlSet\Control\Session Manager";

/// 进程树等待的递归深度上限（SPEC §6.7：防极端进程树栈风险）。
pub const TREE_MAX_DEPTH: usize = 16;
/// 等待总轮次上限（250ms × 240 = 60s/轮组节奏，整体足够大多数卸载器收尾）。
const WAIT_ROUNDS: u32 = 480;

#[link(name = "kernel32")]
extern "system" {
    fn CreateProcessW(
        app_name: *const u16,
        cmd_line: *mut u16,
        proc_attrs: *const c_void,
        thread_attrs: *const c_void,
        inherit: i32,
        flags: u32,
        env: *const c_void,
        cur_dir: *const u16,
        si: *const StartupInfoW,
        pi: *mut ProcessInformation,
    ) -> i32;
    fn WaitForSingleObject(handle: *mut c_void, ms: u32) -> u32;
    fn GetExitCodeProcess(handle: *mut c_void, code: *mut u32) -> i32;
    fn CloseHandle(handle: *mut c_void) -> i32;
    fn GetLongPathNameW(short_path: *const u16, long_path: *mut u16, buf_chars: u32) -> u32;
    fn ExpandEnvironmentStringsW(src: *const u16, dst: *mut u16, buf_chars: u32) -> u32;
    fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> *mut c_void;
    fn Process32FirstW(snap: *mut c_void, entry: *mut ProcessEntry32W) -> i32;
    fn Process32NextW(snap: *mut c_void, entry: *mut ProcessEntry32W) -> i32;
}

#[link(name = "shell32")]
extern "system" {
    fn ShellExecuteExW(info: *mut ShellExecuteInfoW) -> i32;
}

/// STARTUPINFOW（x64 cb=104；显式列全部字段，对齐交给 repr(C)）。
#[repr(C)]
struct StartupInfoW {
    cb: u32,
    _cb_pad: u32, // cb 后对齐填充
    lp_reserved: *const u16,
    lp_desktop: *const u16,
    lp_title: *const u16,
    dw_x: u32,
    dw_y: u32,
    dw_x_size: u32,
    dw_y_size: u32,
    dw_x_count_chars: u32,
    dw_y_count_chars: u32,
    dw_fill_attribute: u32,
    dw_flags: u32,
    w_show_window: u16,
    cb_reserved2: u16,
    // 此处 repr(C) 自动补 4 字节，使后续 8 字节指针 8 对齐。
    lp_reserved2: *mut c_void,
    h_std_input: *mut c_void,
    h_std_output: *mut c_void,
    h_std_error: *mut c_void,
}

impl StartupInfoW {
    fn shown_normal() -> Self {
        Self {
            cb: 104,
            _cb_pad: 0,
            lp_reserved: std::ptr::null(),
            lp_desktop: std::ptr::null(),
            lp_title: std::ptr::null(),
            dw_x: 0,
            dw_y: 0,
            dw_x_size: 0,
            dw_y_size: 0,
            dw_x_count_chars: 0,
            dw_y_count_chars: 0,
            dw_fill_attribute: 0,
            dw_flags: STARTF_USESHOWWINDOW,
            w_show_window: SW_SHOWNORMAL as u16,
            cb_reserved2: 0,
            lp_reserved2: std::ptr::null_mut(),
            h_std_input: std::ptr::null_mut(),
            h_std_output: std::ptr::null_mut(),
            h_std_error: std::ptr::null_mut(),
        }
    }
}

const _: () = assert!(std::mem::size_of::<StartupInfoW>() == 104);

#[repr(C)]
struct ProcessInformation {
    process: *mut c_void,
    thread: *mut c_void,
    pid: u32,
    tid: u32,
}

impl Drop for ProcessInformation {
    fn drop(&mut self) {
        if !self.process.is_null() {
            // 安全性：句柄来自 CreateProcessW/ShellExecuteExW 成功路径。
            unsafe { CloseHandle(self.process) };
        }
        if !self.thread.is_null() {
            unsafe { CloseHandle(self.thread) };
        }
    }
}

/// PROCESSENTRY32W（x64 布局；dwSize=568）。
#[repr(C)]
struct ProcessEntry32W {
    dw_size: u32,
    cnt_usage: u32,
    pid: u32,
    default_heap_id: usize,
    module_id: u32,
    threads: u32,
    parent_pid: u32,
    pri_base: i32,
    flags: u32,
    exe_file: [u16; 260],
}

/// SHELLEXECUTEINFOW（x64 布局，112 字节）。
#[repr(C)]
struct ShellExecuteInfoW {
    cb_size: u32,
    mask: u32,
    hwnd: *mut c_void,
    verb: *const u16,
    file: *const u16,
    parameters: *const u16,
    directory: *const u16,
    show: i32,
    inst_app: *mut c_void,
    id_list: *mut c_void,
    class: *const u16,
    hkey_class: *mut c_void,
    hot_key: u32,
    icon: *mut c_void,
    process: *mut c_void,
}

/// 拉起的进程句柄与 PID（Drop 自动关句柄）。
pub struct Launched {
    pi: ProcessInformation,
}

impl Launched {
    pub fn pid(&self) -> u32 {
        self.pi.pid
    }
    fn handle(&self) -> *mut c_void {
        self.pi.process
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}
fn wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// ExpandEnvironmentStringsW 包装；失败原样返回。
pub fn expand_env(input: &str) -> String {
    let src = wide_null(input);
    let mut buf = vec![0u16; 2048];
    // 安全性：缓冲区长度与指针配对；API 返回所需字符数。
    let n = unsafe { ExpandEnvironmentStringsW(src.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) };
    if n == 0 {
        input.to_string()
    } else {
        String::from_utf16_lossy(&buf[..(n as usize).saturating_sub(1)])
    }
}

/// GetLongPathNameW 包装（8.3 短路径 → 长路径）；失败原样返回。
pub fn to_long_path(input: &str) -> String {
    let src = wide_null(input);
    let mut buf = vec![0u16; 2048];
    // 安全性：同上，输出 UTF-16 长度不超缓冲区。
    let n = unsafe { GetLongPathNameW(src.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) };
    if n == 0 {
        input.to_string()
    } else {
        String::from_utf16_lossy(&buf[..n as usize])
    }
}

/// 拆分卸载命令行为（可执行文件, 参数）。
///
/// 规则（净室自研，与 SHLWAPI PathRemoveArgs 行为对齐的最小子集）：
/// 1) 先展开环境变量（`%SystemRoot%` 等）；
/// 2) 首字符为引号 → 引号内为 exe，引号后为参数；
/// 3) 否则在串中找最早的 `.exe`（大小写不敏感），其尾为 exe 边界；
/// 4) 都不是（msiexec/rundll32 等无扩展名命令）→ 首个空白前为 exe。
///
/// 空白命令返回 None（调用方拒绝拉起）。
pub fn split_exe_args(raw: &str) -> Option<(String, String)> {
    let cmd = expand_env(raw.trim());
    let t = cmd.trim();
    if t.is_empty() {
        return None;
    }
    let (exe, args) = if let Some(rest) = t.strip_prefix('"') {
        let end = rest.find('"')?;
        (rest[..end].to_string(), rest[end + 1..].trim().to_string())
    } else if let Some(idx) = t.to_lowercase().find(".exe") {
        let end = idx + 4;
        (t[..end].trim().to_string(), t[end..].trim().to_string())
    } else if let Some(sp) = t.find(char::is_whitespace) {
        (t[..sp].to_string(), t[sp..].trim().to_string())
    } else {
        (t.to_string(), String::new())
    };
    (!exe.is_empty()).then_some((exe, args))
}

/// 拉起官方卸载器。先 CreateProcessW；若卸载器自身要求提权而本进程未提权
/// （ERROR_ELEVATION_REQUIRED 740），回退 ShellExecuteExW "runas"（弹标准 UAC）。
pub fn launch(exe: &str, args: &str) -> Result<Launched, String> {
    // 8.3 短路径还原（SPEC §6.7）；对 msiexec 这类 PATH 命令长路径还原会失败，原样保留。
    let exe_long = if exe.contains('~') || Path::new(exe).is_absolute() {
        to_long_path(exe)
    } else {
        exe.to_string()
    };
    // 始终给 exe 加引号（路径含空格安全）；CreateProcessW 要求命令行可写。
    let cmdline = if args.is_empty() {
        format!("\"{exe_long}\"")
    } else {
        format!("\"{exe_long}\" {args}")
    };

    let si = StartupInfoW::shown_normal();
    let mut pi = ProcessInformation {
        process: std::ptr::null_mut(),
        thread: std::ptr::null_mut(),
        pid: 0,
        tid: 0,
    };
    let mut wide_cmd = wide(&cmdline);
    wide_cmd.push(0);
    // 工作目录：绝对路径 exe 取其父目录（贴近资源管理器启动语义，避免卸载器按
    // 相对路径找自身数据文件失败）；msiexec 这类 PATH 命令给 NULL（继承）。
    let work_dir: Option<Vec<u16>> = Path::new(&exe_long)
        .parent()
        .filter(|p| p.is_absolute())
        .map(|p| wide_null(&p.to_string_lossy()));
    // 安全性：CreateProcessW 参数按 SDK 契约；NOLIMIT 优先级 0、句柄不继承（FALSE）；
    // 不经 cmd.exe/shell——命令行直交内核解析，无 shell 元字符注入面。
    let ok = unsafe {
        CreateProcessW(
            std::ptr::null(),
            wide_cmd.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            0,
            std::ptr::null(),
            work_dir
                .as_ref()
                .map(|v| v.as_ptr())
                .unwrap_or(std::ptr::null()),
            &si,
            &mut pi,
        )
    };
    if ok != 0 {
        return Ok(Launched { pi });
    }
    let err = std::io::Error::last_os_error();
    if err.raw_os_error() == Some(ERROR_ELEVATION_REQUIRED) {
        return launch_runas(&exe_long, args);
    }
    Err(format!("启动卸载器失败（{cmdline}）: {err}"))
}

/// ShellExecuteExW "runas" 兜底（卸载器 manifest 要求管理员时走标准 UAC 提权）。
fn launch_runas(exe: &str, args: &str) -> Result<Launched, String> {
    let verb = wide_null("runas");
    let file = wide_null(exe);
    let work_dir: Option<Vec<u16>> = Path::new(exe)
        .parent()
        .filter(|p| p.is_absolute())
        .map(|p| wide_null(&p.to_string_lossy()));
    let params = if args.is_empty() {
        Vec::new()
    } else {
        wide_null(args)
    };
    let mut info = ShellExecuteInfoW {
        cb_size: std::mem::size_of::<ShellExecuteInfoW>() as u32,
        mask: SEE_MASK_NOCLOSEPROCESS,
        hwnd: std::ptr::null_mut(),
        verb: verb.as_ptr(),
        file: file.as_ptr(),
        parameters: if params.is_empty() {
            std::ptr::null()
        } else {
            params.as_ptr()
        },
        directory: work_dir
            .as_ref()
            .map(|v| v.as_ptr())
            .unwrap_or(std::ptr::null()),
        show: SW_SHOWNORMAL,
        inst_app: std::ptr::null_mut(),
        id_list: std::ptr::null_mut(),
        class: std::ptr::null(),
        hkey_class: std::ptr::null_mut(),
        hot_key: 0,
        icon: std::ptr::null_mut(),
        process: std::ptr::null_mut(),
    };
    // 安全性：SHELLEXECUTEINFOW 生命周期覆盖调用；NOCLOSEPROCESS 使 hProcess 归调用方。
    let ok = unsafe { ShellExecuteExW(&mut info) };
    if ok == 0 || info.process.is_null() {
        return Err("卸载器需要管理员权限（UAC 未通过或被取消）".into());
    }
    // 仅有进程句柄（无 hThread/tid）；PID 经 GetProcessId 不需要——等待用句柄即可，
    // 进程树根 PID 由句柄快照反查（见 wait_tree）。
    let pid = pid_from_handle(info.process).unwrap_or(0);
    Ok(Launched {
        pi: ProcessInformation {
            process: info.process,
            thread: std::ptr::null_mut(),
            pid,
            tid: 0,
        },
    })
}

#[link(name = "kernel32")]
extern "system" {
    fn GetProcessId(handle: *mut c_void) -> u32;
}
fn pid_from_handle(h: *mut c_void) -> Option<u32> {
    // 安全性：句柄来自 ShellExecuteExW 成功路径。
    let pid = unsafe { GetProcessId(h) };
    (pid != 0).then_some(pid)
}

/// 取进程快照：(PID, PPID) 列表。
pub fn snapshot_processes() -> Vec<(u32, u32)> {
    // 安全性：快照句柄 RAII 关闭；Process32* 缓冲定长 568。
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snap as isize == INVALID_HANDLE {
        return Vec::new();
    }
    struct Snap(*mut c_void);
    impl Drop for Snap {
        fn drop(&mut self) {
            // 安全性：句柄来自 CreateToolhelp32Snapshot 成功路径（已排除 INVALID_HANDLE）。
            unsafe { CloseHandle(self.0) };
        }
    }
    let _g = Snap(snap);

    let mut out = Vec::new();
    let mut e = ProcessEntry32W {
        dw_size: std::mem::size_of::<ProcessEntry32W>() as u32,
        cnt_usage: 0,
        pid: 0,
        default_heap_id: 0,
        module_id: 0,
        threads: 0,
        parent_pid: 0,
        pri_base: 0,
        flags: 0,
        exe_file: [0u16; 260],
    };
    if unsafe { Process32FirstW(snap, &mut e) } != 0 {
        loop {
            out.push((e.pid, e.parent_pid));
            if unsafe { Process32NextW(snap, &mut e) } == 0 {
                break;
            }
        }
    }
    out
}

/// 进程树闭包扩展（纯函数）：以 `seed` 为根按 PPID 做 BFS，深度上限 max_depth。
/// `seen` 跨轮次保留（父进程退出后子进程被"收养"、PPID 链断裂时仍不丢失等待目标）。
/// 返回本轮新发现的后代 PID。
pub fn expand_descendants(
    snapshot: &[(u32, u32)],
    seed: u32,
    max_depth: usize,
    seen: &mut std::collections::HashSet<u32>,
) -> Vec<u32> {
    seen.insert(seed);
    let mut frontier = vec![seed];
    let mut added = Vec::new();
    let mut depth = 0;
    while !frontier.is_empty() && depth < max_depth {
        let parents: std::collections::HashSet<u32> = frontier.drain(..).collect();
        let mut next = Vec::new();
        for &(pid, ppid) in snapshot {
            if parents.contains(&ppid) && !seen.contains(&pid) {
                seen.insert(pid);
                added.push(pid);
                next.push(pid);
            }
        }
        frontier = next;
        depth += 1;
    }
    added
}

/// 等待主进程及其进程树退出。**取消只停止等待，不强杀任何进程**（SPEC §6.7）。
pub fn wait_tree<F: FnMut(&str)>(
    launched: &Launched,
    cancel: &crate::scanner::walk::CancelToken,
    mut on_phase: F,
) -> WaitConclusion {
    let handle = launched.handle();
    // ① 主进程退出（250ms 轮询，响应取消）。
    loop {
        if cancel.is_cancelled() {
            return WaitConclusion::Cancelled;
        }
        // 安全性：句柄有效（Launched 持有）。
        if unsafe { WaitForSingleObject(handle, 250) } == WAIT_OBJECT_0 {
            break;
        }
    }

    // ② 进程树收尾：跨轮保留已见 PID，反复扩展直到无存活后代。
    let root = launched.pid();
    // runas 兜底路径若 GetProcessId 反查失败（pid=0），建树会把所有 PPID=0 的
    // 顶层进程误判为后代——此时退化为"仅等主句柄"（已退出即结论），不做树等待。
    if root == 0 {
        return WaitConclusion::Exited;
    }
    let mut seen = std::collections::HashSet::new();
    seen.insert(root);
    for round in 0..WAIT_ROUNDS {
        if cancel.is_cancelled() {
            return WaitConclusion::Cancelled;
        }
        let snap = snapshot_processes();
        let _added = expand_descendants(&snap, root, TREE_MAX_DEPTH, &mut seen);
        let live: std::collections::HashSet<u32> = snap.iter().map(|(p, _)| *p).collect();
        let remaining: Vec<u32> = seen
            .iter()
            .filter(|p| **p != root && live.contains(*p))
            .copied()
            .collect();
        if remaining.is_empty() {
            return WaitConclusion::Exited;
        }
        if round % 8 == 0 {
            on_phase(&format!("等待卸载器的 {} 个子进程退出…", remaining.len()));
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    WaitConclusion::Timeout
}

/// 等待结论。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitConclusion {
    Exited,
    Cancelled,
    /// 轮次上限后仍有后代存活（卸载器可能仍在后台收尾）。
    Timeout,
}

/// 主进程退出码（259=STILL_ACTIVE 视为未知）。
pub fn exit_code(launched: &Launched) -> Option<u32> {
    let mut code = 259u32;
    // 安全性：句柄有效。
    if unsafe { GetExitCodeProcess(launched.handle(), &mut code) } != 0 && code != 259 {
        Some(code)
    } else {
        None
    }
}

/// 读 PendingFileRenameOperations（REG_MULTI_SZ）条目；键缺失/无值 → 空。
pub fn pending_reboot_entries() -> Vec<String> {
    let Ok(values) = enum_values_view(Hive::LocalMachine, SESSION_MANAGER_KEY, KEY_WOW64_64KEY)
    else {
        return Vec::new();
    };
    let Some(v) = values
        .iter()
        .find(|v| v.name.eq_ignore_ascii_case("PendingFileRenameOperations"))
    else {
        return Vec::new();
    };
    if v.kind != REG_MULTI_SZ {
        return Vec::new();
    }
    let units: Vec<u16> = v
        .data
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .collect();
    units
        .split(|u| *u == 0)
        .map(String::from_utf16_lossy)
        .filter(|s| !s.is_empty())
        .collect()
}

/// 待重启列表中是否包含目标路径（大小写不敏感前缀；剥 `\??\` 前缀）。
pub fn pending_covers(entries: &[String], target: &str) -> bool {
    let norm = |s: &str| {
        s.trim_start_matches(r"\??\")
            .trim_start_matches(r"\\?\")
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_lowercase()
    };
    let t = norm(target);
    if t.is_empty() {
        return false;
    }
    entries.iter().any(|e| {
        let n = norm(e);
        n == t || n.starts_with(&format!("{t}\\"))
    })
}

/// 最终结果分类（纯函数；SPEC §6.7 三层判据）。
/// - 取消优先（不做事后判定）；
/// - 键消失 = 卸载成功（安装目录残留不改变结论，留给 R27-3）；
/// - 键在但目标已进待重启队列 = 待重启完成；
/// - 其余 = 仍在/结果不明。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutcomeKind {
    Removed,
    RebootPending,
    StillPresent,
    Cancelled,
    Timeout,
}

pub fn classify(
    wait: WaitConclusion,
    key_still_present: bool,
    reboot_pending: bool,
) -> OutcomeKind {
    match wait {
        WaitConclusion::Cancelled => OutcomeKind::Cancelled,
        WaitConclusion::Timeout => OutcomeKind::Timeout,
        WaitConclusion::Exited => {
            if !key_still_present {
                OutcomeKind::Removed
            } else if reboot_pending {
                OutcomeKind::RebootPending
            } else {
                OutcomeKind::StillPresent
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_quoted_with_args() {
        let (exe, args) =
            split_exe_args(r#""C:\Program Files\App\unins000.exe" /SILENT /x"#).unwrap();
        assert_eq!(exe, r"C:\Program Files\App\unins000.exe");
        assert_eq!(args, "/SILENT /x");
    }

    #[test]
    fn split_unquoted_exe_with_args() {
        let (exe, args) = split_exe_args(r"C:\Tools\app\un.exe --uninstall").unwrap();
        assert_eq!(exe, r"C:\Tools\app\un.exe");
        assert_eq!(args, "--uninstall");
    }

    #[test]
    fn split_short_path_unquoted() {
        let (exe, args) = split_exe_args(r"C:\PROGRA~1\App\unins000.exe").unwrap();
        assert_eq!(exe, r"C:\PROGRA~1\App\unins000.exe");
        assert!(args.is_empty());
    }

    #[test]
    fn split_msiexec_and_rundll_tokens() {
        let (exe, args) =
            split_exe_args("msiexec /X{12345678-90AB-CDEF-1234-567890ABCDEF}").unwrap();
        assert_eq!(exe, "msiexec");
        assert!(args.starts_with("/X{"));
        let (exe2, args2) =
            split_exe_args("rundll32.exe advpack.dll,LaunchINFSection x.inf").unwrap();
        assert_eq!(exe2, "rundll32.exe");
        assert!(args2.contains("advpack.dll"));
    }

    #[test]
    fn split_rejects_empty() {
        assert!(split_exe_args("   ").is_none());
        assert!(split_exe_args("").is_none());
    }

    #[test]
    fn split_env_var_expanded() {
        // SystemRoot 在任何 Windows 测试机都存在；展开后应得到真实路径前缀
        // （不区分大小写——部分机器为 C:\WINDOWS）。
        let (exe, _) = split_exe_args(r#""%SystemRoot%\System32\msiexec.exe" /x"#).unwrap();
        assert!(
            exe.to_lowercase().contains("windows"),
            "环境变量应展开: {exe}"
        );
        assert!(!exe.contains('%'));
    }

    #[test]
    fn split_extra_edge_cases() {
        // 大写 .EXE 同样识别边界。
        let (exe, args) = split_exe_args(r"C:\Tools\SETUP.EXE -remove").unwrap();
        assert_eq!(exe, r"C:\Tools\SETUP.EXE");
        assert_eq!(args, "-remove");
        // 引号串无参数。
        let (exe2, args2) = split_exe_args(r#""C:\Program Files\App\unins000.exe""#).unwrap();
        assert_eq!(exe2, r"C:\Program Files\App\unins000.exe");
        assert!(args2.is_empty());
        // 未闭合引号 → 拒绝（不猜边界，调用方报错而非乱拉起）。
        assert!(split_exe_args(r#""C:\Program Files\App\unins000.exe /S"#).is_none());
        // 无扩展名单 token（PATH 命令）。
        let (exe3, args3) = split_exe_args("regsvr32 /u x.dll").unwrap();
        assert_eq!(exe3, "regsvr32");
        assert_eq!(args3, "/u x.dll");
    }

    #[test]
    fn tree_closure_cycle_and_depth_zero() {
        // PPID 环（PID 复用/收养可造出环：200→100、300→200、100→300）不得死循环。
        let snap = vec![(100, 300), (200, 100), (300, 200)];
        let mut seen = std::collections::HashSet::new();
        let added = expand_descendants(&snap, 100, TREE_MAX_DEPTH, &mut seen);
        assert_eq!(added.len(), 2);
        assert!(seen.contains(&200) && seen.contains(&300));

        // 深度 0：不扩展任何后代（防御性边界）。
        let mut seen2 = std::collections::HashSet::new();
        let added2 = expand_descendants(&[(100, 0), (200, 100)], 100, 0, &mut seen2);
        assert!(added2.is_empty());
        assert!(seen2.contains(&100) && !seen2.contains(&200));

        // 种子无后代。
        let mut seen3 = std::collections::HashSet::new();
        let added3 = expand_descendants(&[(100, 0)], 100, TREE_MAX_DEPTH, &mut seen3);
        assert!(added3.is_empty());
    }

    #[test]
    fn tree_closure_basic_and_depth() {
        // 100(root) -> 200 -> 300 -> 400；100 另有子 500。
        let snap = vec![(100, 0), (200, 100), (300, 200), (400, 300), (500, 100)];
        let mut seen = std::collections::HashSet::new();
        let added = expand_descendants(&snap, 100, TREE_MAX_DEPTH, &mut seen);
        assert_eq!(added.len(), 4);
        assert!(seen.contains(&400));

        // 深度 1：只到直接子进程。
        let mut seen2 = std::collections::HashSet::new();
        let added2 = expand_descendants(&snap, 100, 1, &mut seen2);
        assert_eq!(added2.len(), 2);
        assert!(seen2.contains(&200) && seen2.contains(&500));
        assert!(!seen2.contains(&300));
    }

    #[test]
    fn tree_closure_persistent_seen_survives_parent_exit() {
        // 第一轮：200 是 100 的子。第二轮：100/200 已退出，300 仍活且 PPID 链断裂
        // （PPID 指向已死的 200）——seen 持久保留使等待方仍记得 300。
        let snap1 = vec![(100, 0), (200, 100), (300, 200)];
        let mut seen = std::collections::HashSet::new();
        expand_descendants(&snap1, 100, TREE_MAX_DEPTH, &mut seen);
        let snap2 = vec![(300, 200), (999, 0)];
        let _ = expand_descendants(&snap2, 100, TREE_MAX_DEPTH, &mut seen);
        assert!(seen.contains(&300), "被收养的孙进程不得从等待集消失");
    }

    #[test]
    fn pending_covers_prefix_and_prefixes() {
        let entries = vec![
            r"\??\C:\Program Files\App\file.dll".to_string(),
            r"C:\Windows\Temp\x".to_string(),
        ];
        assert!(pending_covers(&entries, r"C:\Program Files\App"));
        assert!(pending_covers(&entries, r"C:\Program Files\App\file.dll"));
        assert!(!pending_covers(&entries, r"C:\Program Files\Ap"));
        // C:\Windows 确实覆盖其下条目；无关目录才应判否。
        assert!(pending_covers(&entries, r"C:\Windows"));
        assert!(!pending_covers(&entries, r"C:\Drivers"));
        assert!(!pending_covers(&entries, ""));
    }

    #[test]
    fn pending_covers_normalization() {
        // 正斜杠/尾反斜杠/\\?\ 长路径前缀/大小写差异都应归一后匹配。
        let entries = vec![r"\\?\C:\Program Files\App\sub\del.tmp".to_string()];
        assert!(pending_covers(&entries, r"c:/Program Files/App/"));
        assert!(pending_covers(&entries, r"C:\PROGRAM FILES\APP\sub"));
        // 目录边界：App2 不得被 App 前缀误吞。
        let entries2 = vec![r"\??\C:\Program Files\App2\del.tmp".to_string()];
        assert!(!pending_covers(&entries2, r"C:\Program Files\App"));
    }

    #[test]
    fn classify_outcomes() {
        use WaitConclusion::*;
        assert_eq!(classify(Exited, false, false), OutcomeKind::Removed);
        // 键消失优先于待重启。
        assert_eq!(classify(Exited, false, true), OutcomeKind::Removed);
        assert_eq!(classify(Exited, true, true), OutcomeKind::RebootPending);
        assert_eq!(classify(Exited, true, false), OutcomeKind::StillPresent);
        assert_eq!(classify(Cancelled, true, false), OutcomeKind::Cancelled);
        assert_eq!(classify(Timeout, false, false), OutcomeKind::Timeout);
    }
}
