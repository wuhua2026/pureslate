import { describe, expect, it } from "vitest";
import type { CleanProgressEvent } from "../../types/ipc";
import {
  aggregateCleanProgress,
  computeFreedQuarantined,
  totalProcessed,
} from "../cleanMetrics";

function evt(
  path: string,
  disposition: CleanProgressEvent["disposition"],
  state: CleanProgressEvent["state"],
  doneBytes: number,
): CleanProgressEvent {
  return { txId: "tx", itemPath: path, disposition, doneBytes, totalBytes: doneBytes, state };
}

describe("清理进度聚合与语义（P2-06 cleanMetrics）", () => {
  it("同一 (去向, 路径) 多次节流事件去重，只取最后一条（完成态）", () => {
    const events: CleanProgressEvent[] = [
      evt("/a.tmp", "direct", "ok", 4), // 节流中间态（done<total 前缀，这里模拟）
      evt("/a.tmp", "direct", "ok", 10), // 完成态覆盖
    ];
    const t = aggregateCleanProgress(events);
    // 仅计最后一条：okBytes=10，okCount=1。
    expect(t.direct.okBytes).toBe(10);
    expect(t.direct.okCount).toBe(1);
  });

  it("分去向聚合 ok/fail/skip 计数与字节", () => {
    const events: CleanProgressEvent[] = [
      evt("/d1", "direct", "ok", 100),
      evt("/d2", "direct", "skip", 0),
      evt("/r1", "recycle", "ok", 50),
      evt("/q1", "quarantine", "ok", 30),
      evt("/q2", "quarantine", "fail", 0),
    ];
    const t = aggregateCleanProgress(events);
    expect(t.direct.okBytes).toBe(100);
    expect(t.direct.okCount).toBe(1);
    expect(t.direct.skipCount).toBe(1);
    expect(t.recycle.okBytes).toBe(50);
    expect(t.quarantine.okBytes).toBe(30);
    expect(t.quarantine.failCount).toBe(1);
    expect(totalProcessed(t)).toBe(3);
  });

  it("语义区分：已释放 = direct+recycle，已隔离 = quarantine；fail/skip 不计", () => {
    const events: CleanProgressEvent[] = [
      evt("/d1", "direct", "ok", 100),
      evt("/d2", "direct", "skip", 0),
      evt("/r1", "recycle", "ok", 10),
      evt("/q1", "quarantine", "ok", 30),
      evt("/q2", "quarantine", "fail", 0),
    ];
    const { freedBytes, quarantinedBytes } = computeFreedQuarantined(
      aggregateCleanProgress(events),
    );
    expect(freedBytes).toBe(110); // 100 + 10
    expect(quarantinedBytes).toBe(30); // 仅 ok
  });

  it("空事件返回全零", () => {
    const t = aggregateCleanProgress([]);
    expect(t.direct.okBytes).toBe(0);
    expect(t.recycle.okCount).toBe(0);
    expect(t.quarantine.skipCount).toBe(0);
  });
});