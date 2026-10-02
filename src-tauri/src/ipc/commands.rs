//! IPC 命令 handler（薄层，调内部模块，不含业务）。
//! M0 阶段全部注册为 stub；P1-05 已实现 scan_start/scan_cancel/scan_get_items 业务，
//! 其余命令保持 stub（对应 Phase 2/3 填充）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tauri::{Emitter, Manager, State};

use crate::cleaner::execute::resolve_targets;
use crate::contract::*;
use crate::scanner::walk::CancelToken;
use crate::state::{AppState, ScanContext};

/// 已生成的扫描序号（保证 scanId 单调可区分）。
static SCAN_SEQ: AtomicU64 = AtomicU64::new(0);

/// 清理事务取消令牌（tx_id -> CancelToken）。同刻至多一个清理事务。
static CLEAN_CANCELS: OnceLock<Mutex<HashMap<String, CancelToken>>> = OnceLock::new();

/// 取清理取消登记表（惰性初始化）。
fn clean_cancel_registry() -> std::sync::MutexGuard<'static, HashMap<String, CancelToken>> {
    CLEAN_CANCELS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

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

    let dirs = rules_dirs(&app);
    let app2 = app.clone();
    let cancel2 = cancel.clone();
    let scan_id2 = scan_id.clone();
    let started2 = started_at;

    tauri::async_runtime::spawn_blocking(move || {
        do_scan(&app2, &dirs, &scan_id2, &profile, started2, &cancel2);
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
    dirs: &[PathBuf],
    scan_id: &str,
    profile: &ScanProfile,
    started_at: i64,
    cancel: &CancelToken,
) {
    let app_state = app.state::<AppState>();

    // 规则加载失败：不中断流程，推送空结果让前端安全收场。
    let mut loader = crate::rules::loader::RuleLoader::new();
    let table = match loader.load_dirs(dirs) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("[scan] 规则加载失败，返回空结果: {e}");
            finish_empty(app, app_state, scan_id, started_at);
            return;
        }
    };
    // 白名单只随资源规则目录（更新包不含白名单）。
    // F-2（P4-02 安全审计）：加载失败不得静默——审计 + ScanResult.whitelistOk=false，
    // 清理前 UI 明示降级（常量根仍生效，XML 附加根丢失）。
    let whitelist_ok = match crate::safety::whitelist::load_from_dir(&dirs[0]) {
        Ok(()) => None,
        Err(e) => {
            eprintln!("[scan] whitelist.xml 加载失败（安全过滤降级）: {e}");
            let _ = crate::logging::audit::record(&crate::contract::LogEntry {
                ts: now_ms(),
                op: "scan".into(),
                tx_id: Some(scan_id.to_string()),
                category_id: None,
                path: Some(dirs[0].join("whitelist.xml").to_string_lossy().into_owned()),
                size_bytes: None,
                disposition: None,
                result: Some("fail".into()),
                detail: Some(format!("whitelist.xml 加载失败，XML 附加白名单未生效: {e}")),
            });
            Some(false)
        }
    };

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
        whitelist_ok,
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
        whitelist_ok: None,
    };
    if let Ok(mut scans) = app_state.scans.lock() {
        scans.finish(result.clone(), vec![]);
    }
    let _ = app.emit(events::SCAN_DONE, result);
}

