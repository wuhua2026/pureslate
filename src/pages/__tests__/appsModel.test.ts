/** Apps 页模型单测（R27-1 枚举 / R27-2 执行流）。 */
import { describe, expect, it } from "vitest";
import type { InstalledApp } from "../../types/ipc";
import {
  canUninstall,
  filterApps,
  formatSize,
  installDateLabel,
  scopeLabel,
  summarizeApps,
  uninstallResultMeta,
} from "../appsModel";

function app(partial: Partial<InstalledApp>): InstalledApp {
  return {
    id: "x",
    name: "App",
    uninstallCommand: "",
    isMsi: false,
    arch: "x64",
    scope: "machine",
    ...partial,
  };
}

describe("filterApps", () => {
  const apps = [
    app({ id: "1", name: "Seven Zip", publisher: "Igor" }),
    app({ id: "2", name: "浏览器", publisher: "Google LLC", version: "120" }),
    app({ id: "3", name: "Runtime", scope: "user" }),
  ];

  it("空查询返回全部", () => {
    expect(filterApps(apps, "  ")).toHaveLength(3);
  });

  it("名称/发布者/版本大小写不敏感匹配", () => {
    expect(filterApps(apps, "seven").map((a) => a.id)).toEqual(["1"]);
    expect(filterApps(apps, "google").map((a) => a.id)).toEqual(["2"]);
    expect(filterApps(apps, "120").map((a) => a.id)).toEqual(["2"]);
    expect(filterApps(apps, "浏览器").map((a) => a.id)).toEqual(["2"]);
    expect(filterApps(apps, "nope")).toHaveLength(0);
  });
});

describe("summarizeApps", () => {
  it("按 scope/msi 计数", () => {
    const s = summarizeApps([
      app({ scope: "machine", isMsi: true }),
      app({ scope: "machine" }),
      app({ scope: "user" }),
    ]);
    expect(s).toEqual({ total: 3, machine: 2, user: 1, msi: 1 });
  });
});

describe("文案", () => {
  it("scopeLabel", () => {
    expect(scopeLabel("machine")).toBe("本机");
    expect(scopeLabel("user")).toBe("当前用户");
  });

  it("installDateLabel YYYYMMDD → YYYY-MM-DD", () => {
    expect(installDateLabel("20260930")).toBe("2026-09-30");
    expect(installDateLabel(undefined)).toBe("");
    expect(installDateLabel("2026")).toBe("");
  });

  it("formatSize", () => {
    expect(formatSize(undefined)).toBe("");
    expect(formatSize(0)).toBe("");
    expect(formatSize(5 * 1024 * 1024)).toBe("5.0 MB");
    expect(formatSize(3 * 1024 * 1024 * 1024)).toBe("3.00 GB");
  });
});

describe("R27-2 执行流", () => {
  it("canUninstall 仅认非空交互卸载命令", () => {
    expect(canUninstall(app({ uninstallCommand: '"C:\\App\\un.exe" /S' }))).toBe(true);
    expect(canUninstall(app({ uninstallCommand: "   " }))).toBe(false);
    expect(canUninstall(app({ uninstallCommand: "" }))).toBe(false);
  });

  it("uninstallResultMeta 六结果语气/标题", () => {
    expect(uninstallResultMeta("removed").tone).toBe("ok");
    expect(uninstallResultMeta("reboot_pending").tone).toBe("info");
    expect(uninstallResultMeta("cancelled").tone).toBe("info");
    expect(uninstallResultMeta("still_present").tone).toBe("warn");
    expect(uninstallResultMeta("timeout").tone).toBe("warn");
    expect(uninstallResultMeta("launch_failed").tone).toBe("err");
    // 标题非空（UI 直接渲染）。
    for (const r of [
      "removed",
      "reboot_pending",
      "cancelled",
      "still_present",
      "timeout",
      "launch_failed",
    ] as const) {
      expect(uninstallResultMeta(r).title.length).toBeGreaterThan(0);
    }
  });
});
