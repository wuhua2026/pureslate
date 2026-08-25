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

mod manifest;
mod store;

pub use manifest::{
    add_manifest_entry, list_manifest, load_manifest, update_entry_state, ManifestEntry,
    ManifestState,
};
pub use store::{move_into_quarantine, QuarantineMoveError};

use std::fs;
use std::path::{Path, PathBuf};

/// 隔离区目录名（每卷根下）。
pub const QUARANTINE_DIR: &str = ".pureslate-quarantine";

/// 计算某路径所属卷的隔离区根：`<卷根>\.pureslate-quarantine\`。
/// 非盘符绝对路径（如相对路径）回退到系统盘 `C:`。
pub fn quarantine_root_of(path: &Path) -> PathBuf {
    let volume = path.to_string_lossy().chars().take(2).collect::<String>();
    let root = if volume.len() == 2
        && volume.as_bytes()[0].is_ascii_alphabetic()
        && volume.as_bytes()[1] == b':'
    {
        format!("{}\\", volume)
    } else {
        "C:\\".to_string()
    };
    PathBuf::from(root).join(QUARANTINE_DIR)
}

/// 确保隔离区根目录存在，并设置 hidden+system 属性（Windows）。
/// 目录创建失败传播错误；属性设置失败仅告警不阻断（目录可用性优先）。
pub fn ensure_quarantine_root(root: &Path) -> std::io::Result<()> {
    if !root.exists() {
        fs::create_dir_all(root)?;
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
