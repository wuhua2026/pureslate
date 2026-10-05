/**
 * 扫描会话 store（P1-08 真数据联调）。
 * - mock 模式：定时器模拟进度，逐步揭示 mock 项（实时累计）；
 * - 真实模式：scan_start → 订阅 scan_progress/scan_done → scan_get_items。
 * 跨 /scan → /report 导航持久，报告页读本 store.items。
 */
import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { commands, events, isMockMode } from "../api";
import { buildMockScanResult } from "../mocks/data";
import type {
  FoundBytes,
  ScanDimension,
  ScanItem,
  ScanPhase,
  ScanProfile,
  ScanResult,
} from "../types/ipc";

/** 快速维度（v0.1.2 默认体检）：temp/cache/privacy 为定点目录扫描、startup 为
 * 注册表/计划任务枚举——秒级~分钟级，任何形态（含便携非提权）体验可控。 */
const QUICK_DIMENSIONS: ScanDimension[] = ["temp", "cache", "startup", "privacy"];
/** 深度维度：large（全卷遍历）+ dup（内容哈希）——大容量盘 + 非提权（无 MFT 快速
 * 引擎）时可能十分钟级，改为显式选择（v0.1.2：默认全开曾导致体检"只见临时文件"）。 */
const DEEP_DIMENSIONS: ScanDimension[] = ["large", "dup"];

/** 组合体检 profile：快速四维度恒开，深度维度（large/dup）按 deep 开关。 */
function profileWith(deep: boolean): ScanProfile {
  const dimensions: Partial<Record<ScanDimension, boolean>> = {};
  for (const d of QUICK_DIMENSIONS) dimensions[d] = true;
  for (const d of DEEP_DIMENSIONS) dimensions[d] = deep;
  return { dimensions };
}

export const useScanStore = defineStore("scan", () => {
  const scanId = ref("");
  const running = ref(false);
  const phase = ref<ScanPhase>("walking");
  const percent = ref(0);
  const currentPath = ref("");
  const found = ref<FoundBytes>({ green: 0, yellow: 0, red: 0 });
  const elapsedMs = ref(0);
  const result = ref<ScanResult | null>(null);
  const items = ref<ScanItem[]>([]);
  const error = ref("");

  let timer: number | undefined;
  let unlisten: Array<() => void> = [];

  const foundBytesTotal = computed(() => found.value.green + found.value.yellow + found.value.red);
  const isDone = computed(() => running.value === false && result.value !== null);

  function reset() {
    scanId.value = "";
    phase.value = "walking";
    percent.value = 0;
    currentPath.value = "";
    found.value = { green: 0, yellow: 0, red: 0 };
    elapsedMs.value = 0;
    result.value = null;
    items.value = [];
    error.value = "";
  }

  function summarize(list: ScanItem[]): FoundBytes {
    const s: FoundBytes = { green: 0, yellow: 0, red: 0 };
    for (const it of list) s[it.grade] += it.sizeBytes;
    return s;
  }

  function teardownTimers() {
    if (timer !== undefined) {
      window.clearInterval(timer);
      timer = undefined;
    }
    unlisten.forEach((u) => u());
    unlisten = [];
  }

  // ---- mock 扫描 ----
  function runMock() {
    const data = buildMockScanResult();
    const allItems = data.items;
    const started = Date.now();
    const MOCK_MS = 2600;
    timer = window.setInterval(() => {
      const p = Math.min(100, Math.round(((Date.now() - started) / MOCK_MS) * 100));
      percent.value = p;
      elapsedMs.value = Date.now() - started;
      phase.value = p >= 100 ? "aggregating" : "walking";
      const shown = Math.max(1, Math.ceil((p / 100) * allItems.length));
      const list = allItems.slice(0, shown);
      items.value = list;
      found.value = summarize(list);
      currentPath.value = list.length ? list[list.length - 1].path : "";
      if (p >= 100) {
        window.clearInterval(timer!);
        timer = undefined;
        items.value = allItems;
        found.value = summarize(allItems);
        result.value = { ...data };
        running.value = false;
        phase.value = "aggregating";
      }
    }, 90);
  }

  // ---- 真实扫描 ----
  async function runReal() {
    unlisten.push(
      await events.onScanProgress((p) => {
        if (p.scanId !== scanId.value) return;
        phase.value = p.phase;
        percent.value = p.percent;
        currentPath.value = p.currentPath;
        found.value = p.foundBytes;
        elapsedMs.value = p.elapsedMs;
      }),
    );
    unlisten.push(
      await events.onScanDone(async (r) => {
        if (r.scanId !== scanId.value) return;
        result.value = r;
        running.value = false;
        items.value = await commands.scan_get_items({ scanId: r.scanId, offset: 0, limit: 100000 });
      }),
    );

    const id = await commands.scan_start(profileWith(deepScan.value));
    if (!id) {
      error.value = "扫描启动失败";
      running.value = false;
      return;
    }
    scanId.value = id;
  }

  /** 深度体检开关（v0.1.2）：Home 页"深度体检"按钮置位，Scan.vue onMounted 启动时
   *  读取并纳入 large/dup 维度；默认（快速体检）不含慢维度。 */
  const deepScan = ref(false);
  function setDeepScan(v: boolean) {
    deepScan.value = v;
  }

  async function start() {
    if (running.value) return;
    teardownTimers();
    reset();
    running.value = true;
    if (isMockMode()) {
      runMock();
    } else {
      try {
        await runReal();
      } catch (e) {
        error.value = String(e);
        running.value = false;
      }
    }
  }

  function cancel() {
    if (!running.value) return;
    if (isMockMode()) {
      teardownTimers();
      running.value = false;
      phase.value = "walking";
    } else if (scanId.value) {
      void commands.scan_cancel(scanId.value);
      running.value = false;
    }
  }

  return {
    scanId,
    running,
    phase,
    percent,
    currentPath,
    found,
    elapsedMs,
    result,
    items,
    error,
    deepScan,
    setDeepScan,
    foundBytesTotal,
    isDone,
    start,
    cancel,
  };
});