//! IPC 命令 handler（薄层，调内部模块，不含业务）。
//! M0 阶段全部注册为 stub，返回空/默认值；业务逻辑在后续 Phase 填充。

use crate::contract::*;
use crate::state::AppState;
use tauri::State;

/// 应用元信息（首个打通命令）。
#[tauri::command]
pub fn app_meta(state: State<AppState>) -> AppMeta {
    AppMeta {
        version: state.version.clone(),
        rules_version: state.rules_version.clone(),
        channel: Channel::Github,
    }
}

/// 启动扫描。P1 实现；stub 返回 mock scanId。
#[tauri::command]
pub fn scan_start(_state: State<AppState>, _profile: ScanProfile) -> String {
    "stub-scan-0001".into()
}

/// 取消扫描。
#[tauri::command]
pub fn scan_cancel(_state: State<AppState>, _scan_id: String) -> bool {
    true
}

/// 分页获取扫描项。
#[tauri::command]
pub fn scan_get_items(_state: State<AppState>, _params: ScanGetItemsParams) -> Vec<ScanItem> {
    vec![]
}

/// 执行清理。含 🔴 时 confirm_token 必填（P2 校验）。
#[tauri::command]
pub fn clean_execute(_state: State<AppState>, _params: CleanExecuteParams) -> String {
    "stub-tx-0001".into()
}

/// 取消清理。
#[tauri::command]
pub fn clean_cancel(_state: State<AppState>, _tx_id: String) -> bool {
    true
}

/// 隔离区列表。
#[tauri::command]
pub fn quarantine_list(_state: State<AppState>) -> Vec<QuarantineEntry> {
    vec![]
}

/// 还原隔离项。
#[tauri::command]
pub fn quarantine_restore(
    _state: State<AppState>,
    _params: QuarantineRestoreParams,
) -> RestoreReport {
    RestoreReport { requested: 0, restored: 0, conflict: 0, failures: vec![] }
}

/// 硬删隔离项（需 confirm_token）。
#[tauri::command]
pub fn quarantine_purge(
    _state: State<AppState>,
    _params: QuarantinePurgeParams,
) -> PurgeReport {
    PurgeReport { requested: 0, purged: 0, failures: vec![] }
}

/// 启动项列表。
#[tauri::command]
pub fn startup_list(_state: State<AppState>) -> Vec<StartupEntry> {
    vec![]
}

/// 启动项启停（禁用=备份后移除，不删源程序）。
#[tauri::command]
pub fn startup_toggle(_state: State<AppState>, _params: StartupToggleParams) -> bool {
    true
}

/// 查询审计日志。
#[tauri::command]
pub fn log_query(_state: State<AppState>, _filter: LogQueryFilter) -> Vec<LogEntry> {
    vec![]
}

/// 导出日志。
#[tauri::command]
pub fn log_export(_state: State<AppState>, _path: String) -> bool {
    true
}

/// 读取设置。
#[tauri::command]
pub fn settings_get(state: State<AppState>) -> AppSettings {
    state.settings.clone()
}

/// 写入设置（全量覆盖）。
#[tauri::command]
pub fn settings_set(_state: State<AppState>, settings: AppSettings) -> AppSettings {
    settings
}

/// 检查更新。
#[tauri::command]
pub fn update_check(_state: State<AppState>, _manual: bool) -> UpdateStatus {
    UpdateStatus {
        current_version: "0.1.0".into(),
        latest_version: None,
        has_update: false,
        rules_pack_hash_ok: None,
        channel: Channel::Github,
        checked_at: 0,
    }
}