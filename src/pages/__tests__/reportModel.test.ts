import { describe, expect, it } from "vitest";
import { buildMockScanResult } from "../../mocks/data";
import { buildReportOverview, DISPOSITION_CN, groupCategories } from "../reportModel";

describe("报告模型（R13/R14）", () => {
  const result = buildMockScanResult();

  it("groupCategories 按类目聚合字节数与项数", () => {
    const groups = groupCategories(result.items);
    const dup = groups.find((g) => g.categoryId === "dup.file");
    expect(dup).toBeTruthy();
    expect(dup!.itemCount).toBe(2); // 2 个重复文件
    expect(dup!.totalBytes).toBeGreaterThanOrEqual(2 * 88 * 1024 * 1024);
    groups.forEach((g) => expect(g.reason).toBeTruthy()); // 每项可解释
  });

  it("可释放语义正确：绿=立即释放，黄+红=进隔离区", () => {
    const ov = buildReportOverview(result.items);
    const greenBytes = result.items
      .filter((i) => i.grade === "green")
      .reduce((s, i) => s + i.sizeBytes, 0);
    expect(ov.immediateBytes).toBe(greenBytes);
    expect(ov.immediateBytes).toBeGreaterThan(0);

    const nonGreen = result.items
      .filter((i) => i.grade !== "green")
      .reduce((s, i) => s + i.sizeBytes, 0);
    expect(ov.quarantineBytes).toBe(nonGreen);
    expect(ov.quarantineBytes).toBeGreaterThan(0);
  });

  it("风险分布统计正确", () => {
    const ov = buildReportOverview(result.items);
    const count = (g: string) => result.items.filter((i) => i.grade === g).length;
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

  it("空列表→全零且不抛错", () => {
    const ov = buildReportOverview([]);
    expect(ov.categories).toEqual([]);
    expect(ov.immediateBytes).toBe(0);
    expect(ov.quarantineBytes).toBe(0);
    expect(ov.planA.bytes).toBe(0);
    expect(ov.planB.bytes).toBe(0);
  });

  it("去向预告文案覆盖 direct/recycle/quarantine", () => {
    expect(DISPOSITION_CN.direct).toBe("直清");
    expect(DISPOSITION_CN.recycle).toBe("回收站");
    expect(DISPOSITION_CN.quarantine).toBe("隔离区");
  });

  it("确定性：全量字节守恒（类目汇总=原始总量）", () => {
    const ov = buildReportOverview(result.items);
    const fromCat = ov.categories.reduce((s, c) => s + c.totalBytes, 0);
    const fromItems = result.items.reduce((s, i) => s + i.sizeBytes, 0);
    expect(fromCat).toBe(fromItems);
  });
});