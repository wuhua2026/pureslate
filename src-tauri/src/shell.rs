//! Shell 集成（R30）：在资源管理器中定位并选中指定文件/文件夹。
//!
//! 只读语义：本模块不删除/不移动任何文件，仅调起 Explorer 窗口。
//! 零第三方依赖：`#[link]` 直调 shell32/shlwapi/ole32（AGENTS §7 依赖准入）。

use std::ffi::c_void;
use std::path::Path;

/// PIDL 对调用方是不透明指针，只在 API 之间传递。
type Pidl = *mut c_void;

const S_OK: i32 = 0;
const S_FALSE: i32 = 1;
const COINIT_APARTMENTTHREADED: u32 = 0x2;
const SW_SHOWNORMAL: i32 = 1;

#[link(name = "shell32")]
extern "system" {
    fn SHOpenFolderAndSelectItems(
        pidl_folder: *const c_void,
        cidl: u32,
        apidl: *const *const c_void,
        dw_flags: u32,
    ) -> i32;
    fn ShellExecuteW(
        hwnd: *mut c_void,
        operation: *const u16,
        file: *const u16,
        parameters: *const u16,
        directory: *const u16,
        show_cmd: i32,
    ) -> *mut c_void;
}

#[link(name = "shlwapi")]
extern "system" {
    fn ILCreateFromPathW(psz_path: *const u16) -> Pidl;
}

#[link(name = "ole32")]
extern "system" {
    fn CoTaskMemFree(pv: *mut c_void);
    fn CoInitializeEx(reserved: *mut c_void, co_init: u32) -> i32;
    fn CoUninitialize();
}

fn wide(path: &str) -> Vec<u16> {
    path.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 入参校验（纯函数）：必须是现存的绝对路径，且不是卷根（选中卷根无意义）。
fn validate(path: &str) -> Result<(), String> {
    let p = Path::new(path);
    if !p.is_absolute() {
        return Err("仅支持绝对路径".into());
    }
    if p.parent().is_none() {
        return Err("不支持定位磁盘根目录".into());
    }
    if p.metadata().is_err() {
        return Err("文件或文件夹不存在（可能已被移动或删除）".into());
    }
    Ok(())
}

/// 在资源管理器中打开父文件夹并选中目标。
pub fn reveal(path: &str) -> Result<(), String> {
    validate(path)?;

    // 大多数命令在主线程执行（WebView2 已 STA）；后台线程调用时补一次 STA 初始化，
    // 仅当本线程此前未初始化（S_OK）时配对 CoUninitialize，其余返回码不动套间状态。
    let co_inited =
        unsafe { CoInitializeEx(std::ptr::null_mut(), COINIT_APARTMENTTHREADED) } == S_OK;

    let result = reveal_via_shapi(path);

    if co_inited {
        unsafe { CoUninitialize() };
    }

    // 首选 API 失败（桌面 Shell 异常/路径 PIDL 构造失败）→ explorer.exe /select 兜底。
    if result.is_err() {
        reveal_via_explorer(path)?;
    }
    Ok(())
}

/// 首选：SHOpenFolderAndSelectItems（资源管理器内联高亮，不新开多余窗口）。
fn reveal_via_shapi(path: &str) -> Result<(), String> {
    let wide_path = wide(path);
    let pidl = unsafe { ILCreateFromPathW(wide_path.as_ptr()) };
    if pidl.is_null() {
        return Err("ILCreateFromPathW 返回空".into());
    }
    // RAII：无论后续成败释放 PIDL。
    let _guard = PidlGuard(pidl);

    // cidl=0 + apidl=NULL：pidl_folder 自身即要选中的完全限定项（文档惯用法）。
    let hr = unsafe { SHOpenFolderAndSelectItems(pidl, 0, std::ptr::null(), 0) };
    if hr == S_OK || hr == S_FALSE {
        Ok(())
    } else {
        Err(format!(
            "SHOpenFolderAndSelectItems 失败 HRESULT=0x{:08X}",
            hr as u32
        ))
    }
}

struct PidlGuard(Pidl);
impl Drop for PidlGuard {
    fn drop(&mut self) {
        unsafe { CoTaskMemFree(self.0) };
    }
}

/// 兜底：explorer.exe /select,"路径"（路径作为数据参数传入，不经命令行解释器）。
fn reveal_via_explorer(path: &str) -> Result<(), String> {
    let op: Vec<u16> = "open\0".encode_utf16().collect();
    let exe: Vec<u16> = "explorer.exe\0".encode_utf16().collect();
    // Explorer 接受 /select,路径；路径含空格也无需引号，引号反而会成为路径一部分。
    let params: Vec<u16> = format!("/select,{path}\0").encode_utf16().collect();
    let h = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            op.as_ptr(),
            exe.as_ptr(),
            params.as_ptr(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    // ShellExecute 历史约定：返回值 >32 表示成功。
    if (h as usize) > 32 {
        Ok(())
    } else {
        Err(format!(
            "无法打开资源管理器（ShellExecute 返回 {}）",
            h as usize
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_relative_and_missing() {
        assert!(validate("relative\\path").is_err());
        assert!(validate("C:\\").is_err()); // 卷根
        assert!(validate("Z:\\__pureslate_nonexistent__\\f.txt").is_err());
    }

    #[test]
    fn accepts_existing_absolute() {
        // 测试可执行文件自身必然存在且为绝对路径。
        let exe = std::env::current_exe().expect("current_exe");
        assert!(validate(&exe.to_string_lossy()).is_ok());
    }
}
