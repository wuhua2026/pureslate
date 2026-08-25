<script setup lang="ts">
import { computed } from "vue";
import { useCleanStore } from "../stores/clean";
import { ALL_DISPOSITIONS } from "../stores/cleanMetrics";

const store = useCleanStore();

const DISP_CN: Record<string, string> = {
  direct: "🟢 直清",
  recycle: "🗑 回收站",
  quarantine: "🔒 隔离区",
};

function formatBytes(b: number): string {
  if (b <= 0) return "0 B";
  const gb = b / (1024 * 1024 * 1024);
  if (gb >= 1) return gb >= 1024 ? `${(gb / 1024).toFixed(1)} TB` : `${gb.toFixed(2)} GB`;
  const mb = b / (1024 * 1024);
  if (mb >= 1) return `${mb.toFixed(1)} MB`;
  return `${Math.round(b / 1024)} KB`;
}

// 总体进度百分比（分母 itemTotal，防 0）。
const percent = computed(() => {
  const t = store.itemTotal;
  if (t <= 0) return 0;
  return Math.min(100, Math.round((store.processed / t) * 100));
});

// 每条去向进度行：累计处理字节 + ok/fail/skip 计数。
const rows = computed(() =>
  ALL_DISPOSITIONS.map((d) => {
    const t = store.totals[d];
    return {
      key: d,
      label: DISP_CN[d],
      okBytes: t.okBytes,
      ok: t.okCount,
      fail: t.failCount,
      skip: t.skipCount,
    };
  }),
);

// 完成页首行（SAFETY §4.5：已释放 vs 已隔离，禁止混用"已清理"）。
const summary = computed(() => store.result);

// 最近逐项（倒序，最新在顶）。
const recent = computed(() => [...store.progressEvents].reverse().slice(0, 6));
</script>

<template>
  <main class="exec">
    <header class="exec-head">
      <router-link to="/report" class="back">← 返回报告</router-link>
      <h1>清理执行</h1>
      <p class="sub">逐项处置：直清 / 回收站 / 隔离区</p>
    </header>

    <div v-if="store.error" class="error">执行失败：{{ store.error }}</div>

    <!-- 完成态 -->
    <template v-else-if="store.isDone && summary">
      <section class="done-card">
        <h2 class="done-title">清理完成</h2>
        <p class="semantics">
          <span class="freed">已释放 <strong class="g-num">{{ formatBytes(store.freedBytes) }}</strong></span>
          <span class="sep">·</span>
          <span class="quar">已隔离 <strong class="y-num">{{ formatBytes(store.quarantinedBytes) }}</strong></span>
        </p>
        <p class="sema-note">
          「已释放」= 直清 / 回收站已让渡体积；「已隔离」= 转入隔离区，<b>尚未释放</b>，保留期内可还原。
        </p>
        <div class="counts">
          <span>完成 {{ summary.ok }}</span>
          <span class="fail" v-if="summary.fail > 0">失败 {{ summary.fail }}</span>
          <span class="skip" v-if="summary.skip > 0">跳过 {{ summary.skip }}</span>
        </div>
        <div class="actions">
          <router-link to="/log" class="btn">查看操作日志</router-link>
          <router-link to="/quarantine" class="btn ghost">去隔离区还原</router-link>
        </div>
      </section>
    </template>

    <!-- 执行中 -->
    <section v-else class="card">
      <div class="phase-row">
        <span class="phase">{{ store.running ? "正在清理…" : "准备中…" }}</span>
        <span class="pct">{{ store.processed }} / {{ store.itemTotal }} 项</span>
      </div>
      <div class="bar">
        <div class="bar-fill" :style="{ width: percent + '%' }"></div>
      </div>

      <!-- 分去向进度 -->
      <div class="disp-stats">
        <div v-for="row in rows" :key="row.key" class="stat">
          <div class="stat-head">
            <span class="stat-label">{{ row.label }}</span>
            <span class="stat-bytes">{{ formatBytes(row.okBytes) }}</span>
          </div>
          <div class="stat-counts">
            <span>ok {{ row.ok }}</span>
            <span class="f2" v-if="row.fail > 0">fail {{ row.fail }}</span>
            <span class="s2" v-if="row.skip > 0">skip {{ row.skip }}</span>
          </div>
        </div>
      </div>

      <!-- 逐项最近 -->
      <div class="recent" v-if="recent.length">
        <p class="m-label">最近处置</p>
        <ul class="recent-list">
          <li v-for="(e, i) in recent" :key="i" class="recent-row">
            <span class="st" :class="e.state">{{ { ok: "✓", fail: "✗", skip: "∅" }[e.state] }}</span>
            <span class="rp" :title="e.itemPath">{{ e.itemPath }}</span>
            <span class="rd">→ {{ DISP_CN[e.disposition] }}</span>
          </li>
        </ul>
      </div>

      <div class="actions">
        <button class="btn-cancel" :disabled="!store.running" @click="store.cancel()">
          取消清理
        </button>
        <p class="tip">取消后已完成的项不再回滚；隔离区项仍可还原。</p>
      </div>
    </section>
  </main>
