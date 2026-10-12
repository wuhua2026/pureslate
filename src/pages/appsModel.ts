/** Apps 页纯函数模型（R27-1 只读枚举 / R27-2 执行流）。 */
import type { InstalledApp, UninstallResult } from "../types/ipc";

/** 名称/发布者/版本模糊过滤（大小写不敏感，去空白查询）。 */
export function filterApps(apps: InstalledApp[], query: string): InstalledApp[] {
  const q = query.trim().toLocaleLowerCase();
  if (!q) return apps;
  return apps.filter((a) =>
    [a.name, a.publisher ?? "", a.version ?? ""]
      .join(" ")
      .toLocaleLowerCase()
      .includes(q),
  );
}

export interface AppsSummary {
  total: number;
  machine: number;
  user: number;
  msi: number;
}

export function summarizeApps(apps: InstalledApp[]): AppsSummary {
  return {
    total: apps.length,
    machine: apps.filter((a) => a.scope === "machine").length,
    user: apps.filter((a) => a.scope === "user").length,
    msi: apps.filter((a) => a.isMsi).length,
  };
}

export function scopeLabel(s: InstalledApp["scope"]): string {
  return s === "machine" ? "本机" : "当前用户";
}

/** InstallDate 注册表原值 YYYYMMDD → YYYY-MM-DD；畸形原样返回空串。 */
export function installDateLabel(raw?: string): string {
  if (!raw || raw.length !== 8 || !/^\d{8}$/.test(raw)) return "";
  return `${raw.slice(0, 4)}-${raw.slice(4, 6)}-${raw.slice(6, 8)}`;
}

export function formatSize(bytes?: number): string {
  if (!bytes || bytes <= 0) return "";
  const mb = bytes / (1024 * 1024);
  if (mb >= 1024) return `${(mb / 1024).toFixed(2)} GB`;
  return `${mb.toFixed(1)} MB`;
}

// ---- R27-2 卸载执行 ----

/** 该条目是否可执行：必须登记了非空的交互卸载命令（SPEC §6.7）。 */
export function canUninstall(app: InstalledApp): boolean {
  return app.uninstallCommand.trim().length > 0;
}

export type ResultTone = "ok" | "info" | "warn" | "err";

export interface ResultMeta {
  tone: ResultTone;
  title: string;
}

/** 卸载结果 → 语气与标题（纯函数，供列表页结果态与单测固化）。 */
export function uninstallResultMeta(result: UninstallResult): ResultMeta {
  switch (result) {
    case "removed":
      return { tone: "ok", title: "卸载成功" };
    case "reboot_pending":
      return { tone: "info", title: "待重启后完成" };
    case "cancelled":
      return { tone: "info", title: "已停止等待" };
    case "still_present":
      return { tone: "warn", title: "程序仍在列表中" };
    case "timeout":
      return { tone: "warn", title: "等待超时" };
    case "launch_failed":
      return { tone: "err", title: "未能启动卸载器" };
  }
}
