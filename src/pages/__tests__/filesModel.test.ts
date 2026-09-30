/** Files 页纯函数单测（P3-01）。 */
import { describe, expect, it } from "vitest";
import type { ScanItem } from "../../types/ipc";
import { formatBytes, formatDay, largeItems, sortBySizeDesc } from "../filesModel";

const MB = 1024 * 1024;

function item(partial: Partial<ScanItem>): ScanItem {
  return {
    id: "x",
    categoryId: "temp.user",
    label: "t",
    path: "C:\\a",
    sizeBytes: 1,
    grade: "green",
    disposition: "direct",
    reason: "r",
    ...partial,
  };
}

describe("largeItems", () => {
  it("只保留 large 维度条目", () => {
    const items = [
      item({ categoryId: "large.file", path: "C:\\big.iso", sizeBytes: 512 * MB }),
      item({ categoryId: "temp.user" }),
      item({ categoryId: "dup.file" }),
    ];
    expect(largeItems(items)).toHaveLength(1);
    expect(largeItems(items)[0].path).toBe("C:\\big.iso");
  });
});

describe("sortBySizeDesc", () => {
  it("按大小降序，同大小按路径字典序", () => {
    const items = [
      item({ path: "C:\\b.bin", sizeBytes: 2 * MB }),
      item({ path: "C:\\a.bin", sizeBytes: 2 * MB }),
      item({ path: "C:\\huge.iso", sizeBytes: 900 * MB }),
      item({ path: "C:\\tiny.txt", sizeBytes: 3 }),
    ];
    const sorted = sortBySizeDesc(items);
    expect(sorted.map((i) => i.path)).toEqual([
      "C:\\huge.iso",
      "C:\\a.bin",
      "C:\\b.bin",
      "C:\\tiny.txt",
    ]);
  });

  it("不修改原数组", () => {
    const items = [item({ sizeBytes: 1 }), item({ sizeBytes: 9 })];
    sortBySizeDesc(items);
    expect(items[0].sizeBytes).toBe(1);
  });
});

describe("formatBytes", () => {
  it("各量级换算", () => {
    expect(formatBytes(512 * MB)).toBe("512 MB");
    expect(formatBytes(1.5 * 1024 * MB)).toBe("1.5 GB");
    expect(formatBytes(2048)).toBe("2 KB");
  });
});

describe("formatDay", () => {
  it("缺省值为占位符", () => {
    expect(formatDay(undefined)).toBe("—");
    expect(formatDay(0)).toBe("—");
  });
  it("有值时输出日期串", () => {
    const s = formatDay(Date.UTC(2026, 8, 30));
    expect(s).toContain("2026");
  });
});
