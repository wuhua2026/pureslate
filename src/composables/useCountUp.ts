import { onBeforeUnmount, ref, watch, type Ref } from "vue";

/**
 * R29（SPEC §7）：数字补间——挂载与目标变化时，用 rAF + ease-out 把当前值平滑到目标值。
 * 仅用于"已落定"的汇总数字（报告总量/百分比），高频实时数据（扫描实时字节）不要用。
 *
 * - 动 transform/opacity 无关：本 composable 只改数值，不碰 DOM 样式；
 * - 系统开启「减少动态效果」时直接跳到目标值；
 * - 组件卸载时取消未完成帧，避免对已卸载组件写入。
 */
export function useCountUp(
  target: Ref<number> | (() => number),
  options: { durationMs?: number; immediate?: boolean } = {},
): Ref<number> {
  const durationMs = options.durationMs ?? 600;
  const immediate = options.immediate ?? true;
  const getTarget = typeof target === "function" ? target : () => target.value;

  const display = ref(getTarget());
  let rafId = 0;
  let from = display.value;
  let startTs = 0;

  function prefersReducedMotion(): boolean {
    return (
      typeof window !== "undefined" &&
      window.matchMedia?.("(prefers-reduced-motion: reduce)").matches === true
    );
  }

  // ease-out cubic：起步快、收尾稳，符合"结果落定"语感。
  function easeOut(t: number): number {
    return 1 - Math.pow(1 - t, 3);
  }

  function cancelFrame(): void {
    if (rafId !== 0) {
      cancelAnimationFrame(rafId);
      rafId = 0;
    }
  }

  function animateTo(to: number): void {
    cancelFrame();
    if (durationMs <= 0 || prefersReducedMotion()) {
      display.value = to;
      return;
    }
    from = display.value;
    if (from === to) return;
    startTs = 0;
    const step = (ts: number): void => {
      if (startTs === 0) startTs = ts;
      const t = Math.min(1, (ts - startTs) / durationMs);
      display.value = Math.round(from + (to - from) * easeOut(t));
      if (t < 1) {
        rafId = requestAnimationFrame(step);
      } else {
        display.value = to;
        rafId = 0;
      }
    };
    rafId = requestAnimationFrame(step);
  }

  if (immediate) {
    // 挂载时从 0 补间到初值（首屏叙事感）；若初值就是 0 则保持。
    if (display.value !== 0) {
      display.value = 0;
      animateTo(getTarget());
    }
    watch(getTarget, (to) => animateTo(to));
  } else {
    watch(getTarget, (to) => animateTo(to));
  }

  onBeforeUnmount(cancelFrame);
  return display;
}
