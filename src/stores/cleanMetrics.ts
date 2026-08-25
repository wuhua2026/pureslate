/**
 * 清理进度聚合与「已释放 vs 已隔离」语义计算——纯函数，无 DOM 依赖，可 vitest 直接单测。
 *
 * SAFETY §4.5：禁止混用「已清理」。首行必须区分：
 *   - 已释放（freed）= direct + recycle 去向中 ok 的字节（真正让渡给系统/回收站）；
 *   - 已隔离（quarantined）= quarantine 去向中 ok 的字节（尚未释放，14 天可还原）。
 */
import type { CleanProgressEvent, Disposition, Grade } from "../types/ipc";

/** 三个清理去向的固定顺序（渲染/统计基准）。 */
export const ALL_DISPOSITIONS: Disposition[] = ["direct", "recycle", "quarantine"];

export interface DispositionTotals {
  okBytes: number; // 该去向 ok 项完成字节（done 时的累计）
  okCount: number;
  failCount: number;
  skipCount: number;
}

/** 按去向聚合结果。 */
export type CleanTotals = Record<Disposition, DispositionTotals>;

function freshTotals(): DispositionTotals {
  return { okBytes: 0, okCount: 0, failCount: 0, skipCount: 0 };
}

/** 逐项唯一键：同文件同一去向的多次节流事件视为同一条，取最后一条（完成态）为准。 */
function eventKey(e: CleanProgressEvent): string {
  return `${e.disposition}::${e.itemPath}`;
}

/**
 * 聚合 clean_progress 事件为分去向总计。
 * - 同一 (disposition, itemPath) 可能推送多次（后端 200ms 节流中间态 + 完成态），
 *   这里按 key 覆盖、只保留最后一条；该项的 doneBytes 即完成字节。
 * - ok → 计 okBytes / okCount；fail/skip → 计对应计数（不贡献已释放/隔离字节）。
 */
export function aggregateCleanProgress(events: CleanProgressEvent[]): CleanTotals {
  const totals: CleanTotals = {
    direct: freshTotals(),
    recycle: freshTotals(),
    quarantine: freshTotals(),
  };
  if (events.length === 0) return totals;

  const last = new Map<string, CleanProgressEvent>();
  for (const e of events) last.set(eventKey(e), e);

  for (const e of last.values()) {
    const t = totals[e.disposition];
    if (e.state === "ok") {
      t.okBytes += e.doneBytes;
      t.okCount += 1;
    } else if (e.state === "fail") {
      t.failCount += 1;
    } else {
      t.skipCount += 1;
    }
  }
  return totals;
}

/** 已释放 vs 已隔离（SAFETY §4.5 语义核心）。 */
export function computeFreedQuarantined(
  totals: CleanTotals,
): { freedBytes: number; quarantinedBytes: number } {
  return {
    freedBytes: totals.direct.okBytes + totals.recycle.okBytes,
    quarantinedBytes: totals.quarantine.okBytes,
  };
}

/** 各去向累计完成项数（ok），供 UI 概括。 */
export function totalProcessed(totals: CleanTotals): number {
  return ALL_DISPOSITIONS.reduce((s, d) => s + totals[d].okCount, 0);
}

/** 分去向总字节（ok 项完成字节），供进度条 total。 */
export function totalOkBytes(totals: CleanTotals): number {
  return ALL_DISPOSITIONS.reduce((s, d) => s + totals[d].okBytes, 0);
}

// 去向→分级归属（「进隔离区」的范围：黄/红，供 UI 文案区分）。
export const DISPOSITION_GRADES: Record<Disposition, Grade[]> = {
  direct: ["green"],
  recycle: ["green"],
  quarantine: ["yellow", "red"],
};