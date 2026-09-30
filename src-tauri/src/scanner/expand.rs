//! 规则 target 展开（R03 expand）。SPEC §4.1 target type：
//! `env`（%VAR% 环境变量）｜`path`（绝对路径）｜`knownFolder`（Known Folder 语义）。
//!
//! 只读：仅解析为路径起点，不做任何写操作。
//! 无法展开的 target 返回 `None` 并记日志（宁缺勿扫，白名单在匹配阶段另行强制）。

use std::path::{Path, PathBuf};

use crate::rules::model::{Target, TargetType};

/// 展开单个 target 为起点路径。无法展开返回 `None`。
pub fn expand_target(target: &Target) -> Option<PathBuf> {
    let p = match target.ty {
        TargetType::Env => expand_env(&target.value),
        TargetType::Path => expand_path(&target.value),
        TargetType::KnownFolder => expand_known_folder(&target.value),
    };
    match p {
        Some(p) if p.as_os_str().is_empty() => None,
        other => other,
    }
}

/// 从环境变量展开 target 值（`%TEMP%` → env `TEMP`）。
fn expand_env(value: &str) -> Option<PathBuf> {
    let var = value.trim().trim_start_matches('%').trim_end_matches('%');
    if var.is_empty() {
        return None;
    }
    std::env::var(var)
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

/// 绝对路径直接使用。
fn expand_path(value: &str) -> Option<PathBuf> {
    if value.trim().is_empty() {
        None
    } else {
        Some(PathBuf::from(value))
    }
}

/// Known Folder 语义展开。规则自身对本仓库的可读英文值做映射，落到真实目录；
/// 未知值返回 `None`（该 target 跳过，避免误扫）。
///
/// 净室说明：以下目录为自研映射，不引入 windows-rs Known Folder GUID 常量，
/// 维持零第三方依赖与 <20MB 体积门禁。
fn expand_known_folder(value: &str) -> Option<PathBuf> {
    let user = std::env::var("USERPROFILE").ok().map(PathBuf::from)?;
    let local_app = std::env::var("LOCALAPPDATA")
        .ok()
        .map(PathBuf::from)
        .or_else(|| Some(user.join("AppData\\Local")));
    match value.trim() {
        "Local AppData" => local_app,
        // 用户临时目录的派生入口（同 env %TEMP% 语义，此处为 knownFolder 别名）。
        "Temp" => std::env::var("TEMP").ok().map(PathBuf::from),
        // 微信文件目录默认位于文档下；其它盘符/自定义路径由规则另以 path target 声明。
        "WeChat Files" => {
            let docs = std::env::var("DOCUMENTS")
                .ok()
                .map(PathBuf::from)
                .or_else(|| Some(user.join("Documents")));
            let a = docs.map(|d| d.join("WeChat Files"));
            if a.as_ref().is_some_and(|p| p.exists()) {
                a
            } else {
                Some(user.join("WeChat Files"))
            }
        }
        // 缩略图缓存（Explorer 缩略图数据库/图标缓存目录）。
        "Thumbnail Cache" => local_app.map(|p| p.join("Microsoft\\Windows\\Explorer")),
        // ---- R08 隐私痕迹源（P3-04）：目录不存在视为未安装/未启用，跳过该 target ----
        // Edge 用户数据（历史记录数据库所在）。
        "Edge User Data" => local_app
            .map(|p| p.join("Microsoft\\Edge\\User Data"))
            .filter(|p| p.exists()),
        // Chrome 用户数据（历史记录数据库所在）。
        "Chrome User Data" => local_app
            .map(|p| p.join("Google\\Chrome\\User Data"))
            .filter(|p| p.exists()),
        // 最近文档记录（RecentDocs，资源管理器最近访问 .lnk）。
        "RecentDocs" => std::env::var("APPDATA")
            .ok()
            .map(PathBuf::from)
            .or_else(|| Some(user.join("AppData\\Roaming")))
            .map(|r| r.join("Microsoft\\Windows\\Recent"))
            .filter(|p| p.exists()),
        // 用户内容目录（dup 维度扫描范围：重复文件普遍存于文档/下载/图片/桌面/视频）。
        "Documents" => Some(user.join("Documents")),
        "Downloads" => Some(user.join("Downloads")),
        "Pictures" => Some(user.join("Pictures")),
        "Desktop" => Some(user.join("Desktop")),
        "Videos" => Some(user.join("Videos")),
        _ => {
            eprintln!("[expand] 未知 knownFolder `{value}`，跳过该 target");
            None
        }
    }
}

/// 提取路径所在卷（`C:\x\y` → `C:`）。非盘符路径返回 `None`。
pub fn volume_of(path: &Path) -> Option<String> {
    let s = path.to_string_lossy();
    let mut chars = s.chars();
    let first = chars.next()?;
    let second = chars.next()?;
    if first.is_ascii_alphabetic() && second == ':' {
        Some(format!("{first}:"))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_expands_dropping_percent() {
        let t = Target {
            ty: TargetType::Env,
            value: "%TEMP%".into(),
        };
        let _ = expand_target(&t); // 仅断言不 panic；路径取决于运行环境
    }

    #[test]
    fn unknown_env_is_none() {
        let t = Target {
            ty: TargetType::Env,
            value: "%PURESLATE_NO_SUCH_ENV_xyz%".into(),
        };
        assert!(expand_target(&t).is_none());
    }

    #[test]
    fn path_used_as_is() {
        let t = Target {
            ty: TargetType::Path,
            value: r"C:\Sandbox\scan".into(),
        };
        assert_eq!(
            expand_target(&t).unwrap(),
            PathBuf::from(r"C:\Sandbox\scan")
        );
    }

    #[test]
    fn empty_path_is_none() {
        let t = Target {
            ty: TargetType::Path,
            value: "  ".into(),
        };
        assert!(expand_target(&t).is_none());
    }

    #[test]
    fn known_folder_local_appdata_resolves() {
        let t = Target {
            ty: TargetType::KnownFolder,
            value: "Local AppData".into(),
        };
        let p = expand_target(&t);
        assert!(p.is_some(), "Local AppData 应能展开");
        assert!(volume_of(&p.unwrap()).is_some());
    }

    #[test]
    fn unknown_known_folder_is_none() {
        let t = Target {
            ty: TargetType::KnownFolder,
            value: "NoSuchFolder".into(),
        };
        assert!(expand_target(&t).is_none());
    }

    #[test]
    fn volume_extracts_drive() {
        assert_eq!(volume_of(Path::new(r"C:\Users\x")), Some("C:".into()));
        assert_eq!(volume_of(Path::new(r"relative/path")), None);
        assert_eq!(volume_of(Path::new(r"\\server\share\x")), None);
    }
}