</template>

<style scoped>
.exec {
  display: flex;
  flex-direction: column;
  gap: 1.25rem;
}
.back {
  color: var(--accent);
  font-weight: 600;
}
.exec-head h1 {
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
.disp-stats {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(190px, 1fr));
  gap: 0.75rem;
}
.stat {
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 0.7rem 0.9rem;
  display: flex;
  flex-direction: column;
  gap: 0.3rem;
}
.stat-head {
  display: flex;
  justify-content: space-between;
  align-items: baseline;
}
.stat-label {
  font-size: 0.85rem;
  font-weight: 600;
}
.stat-bytes {
  font-size: 1.05rem;
  font-weight: 700;
}
.stat-counts {
  font-size: 0.75rem;
  color: var(--text-2);
  display: flex;
  gap: 0.5rem;
}
.f2 {
  color: var(--grade-red);
}
.s2 {
  color: var(--text-2);
}
.m-label {
  margin: 0 0 0.4rem;
  font-size: 0.8rem;
  color: var(--text-2);
}
.recent-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 0.35rem;
}
.recent-row {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  font-size: 0.8rem;
}
.st {
  width: 1.1rem;
  text-align: center;
  font-weight: 700;
}
.st.ok {
  color: var(--grade-green);
}
.st.fail {
  color: var(--grade-red);
}
.st.skip {
  color: var(--text-2);
}
.rp {
  flex: 1;
  color: var(--text-2);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.rd {
  color: var(--accent);
  flex-shrink: 0;
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
.done-card {
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 1.75rem 2rem;
  display: flex;
  flex-direction: column;
  gap: 1rem;
}
.done-title {
  margin: 0;
  font-size: 1.3rem;
}
.semantics {
  margin: 0;
  font-size: 1.15rem;
  display: flex;
  align-items: baseline;
  gap: 0.6rem;
  flex-wrap: wrap;
}
.freed {
  color: var(--text);
}
.g-num {
  color: var(--grade-green);
}
.quar {
  color: var(--text);
}
.y-num {
  color: var(--grade-yellow);
}
.sep {
  color: var(--text-2);
}
.sema-note {
  margin: 0;
  font-size: 0.8rem;
  color: var(--text-2);
}
.counts {
  display: flex;
  gap: 0.75rem;
  font-size: 0.9rem;
  color: var(--text-2);
}
.counts .fail {
  color: var(--grade-red);
}
.counts .skip {
  color: var(--text-2);
}
.actions {
  display: flex;
  gap: 0.75rem;
  padding-top: 0.25rem;
}
.btn {
  display: inline-block;
  background: var(--accent);
  color: #fff;
  border: none;
  padding: 0.6rem 1.25rem;
  border-radius: 8px;
  font-weight: 600;
  text-decoration: none;
}
.btn.ghost {
  background: none;
  color: var(--accent);
  border: 1px solid var(--accent);
}
</style>