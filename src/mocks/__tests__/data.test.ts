import { describe, expect, it } from "vitest";
import {
  buildMockScanResult,
  DiskUsageStub,
  QuarantineListStub,
  StartupListStub,
} from "../data";
import type { ScanDimension } from "../../types/ipc";

const ALL_DIMENSIONS: ScanDimension[] = ["temp", "large", "dup", "cache", "startup", "privacy"];

describe("mock 扫描数据（P0-04）", () => {
  const result = buildMockScanResult();

  it("ScanResult 顶层字段完整且类型正确", () => {
    expect(result.scanId).toBeTruthy();
    expect(result.volume).toBeTruthy();
    expect(typeof result.startedAt).toBe("number");
    expect(typeof result.finishedAt).toBe("number");
    expect(result.itemCount).toBeGreaterThan(0);
    expect(result.totalBytes).toMatchObject({
      green: expect.any(Number),
      yellow: expect.any(Number),
      red: expect.any(Number),
    });
    expect(result.aggregates.length).toBeGreaterThan(0);
  });

  it("items 全部含稳定 id / 必填字段", () => {
    for (const it of result.items) {
      expect(it.id.length).toBe(16);
      expect(it.categoryId).toBeTruthy();
      expect(it.path).toBeTruthy();
      expect(it.grade).oneOf(["green", "yellow", "red"]);
      expect(it.disposition).oneOf(["direct", "recycle", "quarantine"]);
      expect(typeof it.sizeBytes).toBe("number");
    }
  });

  it("覆盖全部 6 扫描维度", () => {
    const covered = ALL_DIMENSIONS.every((d) =>
      result.items.some((i) => i.categoryId.startsWith(d === "temp" ? "temp" : d)),
    );
    // 维度以 categoryId 前缀界定；逐维度断言兜底
    const dims = new Set(result.aggregates.map((a) => a.categoryId.split(".")[0]));
    for (const d of ALL_DIMENSIONS) {
      expect(dims.has(d), `缺少维度: ${d}`).toBe(true);
    }
    void covered;
  });

  it("含中英文文件名", () => {
    const hasChinese = result.items.some((i) => /[\u4e00-\u9fa5]/.test(i.path));
    const hasAscii = result.items.some((i) => /^[\x00-\x7F]+$/.test(i.path));
    expect(hasChinese).toBe(true);
    expect(hasAscii).toBe(true);
  });

  it("三档分级均有数据", () => {
    const grades = new Set(result.items.map((i) => i.grade));
    expect(grades.has("green")).toBe(true);
    expect(grades.has("yellow")).toBe(true);
    expect(grades.has("red")).toBe(true);
  });

  it("重复文件组存在且共享 dupGroup", () => {
    const dup = result.items.filter((i) => i.dupGroup);
    expect(dup.length).toBeGreaterThanOrEqual(2);
    const groups = new Set(dup.map((i) => i.dupGroup));
    expect(groups.size).toBeGreaterThanOrEqual(1);
  });

  it("aggregates 聚合总数与 items 一致", () => {
    const sumFromAgg = result.aggregates.reduce((acc, a) => acc + a.itemCount, 0);
    expect(sumFromAgg).toBe(result.itemCount);
  });

  it("磁盘占用 stub（屏 1 Home）字段合法且未超量", () => {
    expect(DiskUsageStub.length).toBeGreaterThan(0);
    for (const d of DiskUsageStub) {
      expect(d.letter).toMatch(/^[A-Z]$/);
      expect(d.label).toBeTruthy();
      expect(d.totalBytes).toBeGreaterThan(0);
      expect(d.freeBytes).toBeGreaterThanOrEqual(0);
      expect(d.freeBytes).toBeLessThanOrEqual(d.totalBytes);
      // 占用比在合法区间
      const pct = (d.totalBytes - d.freeBytes) / d.totalBytes;
      expect(pct).toBeGreaterThanOrEqual(0);
      expect(pct).toBeLessThanOrEqual(1);
    }
  });

  it("隔离区与启动项 stub 字段对齐 Contract", () => {
    expect(QuarantineListStub.length).toBeGreaterThan(0);
    for (const q of QuarantineListStub) {
      expect(q.id).toBeTruthy();
      expect(q.daysLeft).toBeGreaterThan(0);
      expect(q.state).oneOf(["quarantined", "restored", "purged"]);
    }
    expect(StartupListStub.length).toBeGreaterThan(0);
    // P3-03：stub 同时含启用与已禁用（备份可恢复）样本，enabled 只要求布尔。
    expect(StartupListStub.some((s) => s.enabled)).toBe(true);
    expect(StartupListStub.some((s) => !s.enabled)).toBe(true);
    for (const s of StartupListStub) {
      expect(s.id).toBeTruthy();
      expect(typeof s.enabled).toBe("boolean");
      expect(s.source).oneOf(["hkcu_run", "hklm_run", "startup_folder", "task_scheduler"]);
    }
  });
});