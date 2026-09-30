/**
 * mock 命令分发：VITE_MOCK=true 时模拟内核行为。
 * P0-04 将补充覆盖 6 扫描维度的完整假数据；此处先提供契约对齐的空/默认 stub，
 * 保证 api/commands.ts 在 mock 模式可 typecheck 并返回合法类型。
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
  RestoreReport,
  ScanGetItemsParams,
  ScanItem,
  ScanProfile,
  StartupEntry,
  UpdateStatus,
  LogExportParams,
} from "../types/ipc";
import { QuarantineListStub, StartupListStub, buildMockScanResult } from "./data";

export const mockCommands = {
  app_meta: (): Promise<AppMeta> => Promise.resolve({ version: "0.1.0", rulesVersion: "0", channel: "github" }),

  scan_start: (_profile: ScanProfile): Promise<string> => {
    const result = buildMockScanResult();
    return Promise.resolve(result.scanId);
  },

  scan_cancel: (_scanId: string): Promise<boolean> => Promise.resolve(true),

  scan_get_items: (params: ScanGetItemsParams): Promise<ScanItem[]> => {
    const result = buildMockScanResult();
    const start = params.offset;
    const end = start + params.limit;
    return Promise.resolve(result.items.slice(start, end));
  },

  clean_execute: (_params: CleanExecuteParams): Promise<string> => Promise.resolve("mock-tx-0001"),

  clean_cancel: (_txId: string): Promise<boolean> => Promise.resolve(true),

  quarantine_list: (): Promise<QuarantineEntry[]> => Promise.resolve(QuarantineListStub),

  quarantine_restore: (_params: QuarantineRestoreParams): Promise<RestoreReport> =>
    Promise.resolve({ requested: 0, restored: 0, conflict: 0, failures: [] }),

  quarantine_purge: (_params: { ids: string[]; confirmToken: string }): Promise<PurgeReport> =>
    Promise.resolve({ requested: 0, purged: 0, failures: [] }),

  startup_list: (): Promise<StartupEntry[]> => Promise.resolve(StartupListStub),

  startup_toggle: (params: { id: string; enabled: boolean }): Promise<boolean> => {
    // mock 有状态翻转（dev 走查用）：直接改 stub 条目，同会话内 list 可见变化。
    const e = StartupListStub.find((x) => x.id === params.id);
    if (!e) return Promise.resolve(false);
    e.enabled = params.enabled;
    return Promise.resolve(true);
  },

  log_query: (_filter: LogQueryFilter): Promise<LogEntry[]> => Promise.resolve([]),

  log_export: (_params: LogExportParams): Promise<boolean> => Promise.resolve(true),

  settings_get: (): Promise<AppSettings> =>
    Promise.resolve({
      quarantineRetentionDays: 14,
      quarantineAutoPurge: true,
      updateOptIn: false,
      crashUploadOptIn: false,
      expertMode: false,
      mirrorFirst: true,
    }),

  settings_set: (s: AppSettings): Promise<AppSettings> => Promise.resolve(s),

  update_check: (_manual: boolean): Promise<UpdateStatus> =>
    Promise.resolve({
      currentVersion: "0.1.0",
      hasUpdate: false,
      channel: "github",
      checkedAt: Date.now(),
    }),
};