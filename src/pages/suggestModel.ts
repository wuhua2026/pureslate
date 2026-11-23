/**
 * 清理建议 V0（纯前端 · 零契约变更）。
 *
 * 设计：docs/dev/DESIGN-SUGGEST-2026-11-23.md（S3：顶部总结 + 类目徽标 + 例外置顶）。
 * 用户已拍板的五条口径：
 *  1. 方案 A 改名「稳妥清理」，与建议徽标「建议清理」区分；
 *  2. 摘要同时出现在一键清理卡与完整报告；
 *  3. 🟢 可出「建议保留」，但必须有具体依据（可信的近期访问），默认罕见；
 *  4. rebuildable / impacts 规则属性本批不做；
 *  5. verified 不进公开 schema。
 *
 * 硬约束：
 *  - 建议只影响文案与排序，**不参与勾选**——可选集合、方案 A/B、🔴 灰禁、
 *    专家模式与二次确认 token 流程一律不动；
 *  - 只降级不升级（安全方向单向）：🔴 永不出现「建议清理」；
 *  - whitelistOk=false 时整体降为「需要你判断」；
 *  - 固定词表，禁止自由发挥；所有「建议清理」必须带一条具体依据。
 */
import { ref } from "vue";
import type { Disposition, Grade, ScanItem } from "../types/ipc";

/** 审阅型类目：仅供大文件页浏览，不参与任何建议。 */
const REVIEW_ONLY_CATEGORY_ID = "large.file";

/** 判定阈值（设计文档 §5）。 */
const RECENT_ACCESS_MS = 3 * 86400e3; // 近期访问：3 天内
const ATIME_TRUST_MS = 7 * 86400e3; // atime 可信窗口：7 天内
const SIZE_DOMINANT_SHARE = 0.6; // 单个类目占可清理总量 60%
const SIZE_DOMINANT_MIN_ITEMS = 1000; // 且项数 ≥ 1000

/** 固定三态词表（四词：keep 是 clean 的降级分支，见用户决策 3）。 */
export type SuggestState = "clean" | "caveat" | "keep" | "unsure";

export const SUGGEST_LABEL: Record<SuggestState, string> = {
  clean: "建议清理",
  caveat: "可以清理，但…",
  keep: "建议保留",
  unsure: "需要你判断",
};

/** 建议输入：类目级聚合（与报告页同源），不做逐项建议。 */
export interface SuggestCategoryInput {
  categoryId: string;
  label: string;
  grade: Grade;
  disposition: Disposition;
  totalBytes: number;
  itemCount: number;
}

/** atime 证据只需条目上这两条时间戳（聚合层没有 atime，须由 scan.items 提供）。 */
export type AtimeEvidence = Pick<ScanItem, "categoryId" | "mtime" | "atime">;

export interface Suggestion {
  categoryId: string;
  state: SuggestState;
  /** 固定词表文案（徽标直接渲染）。 */
  label: string;
  /** 具体依据短句；「建议清理」必有值。 */
  basis: string;
}

export interface SuggestBucket {
  categories: number;
  items: number;
}

export interface SuggestSummary {
  clean: SuggestBucket;
  caveat: SuggestBucket;
  keep: SuggestBucket;
  unsure: SuggestBucket;
  totalCategories: number;
  totalItems: number;
  /** 顶部一句话摘要（自动跳过 0 值桶）。 */
  headline: string;
}

export interface SuggestInput {
  categories: SuggestCategoryInput[];
  items?: AtimeEvidence[];
  /** 白名单配置是否健康；false 时整体降级为「需要你判断」。 */
  whitelistOk: boolean;
  now?: number;
}

const EMPTY_BUCKET = (): SuggestBucket => ({ categories: 0, items: 0 });

/** 「建议清理」的依据短语：优先说明可重建/可恢复，避免绝对化措辞。 */
function cleanBasis(cat: SuggestCategoryInput): string {
  if (cat.disposition === "recycle") return "移入回收站，可随时恢复";
  if (cat.categoryId.startsWith("temp.")) return "临时文件，删除后由系统或应用自动重建";
  if (cat.categoryId.startsWith("cache.")) return "缓存文件，应用会重新生成";
  if (cat.categoryId.startsWith("dup.")) return "重复副本，保留最早一份";
  return "安全级，可自动重建";
}

/** 可信的近期访问：同一文件 mtime ≤ atime（自检），atime 在 7 天可信窗口内且 3 天内。 */
function hasCredibleRecentAccess(
  categoryId: string,
  items: AtimeEvidence[] | undefined,
  now: number,
): boolean {
  if (!items) return false;
  for (const it of items) {
    if (it.categoryId !== categoryId) continue;
    if (it.mtime === undefined || it.atime === undefined) continue;
    if (it.mtime > it.atime) continue; // atime 早于 mtime → 不可信
    if (it.atime <= now - ATIME_TRUST_MS) continue;
    if (it.atime < now - RECENT_ACCESS_MS) continue;
    return true;
  }
  return false;
}