/// 规则目录链（R22 更新包叠加语义）：资源规则目录 → `<data_root>\rules`（更新包安装目标，
/// 同 id category 覆盖资源目录）。资源目录优先打包目录，开发态回退源码目录。
fn rules_dirs(app: &tauri::AppHandle) -> Vec<PathBuf> {
    let base = if let Ok(res) = app.path().resource_dir() {
        let in_res = res.join("rules");
        if in_res.is_dir() {
            in_res
        } else {
            // 开发态：src-tauri/resources/rules
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/rules")
        }
    } else {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/rules")
    };
    vec![base, crate::updates::user_rules_dir()]
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

/// 执行清理。含 🔴 项而 confirm_token 缺失 → 拒绝（M0 语义落地，SAFETY §4.5）。
/// 返回 tx_id；后台执行并推送 `clean_progress`/`clean_done`。
#[tauri::command]
pub fn clean_execute(
    app: tauri::AppHandle,
    state: State<AppState>,
    params: CleanExecuteParams,
) -> Result<String, String> {
    // 1. 跨会话收集请求的扫描项（按 item.id）。
    let items = {
        let Ok(scans) = state.scans.lock() else {
            return Err("扫描状态不可用".into());
        };
        let mut out: Vec<ScanItem> = Vec::new();
        for ctx in scans.finished.iter().chain(scans.current.iter()) {
            for item in &ctx.items {
                if params.items.contains(&item.id) {
                    out.push(item.clone());
                }
            }
        }
        out
    };
    if items.is_empty() {
        return Err("无可清理项".into());
    }

    // 2. 规则表解析（guard/类目信息）。
    //    F-1 fail-closed（安全审计，P4-02）：规则加载失败 → 拒绝整个清理事务，
    //    不得"守卫退化为不查"（原实现此处为 fail-open，已修正）。
    let mut loader = crate::rules::loader::RuleLoader::new();
    let table = loader
        .load_dirs(&rules_dirs(&app))
        .map_err(|e| format!("规则加载失败，为安全起见已拒绝清理: {e}"))?;

    // 3. 解析为 CleanTarget（类目缺失同样 fail-closed，见 resolve_targets）。
    let targets = resolve_targets(&items, &params.items, &table)?;
    if targets.is_empty() {
        return Err("无可清理项".into());
    }

    // 4. 🔴 校验：含 Red 项须 confirm_token 非空（专家态 + 二次确认的落地点在 UI，此处强制兜底）。
    let has_red = targets.iter().any(|t| t.grade == Grade::Red);
    if has_red && params.confirm_token.as_deref().is_none_or(|s| s.is_empty()) {
        return Err("包含高风险（🔴）项，请二次确认后重试".into());
    }

    // 3. 生成 tx_id + 取消令牌并登记。
    let tx_id = new_clean_id();
    let cancel = CancelToken::new();
    clean_cancel_registry().insert(tx_id.clone(), cancel.clone());

    // 4. 取隔离保留期（quarantine 去向）→ spawn 后台执行。
    let retention = clean_retention(&state);
    let app2 = app.clone();
    let cancel2 = cancel.clone();
    let tx2 = tx_id.clone();

    tauri::async_runtime::spawn_blocking(move || {
        let mut last_emit = Instant::now();
        let throttle = Duration::from_millis(200);
        let app_emit = app2.clone();
        let tx_emit = tx2.clone();
        let mut progress = move |path: String,
                                 disp: Disposition,
                                 st0: CleanProgressState,
                                 done: u64,
                                 total: u64| {
            let emit_now = done >= total || last_emit.elapsed() >= throttle;
            if !emit_now {
                return;
            }
            last_emit = Instant::now();
            let evt = CleanProgressEvent {
                tx_id: tx_emit.clone(),
                item_path: path,
                disposition: disp,
                done_bytes: done,
                total_bytes: total,
                state: st0,
            };
            let _ = app_emit.emit(events::CLEAN_PROGRESS, evt);
        };

        let report =
            crate::cleaner::execute::execute(&tx2, &targets, &cancel2, retention, &mut progress);

        // 完成后清取消登记；推送 done。
        clean_cancel_registry().remove(&tx2);
        let _ = app2.emit(
            events::CLEAN_DONE,
            CleanDoneEvent {
                tx_id: tx2,
                total: report.total,
                ok: report.ok,
                fail: report.fail,
                skip: report.skip,
            },
        );
    });

    Ok(tx_id)
}

/// 取消清理：定位并置位取消令牌。
#[tauri::command]
pub fn clean_cancel(_state: State<AppState>, tx_id: String) -> bool {
    let cancels = clean_cancel_registry();
    match cancels.get(&tx_id) {
        Some(c) => {
            c.cancel();
            true
        }
        None => false,
    }
}

/// 取清理用隔离保留期（读当前设置；默认 14 天）。
fn clean_retention(state: &State<AppState>) -> u32 {
    state
        .settings
        .lock()
        .map(|s| s.quarantine_retention_days)
        .unwrap_or(14)
}

/// 生成唯一清理事务 id。
fn new_clean_id() -> String {
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("clean-{t:020x}")
}

/// 隔离区列表：先跑一遍生命周期（到期自动清除+审计+restored 行清理），
/// 有到期提醒则推 `quarantine_expiry_warning` 事件。
#[tauri::command]
pub fn quarantine_list(app: tauri::AppHandle, _state: State<AppState>) -> Vec<QuarantineEntry> {
    emit_lifecycle_warnings(&app);
    crate::quarantine::list_quarantined_globally()
}

/// 生命周期一遍 + 到期提醒事件（list 与启动钩子共用）。
pub fn emit_lifecycle_warnings(app: &tauri::AppHandle) {
    let report = crate::quarantine::lifecycle::run_pass_globally(crate::quarantine::now_ms());
    if !report.warning_ids.is_empty() {
        let _ = app.emit(
            events::QUARANTINE_EXPIRY_WARNING,
            QuarantineExpiryWarningEvent {
                ids: report.warning_ids,
                days_left: report.days_left_min,
            },
        );
    }
    if report.over_quota {
        eprintln!(
            "[quarantine] 容量超限：used={} quota={} earliest_batch={}",
            report.used_bytes,
            report.quota_bytes,
            report.earliest_batch_ids.len()
        );
    }
}

/// 隔离区容量状态（容量上限策略，SAFETY §4.4）。
#[tauri::command]
pub fn quarantine_status(_state: State<AppState>) -> QuarantineStatus {
    let r = crate::quarantine::lifecycle::run_pass_globally(crate::quarantine::now_ms());
    QuarantineStatus {
        used_bytes: r.used_bytes,
        quota_bytes: r.quota_bytes,
        over_quota: r.over_quota,
        earliest_batch_ids: r.earliest_batch_ids,
    }
}

/// 还原隔离项（按 id；ids 为空 = 还原全部可还原项）。
#[tauri::command]
pub fn quarantine_restore(
    _state: State<AppState>,
    params: QuarantineRestoreParams,
) -> RestoreReport {
    crate::quarantine::restore_globally(&params.ids)
}

/// 硬删隔离项（确认清空/释放最早批次；🔴 语义须 confirm_token，缺失全部拒绝）。
#[tauri::command]
pub fn quarantine_purge(_state: State<AppState>, params: QuarantinePurgeParams) -> PurgeReport {
    crate::quarantine::lifecycle::purge_globally(&params.ids, &params.confirm_token)
}

/// 启动项列表（活动项 + 已禁用备份项，按影响降序）。
#[tauri::command]
pub fn startup_list(_state: State<AppState>) -> Vec<StartupEntry> {
    crate::startup::list()
}

/// 启动项启停（禁用=备份后移除，不删源程序；启用=按备份还原）。
#[tauri::command]
pub fn startup_toggle(_state: State<AppState>, params: StartupToggleParams) -> bool {
    crate::startup::toggle(&params.id, params.enabled)
}

/// 查询审计日志（按 from/to/op 过滤、跨天合并）。
#[tauri::command]
pub fn log_query(state: State<AppState>, filter: LogQueryFilter) -> Vec<LogEntry> {
    let _ = state;
    crate::logging::audit::query(filter.from, filter.to, filter.op.as_deref())
}

/// 导出日志：全量审计日志 → 指定文件（JSONL）。返回是否成功。
#[tauri::command]
pub fn log_export(_state: State<AppState>, path: String) -> bool {
    crate::logging::audit::export_all(Path::new(&path)).is_ok()
}

/// 读取设置（从状态返回；状态在启动时已从 settings.json 恢复）。
#[tauri::command]
pub fn settings_get(state: State<AppState>) -> AppSettings {
    match state.settings.lock() {
        Ok(s) => s.clone(),
        Err(_) => AppSettings::default(),
    }
}

/// 写入设置（全量覆盖）并持久化到 settings.json。
#[tauri::command]
pub fn settings_set(state: State<AppState>, settings: AppSettings) -> AppSettings {
    // 优先持久化（信任底座：即使内存更新失败也不丢配置）。
    if let Err(e) = crate::storage::save_settings(&settings) {
        eprintln!("[settings] 持久化失败: {e}");
    }
    match state.settings.lock() {
        Ok(mut s) => *s = settings.clone(),
        Err(_) => eprintln!("[settings] 内存更新失败（锁不可用）"),
    }
    settings
}

/// 检查更新（R22 · SPEC §6.5）：手动检查无视 optIn；自动路径仅 optIn 开启时联网
/// （红线 #4：联网仅限 R22 更新检查）。规则包 sha256 校验失败即丢弃（rulesPackHashOk=false）。
#[tauri::command]
pub fn update_check(app: tauri::AppHandle, state: State<AppState>, manual: bool) -> UpdateStatus {
    let (opt_in, mirror_first) = match state.settings.lock() {
        Ok(s) => (s.update_opt_in, s.mirror_first),
        Err(_) => (false, true),
    };
    if !manual && !opt_in {
        return UpdateStatus {
            current_version: state.version.clone(),
            latest_version: None,
            has_update: false,
            rules_pack_hash_ok: None,
            channel: Channel::Github,
            checked_at: 0,
        };
    }
    let status = crate::updates::check(&state.version, &state.rules_version, mirror_first);
    crate::updates::record_last_check(crate::logging::audit::now_ms());
    if !manual && status.has_update {
        let _ = app.emit(events::UPDATE_AVAILABLE, status.clone());
    }
    status
}
