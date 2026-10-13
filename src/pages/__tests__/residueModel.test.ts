/** R27-3 残留 UI 模型单测。 */
import { describe, expect, it } from "vitest";
import type { ResidueCandidate } from "../../types/ipc";
import {
  buildSelections,
  initialRowState,
  kindLabel,
  rootTail,
  summarize,
  toggleAll,
} from "../residueModel";

function cand(partial: Partial<ResidueCandidate>): ResidueCandidate {
  return {
    id: "x",
    kind: "install_dir",
    appName: "App",
    rootPath: "C:\\Program Files\\App",
    reason: "r",
    sizeBytes: 100,
    fileCount: 1,
    items: [],
    ...partial,
  };
}

const cands = [
  cand({ id: "c1", sizeBytes: 100 }),
  cand({ id: "c2", kind: "app_data", sizeBytes: 250 }),
  cand({ id: "c3", kind: "start_menu", sizeBytes: 50 }),
];

describe("初始状态", () => {
  it("默认全部勾选且去向为隔离区", () => {
    const s = initialRowState(cands);
    expect(s.size).toBe(3);
    for (const [, v] of s) {
      expect(v.checked).toBe(true);
      expect(v.disposition).toBe("quarantine");
    }
  });

  it("kindLabel 三源", () => {
    expect(kindLabel("install_dir")).toBe("安装目录");
    expect(kindLabel("start_menu")).toBe("开始菜单");
    expect(kindLabel("app_data")).toBe("AppData");
  });
});

describe("汇总与选择", () => {
  it("默认全选、无需 token", () => {
    const sum = summarize(cands, initialRowState(cands));
    expect(sum.selected).toBe(3);
    expect(sum.selectedBytes).toBe(400);
    expect(sum.quarantineCount).toBe(3);
    expect(sum.directCount).toBe(0);
    expect(sum.needsToken).toBe(false);
  });

  it("有 direct 选择 → needsToken 且字节只算勾选项", () => {
    const s = initialRowState(cands);
    s.set("c1", { checked: true, disposition: "direct" });
    s.set("c3", { checked: false, disposition: "quarantine" });
    const sum = summarize(cands, s);
    expect(sum.selected).toBe(2);
    expect(sum.selectedBytes).toBe(350);
    expect(sum.directCount).toBe(1);
    expect(sum.needsToken).toBe(true);
  });

  it("buildSelections 仅输出勾选项且去向正确", () => {
    const s = initialRowState(cands);
    s.set("c2", { checked: true, disposition: "direct" });
    s.set("c3", { checked: false, disposition: "quarantine" });
    const sel = buildSelections(s);
    expect(sel).toHaveLength(2);
    expect(sel).toContainEqual({ candidateId: "c1", disposition: "quarantine" });
    expect(sel).toContainEqual({ candidateId: "c2", disposition: "direct" });
    // 绝不输出 recycle。
    expect(sel.every((x) => x.disposition === "quarantine" || x.disposition === "direct")).toBe(
      true,
    );
  });

  it("toggleAll 不改变去向设置", () => {
    const s = initialRowState(cands);
    s.set("c1", { checked: true, disposition: "direct" });
    const off = toggleAll(s, cands, false);
    expect(summarize(cands, off).selected).toBe(0);
    const on = toggleAll(off, cands, true);
    expect(on.get("c1")?.disposition).toBe("direct");
    expect(summarize(cands, on).needsToken).toBe(true);
  });

  it("rootTail 取末段（正反斜杠）", () => {
    expect(rootTail("C:\\Program Files\\App\\")).toBe("App");
    expect(rootTail("C:/Users/u/Start Menu/7-Zip.lnk")).toBe("7-Zip.lnk");
  });
});
