/**
 * 🔴 二次确认 token（I-2 · P4-06 安全审计收口；SPEC §5 语义落地）。
 *
 * 令牌由**后端签发**（`confirm_token_issue`：仅专家模式可签、一次性消费），
 * 前端负责展示与逐字输入比对；提交值由后端取出消费——前端自造值不再被信任
 * （旧行为：前端 generateToken + 后端仅查非空，可被 webview 注入绕过）。
 * 形如 `PS-XXXX-XXXX`（大写字母+数字 4 位 × 2 段，去易混淆字符，与后端同源）。
 */
import { confirm_token_issue } from "../api/commands";

/** 从后端签发一次性令牌。失败原样抛出（非专家模式/状态不可用），调用方展示。 */
export async function issueToken(): Promise<string> {
  return confirm_token_issue();
}

/** 校验用户输入与期望一致性（忽略首尾空白与大小写，字母数字不变量）。 */
export function verifyToken(input: string, expected: string): boolean {
  const a = input.trim().toUpperCase();
  const b = expected.trim().toUpperCase();
  return a.length > 0 && a === b;
}
