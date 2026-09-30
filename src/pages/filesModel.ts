/**
 * Files 页（大文件）数据模型——纯函数，无 DOM 依赖，可被 vitest 直接单测。
 * P3-01/R05：large 维度条目过滤 + 大小降序排序 + 展示格式化。
 */
import type { ScanItem } from "../types/ipc";

/** large 维度条目（categoryId 前缀 "large."）。 */
export function largeItems(items: ScanItem[]): ScanItem[] {
  return items.filter((i) => i.categoryId.startsWith("large."));
}

/** 大小降序，同大小按路径字典序（与引擎侧确定性排序一致）。 */
export function sortBySizeDesc(items: ScanItem[]): ScanItem[] {
  return [...items].sort((a, b) => b.sizeBytes - a.sizeBytes || a.path.localeCompare(b.path));
}

/** 字节数人类可读。 */
export function formatBytes(n: number): string {
  const KB = 1024;
  const MB = KB * 1024;
  const GB = MB * 1024;
  const TB = GB * 1024;
  if (n >= TB) return `${(n / TB).toFixed(1)} TB`;
  if (n >= GB) return `${(n / GB).toFixed(1)} GB`;
  if (n >= MB) return `${(n / MB).toFixed(0)} MB`;
  return `${(n / KB).toFixed(0)} KB`;
}

/** epoch 毫秒 → 本地日期（缺省 "—"）。 */
export function formatDay(ms?: number): string {
  if (!ms) return "—";
  return new Date(ms).toLocaleDateString("zh-CN", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
  });
}
