//! 扫描引擎（R03）：walk 遍历 + matcher 匹配。

pub mod matcher;
pub mod walk;

pub use matcher::CompiledCategory;
pub use walk::{walk_target, CancelToken};
