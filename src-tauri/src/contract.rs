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
// v0.1.5：修正镜像不一致（原 camelCase "hkcuRun" 与 ipc.ts/mock 的 "hkcu_run" 不符，
// 真实模式来源列显示空白）+ 加性两源（v0.1.5 反馈 4）。
#[serde(rename_all = "snake_case")]
pub enum StartupSource {
    HkcuRun,
    HklmRun,
    StartupFolder,
    StartupFolderAllUsers,
    TaskScheduler,
    ActiveSetup,
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
    /// v0.1.5 加性：系统级已禁用（StartupApproved 判定）——enabled=false 且非本应用
    /// 备份所致；UI 标注"系统已禁用"且不提供恢复入口（不写系统批准状态）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_disabled: Option<bool>,
}

/// 启动项列表 + 来源级失败（v0.2.2 加性新增，`startup_list_report` 返回）。
///
/// `failed_sources` 与 `BootStartupData.failed_sources` 同一套语义：
/// `tasks_root_missing` / `tasks_dir_empty_or_denied` / `tasks_walk_errors_N` /
/// `tasks_parse_failed_N` / `tasks_truncated_2000`。空数组 = 全部来源读取成功。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupListReport {
    pub entries: Vec<StartupEntry>,
    pub failed_sources: Vec<String>,
}

// ---- 卸载辅助（R27 · SPEC §6.7）----
/// 已安装程序条目（只读枚举 Uninstall 注册表四视图）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledApp {
    /// sha256(scope|arch|子键名) 前 16 位；执行批次据此回查注册表定位。
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// 注册表原值 YYYYMMDD（不擅自格式化，展示层转换）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_location: Option<String>,
    /// 可直接拉起的卸载命令（MSI 条目已构造为 msiexec /X{GUID}）。
    pub uninstall_command: String,
    pub is_msi: bool,
    /// "x64" / "x86"（来自注册表视图，非 PE 头探测）。
    pub arch: String,
    /// "machine"（HKLM）/ "user"（HKCU）。
    pub scope: String,
    /// EstimatedSize（注册表单位 KB）换算字节；缺失为 None。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_size_bytes: Option<u64>,
}

/// R27-2 卸载进度事件（阶段文案单向推送；前端不轮询）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UninstallProgressEvent {
    pub tx_id: String,
    pub message: String,
}

/// R27-2 卸载完成事件。result ∈
/// removed / reboot_pending / still_present / cancelled / timeout（拉起失败走命令 Err，不发本事件）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UninstallDoneEvent {
    pub tx_id: String,
    pub app_id: String,
    pub name: String,
    pub result: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<u32>,
    pub key_still_present: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_location_still_present: Option<bool>,
    pub reboot_pending: bool,
    pub detail: String,
    /// R27-3 加性：登记的安装目录（供结果页"扫描残留"展示与后端快照同源核对）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_location: Option<String>,
}

// ---- R27-3 卸载残留（SPEC §6.7） ----
/// `uninstall_residue_clean` 的单项选择（候选组 id + 用户选择的去向）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResidueSelectionParams {
    pub candidate_id: String,
    /// 仅接受 "quarantine"（默认，可还原）/ "direct"（永久删除，需专家 token）。
    pub disposition: Disposition,
}

/// `uninstall_residue_clean` 入参；含任一 direct 选择时必须消费一次性专家令牌。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResidueCleanParams {
    pub selections: Vec<ResidueSelectionParams>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirm_token: Option<String>,
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

// ---- 崩溃安全（R24 · P4-03 加性新增，SPEC §6.6） ----
/// 一份崩溃转储的列表项（`crash_list` 返回）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashDumpInfo {
    pub file_name: String,
    pub size_bytes: u64,
    /// 文件修改时间（epoch ms）。
    pub ts: i64,
}

