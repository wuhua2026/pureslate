/**
 * 报告页（屏 2）数据模型——纯函数，无 DOM 依赖，可被 vitest 直接单测。
 * R13 报告式总览 + R14 逐项可解释 + A/B 方案对比（给选项不给结论）。
 */
import type { Disposition, Grade, ScanItem } from "../types/ipc";

/** 去向预告中文文案（SAFETY §4.5：已释放 vs 已隔离语义区分）。 */
export const DISPOSITION_CN: Record<Disposition, string> = {
  direct: "直清",
  recycle: "回收站",
  quarantine: "隔离区",
};

export interface ReportCategory {
  categoryId: string;
  label: string;
  grade: Grade;
  dispositionLabel: string;
  totalBytes: number;
  itemCount: number;
  reason: string; // 可解释（R14：为什么/风险/能否恢复示意）
}

export interface ReportPlan {
  id: "A" | "B";
  name: string;
  grades: Grade[];
  items: number;
  bytes: number;
  isDefault: boolean;
}

export interface ReportOverview {
  categories: ReportCategory[];
  riskCategories: Record<Grade, number>; // 各类目数
  riskItems: Record<Grade, number>; // 各项数
  immediateBytes: number; // 立即释放（绿）
  quarantineBytes: number; // 进隔离区（黄+红，尚未释放）
  planA: ReportPlan; // 保守：仅绿
  planB: ReportPlan; // 激进：绿+黄
  redCategoryCount: number; // 🔴 被灰禁类目数
}

const DEFAULT_RISK: Record<Grade, number> = { green: 0, yellow: 0, red: 0 };

/** 按类目聚合（保持首现顺序），供逐项可解释渲染。 */
export function groupCategories(items: ScanItem[]): ReportCategory[] {
  const map = new Map<string, { cat: ReportCategory; index: number }>();
  let idx = 0;
  for (const it of items) {
    const hit = map.get(it.categoryId);
    if (hit) {
      hit.cat.totalBytes += it.sizeBytes;
      hit.cat.itemCount += 1;
      continue;
    }
    map.set(it.categoryId, {
      index: idx++,
      cat: {
        categoryId: it.categoryId,
        label: it.label,
        grade: it.grade,
        dispositionLabel: DISPOSITION_CN[it.disposition],
        totalBytes: it.sizeBytes,
        itemCount: 1,
        reason: it.reason,
      },
    });
  }
  return [...map.values()].sort((a, b) => a.index - b.index).map((x) => x.cat);
}

function sumGrades(items: ScanItem[], grades: Grade[]): { items: number; bytes: number } {
  let items_ = 0;
  let bytes = 0;
  for (const it of items) {
    if (grades.includes(it.grade)) {
      items_ += 1;
      bytes += it.sizeBytes;
    }
  }
  return { items: items_, bytes };
}

/** 生成报告总览。可释放语义：绿=立即释放，黄+红=进隔离区（尚未释放）。 */
export function buildReportOverview(items: ScanItem[]): ReportOverview {
  const categories = groupCategories(items);

  const riskCategories: Record<Grade, number> = { ...DEFAULT_RISK };
  const riskItems: Record<Grade, number> = { ...DEFAULT_RISK };
  for (const c of categories) riskCategories[c.grade] += 1;
  for (const it of items) riskItems[it.grade] += 1;

  const green = sumGrades(items, ["green"]);
  const yellow = sumGrades(items, ["yellow"]);
  const red = sumGrades(items, ["red"]);

  return {
    categories,
    riskCategories,
    riskItems,
    immediateBytes: green.bytes, // 立即释放
    quarantineBytes: yellow.bytes + red.bytes, // 进隔离区，可还原
    planA: { id: "A", name: "保守（仅绿色）", grades: ["green"], items: green.items, bytes: green.bytes, isDefault: true },
    planB: { id: "B", name: "激进（绿+黄）", grades: ["green", "yellow"], items: green.items + yellow.items, bytes: green.bytes + yellow.bytes, isDefault: false },
    redCategoryCount: redCategories(categories),
  };
}

function redCategories(categories: ReportCategory[]): number {
  return categories.filter((c) => c.grade === "red").length;
}