/**
 * Quarantine 页数据模型——纯函数，无 DOM 依赖，可被 vitest 直接单测。
 * P3-06/R25：到期排序 + 到期提醒 + 容量/语义文案（SAFETY §4.4/§4.5）。
 */
import type { QuarantineEntry } from "../types/ipc";

/** 剩余天数升序（最先到期在前），同天数按原路径字典序（确定性）。 */
export function sortByExpiryAsc(entries: QuarantineEntry[]): QuarantineEntry[] {
  return [...entries].sort(
    (a, b) => a.daysLeft - b.daysLeft || a.originalPath.localeCompare(b.originalPath),
  );
}

/** 合计字节数。 */
export function totalBytes(entries: QuarantineEntry[]): number {
  return entries.reduce((acc, e) => acc + e.sizeBytes, 0);
}

/** 即将到期（剩余 ≤ daysThreshold 天，默认 3）的条目。 */
export function expiringSoon(entries: QuarantineEntry[], daysThreshold = 3): QuarantineEntry[] {
  return entries.filter((e) => e.daysLeft <= daysThreshold);
}

/** 语义首行（SAFETY §4.5）：严格区分"已隔离（尚未释放）"，禁止混用"已清理"）。 */
export function semanticHeadline(humanBytes: string): string {
  return `已隔离 ${humanBytes} · 尚未释放（保留期内可还原，到期自动清除）`;
}
