<script setup lang="ts">
import { onMounted, watch } from "vue";
import { useRouter } from "vue-router";
import { useScanStore } from "../stores/scan";

const store = useScanStore();
const router = useRouter();

const PHASE_CN: Record<string, string> = {
  walking: "遍历扫描中",
  hashing: "哈希比对中",
  aggregating: "结果聚合中",
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

// 完成 → 跳转报告页。
watch(
  () => store.isDone,
  (done) => {
    if (done) router.push("/report");
  },
);

onMounted(() => {
  void store.start();
});
</script>

<template>
  <main class="scan">
    <header class="scan-head">
      <router-link to="/" class="back">← 首页</router-link>
      <h1>一键体检</h1>
      <p class="sub">只读扫描 · 不删除任何文件 · 全程可在确认后逐项处置</p>
    </header>

    <div v-if="store.error" class="error">扫描失败：{{ store.error }}</div>

    <section class="card" v-else>
      <!-- 阶段与百分比 -->
      <div class="phase-row">
        <span class="phase">{{ PHASE_CN[store.phase] ?? store.phase }}</span>
        <span class="pct">{{ store.percent }}%</span>
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

      <!-- 当前路径 -->
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
.phase-row {
  display: flex;
  justify-content: space-between;
  font-weight: 600;
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
</style>