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
