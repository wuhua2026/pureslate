/** crashModel 纯函数单测（P4-03/R24）。 */
import { describe, expect, it } from "vitest";
import type { CrashDumpPreview, CrashRecoveryReport } from "../../types/ipc";
import { describeException, moduleLine, previewSummary, recoveryNotice } from "../crashModel";

function preview(partial: Partial<CrashDumpPreview>): CrashDumpPreview {
  return {
    fileName: "pureslate-crash-x.dmp",
    sizeBytes: 123,
    ts: 1,
    ...partial,
  };
}

describe("describeException", () => {
  it("常见异常代码给出中文释义", () => {
    expect(describeException(0xc0000005)).toContain("访问冲突");
    expect(describeException(0xc0000409)).toContain("栈缓冲区溢出");
  });

  it("未知代码十六进制大写补零展示", () => {
    expect(describeException(0x1234)).toBe("0x00001234");
  });

  it("无异常代码标注非硬件异常", () => {
    expect(describeException(undefined)).toContain("无异常记录");
  });
});

describe("moduleLine / previewSummary", () => {
  it("模块行用 · 连接，空列表占位", () => {
    expect(moduleLine(preview({ modules: ["ntdll.dll", "pureslate.exe"] }))).toBe(
      "ntdll.dll · pureslate.exe",
    );
    expect(moduleLine(preview({ modules: [] }))).toContain("无模块信息");
  });

  it("预览摘要有错误给原文，否则给异常+模块计数", () => {
    expect(previewSummary(preview({ error: "文件损坏" }))).toContain("文件损坏");
    expect(
      previewSummary(preview({ moduleCount: 3, modules: ["a.dll", "b.dll", "c.dll"] })),
    ).toContain("模块 3 个");
  });
});

describe("recoveryNotice", () => {
  const base: CrashRecoveryReport = {
    ranAt: 1000,
    orphansFound: 3,
    restored: 1,
    conflictRestored: 1,
    adopted: 1,
    irreversible: 0,
    untouched: 0,
    failures: [],
  };

  it("无恢复运行或无孤儿返回 null", () => {
    expect(recoveryNotice(null)).toBeNull();
    expect(recoveryNotice({ ...base, ranAt: 0 })).toBeNull();
    expect(recoveryNotice({ ...base, orphansFound: 0 })).toBeNull();
  });

  it("各分项计数进入文案", () => {
    const text = recoveryNotice(base) ?? "";
    expect(text).toContain("3 项");
    expect(text).toContain("1 项已自动还原");
    expect(text).toContain("移入冲突目录");
    expect(text).toContain("补登记入隔离区");
  });

  it("失败项与不可逆项提示", () => {
    const text =
      recoveryNotice({ ...base, restored: 0, conflictRestored: 0, adopted: 0, irreversible: 2, failures: ["x"] }) ?? "";
    expect(text).toContain("不可逆去向");
    expect(text).toContain("1 项处理失败");
  });
});
