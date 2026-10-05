//! 扫描引擎（R03）：target 展开 + walk 遍历 + mft 直读 + matcher 匹配 + 编排聚合。

pub mod aggregate;
pub mod engine;
pub mod expand;
pub mod matcher;
pub mod mft;
pub mod walk;

pub use aggregate::{category_aggregate, found_bytes};
pub use engine::{run_scan, ScanOutcome};
pub use expand::{expand_target, volume_of};
pub use matcher::CompiledCategory;
pub use mft::{is_admin, walk_matching, MftError, MftScanStats, MftSession};
pub use walk::{walk_target, CancelToken};

/// M5（v0.1.4 · SAFETY §2.7）：OneDrive/云盘占位文件判定——全量哈希或读取会
/// 触发云端按需下载（用户流量浪费 + 扫描变慢）。RECALL_ON_DATA_ACCESS(0x00400000)
/// 或 OFFLINE(0x1000) 任一命中即跳过；非 Windows 恒 false（测试/CI）。
pub(crate) fn is_cloud_placeholder(meta: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x0040_0000;
        const FILE_ATTRIBUTE_OFFLINE: u32 = 0x0000_1000;
        meta.file_attributes() & (FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS | FILE_ATTRIBUTE_OFFLINE)
            != 0
    }
    #[cfg(not(windows))]
    {
        let _ = meta;
        false
    }
}
