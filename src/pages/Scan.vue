<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { app_meta, disk_usage } from "../api/commands";
import { useScanStore } from "../stores/scan";

const store = useScanStore();
const router = useRouter();

// v0.1.2：非提权提示——便携/双击运行通常无管理员权限，MFT 快速引擎不启用，
// 深度维度（大文件全卷遍历/重复文件哈希）在大容量盘上会显著变慢。
const elevated = ref<boolean | null>(null);
const isDeep = store.deepScan;

// v0.1.5 反馈 3：完成态显式出口——自动跳转 3s 未离开/被取消时显示大按钮。
const doneExit = ref(false);
let doneTimer: number | undefined;

// v0.1.5 反馈 2：多卷进度提示——固定盘总数（disk_usage）+ currentPath 盘符推进序号。
const totalVolumes = ref(0);
const seenVolumes = ref<string[]>([]);
const volumeHint = computed(() => {
  if (totalVolumes.value <= 1 || seenVolumes.value.length === 0) return "";
  return `第 ${Math.min(seenVolumes.value.length, totalVolumes.value)} / ${totalVolumes.value} 卷`;
});

watch(
  () => store.currentPath,
  (p) => {
    const v = p.slice(0, 2);
    if (/^[A-Za-z]:$/.test(v) && !seenVolumes.value.includes(v)) {
      seenVolumes.value = [...seenVolumes.value, v];
    }
  },
);

onMounted(async () => {
  try {
    const meta = await app_meta();
    elevated.value = meta.isElevated ?? null;
  } catch {
    elevated.value = null;
  }
  try {
    const disks = await disk_usage();
    totalVolumes.value = disks.length;
  } catch {
    totalVolumes.value = 0;
  }
  void store.start();
});

onUnmounted(() => {
  if (doneTimer !== undefined) window.clearTimeout(doneTimer);
});

const PHASE_CN: Record<string, string> = {
  walking: "遍历扫描中",
  hashing: "哈希比对中",
  aggregating: "结果聚合中",
};
const PHASE_ICON: Record<string, string> = {
  walking: "📂",
  hashing: "🔍",
  aggregating: "🧮",
};

function formatBytes(b: number): string {
  if (b <= 0) return "0 B";
  const gb = b / (1024 * 1024 * 1024);
  if (gb >= 1) return gb >= 1024 ? `${(gb / 1024).toFixed(1)} TB` : `${gb.toFixed(2)} GB`;
  const mb = b / (1024 * 1024);
  if (mb >= 1) return `${mb.toFixed(1)} MB`;
  return `${Math.round(b / 1024)} KB`;
}