/// dump 预览摘要（`crash_preview` 返回；解析失败时 error 非 None，UI 展示原文）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashDumpPreview {
    pub file_name: String,
    pub size_bytes: u64,
    pub ts: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module_count: Option<u32>,
    /// 模块基名预览（上限见内核 parse::PREVIEW_MODULE_CAP）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modules: Option<Vec<String>>,
    /// 异常代码（panic 通道 dump 无异常流 → None）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exception_code: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 上传结果（`crash_upload` 返回）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashUploadReport {
    pub file_name: String,
    pub uploaded: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// 启动孤儿 journal 恢复报告（T-6 协议；`crash_recovery` 返回，ranAt=0 表示尚未运行）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashRecoveryReport {
    pub ran_at: i64,
    pub orphans_found: u64,
    /// manifest 信任锚还原成功。
    pub restored: u64,
    /// 还原时原路径被占用，落 restore-conflict 兜底。
    pub conflict_restored: u64,
    /// 崩溃窗口「已移入未登记」：按命名约定反查补登记（不自动还原）。
    pub adopted: u64,
    /// 不可逆去向（direct/recycle）：仅标记 orphan + 审计，未动作。
    pub irreversible: u64,
    /// 无需处理（移动前崩溃 / 已有终态记录）。
    pub untouched: u64,
    pub failures: Vec<String>,
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
    /// 当前进程是否管理员提权（v0.1.2 加性：MFT 快速引擎启用与否的 UI 提示依据；
    /// 非 Windows / 不可得时省略）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_elevated: Option<bool>,
}

