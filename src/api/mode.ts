/**
 * 运行模式判定：VITE_MOCK=true 时走 mock 层而非真实 invoke。
 */
export function isMockMode(): boolean {
  return import.meta.env.VITE_MOCK === "true";
}