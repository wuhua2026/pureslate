import { describe, expect, it } from "vitest";
import { buildMockScanResult } from "../../mocks/data";
import type { ScanItem } from "../../types/ipc";
import {
  buildReportOverview,
  cleanableItems,
  DISPOSITION_CN,
  groupCategories,
  REVIEW_ONLY_CATEGORY_IDS,
  safeCleanItems,
} from "../reportModel";

describe("报告模型（R13/R14）", () => {
  const result = buildMockScanResult();
  /** R30：报告统计口径只含可清理条目（审阅型 large.file 排除）。 */
  const cleanable = cleanableItems(result.items);

  it("groupCategories 按类目聚合字节数与项数", () => {
    const groups = groupCategories(cleanable);
    const dup = groups.find((g) => g.categoryId === "dup.file");
    expect(dup).toBeTruthy();
    expect(dup!.itemCount).toBe(2); // 2 个重复文件
    expect(dup!.totalBytes).toBeGreaterThanOrEqual(2 * 88 * 1024 * 1024);
    groups.forEach((g) => expect(g.reason).toBeTruthy()); // 每项可解释
  });

  it("可释放语义正确：🟢 direct=立即释放，🟡+🔴=进隔离区", () => {
    const ov = buildReportOverview(result.items);
    const greenDirectBytes = cleanable
      .filter((i) => i.grade === "green" && i.disposition === "direct")
      .reduce((s, i) => s + i.sizeBytes, 0);
    // R31-6：immediateBytes 只算 green+direct（回收站项不立即释放）。
    expect(ov.immediateBytes).toBe(greenDirectBytes);
    expect(ov.immediateBytes).toBeGreaterThan(0);
    expect(ov.immediateBytes).toBeLessThanOrEqual(ov.planA.bytes);

    const nonGreen = cleanable
      .filter((i) => i.grade !== "green")
      .reduce((s, i) => s + i.sizeBytes, 0);
    expect(ov.quarantineBytes).toBe(nonGreen);
    expect(ov.quarantineBytes).toBeGreaterThan(0);
  });

  it("R31-6 口径：green+recycle 不计入 immediateBytes（清空回收站前不释放）", () => {
    const item = (id: string, sizeBytes: number, disposition: ScanItem["disposition"]): ScanItem => ({
      id,
      categoryId: "temp.user",
      label: "临时文件",
      path: `C:\\Temp\\${id}.tmp`,
      sizeBytes,
      grade: "green",
      disposition,
      reason: "可安全重建的临时文件",
    });
    const ov = buildReportOverview([
      item("direct-1", 100, "direct"),
      item("recycle-1", 200, "recycle"),
    ]);
    expect(ov.immediateBytes).toBe(100);
    expect(ov.quarantineBytes).toBe(0);
    // 一键清理（方案 A）仍会处置全部绿色项，但"可释放"数字不虚高。
    expect(ov.planA.items).toBe(2);
    expect(ov.planA.bytes).toBe(300);
  });

  it("R31-6 需确认项：confirmItems/confirmBytes = 🟡+🔴，不进一键清理", () => {
    const ov = buildReportOverview(result.items);
    const confirm = cleanable.filter((i) => i.grade !== "green");
    const confirmBytes = confirm.reduce((s, i) => s + i.sizeBytes, 0);
    expect(ov.confirmItems).toBe(confirm.length);
    expect(ov.confirmBytes).toBe(confirmBytes);
    expect(ov.confirmItems).toBe(ov.riskItems.yellow + ov.riskItems.red);
    expect(ov.confirmBytes).toBe(ov.quarantineBytes);
    // 一键清理口径 = 全部绿色项（与需确认项无交集）。
    expect(ov.planA.items).toBe(ov.riskItems.green);
    expect(ov.confirmItems).toBeGreaterThan(0); // mock 三档齐全，提示文案非空
  });

  it("R31-6 safeCleanItems：只取 🟢 安全项，🟡/🔴 与审阅型大文件一律不入选", () => {
    const safe = safeCleanItems(result.items);
    expect(safe.length).toBeGreaterThan(0);
    expect(safe.length).toBe(buildReportOverview(result.items).planA.items);
    for (const it of safe) {
      expect(it.grade).toBe("green");
      expect(REVIEW_ONLY_CATEGORY_IDS.has(it.categoryId)).toBe(false);
    }
    // 🟡/🔴 条目一个都不能进一键清理。
    expect(safe.some((i) => i.grade === "yellow" || i.grade === "red")).toBe(false);
  });

  it("R31-6 mock 一键清理走查前提：安全项 / 立即释放 / 需确认项均非空", () => {
    const ov = buildReportOverview(result.items);
    expect(safeCleanItems(result.items).length).toBeGreaterThan(0);
    expect(ov.planA.items).toBeGreaterThan(0);
    expect(ov.immediateBytes).toBeGreaterThan(0);
    expect(ov.confirmItems).toBeGreaterThan(0);
  });

  it("风险分布统计正确", () => {
    const ov = buildReportOverview(result.items);
    const count = (g: string) => cleanable.filter((i) => i.grade === g).length;
    expect(ov.riskItems.green).toBe(count("green"));
    expect(ov.riskItems.yellow).toBe(count("yellow"));
    expect(ov.riskItems.red).toBe(count("red"));
    // 三档均有
    expect(ov.riskItems.green).toBeGreaterThan(0);
    expect(ov.riskItems.yellow).toBeGreaterThan(0);
    expect(ov.riskItems.red).toBeGreaterThan(0);
  });

  it("A/B 方案对比：B≥A，且 🔴 从不在方案内（灰禁）", () => {
    const ov = buildReportOverview(result.items);
    expect(ov.planA.grades).toEqual(["green"]);
    expect(ov.planB.grades).toEqual(["green", "yellow"]);
    expect(ov.planB.bytes).toBeGreaterThanOrEqual(ov.planA.bytes);
    // 任何方案都不含 red
    for (const p of [ov.planA, ov.planB]) {
      expect(p.grades).not.toContain("red");
    }
    expect(ov.redCategoryCount).toBeGreaterThan(0);
  });

  it("R31-6 方案命名：推荐清理（只清安全项）/ 深度清理（含需确认项），去掉评审语言", () => {
    const ov = buildReportOverview(result.items);
    expect(ov.planA.name).toBe("推荐清理（只清安全项）");
    expect(ov.planB.name).toBe("深度清理（含需确认项）");
    expect(ov.planA.isDefault).toBe(true);
    expect(ov.planB.isDefault).toBe(false);
  });

  it("空列表→全零且不抛错", () => {
    const ov = buildReportOverview([]);
    expect(ov.categories).toEqual([]);
    expect(ov.immediateBytes).toBe(0);
    expect(ov.quarantineBytes).toBe(0);
    expect(ov.confirmItems).toBe(0);
    expect(ov.confirmBytes).toBe(0);
    expect(ov.planA.bytes).toBe(0);
    expect(ov.planB.bytes).toBe(0);
  });

  it("去向预告文案覆盖 direct/recycle/quarantine", () => {
    expect(DISPOSITION_CN.direct).toBe("直清");
    expect(DISPOSITION_CN.recycle).toBe("回收站");
    expect(DISPOSITION_CN.quarantine).toBe("隔离区");
  });

  it("确定性：可清理字节守恒（类目汇总=过滤后原始总量）", () => {
    const ov = buildReportOverview(result.items);
    const fromCat = ov.categories.reduce((s, c) => s + c.totalBytes, 0);
    const fromItems = cleanable.reduce((s, i) => s + i.sizeBytes, 0);
    expect(fromCat).toBe(fromItems);
  });
});

