/**
 * 开机诊断页数据模型——纯函数，无 DOM 依赖，可被 vitest 直接单测。
 * B1-08：状态文案 / 指标格式化 / 阶段构成 / 启动项分类 / 失败来源翻译。
 */
import type {
  BootApprovedState,
  BootIssue,
  BootIssueKind,
  BootPhaseBreakdown,
  BootPowerData,
  BootSectionStatus,
  BootStartupEntry,
  BootStartupKind,
  BootStartupScope,
  BootTimingSample,
} from "../types/ipc";

/** 段状态中文名。 */
export function statusLabel(s: BootSectionStatus): string {
  switch (s) {
    case "ok":
      return "正常";
    case "partial":
      return "部分可用";
    case "unavailable":
      return "无法读取";
    case "denied":
      return "权限不足";
  }
}

/** 段状态色调（只用于状态，不用于装饰）。 */
export function statusTone(s: BootSectionStatus): "ok" | "warn" | "muted" | "denied" {
  switch (s) {
    case "ok":
      return "ok";
    case "partial":
      return "warn";
    case "unavailable":
      return "muted";
    case "denied":
      return "denied";
  }
}

/** 毫秒 → 可读时长；缺失一律 "—"（不显示 0，设计稿 §6.5）。 */
export function formatMs(ms?: number | null): string {
  if (ms === undefined || ms === null || !Number.isFinite(ms)) return "—";
  if (ms < 1000) return `${Math.round(ms)} ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)} s`;
  const min = Math.floor(ms / 60_000);
  const sec = Math.round((ms % 60_000) / 1000);
  return `${min} 分 ${sec} 秒`;
}

/** RFC3339(UTC) → "2026-11-08 23:12 UTC"；解析不了 "—"。 */
export function formatWhen(iso?: string): string {
  if (!iso || iso.length < 16) return "—";
  const date = iso.slice(0, 10);
  const time = iso.slice(11, 16);
  if (!/^\d{4}-\d{2}-\d{2}$/.test(date) || !/^\d{2}:\d{2}$/.test(time)) return "—";
  return `${date} ${time} UTC`;
}

/** 阶段字段（顺序即堆叠条顺序）。 */
export const PHASE_FIELDS: ReadonlyArray<{ key: keyof BootPhaseBreakdown; label: string }> = [
  { key: "kernelInitMs", label: "内核初始化" },
  { key: "driverInitMs", label: "驱动初始化" },
  { key: "devicesInitMs", label: "设备初始化" },
  { key: "prefetchInitMs", label: "预读取" },
  { key: "smssInitMs", label: "会话管理器" },
  { key: "criticalServicesInitMs", label: "关键服务" },
  { key: "machineProfileMs", label: "计算机配置" },
  { key: "userProfileMs", label: "用户配置" },
  { key: "explorerInitMs", label: "桌面初始化" },
];

export interface PhaseSegment {
  key: string;
  label: string;
  ms: number;
  /** 占**可得阶段**之和的百分比（不声称等于总耗时）。 */
  pct: number;
}

/** 可得阶段构成；缺失字段直接不出现，不做 0 填充。 */
export function phaseSegments(phases?: BootPhaseBreakdown): PhaseSegment[] {
  if (!phases) return [];
  const present = PHASE_FIELDS.filter((f) => {
    const v = phases[f.key];
    return v !== undefined && v !== null && Number.isFinite(v) && v > 0;
  });
  const sum = present.reduce((acc, f) => acc + (phases[f.key] as number), 0);
  if (sum <= 0) return [];
  return present.map((f) => {
    const ms = phases[f.key] as number;
    return { key: f.key, label: f.label, ms, pct: (ms / sum) * 100 };
  });
}

export function kindLabel(k: BootStartupKind): string {
  switch (k) {
    case "run":
      return "注册表 Run";
    case "run_once":
      return "注册表 RunOnce";
    case "startup_folder":
      return "启动文件夹";
    case "service_auto":
      return "自动服务";
    case "scheduled_task_boot":
      return "计划任务 · 开机";
    case "scheduled_task_logon":
      return "计划任务 · 登录";
  }
}

export function scopeLabel(s: BootStartupScope): string {
  return s === "user" ? "当前用户" : "本机";
}

/** StartupApproved 判定文案（仅 HKCU 有审批记录）。 */
export function approvedLabel(state?: BootApprovedState): string {
  switch (state) {
    case "enabled":
      return "已启用";
    case "disabled":
      return "系统已禁用";
    case "unknown":
      return "无审批记录";
    default:
      return "—";
  }
}

