/** 清理建议 V0 单测（纯前端 · 零契约）。 */
import { describe, expect, it } from "vitest";
import type { Grade, ScanItem } from "../../types/ipc";
import {
  buildSuggestions,
  SUGGEST_LABEL,
  setSuggestionsEnabled,
  showSuggestions,
  summarizeSuggestions,
  type SuggestCategoryInput,
} from "../suggestModel";

const DAY = 86400e3;
const NOW = Date.UTC(2026, 10, 23, 12, 0, 0);

function cat(partial: Partial<SuggestCategoryInput> & { categoryId: string }): SuggestCategoryInput {
  return {
    label: partial.categoryId,
    grade: "green",
    disposition: "direct",
    totalBytes: 100,
    itemCount: 1,
    ...partial,
  };
}

function evidence(
  categoryId: string,
  mtime: number | undefined,
  atime: number | undefined,
): Pick<ScanItem, "categoryId" | "mtime" | "atime"> {
  return { categoryId, mtime, atime };
}

function suggest(
  categories: SuggestCategoryInput[],
  opts: { whitelistOk?: boolean; items?: ReturnType<typeof evidence>[] } = {},
) {
  return buildSuggestions({
    categories,
    items: opts.items,
    whitelistOk: opts.whitelistOk ?? true,
    now: NOW,
  });
}

describe("清理建议 V0：三态判定", () => {
  it("固定词表", () => {
    expect(SUGGEST_LABEL.clean).toBe("建议清理");
    expect(SUGGEST_LABEL.caveat).toBe("可以清理，但…");
    expect(SUGGEST_LABEL.keep).toBe("建议保留");
    expect(SUGGEST_LABEL.unsure).toBe("需要你判断");
  });

  it("🟢 direct → 建议清理，且必带依据", () => {
    const [s] = suggest([cat({ categoryId: "temp.user", disposition: "direct" })]);
    expect(s.state).toBe("clean");
    expect(s.label).toBe("建议清理");
    expect(s.basis.length).toBeGreaterThan(0);
  });

  it("🟢 recycle → 建议清理，依据说明进回收站可恢复", () => {
    const [s] = suggest([cat({ categoryId: "temp.user", disposition: "recycle" })]);
    expect(s.state).toBe("clean");
    expect(s.basis).toContain("回收站");
  });

  it("🟡 → 可以清理，但…（不升级为建议清理）", () => {
    const [s] = suggest([
      cat({ categoryId: "cache.wechat", grade: "yellow", disposition: "quarantine" }),
    ]);
    expect(s.state).toBe("caveat");
    expect(s.label).toBe("可以清理，但…");
    expect(s.basis).toContain("隔离区");
  });

  it("🔴 永不出现建议清理 → 需要你判断，附专家模式 + token 说明", () => {
    const [s] = suggest([
      cat({ categoryId: "startup.registry", grade: "red", disposition: "quarantine" }),
    ]);
    expect(s.state).toBe("unsure");
    expect(s.label).not.toBe("建议清理");
    expect(s.basis).toContain("专家模式");
    expect(s.basis).toContain("token");
  });

  it("whitelistOk=false → 全体降为需要你判断（含原本的 🟢）", () => {
    const list = suggest(
      [
        cat({ categoryId: "temp.user", totalBytes: 10 }),
        cat({ categoryId: "cache.wechat", grade: "yellow", disposition: "quarantine" }),
        cat({ categoryId: "startup.registry", grade: "red", disposition: "quarantine" }),
      ],
      { whitelistOk: false },
    );
    expect(list.every((s) => s.state === "unsure")).toBe(true);
    expect(list.every((s) => s.basis.includes("白名单"))).toBe(true);
  });

  it("审阅型 large.file 不产生建议（防御性）", () => {
    const list = suggest([
      cat({ categoryId: "large.file", grade: "yellow", disposition: "quarantine" }),
      cat({ categoryId: "temp.user" }),
    ]);
    expect(list.map((s) => s.categoryId)).toEqual(["temp.user"]);
  });
});

