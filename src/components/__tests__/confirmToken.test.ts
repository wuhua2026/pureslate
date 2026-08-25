import { describe, expect, it } from "vitest";
import { generateToken, verifyToken } from "../confirmToken";

describe("🔴 二次确认 token（P2-06 confirmToken）", () => {
  it("generateToken 形如 PS-XXXX-XXXX（大写字母数字）", () => {
    const t = generateToken();
    expect(t).toMatch(/^PS-[A-Z2-9]{4}-[A-Z2-9]{4}$/);
    // 每次不同
    expect(generateToken()).not.toBe(t);
  });

  it("verifyToken：逐字一致（大小写不敏感、忽略首尾空白）", () => {
    const tk = generateToken();
    expect(verifyToken(tk, tk)).toBe(true);
    expect(verifyToken(tk.toLowerCase(), tk)).toBe(true);
    expect(verifyToken(`  ${tk}  `, tk)).toBe(true);
  });

  it("verifyToken：不一致或为空 → false", () => {
    const tk = generateToken();
    expect(verifyToken("PS-XXXX-XXXX", tk)).toBe(false);
    expect(verifyToken("", tk)).toBe(false);
    expect(verifyToken("   ", tk)).toBe(false);
  });
});