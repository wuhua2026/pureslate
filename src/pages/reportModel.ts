/**
 * 报告页（屏 2）数据模型——纯函数，无 DOM 依赖，可被 vitest 直接单测。
 * R13 报告式总览 + R14 逐项可解释 + A/B 方案对比。
 * R30：审阅型类目（large.file 大文件）从报告全链路排除——大文件是用户个人数据，
 * 只供大文件页发现/浏览，永不进入清理方案（前后端双保险：后端不注册规则 fail-closed）。
 * R31-6：报告页默认进入"一键清理"简化视图——只清 🟢 安全项；🟡/🔴 一律不进一键流程，
 * 作为"需确认项"提示；A/B 方案仅在展开完整报告后展示。
 */
import type { Disposition, Grade, ScanItem } from "../types/ipc";

/** R30 审阅型类目 id：仅供专项页浏览，不计风险、不进方案、不参与汇总。 */
export const REVIEW_ONLY_CATEGORY_IDS = new Set(["large.file"]);

/** 排除审阅型类目后的可清理条目（报告页一切统计的输入）。 */
export function cleanableItems(items: ScanItem[]): ScanItem[] {
  return items.filter((i) => !REVIEW_ONLY_CATEGORY_IDS.has(i.categoryId));
}

/** R31-6 一键清理候选：只取 🟢 安全项（临时文件 / 可重建缓存）。
 *  🟡/🔴 一律不进一键流程（必须走完整报告的人工确认），与专家模式无关。 */
export function safeCleanItems(rawItems: ScanItem[]): ScanItem[] {
  return cleanableItems(rawItems).filter((i) => i.grade === "green");
}

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
  /** 立即释放：仅 🟢 且去向 direct；回收站项要等清空回收站才释放空间，不计入。 */
  immediateBytes: number;
  /** 进隔离区（🟡+🔴，尚未释放）。 */
  quarantineBytes: number;
  /** 需人工确认的项数（🟡+🔴，不进一键清理）。 */
  confirmItems: number;
  /** 需人工确认的字节数（🟡+🔴，不进一键清理）。 */
  confirmBytes: number;
  planA: ReportPlan; // 推荐清理：仅绿（一键清理口径）
  planB: ReportPlan; // 深度清理：绿+黄
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

/** 仅统计去向为 direct 的条目字节（回收站/隔离区在完成前都不算"已释放"）。 */
function sumDirectBytes(items: ScanItem[], grades: Grade[]): number {
  let bytes = 0;
  for (const it of items) {
    if (grades.includes(it.grade) && it.disposition === "direct") bytes += it.sizeBytes;
  }
  return bytes;
}

/** 生成报告总览。可释放语义：🟢 direct=立即释放；🟡+🔴=进隔离区（尚未释放）；
 *  🟢 recycle 进回收站，清空回收站前不计入立即释放。
 *  R30：审阅型类目（large.file）在入口即排除，不进类目卡/风险分布/方案数字。
 *  R31-6：另出 confirmItems/confirmBytes（🟡+🔴）供"一键清理"简化卡提示。 */
export function buildReportOverview(rawItems: ScanItem[]): ReportOverview {
  const items = cleanableItems(rawItems);
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
    immediateBytes: sumDirectBytes(items, ["green"]), // 立即释放：绿 + 直清
    quarantineBytes: yellow.bytes + red.bytes, // 进隔离区，可还原
    confirmItems: yellow.items + red.items, // 🟡/🔴 需人工确认（不进一键清理）
    confirmBytes: yellow.bytes + red.bytes,
    planA: { id: "A", name: "推荐清理（只清安全项）", grades: ["green"], items: green.items, bytes: green.bytes, isDefault: true },
    planB: { id: "B", name: "深度清理（含需确认项）", grades: ["green", "yellow"], items: green.items + yellow.items, bytes: green.bytes + yellow.bytes, isDefault: false },
    redCategoryCount: redCategories(categories),
  };
}

function redCategories(categories: ReportCategory[]): number {
  return categories.filter((c) => c.grade === "red").length;
}