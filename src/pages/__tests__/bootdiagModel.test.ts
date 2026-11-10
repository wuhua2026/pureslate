import { describe, expect, it } from "vitest";
import type {
  BootIssue,
  BootPowerData,
  BootPhaseBreakdown,
  BootStartupEntry,
} from "../../types/ipc";
import {
  approvedLabel,
  failedSourceLabel,
  formatMs,
  formatWhen,
  isSystemTask,
  issueKindLabel,
  latestSample,
  phaseSegments,
  powerContradiction,
  sortedIssues,
  sourceStats,
  statusLabel,
  statusTone,
} from "../bootdiagModel";

describe("状态文案", () => {
  it("四态各有中文名与色调", () => {
    expect(statusLabel("ok")).toBe("正常");
    expect(statusLabel("partial")).toBe("部分可用");
    expect(statusLabel("unavailable")).toBe("无法读取");
    expect(statusLabel("denied")).toBe("权限不足");
    expect(statusTone("ok")).toBe("ok");
    expect(statusTone("denied")).toBe("denied");
  });
});

describe("指标格式化", () => {
  it("缺失显示破折号而不是 0", () => {
    expect(formatMs(undefined)).toBe("—");
    expect(formatMs(null)).toBe("—");
    expect(formatMs(Number.NaN)).toBe("—");
  });

  it("毫秒/秒/分钟三档", () => {
    expect(formatMs(0)).toBe("0 ms");
    expect(formatMs(850)).toBe("850 ms");
    expect(formatMs(34_857)).toBe("34.9 s");
    expect(formatMs(125_000)).toBe("2 分 5 秒");
  });

  it("RFC3339 带 UTC 标识，非法返回破折号", () => {
    expect(formatWhen("2026-11-08T23:12:05Z")).toBe("2026-11-08 23:12 UTC");
    expect(formatWhen("bad")).toBe("—");
    expect(formatWhen(undefined)).toBe("—");
  });
});

describe("阶段构成", () => {
  it("只统计可得字段，百分比以可得之和为分母", () => {
    const phases: BootPhaseBreakdown = {
      kernelInitMs: 1000,
      driverInitMs: 3000,
      explorerInitMs: undefined,
    };
    const segs = phaseSegments(phases);
    expect(segs.map((s) => s.label)).toEqual(["内核初始化", "驱动初始化"]);
    expect(segs[0].pct).toBeCloseTo(25, 5);
    expect(segs[1].pct).toBeCloseTo(75, 5);
  });

  it("全缺失或全 0 → 空数组（不画 0 段）", () => {
    expect(phaseSegments(undefined)).toEqual([]);
    expect(phaseSegments({ kernelInitMs: 0 })).toEqual([]);
  });
});

describe("启动项分类", () => {
  const entry = (kind: BootStartupEntry["kind"], name: string): BootStartupEntry => ({
    id: name,
    name,
    command: "cmd",
    kind,
    scope: "machine",
  });

  it("来源统计固定顺序且保留 0", () => {
    const stats = sourceStats([entry("run", "A"), entry("service_auto", "S")]);
    expect(stats.map((s) => s.count)).toEqual([1, 0, 0, 1, 0, 0]);
    expect(stats[0].label).toBe("注册表 Run");
  });

  it("Microsoft\\ 前缀的计划任务视为系统任务", () => {
    expect(isSystemTask(entry("scheduled_task_boot", "Microsoft\\Windows\\X"))).toBe(true);
    expect(isSystemTask(entry("scheduled_task_logon", "Acme\\Updater"))).toBe(false);
    // Run 项即使名字像路径也不算系统任务。
    expect(isSystemTask(entry("run", "Microsoft\\Windows\\X"))).toBe(false);
  });

  it("审批状态文案覆盖四态", () => {
    expect(approvedLabel("disabled")).toBe("系统已禁用");
    expect(approvedLabel("enabled")).toBe("已启用");
    expect(approvedLabel("unknown")).toBe("无审批记录");
    expect(approvedLabel(undefined)).toBe("—");
  });
});

describe("失败来源翻译", () => {
  it("按内核侧命名翻译成人话", () => {
    expect(failedSourceLabel("tasks_dir_empty_or_denied")).toContain("权限");
    expect(failedSourceLabel("services_scm_code_5")).toContain("5");
    expect(failedSourceLabel("power_caps_status_0xC0000023")).toBe("电源能力查询失败");
    expect(failedSourceLabel("mystery_source")).toBe("mystery_source");
  });
});

describe("事件明细", () => {
  it("降级耗时降序、缺失排后", () => {
    const issues: BootIssue[] = [
      { eventId: 101, kind: "application", degradationTimeMs: 100 },
      { eventId: 102, kind: "driver" },
      { eventId: 103, kind: "service", degradationTimeMs: 900 },
    ];
    expect(sortedIssues(issues).map((i) => i.eventId)).toEqual([103, 101, 102]);
  });

  it("事件类别有中文名", () => {
    expect(issueKindLabel("backgroundOptimization")).toBe("后台优化");
    expect(issueKindLabel("unknown")).toBe("未知");
  });

  it("最新样本取第一项（内核按倒序返回）", () => {
    expect(latestSample([])).toBeUndefined();
    expect(latestSample([{ bootTimeMs: 1 }, { bootTimeMs: 2 }])?.bootTimeMs).toBe(1);
  });
});

describe("电源矛盾态", () => {
  const base: BootPowerData = { fastStartupRegistryPresent: true };

  it("开关=1 但无休眠文件 → 黄色提示", () => {
    expect(powerContradiction({ ...base, fastStartupEnabled: true, hiberFilePresent: false })).toContain(
      "没有休眠文件",
    );
  });

  it("开关=1 但能力位为否 → 提示", () => {
    expect(powerContradiction({ ...base, fastStartupEnabled: true, hiberbootCapability: false })).toContain(
      "能力位",
    );
  });

  it("一致或无数据 → 不提示", () => {
    expect(
      powerContradiction({ ...base, fastStartupEnabled: true, hiberFilePresent: true, hiberbootCapability: true }),
    ).toBeNull();
    expect(powerContradiction(undefined)).toBeNull();
  });
});
