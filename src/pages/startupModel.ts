/**
 * Startup 页数据模型——纯函数，无 DOM 依赖，可被 vitest 直接单测。
 * P3-03/R07：影响排序 + 启用/禁用分组 + 展示格式化 + 禁用风险/失败文案。
 */
import type { Impact, StartupEntry, StartupSource } from "../types/ipc";

/** 影响档位排序权重（高→中→低）。 */
export const IMPACT_RANK: Record<Impact, number> = { high: 0, medium: 1, low: 2 };

/** 影响降序；同档按名称字典序（确定性，与引擎侧排序约定一致）。 */
export function sortByImpactDesc(entries: StartupEntry[]): StartupEntry[] {
  return [...entries].sort(
    (a, b) => IMPACT_RANK[a.impact] - IMPACT_RANK[b.impact] || a.name.localeCompare(b.name),
  );
}

/** 按启用状态分组（已禁用 = 备份在 PureSlate，可随时恢复）。 */
export function splitByEnabled(entries: StartupEntry[]): {
  enabled: StartupEntry[];
  disabled: StartupEntry[];
} {
  return {
    enabled: entries.filter((e) => e.enabled),
    disabled: entries.filter((e) => !e.enabled),
  };
}

/** 来源中文名。 */
export function sourceLabel(s: StartupSource): string {
  switch (s) {
    case "hkcu_run":
      return "当前用户 · 注册表";
    case "hklm_run":
      return "本机 · 注册表";
    case "startup_folder":
      return "启动文件夹";
    case "task_scheduler":
      return "计划任务";
  }
}

/** 影响档位中文名（带"影响"字样，避免与安全分级 🟢🟡🔴 混淆）。 */
export function impactLabel(i: Impact): string {
  switch (i) {
    case "high":
      return "高影响";
    case "medium":
      return "中影响";
    case "low":
      return "低影响";
  }
}

/** 发布者缺省文案。 */
export function publisherLabel(p?: string): string {
  return p && p.trim() ? p : "未知发布者";
}

/** 禁用风险提示（确认面板文案）：明确"不删程序、可恢复"。 */
export function disableRiskText(e: StartupEntry): string {
  return `禁用后「${e.name}」不再开机自动运行；程序本身不会被删除，可随时在"已禁用"区恢复。`;
}

/** toggle 失败提示（HKLM 级需管理员，其余提示重试）。 */
export function toggleFailText(e: StartupEntry): string {
  return e.source === "hklm_run"
    ? `「${e.name}」位于本机级注册表，禁用/恢复需要管理员权限，未执行`
    : `「${e.name}」操作失败，请稍后重试`;
}
