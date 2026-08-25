//! 审计日志（SPEC §4.4，R09 / M8）。
//!
//! 落盘 `%LOCALAPPDATA%\PureSlate\logs\audit-YYYY-MM-DD.jsonl`，只追加、按天滚动，
//! 覆盖全部破坏性操作（clean/restore/purge/auto_purge/disable_startup）与 scan（M8=100%）。

pub mod audit;