/** 系统自带任务（`Microsoft\...` 路径），默认折叠。 */
export function isSystemTask(e: BootStartupEntry): boolean {
  return (
    (e.kind === "scheduled_task_boot" || e.kind === "scheduled_task_logon") &&
    e.name.toLowerCase().startsWith("microsoft\\")
  );
}

/** 按来源统计（固定顺序，0 也显示，便于对照）。 */
export function sourceStats(
  entries: BootStartupEntry[],
): Array<{ kind: BootStartupKind; label: string; count: number }> {
  const kinds: BootStartupKind[] = [
    "run",
    "run_once",
    "startup_folder",
    "service_auto",
    "scheduled_task_boot",
    "scheduled_task_logon",
  ];
  return kinds.map((kind) => ({
    kind,
    label: kindLabel(kind),
    count: entries.filter((e) => e.kind === kind).length,
  }));
}

export function issueKindLabel(k: BootIssueKind): string {
  switch (k) {
    case "application":
      return "应用";
    case "driver":
      return "驱动";
    case "service":
      return "服务";
    case "device":
      return "设备";
    case "machinePolicy":
      return "计算机策略";
    case "userPolicy":
      return "用户策略";
    case "backgroundOptimization":
      return "后台优化";
    case "sessionManager":
      return "会话管理器";
    case "unknown":
      return "未知";
  }
}

/** 明细排序：降级耗时降序（缺失排后），再按总耗时、事件号。 */
export function sortedIssues(issues: BootIssue[]): BootIssue[] {
  const rank = (v?: number): number => (v === undefined || !Number.isFinite(v) ? -1 : v);
  return [...issues].sort(
    (a, b) =>
      rank(b.degradationTimeMs) - rank(a.degradationTimeMs) ||
      rank(b.totalTimeMs) - rank(a.totalTimeMs) ||
      a.eventId - b.eventId,
  );
}

/** 最新一次开机样本（事件按倒序取回，`samples[0]` 最新）。 */
export function latestSample(samples: BootTimingSample[]): BootTimingSample | undefined {
  return samples.length > 0 ? samples[0] : undefined;
}

/** 快速启动/休眠的矛盾态提示（黄色，不自动修复）。 */
export function powerContradiction(d?: BootPowerData): string | null {
  if (!d) return null;
  if (d.fastStartupEnabled === true && d.hiberFilePresent === false) {
    return "注册表记录快速启动已开启，但系统当前没有休眠文件——快速启动实际不会生效。";
  }
  if (d.fastStartupEnabled === true && d.hiberbootCapability === false) {
    return "注册表记录快速启动已开启，但系统/固件未报告支持快速启动（Hiberboot 能力位为否）。";
  }
  if (d.fastStartupRegistryPresent && d.fastStartupEnabled === undefined) {
    return "快速启动的注册表值存在，但类型或长度异常，无法判断开关状态。";
  }
  return null;
}

/** 失败子源 → 人话（内核侧命名见 bootdiag/*.rs 的 failed_sources）。 */
export function failedSourceLabel(source: string): string {
  if (source.startsWith("hkcu_")) return "当前用户注册表";
  if (source.startsWith("hklm_")) return "本机注册表";
  if (source.startsWith("startup_folder")) return "启动文件夹";
  if (source.startsWith("startup_approved")) return "自启审批状态";
  if (source.startsWith("services_scm_code_")) return `服务管理器（错误码 ${source.slice(18)}）`;
  if (source.startsWith("services_query_failed_"))
    return `部分服务配置读取失败（${source.slice(22)} 个）`;
  if (source === "tasks_dir_empty_or_denied") return "计划任务目录不可枚举（多为权限限制）";
  if (source.startsWith("tasks_walk_errors_")) return "计划任务目录部分不可读";
  if (source.startsWith("tasks_parse_failed_")) return "部分计划任务 XML 解析失败";
  if (source.startsWith("tasks_truncated_")) return "计划任务数量超上限，已截断";
  if (source === "tasks_root_missing") return "计划任务目录不存在";
  if (source.startsWith("msi_enum_code_")) return `MSI 产品登记枚举失败（错误码 ${source.slice(14)}）`;
  if (source.startsWith("power_registry_code_")) return "电源配置注册表读取失败";
  if (source.startsWith("power_caps_status_")) return "电源能力查询失败";
  if (source.startsWith("event_")) return "事件日志";
  return source;
}