describe("R30 大文件审阅型类目排除", () => {
  const result = buildMockScanResult();
  const large = result.items.filter((i) => REVIEW_ONLY_CATEGORY_IDS.has(i.categoryId));

  it("mock 数据中确有 large.file 条目（测试前提成立）", () => {
    expect(large.length).toBeGreaterThan(0);
  });

  it("cleanableItems 剔除 large.file，但条目本身仍存在（供大文件页浏览）", () => {
    const kept = cleanableItems(result.items);
    expect(kept.some((i) => i.categoryId === "large.file")).toBe(false);
    expect(kept.length).toBe(result.items.length - large.length);
    // 原数组不被修改
    expect(result.items.some((i) => i.categoryId === "large.file")).toBe(true);
  });

  it("报告总览不出现大文件类目，方案/风险/汇总字节均不含其体积", () => {
    const largeBytes = large.reduce((s, i) => s + i.sizeBytes, 0);
    const ov = buildReportOverview(result.items);
    expect(ov.categories.some((c) => REVIEW_ONLY_CATEGORY_IDS.has(c.categoryId))).toBe(false);

    // 绿（方案 A）+ 黄红（隔离区）= 全部可清理体积。
    const totalInOverview = ov.planA.bytes + ov.quarantineBytes;
    const allCleanable = cleanableItems(result.items).reduce((s, i) => s + i.sizeBytes, 0);
    expect(totalInOverview).toBe(allCleanable);

    // 方案 B 数字不得包含大文件体积（真机回归：曾出现 40558 项/181.7GB）。
    const rawAll = result.items.reduce((s, i) => s + i.sizeBytes, 0);
    expect(ov.planA.bytes + ov.quarantineBytes).toBe(rawAll - largeBytes);
    expect(largeBytes).toBeGreaterThan(0);
  });
});
