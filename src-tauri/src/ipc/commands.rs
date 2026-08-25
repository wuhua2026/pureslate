//! IPC 命令 handler（薄层，调内部模块，不含业务）。
//! M0 阶段全部注册为 stub；P1-05 已实现 scan_start/scan_cancel/scan_get_items 业务，
//! 其余命令保持 stub（对应 Phase 2/3 填充）。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tauri::{Emitter, Manager, State};

use crate::contract::*;
use crate::scanner::walk::CancelToken;
use crate::state::{AppState, ScanContext};

/// 已生成的扫描序号（保证 scanId 单调可区分）。
static SCAN_SEQ: AtomicU64 = AtomicU64::new(0);

/// 应用元信息（首个打通命令）。
#[tauri::command]
pub fn app_meta(state: State<AppState>) -> AppMeta {
    AppMeta {
        version: state.version.clone(),
        rules_version: state.rules_version.clone(),
        channel: Channel::Github,
    }
}

/// 启动扫描（异步）：登记会话 → 后台线程执行各维度 → 事件推送进度/完成。
#[tauri::command]
pub async fn scan_start(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    profile: ScanProfile,
) -> Result<String, String> {
    let scan_id = new_scan_id();
    let started_at = now_ms();
    let cancel = CancelToken::new();

    {
        let Ok(mut scans) = state.scans.lock() else {
            return Err("扫描状态不可用".into());
        };
        scans.begin(ScanContext {
            scan_id: scan_id.clone(),
            started_at,
            cancel: cancel.clone(),
            ..Default::default()
        });
    }

    let rules_dir = resolve_rules_dir(&app);
    let app2 = app.clone();
    let cancel2 = cancel.clone();
    let scan_id2 = scan_id.clone();
    let started2 = started_at;

    tauri::async_runtime::spawn_blocking(move || {
        do_scan(&app2, &rules_dir, &scan_id2, &profile, started2, &cancel2);
    });

    Ok(scan_id)
}

/// 取消扫描：定位会话并置位取消令牌（只读，随时可停）。
#[tauri::command]
pub fn scan_cancel(state: State<AppState>, scan_id: String) -> bool {
    let Ok(scans) = state.scans.lock() else {
        return false;
    };
    match scans.by_id(&scan_id) {
        Some(ctx) => {
            ctx.cancel.cancel();
            true
        }
        None => false,
    }
}

/// 分页获取扫描项；可按 grade / categoryId 过滤。
#[tauri::command]
pub fn scan_get_items(state: State<AppState>, params: ScanGetItemsParams) -> Vec<ScanItem> {
    let Ok(scans) = state.scans.lock() else {
        return vec![];
    };
    let Some(ctx) = scans.by_id(&params.scan_id) else {
        return vec![];
    };
    let filtered: Vec<&ScanItem> = ctx
        .items
        .iter()
        .filter(|it| match &params.filter {
            None => true,
            Some(f) => {
                let grade_ok = f.grade.is_none_or(|g| it.grade == g);
                let cat_ok = f.category_id.as_deref().is_none_or(|c| it.category_id == c);
                grade_ok && cat_ok
            }
        })
        .collect();
    filtered
        .into_iter()
        .skip(params.offset as usize)
        .take(params.limit as usize)
        .cloned()
        .collect()
}

// ---- 内部辅助（扫描编排） ----

/// 后台执行扫描：加载规则 → 白名单 → 逐维度 → 节流推送进度 → 落结果 → 推送完成。
fn do_scan(
    app: &tauri::AppHandle,
    rules_dir: &Path,
    scan_id: &str,
    profile: &ScanProfile,
    started_at: i64,
    cancel: &CancelToken,
) {
    let app_state = app.state::<AppState>();

    // 规则加载失败：不中断流程，推送空结果让前端安全收场。
    let mut loader = crate::rules::loader::RuleLoader::new();
    let table = match loader.load_dir(rules_dir) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("[scan] 规则加载失败，返回空结果: {e}");
            finish_empty(app, app_state, scan_id, started_at);
            return;
        }
    };
    let _ = crate::safety::whitelist::load_from_dir(rules_dir);

    let mut last_emit = Instant::now();
    let throttle = Duration::from_millis(200);
    let mut progress = move |phase, done, total, path, found_bytes| {
        let emit_now = done >= total || last_emit.elapsed() >= throttle;
        if !emit_now {
            return;
        }
        last_emit = Instant::now();
        let percent = if total == 0 {
            100.0
        } else {
            (done as f64 / total as f64 * 100.0).min(100.0)
        };
        let evt = ScanProgressEvent {
            scan_id: scan_id.to_string(),
            phase,
            percent,
            current_path: path,
            found_bytes,
            elapsed_ms: now_ms() - started_at,
        };
        let _ = app.emit(events::SCAN_PROGRESS, evt);
    };

    let outcome = crate::scanner::engine::run_scan(&table, profile, cancel, &mut progress);

    let finished_at = now_ms();
    let result = ScanResult {
        scan_id: scan_id.to_string(),
        started_at,
        finished_at,
        volume: outcome.volume,
        aggregates: outcome.aggregates,
        item_count: outcome.items.len() as u64,
        total_bytes: outcome.found,
    };
    let items = outcome.items;

    if let Ok(mut scans) = app_state.scans.lock() {
        scans.finish(result.clone(), items);
    }
    let _ = app.emit(events::SCAN_DONE, result);
}

/// 规则加载失败时的收尾：仍以空结果登记并推送完成事件。
fn finish_empty(
    app: &tauri::AppHandle,
    app_state: tauri::State<AppState>,
    scan_id: &str,
    started_at: i64,
) {
    let now = now_ms();
    let result = ScanResult {
        scan_id: scan_id.to_string(),
        started_at,
        finished_at: now,
        volume: "C:".to_string(),
        aggregates: vec![],
        item_count: 0,
        total_bytes: FoundBytes {
            green: 0,
            yellow: 0,
            red: 0,
        },
    };
    if let Ok(mut scans) = app_state.scans.lock() {
        scans.finish(result.clone(), vec![]);
    }
    let _ = app.emit(events::SCAN_DONE, result);
}

/// 定位规则目录：优先资源目录（打包后），否则退回源码目录（开发态）。
fn resolve_rules_dir(app: &tauri::AppHandle) -> PathBuf {
    if let Ok(res) = app.path().resource_dir() {
        let in_res = res.join("rules");
        if in_res.is_dir() {
            return in_res;
        }
    }
    // 开发态：src-tauri/resources/rules
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/rules")
}

/// 生成单调可区分的 scanId（时间戳纳秒 + 序号，不引入 uuid 依赖）。
fn new_scan_id() -> String {
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SCAN_SEQ.fetch_add(1, AtomicOrdering::Relaxed);
    format!("scan-{t:020x}-{seq}")
}

/// 当前 epoch 毫秒（i64）。
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ---- 以下为 Phase 2/3 命令（stub） ----

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
    RestoreReport {
        requested: 0,
        restored: 0,
        conflict: 0,
        failures: vec![],
    }
}

/// 硬删隔离项（需 confirm_token）。
#[tauri::command]
pub fn quarantine_purge(_state: State<AppState>, _params: QuarantinePurgeParams) -> PurgeReport {
    PurgeReport {
        requested: 0,
        purged: 0,
        failures: vec![],
    }
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
