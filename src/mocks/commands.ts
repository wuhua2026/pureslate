/**
 * mock 命令分发：VITE_MOCK=true 时模拟内核行为。
 * P0-04 将补充覆盖 6 扫描维度的完整假数据；此处先提供契约对齐的空/默认 stub，
 * 保证 api/commands.ts 在 mock 模式可 typecheck 并返回合法类型。
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
import { QuarantineListStub, StartupListStub, buildMockScanResult } from "./data";

export const mockCommands = {
  app_meta: (): Promise<AppMeta> =>
    Promise.resolve({ version: "0.1.4", rulesVersion: "0", channel: "github", isElevated: false }),

  // v0.1.2：mock 直接复用磁盘 stub（真实模式走 disk_usage 命令）。
  disk_usage: (): Promise<DiskUsageInfo[]> => import("./data").then((m) => m.DiskUsageStub),

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

  // P3-06：mock 有状态（dev 走查）——list 只回可管理项；restore/purge 翻转状态。
  quarantine_list: (): Promise<QuarantineEntry[]> =>
    Promise.resolve(QuarantineListStub.filter((q) => q.state === "quarantined")),

  quarantine_restore: (params: QuarantineRestoreParams): Promise<RestoreReport> => {
    const targets = QuarantineListStub.filter(
      (q) => q.state === "quarantined" && (params.ids.length === 0 || params.ids.includes(q.id)),
    );
    const failures = params.ids
      .filter((id) => !QuarantineListStub.some((q) => q.id === id))
      .map((id) => ({ id, reason: "未找到该条目" }));
    for (const q of targets) q.state = "restored";
    return Promise.resolve({
      requested: targets.length + failures.length,
      restored: targets.length,
      conflict: 0,
      failures,
    });
  },

  quarantine_purge: (params: { ids: string[]; confirmToken: string }): Promise<PurgeReport> => {
    if (!params.confirmToken.trim()) {
      return Promise.resolve({
        requested: params.ids.length,
        purged: 0,
        failures: params.ids.map((id) => ({ id, reason: "缺少确认令牌（需二次确认）" })),
      });
    }
    let purged = 0;
    const failures: { id: string; reason: string }[] = [];
    for (const q of QuarantineListStub) {
      if (!params.ids.includes(q.id)) continue;
      if (q.state === "quarantined") {
        q.state = "purged";
        purged += 1;
      } else {
        failures.push({ id: q.id, reason: "该条目已处理（非已隔离状态）" });
      }
    }
    for (const id of params.ids) {
      if (!QuarantineListStub.some((q) => q.id === id)) {
        failures.push({ id, reason: "未找到该条目" });
      }
    }
    return Promise.resolve({ requested: params.ids.length, purged, failures });
  },

  quarantine_status: (): Promise<QuarantineStatus> => {
    const used = QuarantineListStub.filter((q) => q.state === "quarantined").reduce(
      (acc, q) => acc + q.sizeBytes,
      0,
    );
    return Promise.resolve({
      usedBytes: used,
      quotaBytes: 5 * 1024 * 1024 * 1024,
      overQuota: false,
      earliestBatchIds: [],
    });
  },

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

  // I-2（P4-06）：mock 固定令牌（dev 走查 token 流）。
  confirm_token_issue: (): Promise<string> => Promise.resolve("PS-MOCK-MOCK"),

  // ---- R24 崩溃安全（P4-03）：mock 固定样本，供 dev 走查预览/上传/恢复横幅 UI ----

  crash_list: (): Promise<CrashDumpInfo[]> =>
    Promise.resolve([
      {
        fileName: "pureslate-crash-20261004-120000-1234.dmp",
        sizeBytes: 1_204_224,
        ts: Date.now() - 3_600_000,
      },
    ]),

  crash_preview: (params: CrashPreviewParams): Promise<CrashDumpPreview> =>
    Promise.resolve({
      fileName: params.fileName,
      sizeBytes: 1_204_224,
      ts: Date.now() - 3_600_000,
      moduleCount: 3,
      modules: ["ntdll.dll", "kernel32.dll", "pureslate.exe"],
      exceptionCode: 0xc0000005,
    }),

  crash_upload: (params: CrashUploadParams): Promise<CrashUploadReport> =>
    Promise.resolve({ fileName: params.fileName, uploaded: true }),

  crash_recovery: (): Promise<CrashRecoveryReport> =>
    Promise.resolve({
      ranAt: Date.now() - 60_000,
      orphansFound: 2,
      restored: 1,
      conflictRestored: 0,
      adopted: 1,
      irreversible: 0,
      untouched: 0,
      failures: [],
    }),
};