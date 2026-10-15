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
import { InstalledAppStub, QuarantineListStub, StartupListStub, buildMockScanResult } from "./data";

/** R27-3 mock 状态：最近一次被卸载的程序（供残留扫描造样本）。 */
let lastRemovedApp: { name: string; installLocation: string } | null = null;

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

  uninstall_list: (): Promise<InstalledApp[]> => Promise.resolve(InstalledAppStub),

  // R30：mock 下无法真正调起 Explorer，返回成功即可走查交互。
  shell_reveal: (_path: string): Promise<null> => Promise.resolve(null),

  // R27-2：mock 有状态（dev 走查）——执行后从 stub 移除，重查列表可见条目消失。
  // 进度/完成事件由页面 mock 分支按阶段模拟（与 scan store mock 同约定）。
  uninstall_run: (appId: string): Promise<string> => {
    const idx = InstalledAppStub.findIndex((a) => a.id === appId);
    if (idx < 0) return Promise.reject(new Error("该程序已不在可卸载列表中（可能已卸载）"));
    lastRemovedApp = {
      name: InstalledAppStub[idx].name,
      installLocation: InstalledAppStub[idx].installLocation ?? "",
    };
    InstalledAppStub.splice(idx, 1);
    return Promise.resolve(`uninstall-mock-${Date.now()}`);
  },

  uninstall_cancel: (_txId: string): Promise<boolean> => Promise.resolve(true),

  // R27-3：mock 残留扫描（基于最近一次 mock 卸载的程序造三源样本）。
  uninstall_residue_scan: (): Promise<import("../types/ipc").ResidueCandidate[]> => {
    if (!lastRemovedApp) {
      return Promise.reject(new Error("请先完成一次卸载，再扫描其残留"));
    }
    const { name, installLocation } = lastRemovedApp;
    const loc = installLocation || `C:\\Program Files\\${name}`;
    const item = (
      id: string,
      path: string,
      size: number,
    ): import("../types/ipc").ScanItem => ({
      id,
      categoryId: "residue.uninstall",
      label: path.split("\\").pop() ?? path,
      path,
      sizeBytes: size,
      grade: "yellow",
      disposition: "quarantine",
      reason: "卸载后残留（mock）",
    });
    return Promise.resolve([
      {
        id: "mock-res-install",
        kind: "install_dir",
        appName: name,
        rootPath: loc,
        reason: `官方卸载器已运行且卸载条目已消失，但登记的安装目录仍存在（2 个文件）`,
        sizeBytes: 4096 + 2048,
        fileCount: 2,
        items: [
          item("mock-res-f1", `${loc}\\app.dll`, 4096),
          item("mock-res-f2", `${loc}\\lang\\zh.txt`, 2048),
        ],
      },
      {
        id: "mock-res-appdata",
        kind: "app_data",
        appName: name,
        rootPath: `C:\\Users\\tester\\AppData\\Roaming\\${name}`,
        reason: `AppData\\Roaming 下仍存在与「${name}」名称一致的配置目录（1 个文件）`,
        sizeBytes: 512,
        fileCount: 1,
        items: [item("mock-res-f3", `C:\\Users\\tester\\AppData\\Roaming\\${name}\\prefs.ini`, 512)],
      },
    ]);
  },

  uninstall_residue_clean: (
    params: import("../types/ipc").ResidueCleanParams,
  ): Promise<string> => {
    if (params.selections.length === 0) return Promise.reject(new Error("未选择任何残留项"));
    return Promise.resolve(`residue-mock-${Date.now()}`);
  },

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