function fmtElapsed(ms: number): string {
  const s = Math.max(0, Math.round(ms / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

// 完成 → 跳转报告页（v0.1.5 反馈 3：取消后不自动跳，给"查看已收集结果"入口；
// 正常完成 3s 未离开也显示显式出口按钮兜底）。
watch(
  () => store.isDone,
  (done) => {
    if (!done) return;
    if (!store.cancelled) router.push("/report");
    const delay = store.cancelled ? 0 : 3000;
    doneTimer = window.setTimeout(() => {
      doneExit.value = true;
    }, delay);
  },
);
</script>

<template>
  <main class="scan">
    <header class="scan-head">
      <router-link to="/" class="back">← 首页</router-link>
      <h1>{{ isDeep ? "深度体检" : "一键体检" }}</h1>
      <p class="sub">只读扫描 · 不删除任何文件 · 全程可在确认后逐项处置</p>
    </header>

    <div v-if="store.error" class="error">扫描失败：{{ store.error }}</div>

    <p v-else-if="isDeep && elevated === false" class="slow-hint">
      当前未以管理员运行：大文件/重复文件维度使用慢速遍历，大容量盘耗时明显变长。
      可取消后右键"以管理员身份运行"本程序再次体检（快速体检不受影响）。
    </p>

    <!-- v0.1.5 反馈 3：取消后的部分结果入口 / 完成态兜底出口 -->
    <section v-else-if="store.isDone && doneExit" class="card done-card">
      <p class="done-text">{{ store.cancelled ? "体检已取消" : "体检完成" }}</p>
      <p class="done-sub">
        {{ store.cancelled ? "已收集的部分结果可查看（不写入任何文件）" : "正在进入报告…" }}
      </p>
      <button class="btn-done" @click="router.push('/report')">
        {{ store.cancelled ? "查看已收集结果" : "体检完成 → 查看报告" }}
      </button>
    </section>

    <section class="card" v-else>
      <!-- v0.1.5 反馈 2：旋转扫描动效 + 阶段图标 -->
      <div class="scan-visual">
        <div class="radar" :class="{ paused: !store.running }">
          <div class="radar-core">{{ PHASE_ICON[store.phase] ?? "📂" }}</div>
        </div>
        <div class="phase-col">
          <span class="phase">
            {{ PHASE_CN[store.phase] ?? store.phase }}
            <span v-if="volumeHint" class="vol">（{{ volumeHint }}）</span>
          </span>
          <span class="pct">{{ store.percent }}%</span>
        </div>
      </div>
      <div class="bar">
        <div class="bar-fill" :style="{ width: store.percent + '%' }"></div>
      </div>

      <!-- 实时累计 -->
      <div class="metrics">
        <div class="metric">
          <span class="m-label">实时累计</span>
          <span class="m-val">{{ formatBytes(store.foundBytesTotal) }}</span>
        </div>
        <div class="metric split">
          <span class="m-sub g">🟢 {{ formatBytes(store.found.green) }}</span>
          <span class="m-sub y">🟡 {{ formatBytes(store.found.yellow) }}</span>
          <span class="m-sub r">🔴 {{ formatBytes(store.found.red) }}</span>
        </div>
        <div class="metric">
          <span class="m-label">已用时</span>
          <span class="m-val">{{ fmtElapsed(store.elapsedMs) }}</span>
        </div>
      </div>

      <!-- 当前路径（v0.1.5 反馈 2：滚动高亮） -->
      <div class="path-block">
        <span class="m-label">当前扫描</span>
        <span class="path" :title="store.currentPath">{{ store.currentPath || "初始化…" }}</span>
      </div>

      <!-- 取消 -->
      <div class="actions">
        <button class="btn-cancel" :disabled="!store.running" @click="store.cancel()">
          取消体检
        </button>
        <p class="tip">可随时取消，已扫描部分不写入任何文件。完成自动进入报告。</p>
      </div>
    </section>
  </main>
</template>

<style scoped>
.scan {
  display: flex;
  flex-direction: column;
  gap: 1.25rem;
}
.back {
  color: var(--accent);
  font-weight: 600;
}
.scan-head h1 {
  margin: 0.5rem 0 0.25rem;
}
.sub {
  margin: 0;
  color: var(--text-2);
  font-size: 0.9rem;
}
.error {
  color: var(--grade-red);
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 1rem 1.25rem;
}
.card {
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 1.5rem;
  display: flex;
  flex-direction: column;
  gap: 1.25rem;
}
.done-card {
  align-items: center;
  text-align: center;
}
.done-text {
  margin: 0;
  font-size: 1.2rem;
  font-weight: 700;
  color: var(--text);
}
.done-sub {
  margin: 0.25rem 0 0;
  color: var(--text-2);
  font-size: 0.85rem;
}
.btn-done {
  margin-top: 0.75rem;
  padding: 0.7rem 2.2rem;
  border: none;
  border-radius: 10px;
  background: var(--accent);
  color: #fff;
  font-size: 1rem;
  font-weight: 600;
  cursor: pointer;
}
/* v0.1.5 反馈 2：旋转扫描动效（纯 CSS，零依赖） */
.scan-visual {
  display: flex;
  align-items: center;
  gap: 1.25rem;
}
.radar {
  position: relative;
  width: 72px;
  height: 72px;
  flex: none;
  border-radius: 50%;
  background: conic-gradient(from 0deg, var(--accent) 0deg, var(--accent) 40deg, transparent 40deg);
  animation: radar-spin 1.4s linear infinite;
}
.radar.paused {
  animation-play-state: paused;
  opacity: 0.35;
}
.radar-core {
  position: absolute;
  inset: 8px;
  border-radius: 50%;
  background: var(--bg);
  border: 1px solid var(--border);
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 1.4rem;
}
@keyframes radar-spin {
  to {
    transform: rotate(360deg);
  }
}
.phase-col {
  display: flex;
  flex-direction: column;
  gap: 0.3rem;
  font-weight: 600;
}
.phase .vol {
  color: var(--accent);
  font-size: 0.85rem;
}
.pct {
  font-size: 1.6rem;
  font-weight: 700;
}
.bar {
  height: 14px;
  border-radius: 7px;
  background: var(--bg);
  border: 1px solid var(--border);
  overflow: hidden;
}
.bar-fill {
  height: 100%;
  background: var(--accent);
  border-radius: 7px;
  transition: width 0.15s linear;
}
.metrics {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 1rem;
}
.metric {
  display: flex;
  flex-direction: column;
  gap: 0.3rem;
}
.metric.split {
  flex-direction: row;
  align-items: center;
  gap: 0.6rem;
}
.m-label {
  font-size: 0.8rem;
  color: var(--text-2);
}
.m-val {
  font-size: 1.3rem;
  font-weight: 700;
}
.m-sub {
  font-size: 0.85rem;
}
.g {
  color: var(--grade-green);
}
.y {
  color: var(--grade-yellow);
}
.r {
  color: var(--grade-red);
}
.path-block {
  display: flex;
  flex-direction: column;
  gap: 0.3rem;
  min-width: 0;
}
.path {
  font-size: 0.85rem;
  color: var(--text-2);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  font-family: ui-monospace, monospace;
}
.actions {
  display: flex;
  align-items: center;
  gap: 1rem;
}
.btn-cancel {
  background: var(--grade-yellow);
  color: #fff;
  border: none;
  padding: 0.6rem 1.25rem;
  border-radius: 8px;
  font-weight: 600;
  cursor: pointer;
}
.btn-cancel:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.tip {
  margin: 0;
  font-size: 0.8rem;
  color: var(--text-2);
}
.slow-hint {
  color: var(--grade-yellow);
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 1rem 1.25rem;
  margin: 0;
}
</style>
