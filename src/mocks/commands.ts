/**
 * mock 命令分发：VITE_MOCK=true 时模拟内核行为。
 * P0-04 将补充覆盖 6 扫描维度的完整假数据；此处先提供契约对齐的空/默认 stub，
 * 保证 api/commands.ts 在 mock 模式可 typecheck 并返回合法类型。
 */
import type {
  AppMeta,
  AppSettings,
  BootDiagnosisReport,
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

  // 开机诊断 mock：覆盖 ok / partial / denied 三种状态，供 UI 走查。
  boot_diagnose: (): Promise<BootDiagnosisReport> =>
    Promise.resolve({
      collectedAt: new Date().toISOString().replace(/\.\d{3}Z$/, "Z"),
      isElevated: false,
      timing: {
        status: "partial",
        reason: "事件日志通道未启用，以下为历史记录",
        data: {
          channelEnabled: false,
          truncated: false,
          samples: [
            {
              bootedAt: "2026-11-08T23:12:05Z",
              bootTimeMs: 34857,
              mainPathBootTimeMs: 12657,
              postBootTimeMs: 22200,
              degradationDeltaMs: 0,
              isDegradation: false,
              startupAppCount: 18,
              phases: {
                kernelInitMs: 4210,
                driverInitMs: 5120,
                devicesInitMs: 1880,
                prefetchInitMs: 940,
                smssInitMs: 210,
                criticalServicesInitMs: 2600,
                userProfileMs: 3400,
                machineProfileMs: 1200,
                explorerInitMs: 1700,
              },
            },
            {
              bootedAt: "2026-11-09T10:02:41Z",
              bootTimeMs: 41230,
              mainPathBootTimeMs: 14200,
              postBootTimeMs: 27030,
              degradationDeltaMs: 6373,
              isDegradation: true,
              startupAppCount: 20,
            },
          ],
          issues: [
            {
              eventId: 101,
              kind: "application",
              name: "OneDrive.exe",
              friendlyName: "Microsoft OneDrive",
              version: "24.1.0.0",
              totalTimeMs: 3200,
              degradationTimeMs: 2100,
              at: "2026-11-09T10:02:41Z",
            },
            {
              eventId: 102,
              kind: "driver",
              name: "\\SystemRoot\\System32\\drivers\\nvlddmkm.sys",
              friendlyName: "NVIDIA Windows Kernel Mode Driver",
              version: "32.0.15.6094",
              totalTimeMs: 2600,
              at: "2026-11-09T10:02:41Z",
            },
          ],
        },
      },
      startup: {
        status: "partial",
        reason: "1 个子源读取失败：tasks_dir_denied",
        data: {
          failedSources: ["tasks_dir_denied"],
          entries: [
            {
              id: "mock-run-hkcu-onedrive",
              name: "OneDrive",
              command: "C:\\Users\\me\\AppData\\Local\\Microsoft\\OneDrive\\OneDrive.exe /background",
              kind: "run",
              scope: "user",
              enabled: true,
              approvedState: "enabled",
            },
            {
              id: "mock-svc-autostart",
              name: "Windows Update",
              command: "C:\\WINDOWS\\system32\\svchost.exe -k netsvcs -p",
              kind: "service_auto",
              scope: "machine",
              enabled: true,
              delayedAutoStart: false,
              detail: "wuauserv",
            },
            {
              id: "mock-task-logon",
              name: "Vendor\\Updater",
              command: "C:\\Program Files\\Vendor\\updater.exe /silent",
              kind: "scheduled_task_logon",
              scope: "machine",
            },
          ],
        },
      },
      residue: {
        status: "ok",
        data: {
          unknownCount: 3,
          entries: [
            {
              id: "mock-residue-1",
              name: "OldTool 2.1",
              publisher: "Example Corp",
              version: "2.1.0",
              scope: "machine",
              arch: "x64",
              isMsi: false,
              installLocation: "C:\\Program Files\\OldTool",
              missingInstallDir: true,
              missingUninstaller: true,
              reason: "安装目录与卸载程序均不存在",
            },
          ],
        },
      },
      power: {
        status: "ok",
        data: {
          fastStartupEnabled: true,
          fastStartupRegistryPresent: true,
          hibernateSupported: true,
          hiberFilePresent: false,
          hiberbootCapability: true,
          modernStandby: false,
        },
      },
    }),

  uninstall_list: (): Promise<InstalledApp[]> => Promise.resolve(InstalledAppStub),

  // R30：mock 下无法真正调起 Explorer，返回成功即可走查交互。
  shell_reveal: (_path: string): Promise<null> => Promise.resolve(null),

  // R30：浏览器 mock 无法提取真实图标，返回空 map（走默认字形回退分支）。
  uninstall_app_icons: (_appIds: string[]): Promise<Record<string, string>> =>
    Promise.resolve({}),

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