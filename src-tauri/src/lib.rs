// PureSlate 内核入口。M0 阶段：注册 IPC 契约 stub 与共享状态。

pub mod cleaner;
pub mod contract;
pub mod guard;
pub mod ipc;
pub mod logging;
pub mod quarantine;
pub mod rules;
pub mod safety;
pub mod scanner;
pub mod startup;
pub mod state;
pub mod storage;
pub mod updates;

use crate::state::AppState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            app.manage(AppState::new());
            // 启动即跑一遍隔离区生命周期（到期自动清除+审计；到期提醒事件推送）。
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                crate::ipc::commands::emit_lifecycle_warnings(&handle);
            });
            // R22 opt-in 周查：距上次检查 ≥7 天且开启 updateOptIn 才联网（红线 #4）。
            let handle2 = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let settings = crate::storage::load_settings();
                let now = crate::logging::audit::now_ms();
                if crate::updates::should_auto_check(
                    settings.update_opt_in,
                    crate::updates::read_last_check(),
                    now,
                ) {
                    let state = handle2.state::<AppState>();
                    let status = crate::updates::check(
                        &state.version,
                        &state.rules_version,
                        settings.mirror_first,
                    );
                    crate::updates::record_last_check(now);
                    if status.has_update {
                        use tauri::Emitter as _;
                        let _ = handle2.emit(crate::contract::events::UPDATE_AVAILABLE, status);
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ipc::commands::app_meta,
            ipc::commands::scan_start,
            ipc::commands::scan_cancel,
            ipc::commands::scan_get_items,
            ipc::commands::clean_execute,
            ipc::commands::clean_cancel,
            ipc::commands::quarantine_list,
            ipc::commands::quarantine_restore,
            ipc::commands::quarantine_purge,
            ipc::commands::quarantine_status,
            ipc::commands::startup_list,
            ipc::commands::startup_toggle,
            ipc::commands::log_query,
            ipc::commands::log_export,
            ipc::commands::settings_get,
            ipc::commands::settings_set,
            ipc::commands::update_check,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
