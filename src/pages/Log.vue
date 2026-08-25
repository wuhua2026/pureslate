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
    if (!ok) error.value = "导出失败（检查应用运行目录写入权限）";
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
  <main class="log">
    <header class="log-head">
      <router-link to="/" class="back">← 首页</router-link>
      <h1>操作日志</h1>
      <p class="sub">今日清理 / 还原 / 清除 / 禁用启动项全记录（JSONL 只追加）</p>
    </header>

    <div v-if="error" class="error">{{ error }}</div>

    <section class="card">
      <div class="toolbar">
        <span class="m-label">明细（{{ entries.length }}）</span>
        <div class="tools">
          <button class="tbtn" :disabled="loading" @click="load">刷新</button>
          <button class="tbtn primary" :disabled="exporting" @click="doExport">
            {{ exporting ? "导出中…" : "导出" }}
          </button>
        </div>
      </div>
      <p v-if="exported" class="ok-tip">
        已导出到应用运行目录 <code>pureslate-audit-export-*.jsonl</code>
      </p>

      <div class="state" v-if="loading">加载中…</div>
      <div class="state empty" v-else-if="entries.length === 0">今日暂无操作记录。</div>
      <table v-else class="tbl">
        <thead>
          <tr>
            <th>时间</th>
            <th>操作</th>
            <th>去向</th>
            <th>结果</th>
            <th>对象</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="(l, i) in entries" :key="i">
            <td class="mono">{{ fmtTime(l.ts) }}</td>
            <td>{{ opText(l.op) }}</td>
            <td>{{ l.disposition ? DISP_CN[l.disposition] : "—" }}</td>
            <td :class="l.result === 'fail' ? 'fail' : ''">{{ stateText(l) || "—" }}</td>
            <td class="obj" :title="l.path || l.txId || ''">{{ l.path || l.txId || "" }}</td>
          </tr>
        </tbody>
      </table>
    </section>
  </main>
</template>

<style scoped>
.log {
  display: flex;
  flex-direction: column;
  gap: 1.25rem;
}
.back {
  color: var(--accent);
  font-weight: 600;
}
.log-head h1 {
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
  padding: 1.25rem 1.5rem;
}
.toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 0.75rem;
}
.m-label {
  font-size: 0.85rem;
  color: var(--text-2);
}
.tools {
  display: flex;
  gap: 0.5rem;
}
.tbtn {
  background: none;
  border: 1px solid var(--border);
  color: var(--text);
  border-radius: 6px;
  padding: 0.4rem 0.9rem;
  font-size: 0.85rem;
  cursor: pointer;
}
.tbtn.primary {
  background: var(--accent);
  border-color: var(--accent);
  color: #fff;
}
.tbtn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.ok-tip {
  margin: 0 0 0.75rem;
  font-size: 0.8rem;
  color: var(--grade-green);
}
.state {
  padding: 1.5rem;
  color: var(--text-2);
  text-align: center;
}
.tbl {
  width: 100%;
  border-collapse: collapse;
  font-size: 0.82rem;
}
.tbl th,
.tbl td {
  text-align: left;
  padding: 0.5rem 0.6rem;
  border-bottom: 1px solid var(--border);
  vertical-align: top;
}
.tbl th {
  color: var(--text-2);
  font-weight: 600;
}
.mono {
  font-family: ui-monospace, monospace;
}
.fail {
  color: var(--grade-red);
}
.obj {
  color: var(--text-2);
  max-width: 420px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>