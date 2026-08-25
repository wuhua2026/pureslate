//! 回收站删除路径（R04 · SPEC §6.2 / SAFETY §6.2）。
//!
//! 🟢 recycle 语义：把文件移入系统回收站（可恢复），用 `SHFileOperationW`（shell32，
//! `FO_DELETE + FOF_ALLOWUNDO`），零第三方依赖。非 Windows / 非文件路径 → 报错不删。

use std::path::Path;

/// 将文件移入系统回收站。返回 `Ok(())` 表示回收站操作成功提交。
pub fn delete_to_recycle_bin(path: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        delete_win(path)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err(std::io::Error::other("回收站仅支持 Windows"))
    }
}

#[cfg(windows)]
fn delete_win(path: &Path) -> std::io::Result<()> {
    use std::ffi::c_void;

    const FO_DELETE: u32 = 3;
    const FOF_SILENT: u16 = 0x0004;
    const FOF_NOCONFIRMATION: u16 = 0x0010;
    const FOF_ALLOWUNDO: u16 = 0x0040;

    #[repr(C)]
    #[allow(non_snake_case)] // Win32 结构体字段名（FFI 布局，保留原生命名）
    #[allow(clippy::upper_case_acronyms)] // Win32 API 结构体名，保留原生命名
    struct SHFILEOPSTRUCTW {
        hwnd: *mut c_void,
        wFunc: u32,
        pFrom: *const u16,
        pTo: *const u16,
        fFlags: u16,
        fAnyOperationsAborted: i32,
        hNameMappings: *mut c_void,
        lpszProgressTitle: *const u16,
    }

    #[link(name = "shell32")]
    unsafe extern "system" {
        fn SHFileOperationW(lpFileOp: *mut SHFILEOPSTRUCTW) -> i32;
    }

    // pFrom 须为双 NUL 结尾的宽字符串（可含多个通配路径），这里单个完整路径。
    let wide: Vec<u16> = path
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .chain(std::iter::once(0))
        .collect();
    if !wide.first().is_some_and(|c| *c != 0) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "空路径不可回收",
        ));
    }

    let mut op = SHFILEOPSTRUCTW {
        hwnd: std::ptr::null_mut(),
        wFunc: FO_DELETE,
        pFrom: wide.as_ptr(),
        pTo: std::ptr::null(),
        fFlags: FOF_SILENT | FOF_NOCONFIRMATION | FOF_ALLOWUNDO,
        fAnyOperationsAborted: 0,
        hNameMappings: std::ptr::null_mut(),
        lpszProgressTitle: std::ptr::null(),
    };

    let rc = unsafe { SHFileOperationW(&mut op as *mut SHFILEOPSTRUCTW) };
    if rc != 0 || op.fAnyOperationsAborted != 0 {
        return Err(std::io::Error::other(format!(
            "回收站操作失败(rc={rc}, aborted={})",
            op.fAnyOperationsAborted
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_path_rejected() {
        // 非 Windows 直接报"仅支持 Windows"；Windows 上空路径被回收站模块拒绝。
        assert!(delete_to_recycle_bin(Path::new("")).is_err());
    }
}
