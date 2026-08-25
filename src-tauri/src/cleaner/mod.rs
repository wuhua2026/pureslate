//! 清理执行（R04 · SPEC §6.2 / SAFETY §3）。

pub mod execute;
pub mod journal;
pub mod recycle;

pub use crate::contract::CleanProgressState;
pub use execute::{execute, CleanFailure, CleanProgress, CleanReport, CleanTarget};
