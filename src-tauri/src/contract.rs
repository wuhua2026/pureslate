//! IPC 契约 Rust 端（serde 镜像）。
//!
//! 与 `src/types/ipc.ts` 为镜像两端，M0 后视为冻结。任何修改须：
//! 1) 双端同步；2) 记占 docs/dev/CHANGELOG.md；3) 不破坏既有签名（新增字段须 optional）。

use serde::{Deserialize, Serialize};

// ---- 基础枚举 ----
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Grade {
    Green,
    Yellow,
    Red,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Disposition {
    Direct,
    Recycle,
    Quarantine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScanDimension {
    Temp,
    Large,
    Dup,
    Cache,
    Startup,
    Privacy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum QuarantineState {
    Quarantined,
    Restored,
    Purged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StartupSource {
    HkcuRun,
    HklmRun,
    StartupFolder,
    TaskScheduler,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Impact {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Channel {
    Github,
    Mirror,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScanPhase {
    Walking,
    Hashing,
    Aggregating,
}

// ---- 扫描 ----
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProfile {
    #[serde(default)]
    pub dimensions: std::collections::HashMap<ScanDimension, bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanItem {
    pub id: String, // sha1(categoryId + path) 前 16 位
    pub category_id: String,
    pub label: String,
    pub path: String,
    pub size_bytes: u64,
    pub grade: Grade,
    pub disposition: Disposition,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mtime: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub atime: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dup_group: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryAggregate {
    pub category_id: String,
    pub label: String,
    pub grade: Grade,
    pub disposition: Disposition,
    pub total_bytes: u64,
    pub item_count: u64,
    pub reason: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundBytes {
    pub green: u64,
    pub yellow: u64,
    pub red: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub scan_id: String,
    pub started_at: i64,
    pub finished_at: i64,
    pub volume: String,
    pub aggregates: Vec<CategoryAggregate>,
    pub item_count: u64,
    pub total_bytes: FoundBytes,
    /// F-2（P4-02 加性）：whitelist.xml 加载失败时为 `Some(false)`——UI 须在清理前
    /// 明示降级；正常为 None（序列化省略，旧消费者不受影响）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub whitelist_ok: Option<bool>,
}

// ---- 隔离区 ----
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuarantineEntry {
    pub id: String,
    pub original_path: String,
    pub size_bytes: u64,
    pub grade: Grade,
    pub category_id: String,
    pub moved_at: i64,
    pub expires_at: i64,
    pub days_left: i64,
    pub state: QuarantineState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreFailure {
    pub id: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreReport {
    pub requested: u64,
    pub restored: u64,
    pub conflict: u64,
    pub failures: Vec<RestoreFailure>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PurgeReport {
    pub requested: u64,
    pub purged: u64,
    pub failures: Vec<RestoreFailure>,
}

/// 隔离区容量状态（P3-06 加性新增，`quarantine_status` 返回）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuarantineStatus {
    pub used_bytes: u64,
    pub quota_bytes: u64,
    pub over_quota: bool,
    /// 超限时"释放最早批次"建议 id（不超限为空）。
    pub earliest_batch_ids: Vec<String>,
}

// ---- 启动项 ----
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupEntry {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    pub command: String,
    pub source: StartupSource,
    pub impact: Impact,
    pub enabled: bool,
}

// ---- 更新 ----
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub current_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_version: Option<String>,
    pub has_update: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rules_pack_hash_ok: Option<bool>,
    pub channel: Channel,
    pub checked_at: i64,
}

// ---- 日志 ----
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogQueryFilter {
    pub from: i64,
    pub to: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub op: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub ts: i64,
    pub op: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disposition: Option<Disposition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

// ---- 设置 ----
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub quarantine_retention_days: u32,
    pub quarantine_auto_purge: bool,
    pub update_opt_in: bool,
    pub crash_upload_opt_in: bool,
    pub expert_mode: bool,
    pub mirror_first: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            quarantine_retention_days: 14,
            quarantine_auto_purge: true,
            update_opt_in: false,
            crash_upload_opt_in: false,
            expert_mode: false,
            mirror_first: true,
        }
    }
}

// ---- 应用信息 ----
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppMeta {
    pub version: String,
    pub rules_version: String,
    pub channel: Channel,
}

// ---- 事件 payload ----
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgressEvent {
    pub scan_id: String,
    pub phase: ScanPhase,
    pub percent: f64,
    pub current_path: String,
    pub found_bytes: FoundBytes,
    pub elapsed_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanProgressEvent {
    pub tx_id: String,
    pub item_path: String,
    pub disposition: Disposition,
    pub done_bytes: u64,
    pub total_bytes: u64,
    pub state: CleanProgressState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CleanProgressState {
    Ok,
    Fail,
    Skip,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuarantineExpiryWarningEvent {
    pub ids: Vec<String>,
    pub days_left: i64,
}

/// 清理事务完成事件 payload（done 时仅发一次；前端以此从执行页切到完成页）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanDoneEvent {
    pub tx_id: String,
    pub total: u64,
    pub ok: u64,
    pub fail: u64,
    pub skip: u64,
}

// ---- 命令参数（与 TS api 层对应） ----
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanGetItemsParams {
    pub scan_id: String,
    pub offset: u64,
    pub limit: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<ScanItemsFilter>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanItemsFilter {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grade: Option<Grade>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanExecuteParams {
    pub items: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirm_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuarantineRestoreParams {
    pub ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuarantinePurgeParams {
    pub ids: Vec<String>,
    pub confirm_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupToggleParams {
    pub id: String,
    pub enabled: bool,
}

// ---- 事件名常量 ----
pub mod events {
    pub const SCAN_PROGRESS: &str = "scan_progress";
    pub const SCAN_DONE: &str = "scan_done";
    pub const CLEAN_PROGRESS: &str = "clean_progress";
    pub const CLEAN_DONE: &str = "clean_done";
    pub const QUARANTINE_EXPIRY_WARNING: &str = "quarantine_expiry_warning";
    pub const UPDATE_AVAILABLE: &str = "update_available";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_meta_serializes_camel_case() {
        let meta = AppMeta {
            version: "0.1.0".into(),
            rules_version: "0".into(),
            channel: Channel::Github,
        };
        let json = serde_json::to_string(&meta).unwrap();
        assert!(json.contains("\"rulesVersion\""));
        assert!(json.contains("\"channel\":\"github\""));
    }

    #[test]
    fn app_meta_roundtrip_deserialize() {
        let json = r#"{"version":"0.1.0","rulesVersion":"1","channel":"github"}"#;
        let meta: AppMeta = serde_json::from_str(json).unwrap();
        assert_eq!(meta.channel, Channel::Github);
    }
}
