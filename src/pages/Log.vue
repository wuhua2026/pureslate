<script setup lang="ts">
import { onMounted, ref } from "vue";
import { commands } from "../api";
import type { LogEntry, LogQueryFilter } from "../types/ipc";

// 屏 4 操作日志：log_query + 导出。默认查询今日。
const entries = ref<LogEntry[]>([]);
const loading = ref(false);
const exporting = ref(false);
const exported = ref(false);
const error = ref<string>("");

const DISP_CN: Record<string, string> = {
  direct: "直清",
  recycle: "回收站",
  quarantine: "隔离区",
};

function todayFilter(): LogQueryFilter {
  const now = new Date();
  const start = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
  return { from: start, to: now.getTime() };
}

async function load() {
  loading.value = true;
  error.value = "";
  try {
    entries.value = await commands.log_query(todayFilter());
  } catch (e) {
    error.value = String(e);
  } finally {
    loading.value = false;
  }
}

function fmtTime(ms: number): string {
  const d = new Date(ms);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
}

function opText(op: string): string {
  const map: Record<string, string> = {
    clean: "清理",
    restore: "还原",
    purge: "清除隔离项",
    disable_startup: "禁用启动项",
    scan: "扫描",
  };
  return map[op] ?? op;
}

async function doExport() {
  exporting.value = true;
  exported.value = false;
  try {
    const ok = await commands.log_export({ path: `pureslate-audit-export-${Date.now()}.jsonl` });
    exported.value = ok;
    // I-1（P4-06）：导出固定落数据目录 exports\（后端剥离目录成分 + 防覆盖）。
    if (!ok) error.value = "导出失败（导出文件名不合法或磁盘不可写）";
  } catch (e) {
    error.value = String(e);
  } finally {
    exporting.value = false;
  }
}

const stateText = (l: LogEntry) => {
  const m: Record<string, string> = { ok: "成功", fail: "失败", skip: "跳过" };
  return l.result ? m[l.result] ?? l.result : "";
};

onMounted(load);
</script>

<template>
  <section class="page anim-rise d1">
    <header class="head">
      <h1>操作日志</h1>
      <p class="sub">今日清理 / 还原 / 清除 / 禁用启动项全记录（JSONL 只追加）</p>
    </header>

    <div v-if="error" class="error">{{ error }}</div>

    <div class="toolbar">
      <span class="chip">今日记录 {{ entries.length }}</span>
      <span class="hint">JSONL 只追加 · 全量可追溯</span>
      <div class="tools">
        <button class="tbtn" :disabled="loading" @click="load">刷新</button>
        <button class="tbtn primary" :disabled="exporting" @click="doExport">
          {{ exporting ? "导出中…" : "导出" }}
        </button>
      </div>
    </div>
    <p v-if="exported" class="ok-tip">
      已导出到数据目录 <code>exports\pureslate-audit-export-*.jsonl</code>
    </p>

    <section class="data" aria-label="操作日志明细">
      <div class="state" v-if="loading">加载中…</div>
      <div class="state empty" v-else-if="entries.length === 0">
        <strong>今日暂无操作记录</strong>
        <span>清理、还原、清除隔离项等操作会逐条记录在这里。</span>
      </div>
      <div v-else class="rows">
        <article v-for="(l, i) in entries" :key="i" class="row">
          <span class="r-time">{{ fmtTime(l.ts) }}</span>
          <span class="r-op">{{ opText(l.op) }}</span>
          <span class="r-disp" :data-disp="l.disposition ?? ''">
            {{ l.disposition ? DISP_CN[l.disposition] : "—" }}
          </span>
          <span class="r-result" :class="l.result === 'fail' ? 'fail' : ''">
            {{ stateText(l) || "—" }}
          </span>
          <span class="r-obj" :title="l.path || l.txId || ''">{{ l.path || l.txId || "" }}</span>
        </article>
      </div>
    </section>
  </section>
</template>