/** 类目级建议（纯函数）。输入顺序即输出顺序；排序由调用方决定。 */
export function buildSuggestions(input: SuggestInput): Suggestion[] {
  const now = input.now ?? Date.now();
  const totalCleanable = input.categories
    .filter((c) => c.categoryId !== REVIEW_ONLY_CATEGORY_ID)
    .reduce((sum, c) => sum + c.totalBytes, 0);

  const out: Suggestion[] = [];
  for (const cat of input.categories) {
    if (cat.categoryId === REVIEW_ONLY_CATEGORY_ID) continue; // 审阅型不出现

    // 只降级不升级：危险 / 白名单降级 → 需要你判断。
    if (!input.whitelistOk) {
      out.push({
        categoryId: cat.categoryId,
        state: "unsure",
        label: SUGGEST_LABEL.unsure,
        basis: "安全白名单降级，本次不建议清理",
      });
      continue;
    }
    if (cat.grade === "red") {
      out.push({
        categoryId: cat.categoryId,
        state: "unsure",
        label: SUGGEST_LABEL.unsure,
        basis: "危险项默认禁用；需专家模式 + 二次确认 token",
      });
      continue;
    }
    if (cat.grade === "yellow") {
      out.push({
        categoryId: cat.categoryId,
        state: "caveat",
        label: SUGGEST_LABEL.caveat,
        basis: "谨慎项，移入隔离区，保留期内可还原",
      });
      continue;
    }

    // 🟢：默认可建议清理；例外只做降级。
    if (hasCredibleRecentAccess(cat.categoryId, input.items, now)) {
      out.push({
        categoryId: cat.categoryId,
        state: "keep",
        label: SUGGEST_LABEL.keep,
        basis: "近期仍被访问（3 天内），暂不建议清理",
      });
      continue;
    }
    if (
      totalCleanable > 0 &&
      cat.totalBytes / totalCleanable >= SIZE_DOMINANT_SHARE &&
      cat.itemCount >= SIZE_DOMINANT_MIN_ITEMS
    ) {
      out.push({
        categoryId: cat.categoryId,
        state: "caveat",
        label: SUGGEST_LABEL.caveat,
        basis: "本次清理的主要体量，建议先确认路径",
      });
      continue;
    }
    out.push({
      categoryId: cat.categoryId,
      state: "clean",
      label: SUGGEST_LABEL.clean,
      basis: cleanBasis(cat),
    });
  }
  return out;
}

/** 汇总（用于顶部摘要条）：按类目聚合的项数计「项」，不做逐项建议。 */
export function summarizeSuggestions(
  list: Suggestion[],
  categories: SuggestCategoryInput[],
): SuggestSummary {
  const counts = new Map(categories.map((c) => [c.categoryId, c.itemCount]));
  const summary: SuggestSummary = {
    clean: EMPTY_BUCKET(),
    caveat: EMPTY_BUCKET(),
    keep: EMPTY_BUCKET(),
    unsure: EMPTY_BUCKET(),
    totalCategories: list.length,
    totalItems: 0,
    headline: "",
  };
  for (const s of list) {
    const items = counts.get(s.categoryId) ?? 0;
    summary[s.state].categories += 1;
    summary[s.state].items += items;
    summary.totalItems += items;
  }
  summary.headline = headlineOf(summary);
  return summary;
}

function headlineOf(s: SuggestSummary): string {
  const parts: string[] = [];
  if (s.clean.items > 0) parts.push(`建议清理 ${s.clean.items} 项`);
  if (s.keep.items > 0) parts.push(`建议保留 ${s.keep.items} 项`);
  if (s.caveat.items > 0) parts.push(`可以清理，但需确认 ${s.caveat.items} 项`);
  if (s.unsure.items > 0) parts.push(`需要你判断 ${s.unsure.items} 项`);
  return parts.length > 0 ? parts.join(" · ") : "本次扫描没有需要建议的项目";
}

// ---- 显示开关（前端偏好，不入 IPC 契约） ----
const STORAGE_KEY = "pureslate.showSuggestions";

function readEnabled(): boolean {
  try {
    if (typeof localStorage === "undefined") return true;
    return localStorage.getItem(STORAGE_KEY) !== "0";
  } catch {
    return true; // 隐私模式/存储受限：回退默认开，不阻断
  }
}

/** 建议显示开关（默认开）。关闭后报告页不渲染任何建议元素。 */
export const showSuggestions = ref(readEnabled());

export function setSuggestionsEnabled(on: boolean): void {
  showSuggestions.value = on;
  try {
    if (typeof localStorage !== "undefined") {
      localStorage.setItem(STORAGE_KEY, on ? "1" : "0");
    }
  } catch {
    // 仅内存生效即可，不影响功能。
  }
}
