/**
 * IPC 业务命令封装。全部 16 个命令在此统一暴露。
 * mock 模式（VITE_MOCK=true）下转发到 mock 服务，否则走真实 invoke。
 */
import type {
  AppMeta,
  AppSettings,
  CleanExecuteParams,
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

export const scan_start = (profile: ScanProfile) =>
  useMock() ? mockCommands.scan_start(profile) : invokeApp<string>("scan_start", { profile });

export const scan_cancel = (scanId: string) =>
  useMock() ? mockCommands.scan_cancel(scanId) : invokeApp<boolean>("scan_cancel", { scanId });

export const scan_get_items = (params: ScanGetItemsParams) =>
  useMock()
    ? mockCommands.scan_get_items(params)
    : invokeApp<ScanItem[]>("scan_get_items", params);

export const clean_execute = (params: CleanExecuteParams) =>
  useMock() ? mockCommands.clean_execute(params) : invokeApp<string>("clean_execute", params);

export const clean_cancel = (txId: string) =>
  useMock() ? mockCommands.clean_cancel(txId) : invokeApp<boolean>("clean_cancel", { txId });

export const quarantine_list = () =>
  useMock()
    ? mockCommands.quarantine_list()
    : invokeApp<QuarantineEntry[]>("quarantine_list");

export const quarantine_restore = (params: QuarantineRestoreParams) =>
  useMock()
    ? mockCommands.quarantine_restore(params)
    : invokeApp<RestoreReport>("quarantine_restore", params);

export const quarantine_purge = (params: { ids: string[]; confirmToken: string }) =>
  useMock()
    ? mockCommands.quarantine_purge(params)
    : invokeApp<PurgeReport>("quarantine_purge", params);

export const quarantine_status = () =>
  useMock()
    ? mockCommands.quarantine_status()
    : invokeApp<QuarantineStatus>("quarantine_status");

export const startup_list = () =>
  useMock() ? mockCommands.startup_list() : invokeApp<StartupEntry[]>("startup_list");

export const startup_toggle = (params: { id: string; enabled: boolean }) =>
  useMock()
    ? mockCommands.startup_toggle(params)
    : invokeApp<boolean>("startup_toggle", params);

export const log_query = (filter: LogQueryFilter) =>
  useMock() ? mockCommands.log_query(filter) : invokeApp<LogEntry[]>("log_query", filter);

export const log_export = (params: LogExportParams) =>
  useMock() ? mockCommands.log_export(params) : invokeApp<boolean>("log_export", params);

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