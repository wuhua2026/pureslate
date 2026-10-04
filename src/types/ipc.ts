/**
 * PureSlate IPC 契约（TS 端 · 唯一事实源）
 *
 * 本文件与 src-tauri/src/contract.rs 为镜像两端，M0 后视为冻结。
 * 任何修改须：1) 双端同步；2) 记 docs/dev/CHANGELOG.md；3) 新增字段须 optional。
 * 组件内禁止重复定义类型，一律从此处导入。
 */

// ---- 基础枚举 ----
export type Grade = "green" | "yellow" | "red";
export type Disposition = "direct" | "recycle" | "quarantine";
export type ScanDimension = "temp" | "large" | "dup" | "cache" | "startup" | "privacy";
export type QuarantineState = "quarantined" | "restored" | "purged";
export type StartupSource = "hkcu_run" | "hklm_run" | "startup_folder" | "task_scheduler";
export type Impact = "high" | "medium" | "low";
export type Channel = "github" | "mirror";
export type ScanPhase = "walking" | "hashing" | "aggregating";

// ---- 扫描 ----
export interface ScanProfile {
  dimensions: Partial<Record<ScanDimension, boolean>>;
}

export interface ScanItem {
  id: string; // sha1(categoryId + path) 前 16 位
  categoryId: string;
  label: string;
  path: string;
  sizeBytes: number;
  grade: Grade;
  disposition: Disposition; // 去向预告
  reason: string;
  mtime?: number;
  atime?: number; // epoch ms
  dupGroup?: string;
}

export interface CategoryAggregate {
  categoryId: string;
  label: string;
  grade: Grade;
  disposition: Disposition;
  totalBytes: number;
  itemCount: number;
  reason: string;
}

export interface FoundBytes {
  green: number;
  yellow: number;
  red: number;
}

export interface ScanResult {
  scanId: string;
  startedAt: number;
  finishedAt: number;
  volume: string; // 如 C:
  aggregates: CategoryAggregate[];
  itemCount: number;
  totalBytes: FoundBytes;
  /** F-2（P4-02 加性）：whitelist.xml 加载失败时为 false——UI 须在清理前明示降级。 */
  whitelistOk?: boolean;
}

// ---- 隔离区 ----
export interface QuarantineEntry {
  id: string;
  originalPath: string;
  sizeBytes: number;
  grade: Grade;
  categoryId: string;
  movedAt: number;
  expiresAt: number;
  daysLeft: number;
  state: QuarantineState;
}

export interface RestoreFailure {
  id: string;
  reason: string;
}

export interface RestoreReport {
  requested: number;
  restored: number;
  conflict: number;
  failures: RestoreFailure[];
}

export interface PurgeReport {
  requested: number;
  purged: number;
  failures: RestoreFailure[];
}

/** 隔离区容量状态（P3-06 加性新增，`quarantine_status` 返回）。 */
export interface QuarantineStatus {
  usedBytes: number;
  quotaBytes: number;
  overQuota: boolean;
  /** 超限时"释放最早批次"建议 id（不超限为空）。 */
  earliestBatchIds: string[];
}

// ---- 启动项 ----
export interface StartupEntry {
  id: string;
  name: string;
  publisher?: string;
  command: string;
  source: StartupSource;
  impact: Impact;
  enabled: boolean;
}

// ---- 更新 ----
export interface UpdateStatus {
  currentVersion: string;
  latestVersion?: string;
  hasUpdate: boolean;
  rulesPackHashOk?: boolean;
  channel: Channel;
  checkedAt: number;
}

// ---- 崩溃安全（R24 · P4-03 加性新增，SPEC §6.6） ----
export interface CrashDumpInfo {
  fileName: string;
  sizeBytes: number;
  /** 文件修改时间（epoch ms）。 */
  ts: number;
}

export interface CrashDumpPreview {
  fileName: string;
  sizeBytes: number;
  ts: number;
  moduleCount?: number;
  /** 模块基名预览（上限见内核 parse::PREVIEW_MODULE_CAP）。 */
  modules?: string[];
  /** 异常代码（panic 通道 dump 无异常流 → undefined）。 */
  exceptionCode?: number;
  error?: string;
}

export interface CrashUploadReport {
  fileName: string;
  uploaded: boolean;
  detail?: string;
}

/** 启动孤儿 journal 恢复报告（T-6 协议；ranAt=0 表示尚未运行）。 */
export interface CrashRecoveryReport {
  ranAt: number;
  orphansFound: number;
  restored: number;
  conflictRestored: number;
  adopted: number;
  irreversible: number;
  untouched: number;
  failures: string[];
}

// ---- 日志 ----
export interface LogQueryFilter {
  from: number;
  to: number;
  op?: string;
}

export interface LogEntry {
  ts: number;
  op: string;
  txId?: string;
  categoryId?: string;
  path?: string;
  sizeBytes?: number;
  disposition?: Disposition;
  result?: string;
  detail?: string;
}

// ---- 设置 ----
export interface AppSettings {
  quarantineRetentionDays: number; // 默认 14，范围 7–30
  quarantineAutoPurge: boolean; // 默认 true
  updateOptIn: boolean; // 默认 false
  crashUploadOptIn: boolean; // 默认 false
  expertMode: boolean; // 默认 false
  mirrorFirst: boolean; // 默认 true
}

// ---- 应用信息 ----
export interface AppMeta {
  version: string;
  rulesVersion: string;
  channel: Channel;
}

// ---- 事件 payload ----
export interface ScanProgressEvent {
  scanId: string;
  phase: ScanPhase;
  percent: number;
  currentPath: string;
  foundBytes: FoundBytes;
  elapsedMs: number;
}

export interface CleanProgressEvent {
  txId: string;
  itemPath: string;
  disposition: Disposition;
  doneBytes: number;
  totalBytes: number;
  state: "ok" | "fail" | "skip";
}

export interface QuarantineExpiryWarningEvent {
  ids: string[];
  daysLeft: number;
}

export interface CleanDoneEvent {
  txId: string;
  total: number;
  ok: number;
  fail: number;
  skip: number;
}

// ---- 命令参数（IPC 命令用 snake_case，TS 端参数对象用 camelCase，由 api 层转换）----
export interface ScanGetItemsParams {
  scanId: string;
  offset: number;
  limit: number;
  filter?: { grade?: Grade; categoryId?: string };
}

export interface CleanExecuteParams {
  items: string[];
  confirmToken?: string;
}

export interface QuarantineRestoreParams {
  ids: string[];
}

export interface QuarantinePurgeParams {
  ids: string[];
  confirmToken: string;
}

export interface StartupToggleParams {
  id: string;
  enabled: boolean;
}

export interface CrashPreviewParams {
  fileName: string;
}

export interface CrashUploadParams {
  fileName: string;
}

export interface LogExportParams {
  path: string;
}

export interface UpdateCheckParams {
  manual: boolean;
}

// ---- 事件名常量（防止拼写漂移）----
export const IPCEvents = {
  scanProgress: "scan_progress",
  scanDone: "scan_done",
  cleanProgress: "clean_progress",
  cleanDone: "clean_done",
  quarantineExpiryWarning: "quarantine_expiry_warning",
  updateAvailable: "update_available",
} as const;