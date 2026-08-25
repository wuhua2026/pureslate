/**
 * 🔴 二次确认 token（SAFETY §8 / SPEC §5 专家模式语义）。
 * 形如 `PS-XXXX-XXXX`（大写字母+数字 4 位 × 2 段）。前端生成并展示，
 * 用户须逐字输入一致才放行清理（ACKnowledge 式二次确认）；后端仅兜底判断非空。
 */
const CHARS = "ABCDEFGHJKMNPQRSTUVWXYZ23456789"; // 去掉易混淆 I/L/O/0/1

function randomSegment(len: number): string {
  const out = new Array<string>(len);
  // node 18+ / 现代浏览器均提供全局 crypto.getRandomValues。
  const bytes = new Uint8Array(len);
  globalThis.crypto.getRandomValues(bytes);
  for (let i = 0; i < len; i++) out[i] = CHARS[bytes[i] % CHARS.length];
  return out.join("");
}

/** 生成一次性 token。 */
export function generateToken(): string {
  return `PS-${randomSegment(4)}-${randomSegment(4)}`;
}

/** 校验用户输入与期望一致性（忽略首尾空白与大小写，字母数字不变量）。 */
export function verifyToken(input: string, expected: string): boolean {
  const a = input.trim().toUpperCase();
  const b = expected.trim().toUpperCase();
  return a.length > 0 && a === b;
}