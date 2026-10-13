/**
 * IPC 业务命令封装。全部 16 个命令在此统一暴露。
 * mock 模式（VITE_MOCK=true）下转发到 mock 服务，否则走真实 invoke。
 */
import type {
  AppMeta,
  AppSettings,
  CleanExecuteParams,
  CrashDumpInfo,
  CrashDumpPreview,
  CrashPreviewParams,
  CrashRecoveryReport,
  CrashUploadParams,
  CrashUploadReport,
  DiskUsageInfo,
  InstalledApp,
  LogEntry,
  LogQueryFilter,
  PurgeReport,
  QuarantineEntry,
  QuarantineRestoreParams,
  QuarantineStatus,
  RestoreReport,
  ScanGetItemsParams,
  ScanItem,
  ScanProfile,
  StartupEntry,
  UpdateStatus,
  LogExportParams,
} from "../types/ipc";
import { invokeApp, __mockEnabled } from "./client";
import { mockCommands } from "../mocks/commands";

function useMock(): boolean {
  return __mockEnabled();
}

export const app_meta = () =>
  useMock() ? mockCommands.app_meta() : invokeApp<AppMeta>("app_meta");

// v0.1.2：固定磁盘容量枚举（首页磁盘条真实数据）。
export const disk_usage = () =>
  useMock() ? mockCommands.disk_usage() : invokeApp<DiskUsageInfo[]>("disk_usage");

export const scan_start = (profile: ScanProfile) =>
  useMock() ? mockCommands.scan_start(profile) : invokeApp<string>("scan_start", { profile });

export const scan_cancel = (scanId: string) =>
  useMock() ? mockCommands.scan_cancel(scanId) : invokeApp<boolean>("scan_cancel", { scanId });

export const scan_get_items = (params: ScanGetItemsParams) =>
  useMock()
    ? mockCommands.scan_get_items(params)
    // Tauri v2：结构体参数必须按 Rust 端参数名 `params` 包一层（平铺会报
    // missing required key params；mock 直调不经过此序列化，故此前未暴露）。
    : invokeApp<ScanItem[]>("scan_get_items", { params });

export const clean_execute = (params: CleanExecuteParams) =>
  useMock()
    ? mockCommands.clean_execute(params)
    : invokeApp<string>("clean_execute", { params });

export const clean_cancel = (txId: string) =>
  useMock() ? mockCommands.clean_cancel(txId) : invokeApp<boolean>("clean_cancel", { txId });

export const quarantine_list = () =>
  useMock()
    ? mockCommands.quarantine_list()
    : invokeApp<QuarantineEntry[]>("quarantine_list");

export const quarantine_restore = (params: QuarantineRestoreParams) =>
  useMock()
    ? mockCommands.quarantine_restore(params)
    : invokeApp<RestoreReport>("quarantine_restore", { params });

export const quarantine_purge = (params: { ids: string[]; confirmToken: string }) =>
  useMock()
    ? mockCommands.quarantine_purge(params)
    : invokeApp<PurgeReport>("quarantine_purge", { params });

export const quarantine_status = () =>
  useMock()
    ? mockCommands.quarantine_status()
    : invokeApp<QuarantineStatus>("quarantine_status");

export const startup_list = () =>
  useMock() ? mockCommands.startup_list() : invokeApp<StartupEntry[]>("startup_list");

/** R27-1：已安装程序只读列表（SPEC §6.7）。 */
export const uninstall_list = () =>
  useMock()
    ? mockCommands.uninstall_list()
    : invokeApp<InstalledApp[]>("uninstall_list");

/** R27-2：执行卸载编排；返回 txId，进度/完成经事件推送。 */
export const uninstall_run = (appId: string) =>
  useMock()
    ? mockCommands.uninstall_run(appId)
    : invokeApp<string>("uninstall_run", { appId });

/** R27-2：取消等待（不强杀卸载器进程）。 */
export const uninstall_cancel = (txId: string) =>
  useMock()
    ? mockCommands.uninstall_cancel(txId)
    : invokeApp<boolean>("uninstall_cancel", { txId });

/** R27-3：扫描最近一次卸载的残留（只读三源枚举）。 */
export const uninstall_residue_scan = () =>
  useMock()
    ? mockCommands.uninstall_residue_scan()
    : invokeApp<import("../types/ipc").ResidueCandidate[]>("uninstall_residue_scan");

/** R27-3：执行残留清理（去向 quarantine/direct；direct 需专家令牌）。 */
export const uninstall_residue_clean = (
  params: import("../types/ipc").ResidueCleanParams,
) =>
  useMock()
    ? mockCommands.uninstall_residue_clean(params)
    : invokeApp<string>("uninstall_residue_clean", { params });

export const startup_toggle = (params: { id: string; enabled: boolean }) =>
  useMock()
    ? mockCommands.startup_toggle(params)
    : invokeApp<boolean>("startup_toggle", { params });

export const log_query = (filter: LogQueryFilter) =>
  useMock()
    ? mockCommands.log_query(filter)
    : invokeApp<LogEntry[]>("log_query", { filter });

export const log_export = (params: LogExportParams) =>
  useMock()
    ? mockCommands.log_export(params)
    // 后端参数名是 path: String（非结构体），按名传字符串。
    : invokeApp<boolean>("log_export", { path: params.path });

export const settings_get = () =>
  useMock() ? mockCommands.settings_get() : invokeApp<AppSettings>("settings_get");

export const settings_set = (settings: AppSettings) =>
  useMock()
    ? mockCommands.settings_set(settings)
    : invokeApp<AppSettings>("settings_set", { settings });

export const update_check = (manual: boolean) =>
  useMock()
    ? mockCommands.update_check(manual)
    : invokeApp<UpdateStatus>("update_check", { manual });

// ---- I-2（P4-06）：🔴 二次确认令牌后端签发（一次性消费） ----

export const confirm_token_issue = () =>
  useMock()
    ? mockCommands.confirm_token_issue()
    : invokeApp<string>("confirm_token_issue");

// ---- R24 崩溃安全（P4-03） ----

export const crash_list = () =>
  useMock() ? mockCommands.crash_list() : invokeApp<CrashDumpInfo[]>("crash_list");

export const crash_preview = (params: CrashPreviewParams) =>
  useMock()
    ? mockCommands.crash_preview(params)
    : invokeApp<CrashDumpPreview>("crash_preview", { params });

export const crash_upload = (params: CrashUploadParams) =>
  useMock()
    ? mockCommands.crash_upload(params)
    : invokeApp<CrashUploadReport>("crash_upload", { params });

export const crash_recovery = () =>
  useMock()
    ? mockCommands.crash_recovery()
    : invokeApp<CrashRecoveryReport>("crash_recovery");