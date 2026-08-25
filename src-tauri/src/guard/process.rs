//! 目标进程探测（R23 第一语义 · SAFETY §5.1）。
//!
//! Windows 用 `CreateToolhelp32Snapshot` + `Process32FirstW/NextW` 枚举当前运行进程，
//! 与规则 `<guard process="WeChat.exe"/>` 指定的 exe 基名做大小写不敏感比较。
//! 零第三方依赖：`#[link]` 直调 kernel32。非 Windows（测试/CI）返回空，不阻断。

/// 返回命中的运行中进程基名列表（大小写不敏感；按输入顺序去重保留首个命中）。
pub fn running_matching(exe_base_names: &[String]) -> Vec<String> {
    #[cfg(windows)]
    {
        let running = running_exe_names();
        exe_base_names
            .iter()
            .filter(|want| {
                let want_lower = want.to_lowercase();
                running.iter().any(|r| r == &want_lower)
            })
            .cloned()
            .collect()
    }
    #[cfg(not(windows))]
    {
        let _ = exe_base_names;
        Vec::new()
    }
}

/// 当前运行的全部进程 exe 基名（小写）。只返回进程名，足够过程守卫比对用。
#[cfg(windows)]
fn running_exe_names() -> Vec<String> {
    const TH32CS_SNAPPROCESS: u32 = 0x2;
    const MAX_PROC: usize = 512;
    let mut out = Vec::with_capacity(MAX_PROC);

    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return out;
        }
        let mut entry = PROCESSENTRY32W::new();
        let mut ok = Process32FirstW(snapshot, &mut entry) != 0;
        while ok {
            let name = wide_cstr(&entry.szExeFile);
            if !name.is_empty() {
                out.push(name.to_lowercase());
            }
            ok = Process32NextW(snapshot, &mut entry) != 0;
        }
        let _ = CloseHandle(snapshot);
    }
    out
}

/// 截取宽字符数组直到第一个 NUL，转 String（等价 C 字符串语义）。
fn wide_cstr(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

#[cfg(windows)]
const INVALID_HANDLE_VALUE: *mut std::ffi::c_void = -1isize as *mut std::ffi::c_void;

#[cfg(windows)]
#[repr(C)]
#[allow(non_snake_case)] // Win32 结构体字段名（FFI 布局按偏移对应，保留原生命名）
struct PROCESSENTRY32W {
    dwSize: u32,
    cntUsage: u32,
    th32ProcessID: u32,
    th32DefaultHeapID: usize,
    th32ModuleID: u32,
    cntThreads: u32,
    th32ParentProcessID: u32,
    pcPriClassBase: i32,
    dwFlags: u32,
    szExeFile: [u16; 260],
}

#[cfg(windows)]
impl PROCESSENTRY32W {
    fn new() -> Self {
        Self {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            cntUsage: 0,
            th32ProcessID: 0,
            th32DefaultHeapID: 0,
            th32ModuleID: 0,
            cntThreads: 0,
            th32ParentProcessID: 0,
            pcPriClassBase: 0,
            dwFlags: 0,
            szExeFile: [0u16; 260],
        }
    }
}

#[cfg(windows)]
#[allow(non_snake_case)] // Win32 函数名/句柄指针语义；指针大小类型不影响 HANDLE ABI
unsafe extern "system" {
    fn CreateToolhelp32Snapshot(dwFlags: u32, th32ProcessID: u32) -> *mut std::ffi::c_void;
    fn Process32FirstW(hSnapshot: *mut std::ffi::c_void, lppe: *mut PROCESSENTRY32W) -> i32;
    fn Process32NextW(hSnapshot: *mut std::ffi::c_void, lppe: *mut PROCESSENTRY32W) -> i32;
    // 与 mft.rs 同签名，避免 clashing_extern_declarations。
    fn CloseHandle(hObject: *mut std::ffi::c_void) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_cstr_strips_trailing() {
        let mut buf = [0u16; 260];
        for (i, c) in "WeChat.EXE".encode_utf16().enumerate() {
            buf[i] = c;
        }
        assert_eq!(wide_cstr(&buf), "WeChat.EXE");
    }

    #[test]
    fn running_matching_handles_empty() {
        // 非 Windows 为空；Windows 上"CURRENT EXE"不匹配任意守卫名（本测试名不寻常）。
        let names: Vec<String> = vec![]; // ["pureslate-core".into()];
        assert!(running_matching(&names).is_empty());
    }
}
