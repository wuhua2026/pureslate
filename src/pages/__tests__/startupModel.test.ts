/** Startup 页纯函数单测（P3-03）。 */
import { describe, expect, it } from "vitest";
import type { StartupEntry } from "../../types/ipc";
import {
  disableRiskText,
  impactLabel,
  publisherLabel,
  sortByImpactDesc,
  sourceLabel,
  splitByEnabled,
  toggleFailText,
} from "../startupModel";

function entry(partial: Partial<StartupEntry>): StartupEntry {
  return {
    id: "x",
    name: "App",
    command: "C:\\app.exe",
    source: "hkcu_run",
    impact: "medium",
    enabled: true,
    ...partial,
  };
}

describe("sortByImpactDesc", () => {
  it("高→中→低，同档按名称字典序", () => {
    const items = [
      entry({ name: "B", impact: "low" }),
      entry({ name: "Z", impact: "high" }),
      entry({ name: "A", impact: "low" }),
      entry({ name: "M", impact: "medium" }),
    ];
    expect(sortByImpactDesc(items).map((e) => e.name)).toEqual(["Z", "M", "A", "B"]);
  });

  it("不修改原数组", () => {
    const items = [entry({ impact: "low" }), entry({ impact: "high" })];
    sortByImpactDesc(items);
    expect(items[0].impact).toBe("low");
  });
});

describe("splitByEnabled", () => {
  it("按 enabled 分两组", () => {
    const items = [
      entry({ id: "1", enabled: true }),
      entry({ id: "2", enabled: false }),
      entry({ id: "3", enabled: true }),
    ];
    const g = splitByEnabled(items);
    expect(g.enabled.map((e) => e.id)).toEqual(["1", "3"]);
    expect(g.disabled.map((e) => e.id)).toEqual(["2"]);
  });
});

describe("文案函数", () => {
  it("来源/影响中文名全覆盖", () => {
    expect(sourceLabel("hkcu_run")).toContain("当前用户");
    expect(sourceLabel("hklm_run")).toContain("本机");
    expect(sourceLabel("startup_folder")).toBe("启动文件夹");
    expect(sourceLabel("task_scheduler")).toBe("计划任务");
    expect(impactLabel("high")).toBe("高影响");
    expect(impactLabel("medium")).toBe("中影响");
    expect(impactLabel("low")).toBe("低影响");
  });

  it("发布者缺省/空白 → 未知发布者", () => {
    expect(publisherLabel(undefined)).toBe("未知发布者");
    expect(publisherLabel("  ")).toBe("未知发布者");
    expect(publisherLabel("某公司")).toBe("某公司");
  });

  it("禁用风险文案含名称与可恢复承诺", () => {
    const t = disableRiskText(entry({ name: "影音伴侣" }));
    expect(t).toContain("影音伴侣");
    expect(t).toContain("不会");
    expect(t).toContain("恢复");
  });

  it("HKLM 失败提示管理员权限，其余提示重试", () => {
    expect(toggleFailText(entry({ source: "hklm_run" }))).toContain("管理员");
    expect(toggleFailText(entry({ source: "hkcu_run" }))).toContain("失败");
    expect(toggleFailText(entry({ source: "task_scheduler" }))).toContain("失败");
  });
});
