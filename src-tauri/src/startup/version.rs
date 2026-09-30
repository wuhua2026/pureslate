//! 发布者信息（R07）：从命令行解析可执行文件 → 读版本资源 CompanyName。
//!
//! 沿用零依赖决策：`#[link]` 直调 version.dll。

use std::ffi::c_void;
use std::path::{Path, PathBuf};

#[link(name = "version")]
extern "system" {
    fn GetFileVersionInfoSizeW(file: *const u16, handle: *mut u32) -> u32;
    fn GetFileVersionInfoW(file: *const u16, handle: u32, len: u32, data: *mut u8) -> i32;
    fn VerQueryValueW(
        block: *const c_void,
        subblock: *const u16,
        buf: *mut *mut c_void,
        len: *mut u32,
    ) -> i32;
}

/// 从命令行提取可执行文件路径（纯函数，便于单测）。
/// 规则：带引号取首对引号内内容；否则取首个空白分隔 token；只接受绝对路径。
pub fn exe_path_from_command(cmd: &str) -> Option<PathBuf> {
    let t = cmd.trim();
    if t.is_empty() {
        return None;
    }
    let first = if let Some(rest) = t.strip_prefix('"') {
        rest.split('"').next()?
    } else {
        t.split_whitespace().next()?
    };
    if first.is_empty() {
        return None;
    }
    let p = PathBuf::from(first);
    p.is_absolute().then_some(p)
}

/// 读取文件版本资源的 CompanyName 作为发布者。无版本资源/失败 → None（尽力而为）。
pub fn file_publisher(exe: &Path) -> Option<String> {
    let wide: Vec<u16> = exe
        .as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    // 安全性：size 由 API 返回，缓冲区按其分配；返回 0 表示无版本资源，直接放弃。
    let size = unsafe { GetFileVersionInfoSizeW(wide.as_ptr(), std::ptr::null_mut()) };
    if size == 0 {
        return None;
    }
    let mut buf = vec![0u8; size as usize];
    // 安全性：缓冲区长度与 size 配对传入。
    if unsafe { GetFileVersionInfoW(wide.as_ptr(), 0, size, buf.as_mut_ptr()) } == 0 {
        return None;
    }

    // 1) 语言/代码页表（VarFileInfo\Translation，指向 u16 数对）。
    let sub = to_wide("\\VarFileInfo\\Translation");
    let mut block: *mut c_void = std::ptr::null_mut();
    let mut len: u32 = 0;
    // 安全性：buf 指向 GetFileVersionInfoW 成功填充的数据块；输出指针由 API 写入。
    if unsafe {
        VerQueryValueW(
            buf.as_ptr() as *const c_void,
            sub.as_ptr(),
            &mut block,
            &mut len,
        )
    } == 0
        || len < 4
    {
        return None;
    }
    // 安全性：block 指向版本资源内部（u16 对齐），len≥4 保证可读两个 u16。
    let (lang, cp) = unsafe {
        let p = block as *const u16;
        (*p, *p.add(1))
    };

    // 2) CompanyName（常见格式：lang+codepage 小写十六进制拼接）。
    let query = format!("\\StringFileInfo\\{lang:04x}{cp:04x}\\CompanyName");
    let sub2 = to_wide(&query);
    let mut s: *mut c_void = std::ptr::null_mut();
    let mut slen: u32 = 0;
    // 安全性：同上，输出指针/长度由 API 写入。
    if unsafe {
        VerQueryValueW(
            buf.as_ptr() as *const c_void,
            sub2.as_ptr(),
            &mut s,
            &mut slen,
        )
    } == 0
        || slen == 0
    {
        return None;
    }
    // 安全性：s 指向 NUL 结尾 UTF-16 串，slen 含结尾 NUL。
    let units = unsafe { std::slice::from_raw_parts(s as *const u16, slen as usize - 1) };
    let out = String::from_utf16_lossy(units).trim().to_string();
    (!out.is_empty()).then_some(out)
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exe_path_quoted_with_args() {
        assert_eq!(
            exe_path_from_command("\"C:\\Program Files\\App\\a.exe\" /bg"),
            Some(PathBuf::from("C:\\Program Files\\App\\a.exe"))
        );
    }

    #[test]
    fn exe_path_unquoted() {
        assert_eq!(
            exe_path_from_command("C:\\Windows\\notepad.exe"),
            Some(PathBuf::from("C:\\Windows\\notepad.exe"))
        );
    }

    #[test]
    fn exe_path_relative_or_empty_rejected() {
        assert_eq!(exe_path_from_command("rundll32.dll,Setup"), None);
        assert_eq!(exe_path_from_command(""), None);
        assert_eq!(exe_path_from_command("   "), None);
    }

    #[test]
    fn exe_path_missing_closing_quote() {
        assert_eq!(
            exe_path_from_command("\"C:\\x\\a.exe"),
            Some(PathBuf::from("C:\\x\\a.exe"))
        );
    }

    #[test]
    fn publisher_from_system_exe_if_present() {
        let exe = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("C:\\Windows"))
            .join("explorer.exe");
        if exe.is_file() {
            let p = file_publisher(&exe);
            assert!(p.is_some(), "explorer.exe 应有版本资源 CompanyName");
        }
    }
}
