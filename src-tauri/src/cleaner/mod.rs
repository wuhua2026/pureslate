//! 清理执行（R04 · SPEC §6.2 / SAFETY §3）。

pub mod dup;
pub mod execute;
pub mod journal;
pub mod preflight;
pub mod recycle;

/// R27-3 卸载残留类目的固定 category_id：
/// ①ScanItem/审计/manifest 行以此标识来源；②隔离区还原时，仅该类目可豁免
/// §2.2 程序目录软根（硬保护仍拦；见 `quarantine::restore` 与 SAFETY §2.2 条款）。
pub const RESIDUE_CATEGORY_ID: &str = "residue.uninstall";

pub use crate::contract::CleanProgressState;
pub use execute::{
    execute, resolve_targets, CleanFailure, CleanProgress, CleanReport, CleanTarget,
};
