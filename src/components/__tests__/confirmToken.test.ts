import { describe, expect, it } from "vitest";
import { verifyToken } from "../confirmToken";

// I-2（P4-06）：令牌改由后端签发（confirm_token_issue），前端仅保留输入比对
// 纯函数；generateToken 已移除（前端自造值不再被后端信任）。
describe("🔴 二次确认 token（I-2 后端签发后的前端比对）", () => {
  it("verifyToken：逐字一致（大小写不敏感、忽略首尾空白）", () => {
    const tk = "PS-AB2D-EFGH";
    expect(verifyToken(tk, tk)).toBe(true);
    expect(verifyToken(tk.toLowerCase(), tk)).toBe(true);
    expect(verifyToken(`  ${tk}  `, tk)).toBe(true);
  });

  it("verifyToken：不一致或为空 → false", () => {
    const tk = "PS-AB2D-EFGH";
    expect(verifyToken("PS-XXXX-XXXX", tk)).toBe(false);
    expect(verifyToken("", tk)).toBe(false);
    expect(verifyToken("   ", tk)).toBe(false);
  });
});
