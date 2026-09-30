/** Privacy 页纯函数单测（P3-05）。 */
import { describe, expect, it } from "vitest";
import type { ScanItem } from "../../types/ipc";
import {
  groupByCategory,
  guardHint,
  privacyItems,
  restorableNote,
  selectedTotalBytes,
} from "../privacyModel";

function item(partial: Partial<ScanItem>): ScanItem {
  return {
    id: "x",
    categoryId: "temp.user",
    label: "t",
    path: "C:\\a",
    sizeBytes: 1,
    grade: "yellow",
    disposition: "quarantine",
    reason: "r",
    ...partial,
  };
}

describe("privacyItems", () => {
  it("只保留 privacy 维度条目", () => {
    const items = [
      item({ categoryId: "privacy.edge-history", path: "C:\\Edge\\History" }),
      item({ categoryId: "temp.user" }),
      item({ categoryId: "privacy.recent-docs", path: "C:\\Recent\\a.lnk" }),
    ];
    expect(privacyItems(items)).toHaveLength(2);
    expect(privacyItems(items).every((i) => i.categoryId.startsWith("privacy."))).toBe(true);
  });
});

describe("groupByCategory", () => {
  it("按类目分组且字典序，label/reason 取组内首项", () => {
    const items = [
      item({ id: "1", categoryId: "privacy.recent-docs", label: "最近文档记录", reason: "最近访问记录" }),
      item({ id: "2", categoryId: "privacy.edge-history", label: "Edge 浏览历史", reason: "Edge 历史数据库" }),
      item({ id: "3", categoryId: "privacy.edge-history", label: "Edge 浏览历史", reason: "Edge 历史数据库" }),
    ];
    const groups = groupByCategory(items);
    expect(groups.map((g) => g.categoryId)).toEqual([
      "privacy.edge-history",
      "privacy.recent-docs",
    ]);
    expect(groups[0].items).toHaveLength(2);
    expect(groups[0].label).toBe("Edge 浏览历史");
    expect(groups[1].reason).toBe("最近访问记录");
  });

  it("空输入返回空数组", () => {
    expect(groupByCategory([])).toEqual([]);
  });
});

describe("guardHint", () => {
  it("浏览器类目给出退出提示，其余为空", () => {
    expect(guardHint("privacy.edge-history")).toContain("Edge");
    expect(guardHint("privacy.chrome-history")).toContain("Chrome");
    expect(guardHint("privacy.recent-docs")).toBe("");
  });
});

describe("restorableNote", () => {
  it("quarantine 标注可还原，其余标注直接清理", () => {
    expect(restorableNote("quarantine")).toContain("14 天内可还原");
    expect(restorableNote("direct")).toBe("直接清理");
    expect(restorableNote("recycle")).toBe("直接清理");
  });
});

describe("selectedTotalBytes", () => {
  it("只累计选中项字节", () => {
    const items = [
      item({ id: "a", sizeBytes: 100 }),
      item({ id: "b", sizeBytes: 50 }),
      item({ id: "c", sizeBytes: 25 }),
    ];
    expect(selectedTotalBytes(items, new Set(["a", "c"]))).toBe(125);
    expect(selectedTotalBytes(items, new Set())).toBe(0);
  });
});
