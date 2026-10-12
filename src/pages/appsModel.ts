/** Apps 页纯函数模型（R27-1 只读枚举）。 */
import type { InstalledApp } from "../types/ipc";

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
