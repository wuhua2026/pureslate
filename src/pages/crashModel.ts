/**
 * 崩溃数据展示模型（P4-03/R24）——纯函数，无 DOM 依赖，可被 vitest 直接单测。
 * Settings 页崩溃卡片用：异常代码释义、模块列表行、上传文案。
 */
import type { CrashDumpPreview, CrashRecoveryReport } from "../types/ipc";

/** 异常代码 → 释义（None = 非异常崩溃，如 panic）。 */
export function describeException(code?: number): string {
  if (code == null) return "无异常记录（非硬件异常，如程序自身错误退出）";
  switch (code >>> 0) {
    case 0xc0000005:
      return "0xC0000005 内存访问冲突";
    case 0xc0000409:
      return "0xC0000409 栈缓冲区溢出保护触发";
    case 0x80000003:
      return "0x80000003 断点异常";
    default:
      return `0x${(code >>> 0).toString(16).toUpperCase().padStart(8, "0")}`;
  }
}

/** 预览模块行（" · " 连接；空列表给占位文案）。 */
export function moduleLine(p: CrashDumpPreview): string {
  const mods = p.modules ?? [];
  if (mods.length === 0) return "（无模块信息）";
  return mods.join(" · ");
}

/** 预览摘要行（供列表内直接展示）。 */
export function previewSummary(p: CrashDumpPreview): string {
  if (p.error) return `解析失败：${p.error}`;
  return `${describeException(p.exceptionCode)} · 加载模块 ${p.moduleCount ?? 0} 个`;
}

/** 启动恢复通知文案（无恢复事件返回 null；Home 横幅用）。 */
export function recoveryNotice(r: CrashRecoveryReport | null | undefined): string | null {
  if (!r || !r.ranAt || r.orphansFound === 0) return null;
  const detail: string[] = [];
  if (r.restored > 0) detail.push(`${r.restored} 项已自动还原`);
  if (r.conflictRestored > 0) detail.push(`${r.conflictRestored} 项因原路径占用移入冲突目录`);
  if (r.adopted > 0) detail.push(`${r.adopted} 项已补登记入隔离区（14 天内可还原）`);
  if (r.irreversible > 0) detail.push(`${r.irreversible} 项为不可逆去向，已记录待人工核查`);
  if (r.failures.length > 0) detail.push(`${r.failures.length} 项处理失败`);
  const body = detail.length > 0 ? detail.join("，") : "无需处理";
  return `检测到上次清理被中断：共 ${r.orphansFound} 项待处理，${body}。`;
}