describe("清理建议 V0：例外只降级", () => {
  it("🟢 + 可信的近期访问 → 建议保留（用户决策 3）", () => {
    const [s] = suggest([cat({ categoryId: "temp.user" })], {
      items: [evidence("temp.user", NOW - 10 * DAY, NOW - DAY)],
    });
    expect(s.state).toBe("keep");
    expect(s.label).toBe("建议保留");
    expect(s.basis).toContain("近期");
  });

  it("atime 不可信（早于 mtime）→ 维持建议清理", () => {
    const [s] = suggest([cat({ categoryId: "temp.user" })], {
      items: [evidence("temp.user", NOW - DAY, NOW - 2 * DAY)],
    });
    expect(s.state).toBe("clean");
  });

  it("atime 超出 7 天可信窗口 → 维持建议清理", () => {
    const [s] = suggest([cat({ categoryId: "temp.user" })], {
      items: [evidence("temp.user", NOW - 20 * DAY, NOW - 10 * DAY)],
    });
    expect(s.state).toBe("clean");
  });

  it("体量集中（≥60% 且 ≥1000 项）→ 降级为可以清理，但…", () => {
    const list = suggest([
      cat({ categoryId: "temp.user", totalBytes: 600, itemCount: 1000 }),
      cat({ categoryId: "cache.wechat", totalBytes: 400, itemCount: 10 }),
    ]);
    const big = list.find((s) => s.categoryId === "temp.user")!;
    const small = list.find((s) => s.categoryId === "cache.wechat")!;
    expect(big.state).toBe("caveat");
    expect(big.basis).toContain("体量");
    expect(small.state).toBe("clean");
  });

  it("体量集中但不足 1000 项 → 不降级", () => {
    const [s] = suggest([cat({ categoryId: "temp.user", totalBytes: 600, itemCount: 999 })]);
    expect(s.state).toBe("clean");
  });

  it("例外不会把 🟡/🔴 升级（单向降级）", () => {
    const yellow: Grade = "yellow";
    const red: Grade = "red";
    const list = suggest(
      [
        cat({ categoryId: "cache.wechat", grade: yellow, disposition: "quarantine" }),
        cat({ categoryId: "startup.registry", grade: red, disposition: "quarantine" }),
      ],
      { items: [evidence("cache.wechat", NOW - 10 * DAY, NOW - DAY)] },
    );
    expect(list.find((s) => s.categoryId === "cache.wechat")!.state).toBe("caveat");
    expect(list.find((s) => s.categoryId === "startup.registry")!.state).toBe("unsure");
  });
});

describe("清理建议 V0：摘要与开关", () => {
  it("摘要按类目项数计「项」，并跳过 0 值桶", () => {
    const categories = [
      cat({ categoryId: "temp.user", totalBytes: 200, itemCount: 8 }),
      cat({ categoryId: "cache.wechat", grade: "yellow", disposition: "quarantine", itemCount: 3 }),
      cat({ categoryId: "startup.registry", grade: "red", disposition: "quarantine", itemCount: 1 }),
    ];
    const list = suggest(categories);
    const s = summarizeSuggestions(list, categories);
    expect(s.clean).toEqual({ categories: 1, items: 8 });
    expect(s.caveat).toEqual({ categories: 1, items: 3 });
    expect(s.unsure).toEqual({ categories: 1, items: 1 });
    expect(s.keep).toEqual({ categories: 0, items: 0 });
    expect(s.totalCategories).toBe(3);
    expect(s.totalItems).toBe(12);
    expect(s.headline).toBe("建议清理 8 项 · 可以清理，但需确认 3 项 · 需要你判断 1 项");
    expect(s.headline).not.toContain("建议保留");
  });

  it("空输入 → 空摘要", () => {
    const s = summarizeSuggestions([], []);
    expect(s.totalCategories).toBe(0);
    expect(s.headline).toBe("本次扫描没有需要建议的项目");
  });

  it("建议开关默认开，且可关闭/再开启（不入 IPC 契约）", () => {
    expect(showSuggestions.value).toBe(true); // node 无 localStorage → 默认开
    setSuggestionsEnabled(false);
    expect(showSuggestions.value).toBe(false);
    setSuggestionsEnabled(true);
    expect(showSuggestions.value).toBe(true);
  });
});
