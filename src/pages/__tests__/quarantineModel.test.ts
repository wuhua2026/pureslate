/** Quarantine 页纯函数单测（P3-06）。 */
import { describe, expect, it } from "vitest";
import type { QuarantineEntry } from "../../types/ipc";
import { expiringSoon, semanticHeadline, sortByExpiryAsc, totalBytes } from "../quarantineModel";

function entry(partial: Partial<QuarantineEntry>): QuarantineEntry {
  return {
    id: "x",
    originalPath: "C:\\a",
    sizeBytes: 1,
    grade: "yellow",
    categoryId: "cache.wechat",
    movedAt: 0,
    expiresAt: 0,
    daysLeft: 10,
    state: "quarantined",
    ...partial,
  };
}

describe("sortByExpiryAsc", () => {
  it("剩余天数升序，同天数按路径字典序", () => {
    const entries = [
      entry({ id: "b", originalPath: "C:\\b", daysLeft: 5 }),
      entry({ id: "z", originalPath: "C:\\z", daysLeft: 1 }),
      entry({ id: "a", originalPath: "C:\\a", daysLeft: 5 }),
    ];
    expect(sortByExpiryAsc(entries).map((e) => e.id)).toEqual(["z", "a", "b"]);
  });

  it("不修改原数组", () => {
    const entries = [entry({ daysLeft: 9 }), entry({ daysLeft: 1 })];
    sortByExpiryAsc(entries);
    expect(entries[0].daysLeft).toBe(9);
  });
});

describe("totalBytes", () => {
  it("合计字节数", () => {
    expect(
      totalBytes([entry({ sizeBytes: 100 }), entry({ sizeBytes: 50 }), entry({ sizeBytes: 25 })]),
    ).toBe(175);
  });
});

describe("expiringSoon", () => {
  it("只保留 ≤3 天的条目", () => {
    const entries = [
      entry({ id: "a", daysLeft: 1 }),
      entry({ id: "b", daysLeft: 3 }),
      entry({ id: "c", daysLeft: 4 }),
    ];
    expect(expiringSoon(entries).map((e) => e.id)).toEqual(["a", "b"]);
  });
});

describe("semanticHeadline", () => {
  it("语义首行区分已隔离与已释放（SAFETY §4.5）", () => {
    const s = semanticHeadline("1.2 GB");
    expect(s).toContain("已隔离 1.2 GB");
    expect(s).toContain("尚未释放");
    expect(s).not.toContain("已清理");
  });
});