/// 单个固定磁盘的容量信息（v0.1.2 加性新增，`disk_usage` 返回；首页磁盘条真实数据）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskUsageInfo {
    /// 盘符字母（"C"）。
    pub letter: String,
    /// 卷标（用户命名的卷名；不可得为 None）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub total_bytes: u64,
    pub free_bytes: u64,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashPreviewParams {
    pub file_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashUploadParams {
    pub file_name: String,
}

// ---- 事件名常量 ----
pub mod events {
    pub const SCAN_PROGRESS: &str = "scan_progress";
    pub const SCAN_DONE: &str = "scan_done";
    pub const CLEAN_PROGRESS: &str = "clean_progress";
    pub const CLEAN_DONE: &str = "clean_done";
    pub const QUARANTINE_EXPIRY_WARNING: &str = "quarantine_expiry_warning";
    pub const UPDATE_AVAILABLE: &str = "update_available";
    pub const UNINSTALL_PROGRESS: &str = "uninstall_progress";
    pub const UNINSTALL_DONE: &str = "uninstall_done";
}

// ---- 开机诊断（v0.2.x 加性 · 只读）----
// 设计稿：docs/dev/DESIGN-BOOTDIAG-2026-11-10.md §4。只新增类型与命令，
// 不改动既有契约；所有"拿不到"都通过 BootSection 状态表达，而非报错。

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BootSectionStatus {
    Ok,
    Partial,
    Unavailable,
    Denied,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootSection<T> {
    pub status: BootSectionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
}

impl<T> BootSection<T> {
    pub fn ok(data: T) -> Self {
        Self {
            status: BootSectionStatus::Ok,
            reason: None,
            data: Some(data),
        }
    }

    pub fn with_reason(
        status: BootSectionStatus,
        reason: impl Into<String>,
        data: Option<T>,
    ) -> Self {
        let reason = reason.into();
        Self {
            status,
            reason: if reason.is_empty() {
                None
            } else {
                Some(reason)
            },
            data,
        }
    }
}

impl<T> Default for BootSection<T> {
    fn default() -> Self {
        Self {
            status: BootSectionStatus::Unavailable,
            reason: None,
            data: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootTimingData {
    pub channel_enabled: bool,
    pub samples: Vec<BootTimingSample>,
    pub issues: Vec<BootIssue>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootTimingSample {
    /// RFC3339，来自事件 100 的 TimeCreated。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub booted_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boot_time_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub main_path_boot_time_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_boot_time_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degradation_delta_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_degradation: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub startup_app_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phases: Option<BootPhaseBreakdown>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootPhaseBreakdown {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kernel_init_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub driver_init_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub devices_init_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefetch_init_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub smss_init_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub critical_services_init_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_profile_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub machine_profile_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explorer_init_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BootIssueKind {
    Application,
    Driver,
    Service,
    Device,
    MachinePolicy,
    UserPolicy,
    BackgroundOptimization,
    SessionManager,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootIssue {
    pub event_id: u16,
    pub kind: BootIssueKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub friendly_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_time_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degradation_time_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BootStartupKind {
    Run,
    RunOnce,
    StartupFolder,
    ServiceAuto,
    ScheduledTaskBoot,
    ScheduledTaskLogon,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BootStartupScope {
    User,
    Machine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BootApprovedState {
    Enabled,
    Disabled,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootStartupEntry {
    pub id: String,
    pub name: String,
    pub command: String,
    pub kind: BootStartupKind,
    pub scope: BootStartupScope,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approved_state: Option<BootApprovedState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delayed_auto_start: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootStartupData {
    pub entries: Vec<BootStartupEntry>,
    /// 枚举失败的子源（如 tasks_dir_denied），用于把"0 项"与"读不到"区分开。
    pub failed_sources: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootResidueData {
    pub entries: Vec<BootResidueEntry>,
    /// 因信息不足（如 MSI 安装位置未知）而无法判定的条目数，不冒充残留。
    pub unknown_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootResidueEntry {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub scope: BootStartupScope,
    /// "x64" / "x86"
    pub arch: String,
    pub is_msi: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub msi_product_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uninstall_command: Option<String>,
    pub missing_install_dir: bool,
    pub missing_uninstaller: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootPowerData {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fast_startup_enabled: Option<bool>,
    pub fast_startup_registry_present: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hibernate_supported: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hiber_file_present: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hiberboot_capability: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modern_standby: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootDiagnosisReport {
    pub collected_at: String,
    pub is_elevated: bool,
    pub timing: BootSection<BootTimingData>,
    pub startup: BootSection<BootStartupData>,
    pub residue: BootSection<BootResidueData>,
    pub power: BootSection<BootPowerData>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boot_diagnosis_serializes_camel_case_and_omits_none() {
        // 状态/字段命名与 ipc.ts 镜像一致；None 字段不出现在 JSON 中。
        let report = BootDiagnosisReport {
            collected_at: "2026-11-10T00:00:00Z".into(),
            is_elevated: false,
            timing: BootSection::with_reason(BootSectionStatus::Denied, "事件日志访问被拒绝", None),
            startup: BootSection::ok(BootStartupData::default()),
            residue: BootSection::ok(BootResidueData::default()),
            power: BootSection::ok(BootPowerData {
                fast_startup_enabled: Some(true),
                fast_startup_registry_present: true,
                hibernate_supported: Some(true),
                hiber_file_present: Some(false),
                hiberboot_capability: Some(true),
                modern_standby: Some(false),
            }),
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"collectedAt\""));
        assert!(json.contains("\"status\":\"denied\""));
        assert!(json.contains("\"fastStartupEnabled\":true"));
        assert!(json.contains("\"entries\":[]"));
        assert!(!json.contains("\"data\":null"));
    }

    #[test]
    fn boot_startup_kind_uses_snake_case() {
        let json = serde_json::to_string(&BootStartupKind::ScheduledTaskLogon).unwrap();
        assert_eq!(json, "\"scheduled_task_logon\"");
        let json = serde_json::to_string(&BootStartupKind::Run).unwrap();
        assert_eq!(json, "\"run\"");
    }

    #[test]
    fn app_meta_serializes_camel_case() {
        let meta = AppMeta {
            version: "0.1.0".into(),
            rules_version: "0".into(),
            channel: Channel::Github,
            // v0.1.2 加性：None 时序列化省略（旧消费者不受影响）。
            is_elevated: Some(true),
        };
        let json = serde_json::to_string(&meta).unwrap();
        assert!(json.contains("\"rulesVersion\""));
        assert!(json.contains("\"channel\":\"github\""));
        assert!(json.contains("\"isElevated\":true"));
    }

    #[test]
    fn app_meta_roundtrip_deserialize() {
        // 无 isElevated 的旧载荷兼容（Option 缺省 None）。
        let json = r#"{"version":"0.1.0","rulesVersion":"1","channel":"github"}"#;
        let meta: AppMeta = serde_json::from_str(json).unwrap();
        assert_eq!(meta.channel, Channel::Github);
        assert_eq!(meta.is_elevated, None);
    }
}
