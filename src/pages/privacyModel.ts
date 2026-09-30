/**
 * Privacy 页数据模型——纯函数，无 DOM 依赖，可被 vitest 直接单测。
 * P3-05/R08：privacy 维度条目过滤 + 类目分组 + 守卫/可恢复标注文案。
 */
import type { Disposition, ScanItem } from "../types/ipc";

/** privacy 维度条目（categoryId 前缀 "privacy."，与 privacy-traces.xml 类目对齐）。 */
export function privacyItems(items: ScanItem[]): ScanItem[] {
  return items.filter((i) => i.categoryId.startsWith("privacy."));
}

/** 一个隐私类目分组。 */
export interface PrivacyCategoryGroup {
  categoryId: string;
  label: string;
  reason: string;
  items: ScanItem[];
}

/** 按类目分组（categoryId 字典序，确定性；label/reason 取组内首项）。 */
export function groupByCategory(items: ScanItem[]): PrivacyCategoryGroup[] {
  const map = new Map<string, PrivacyCategoryGroup>();
  for (const it of items) {
    let g = map.get(it.categoryId);
    if (!g) {
      g = { categoryId: it.categoryId, label: it.label, reason: it.reason, items: [] };
      map.set(it.categoryId, g);
    }
    g.items.push(it);
  }
  return [...map.values()].sort((a, b) => a.categoryId.localeCompare(b.categoryId));
}

/** 类目守卫提示：浏览器类目运行中将被整类阻止（SAFETY §5.1）。 */
export function guardHint(categoryId: string): string {
  if (categoryId === "privacy.edge-history") return "执行前请退出 Edge，运行中将整类阻止";
  if (categoryId === "privacy.chrome-history") return "执行前请退出 Chrome，运行中将整类阻止";
  return "";
}

/** 去向标注：可恢复语义（SAFETY §4.5，UI 须区分"可还原"而非笼统"已清理"）。 */
export function restorableNote(disposition: Disposition): string {
  return disposition === "quarantine" ? "入隔离区 · 14 天内可还原" : "直接清理";
}

/** 选中项合计字节。 */
export function selectedTotalBytes(items: ScanItem[], ids: Set<string>): number {
  return items.filter((i) => ids.has(i.id)).reduce((acc, i) => acc + i.sizeBytes, 0);
}