<style scoped>
.page {
  max-width: 1080px;
  margin: 0 auto;
}
.head h1 {
  margin: 0;
  font-size: 1.6rem;
  font-weight: 720;
  letter-spacing: -0.02em;
  color: var(--text, #1f2430);
}
.sub {
  margin: 4px 0 18px;
  color: var(--text-2);
  font-size: 0.85rem;
  line-height: 1.65;
}
.error {
  color: var(--grade-red);
  background: rgba(255, 244, 242, 0.85);
  border: 1px solid rgba(214, 58, 47, 0.24);
  border-radius: var(--r-md);
  padding: 0.8rem 1rem;
  margin-bottom: 12px;
  box-shadow: var(--shadow-1);
  font-size: 0.84rem;
}
.toolbar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px 10px;
  margin-bottom: 12px;
}
.chip {
  padding: 3px 11px;
  border-radius: var(--r-pill);
  background: rgba(255, 255, 255, 0.8);
  border: 1px solid var(--glass-line);
  font-size: 0.78rem;
  font-weight: 600;
  color: var(--text-2, #5a6472);
  font-variant-numeric: tabular-nums;
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.7);
}
.hint {
  font-size: 0.78rem;
  color: var(--text-2);
}
.tools {
  display: flex;
  gap: 0.5rem;
  margin-left: auto;
}
.tbtn {
  background: rgba(255, 255, 255, 0.72);
  border: 1px solid var(--border-strong);
  color: var(--text);
  border-radius: 9px;
  padding: 0.42rem 1rem;
  font-size: 0.84rem;
  font-weight: 600;
  cursor: pointer;
  box-shadow: 0 2px 6px -3px rgba(16, 28, 52, 0.2);
  transition:
    transform var(--dur-fast) var(--ease-out),
    border-color var(--dur-fast) var(--ease-inout),
    color var(--dur-fast) var(--ease-inout),
    background var(--dur-fast) var(--ease-inout),
    opacity var(--dur-fast) var(--ease-inout);
}
.tbtn:hover:not(:disabled) {
  border-color: rgba(37, 99, 235, 0.4);
  color: var(--accent);
  background: #fff;
}
.tbtn:active {
  transform: scale(0.97);
}
.tbtn.primary {
  background: linear-gradient(180deg, #3b7bf5 0%, #2563eb 55%, #1d51d8 100%);
  border-color: transparent;
  color: #fff;
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.3),
    0 6px 16px -8px rgba(37, 99, 235, 0.55);
}
.tbtn.primary:hover:not(:disabled) {
  color: #fff;
  filter: brightness(1.05);
}
.tbtn:disabled {
  opacity: 0.45;
  box-shadow: none;
  cursor: not-allowed;
}
.ok-tip {
  margin: 0 0 12px;
  font-size: 0.8rem;
  color: var(--grade-green);
}
.data {
  background: var(--surface);
  border: 1px solid var(--glass-line);
  border-radius: var(--r-lg);
  box-shadow:
    var(--glass-sheen),
    var(--shadow-1);
  backdrop-filter: var(--glass-blur);
  -webkit-backdrop-filter: var(--glass-blur);
  overflow: hidden;
}
.state {
  padding: 1.5rem;
  color: var(--text-2);
  text-align: center;
}
.state.empty {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 3rem 1.5rem;
}
.state.empty strong {
  font-size: 0.98rem;
  font-weight: 650;
  color: var(--text);
}
.state.empty span {
  font-size: 0.84rem;
}
.rows {
  display: flex;
  flex-direction: column;
}
.row {
  display: grid;
  grid-template-columns: 72px 64px 62px 56px minmax(0, 1fr);
  align-items: center;
  gap: 10px;
  padding: 9px 16px;
  border-bottom: 1px solid var(--border);
  font-size: 0.82rem;
  transition: background var(--dur-fast) var(--ease-inout);
}
.row:last-child {
  border-bottom: none;
}
.row:hover {
  background: rgba(255, 255, 255, 0.72);
}
.r-time {
  font-family: ui-monospace, monospace;
  font-variant-numeric: tabular-nums;
  color: var(--text-2);
}
.r-op {
  font-weight: 600;
}
.r-disp {
  justify-self: start;
  padding: 1px 8px;
  border-radius: var(--r-pill);
  font-size: 0.72rem;
  font-weight: 600;
  color: var(--text-2);
  background: rgba(19, 32, 54, 0.05);
}
.r-disp[data-disp="direct"] {
  color: var(--grade-green, #12a150);
  background: rgba(18, 161, 80, 0.1);
}
.r-disp[data-disp="quarantine"] {
  color: var(--grade-yellow, #b26a00);
  background: rgba(217, 151, 28, 0.13);
}
.r-result {
  color: var(--text-2);
}
.r-result.fail {
  color: var(--grade-red);
  font-weight: 600;
}
.r-obj {
  color: var(--text-2);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  font-family: Consolas, monospace;
  font-size: 0.76rem;
}
@media (max-width: 760px) {
  .row {
    grid-template-columns: 64px 56px 1fr;
    row-gap: 4px;
  }
  .r-obj {
    grid-column: 1 / -1;
  }
}
</style>