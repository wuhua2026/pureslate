//! 隔离区（R25）。
//!
//! 模块职责（SPEC §4.3 / SAFETY §4）：
//! - `store`：把用户文件移入隔离区（同盘 rename / 跨盘 copy+hash 校验+删源），目标目录 hidden+system；
//! - `manifest`：卷内 `<卷根>\.pureslate-quarantine\manifest.jsonl` 逐行 JSON 记录与查询；
//! - `restore`：按 manifest 还原（P2-02）；
//! - `lifecycle`：保留期/到期清除/容量上限（P3-06）。
//!
//! 目录哨兵：隔离区根路径由 `quarantine_root_of()` 计算；白名单已在 `safety/whitelist` 强制
//! 排除（SAFETY §2.6），隔离区文件绝不经通用清理路径。

pub mod lifecycle;
mod manifest;
mod restore;
mod store;

pub use lifecycle::{
    purge_across, purge_globally, run_pass_at_root, run_pass_globally, LifecycleReport,
};
pub use manifest::{
    add_manifest_entry, list_manifest, load_manifest, prune_restored_before, update_entry_state,
    ManifestEntry, ManifestState,
};
pub use restore::{
    list_quarantined_globally, manifest_to_contract, restore_globally, restore_one, RestoreOutcome,
};
pub use store::{move_into_quarantine, sha256_file, QuarantineInput, QuarantineMoveError};

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// 隔离区目录名（每卷根下）。
pub const QUARANTINE_DIR: &str = ".pureslate-quarantine";

/// 当前 epoch 毫秒探针（隔离区模块统一时钟入口；保留期/daysLeft 计算用）。
pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 枚举全部**固定盘**根路径（`C:\`…）。Windows 用 `GetDriveTypeW == DRIVE_FIXED`；
/// 非 Windows（测试/CI）返回空，调用方注入沙箱根。
pub fn fixed_drive_roots() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        let mut v = Vec::new();
        for code in b'A'..=b'Z' {
            let root = format!("{}:\\", code as char);
            if drive_type_fixed(&root) {
                v.push(PathBuf::from(root));
            }
        }
        v
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// 全部**已存在**的隔离区根：`<固定盘>\.pureslate-quarantine`。
pub fn all_quarantine_roots() -> Vec<PathBuf> {
    fixed_drive_roots()
        .into_iter()
        .map(|r| r.join(QUARANTINE_DIR))
        .filter(|p| p.is_dir())
        .collect()
}

#[cfg(windows)]
fn drive_type_fixed(root: &str) -> bool {
    let wide: Vec<u16> = root.encode_utf16().collect();
    let mut cstr = wide.clone();
    cstr.push(0);
    // DRIVE_FIXED = 3
    unsafe { GetDriveTypeW(cstr.as_ptr()) == 3 }
}

#[cfg(windows)]
unsafe extern "system" {
    fn GetDriveTypeW(lp_root_path_name: *const u16) -> u32;
}

/// 计算某路径所属卷的隔离区根：`<卷根>\.pureslate-quarantine\`。
/// 兼容 `\\?\C:\...` 扩展长度前缀（P4-02 长路径边界）：在整串中定位首个盘符段。
/// 无盘符路径（如相对路径）回退到系统盘 `C:`。
pub fn quarantine_root_of(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    let bytes = s.as_bytes();
    let mut drive: Option<String> = None;
    for i in 0..bytes.len().saturating_sub(1) {
        if bytes[i].is_ascii_alphabetic() && bytes[i + 1] == b':' {
            let sep_ok = i == 0 || bytes[i - 1] == b'\\' || bytes[i - 1] == b'/';
            if sep_ok {
                drive = Some(s[i..i + 2].to_string());
                break;
            }
        }
    }
    let root = format!("{}\\", drive.unwrap_or_else(|| "C:".into()));
    PathBuf::from(root).join(QUARANTINE_DIR)
}

/// 确保隔离区根目录存在，并设置 hidden+system 属性（Windows）。
/// **T-3 reparse 校验（P4-02 安全审计）**：根路径本身命中 junction/symlink 即拒绝——
/// 防止隔离区目录被替换为指向任意位置的链接（否则移入=向链接目标写文件）。
/// 目录创建失败传播错误；属性设置失败仅告警不阻断（目录可用性优先）。
pub fn ensure_quarantine_root(root: &Path) -> std::io::Result<()> {
    if !root.exists() {
        fs::create_dir_all(root)?;
    }
    // symlink_metadata 不跟随链接：junction/symlink 均被 std 映射为 is_symlink。
    let meta = fs::symlink_metadata(root)?;
    if meta.file_type().is_symlink() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "隔离区根命中 junction/symlink，拒绝使用: {}",
                root.to_string_lossy()
            ),
        ));
    }
    if !meta.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("隔离区根不是目录: {}", root.to_string_lossy()),
        ));
    }
    set_hidden_system(root);
    Ok(())
}

/// 对目录附加 hidden + system 属性。Windows 用 `#[link]` 直调 kernel32（零依赖）；
/// 非 Windows 平台（测试/CI）静默跳过。
#[cfg(windows)]
pub(crate) fn set_hidden_system(dir: &Path) {
    // Win32: 拼接 UTF-16 宽路径，SetFileAttributesW + FILE_ATTRIBUTE_HIDDEN|SYSTEM
    // 在测试临时目录上仅设 HIDDEN 也会被清理脚本忽略；系统属性不做强制校验。
    let wide: Vec<u16> = dir.to_string_lossy().encode_utf16().collect();
    let mut cstr = wide.clone();
    cstr.push(0);
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
    set_file_attributes(cstr.as_ptr(), FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM);
}

#[cfg(windows)]
unsafe extern "system" {
    fn SetFileAttributesW(lp_file_name: *const u16, dw_file_attributes: u32) -> i32;
}

#[cfg(windows)]
#[inline]
fn set_file_attributes(path: *const u16, attrs: u32) {
    unsafe {
        SetFileAttributesW(path, attrs);
    }
}

#[cfg(not(windows))]
pub(crate) fn set_hidden_system(_dir: &Path) {}
