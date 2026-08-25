/**
 * 清理会话 store（P2-06 执行户）。
 * - mock 模式：定时推送假 clean_progress / clean_done；
 * - 真实模式：clean_execute → 订阅 clean_progress / clean_done → 终端态。
 * 分去向进度与「已释放 vs 已隔离」语义计算委托纯函数 cleanMetrics（可单测）。
 */
import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { commands, events, isMockMode } from "../api";
import { buildMockCleanProgressEvents } from "../mocks/data";
import type { CleanDoneEvent, CleanProgressEvent } from "../types/ipc";
import {
  aggregateCleanProgress,
  computeFreedQuarantined,
  totalOkBytes,
  totalProcessed,
  type CleanTotals,
} from "./cleanMetrics";

export const useCleanStore = defineStore("clean", () => {
  const txId = ref("");
  const running = ref(false);
  const error = ref("");
  const progressEvents = ref<CleanProgressEvent[]>([]);
  const result = ref<CleanDoneEvent | null>(null);
  // 本次待清理项总数（进度条分母；mock 用假事件数）。
  const itemTotal = ref(0);

  let unlisten: Array<() => void> = [];
  let timer: number | undefined;
  // 本次待清理 `item.id` 快照（start 时固定，避免后续变更漂移）。
  let itemsSnapshot: string[] = [];

  const totals = computed<CleanTotals>(() => aggregateCleanProgress(progressEvents.value));
  const freedQuarantined = computed(() => computeFreedQuarantined(totals.value));
  /** 已释放 = direct/回收站 完成体积（首行语义，SAFETY §4.5）。 */
  const freedBytes = computed(() => freedQuarantined.value.freedBytes);
  /** 已隔离 = 隔离区完成体积（尚未释放）。 */
  const quarantinedBytes = computed(() => freedQuarantined.value.quarantinedBytes);
  const doneBytes = computed(() => totalOkBytes(totals.value));
  const processed = computed(() => totalProcessed(totals.value));
  const isDone = computed(() => running.value === false && result.value !== null);

  function reset() {
    itemsSnapshot = [];
    itemTotal.value = 0;
    progressEvents.value = [];
    result.value = null;
    error.value = "";
  }

  function teardown() {
    if (timer !== undefined) {
      window.clearInterval(timer);
      timer = undefined;
    }
    unlisten.forEach((u) => u());
    unlisten = [];
  }

  // ---- mock 清理：逐步推送完成态假事件 ----
  function runMock() {
    const fake = buildMockCleanProgressEvents();
    txId.value = "mock-tx-0001";
    itemTotal.value = fake.length;
    let i = 0;
    timer = window.setInterval(() => {
      if (i >= fake.length) {
        window.clearInterval(timer!);
        timer = undefined;
        result.value = {
          txId: txId.value,
          total: fake.length,
          ok: fake.length - 1,
          fail: 0,
          skip: 1,
        };
        running.value = false;
        return;
      }
      progressEvents.value.push(fake[i]);
      i += 1;
    }, 260);
  }

  // ---- 真实清理 ----
  async function runReal(confirmToken?: string) {
    unlisten.push(
      await events.onCleanProgress((p) => {
        if (p.txId !== txId.value) return;
        progressEvents.value.push(p);
      }),
    );
    unlisten.push(
      await events.onCleanDone((r) => {
        if (r.txId !== txId.value) return;
        result.value = r;
        running.value = false;
      }),
    );

    let id: string;
    try {
      id = await commands.clean_execute({
        items: itemsSnapshot,
        ...(confirmToken ? { confirmToken } : {}),
      });
    } catch (e) {
      // 命令异常（如含 🔴 无 token 被拒绝）→ 置错，running 由 start 兜底置 false。
      error.value = String(e);
      running.value = false;
      return;
    }
    if (!id) {
      error.value = "清理未启动";
      running.value = false;
      return;
    }
    txId.value = id;
    itemTotal.value = itemsSnapshot.length;
  }

  async function start(items: string[], confirmToken?: string) {
    if (running.value) return;
    teardown();
    reset();
    itemsSnapshot = items;
    running.value = true;
    if (isMockMode()) {
      runMock();
    } else {
      try {
        await runReal(confirmToken);
      } catch (e) {
        error.value = String(e);
        running.value = false;
      }
    }
  }

  function cancel() {
    if (!running.value) return;
    if (isMockMode()) {
      teardown();
      running.value = false;
    } else if (txId.value) {
      void commands.clean_cancel(txId.value);
      running.value = false;
    }
  }

  return {
    txId,
    running,
    error,
    progressEvents,
    result,
    totals,
    freedBytes,
    quarantinedBytes,
    doneBytes,
    processed,
    itemTotal,
    isDone,
    start,
    cancel,
  };
});