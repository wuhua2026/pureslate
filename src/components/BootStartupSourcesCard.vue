<script setup lang="ts">
import { computed, ref } from "vue";
import type { BootSection, BootStartupData } from "../types/ipc";
import {
  approvedLabel,
  failedSourceLabel,
  isSystemTask,
  scopeLabel,
  sourceStats,
} from "../pages/bootdiagModel";
import BootStatusChip from "./BootStatusChip.vue";

const props = defineProps<{ section: BootSection<BootStartupData> }>();

const data = computed(() => props.section.data);
const stats = computed(() => sourceStats(data.value?.entries ?? []));
const registryCount = computed(() =>
  stats.value
    .filter((s) => s.kind === "run" || s.kind === "run_once" || s.kind === "startup_folder")
    .reduce((acc, s) => acc + s.count, 0),
);
const services = computed(() => (data.value?.entries ?? []).filter((e) => e.kind === "service_auto"));
const tasks = computed(() =>
  (data.value?.entries ?? []).filter(
    (e) => e.kind === "scheduled_task_boot" || e.kind === "scheduled_task_logon",
  ),
);
const userTasks = computed(() => tasks.value.filter((e) => !isSystemTask(e)));
const systemTasks = computed(() => tasks.value.filter((e) => isSystemTask(e)));
const failed = computed(() => data.value?.failedSources ?? []);

// 系统任务默认折叠（设计稿 §3.5-4），计数仍可见。
const showSystemTasks = ref(false);
</script>

<template>
  <article class="card">
    <header class="card-head">
      <div>
        <h2>启动项来源</h2>
        <p class="hint">注册表 / 启动文件夹 / 自动服务 / 计划任务（只读统计）</p>
      </div>
      <BootStatusChip :status="section.status" />
    </header>

    <p v-if="section.reason" class="reason">{{ section.reason }}</p>

    <div v-if="data" class="stats">
      <span v-for="s in stats" :key="s.kind" class="stat" :class="{ zero: s.count === 0 }">
        {{ s.label }} <b>{{ s.count }}</b>
      </span>
    </div>

    <div v-if="failed.length > 0" class="failed">
      <p class="failed-title">以下来源读不到，结果不完整：</p>
      <ul>
        <li v-for="f in failed" :key="f">{{ failedSourceLabel(f) }} <code>{{ f }}</code></li>
      </ul>
    </div>

    <p v-if="registryCount > 0" class="pointer">
      注册表与启动文件夹共 {{ registryCount }} 项，逐条见下方启动项列表。
    </p>

    <div v-if="services.length > 0" class="block">
      <h3>自动启动服务（{{ services.length }}）</h3>
      <table>
        <thead>
          <tr>
            <th>服务名</th>
            <th>内部名</th>
            <th class="num">延迟启动</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="s in services" :key="s.id">
            <td class="ellipsis" :title="s.name">{{ s.name }}</td>
            <td class="ellipsis dim" :title="s.detail">{{ s.detail || "—" }}</td>
            <td class="num">
              {{ s.delayedAutoStart === undefined ? "—" : s.delayedAutoStart ? "是" : "否" }}
            </td>
          </tr>
        </tbody>
      </table>
    </div>

    <div v-if="userTasks.length > 0" class="block">
      <h3>计划任务（开机/登录触发，{{ userTasks.length }}）</h3>
      <table>
        <thead>
          <tr>
            <th>任务</th>
            <th>范围</th>
            <th>触发</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="t in userTasks" :key="t.id">
            <td class="ellipsis" :title="t.name">{{ t.name }}</td>
            <td>{{ scopeLabel(t.scope) }}</td>
            <td>{{ t.detail || "—" }}</td>
          </tr>
        </tbody>
      </table>
    </div>

    <div v-if="systemTasks.length > 0" class="block">
      <button class="toggle" type="button" @click="showSystemTasks = !showSystemTasks">
        {{ showSystemTasks ? "收起" : "展开" }}系统任务（{{ systemTasks.length }}）
      </button>
      <table v-if="showSystemTasks">
        <thead>
          <tr>
            <th>任务</th>
            <th>触发</th>
            <th>审批</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="t in systemTasks" :key="t.id">
            <td class="ellipsis" :title="t.name">{{ t.name }}</td>
            <td>{{ t.detail || "—" }}</td>
            <td>{{ approvedLabel(t.approvedState) }}</td>
          </tr>
        </tbody>
      </table>
    </div>

    <p v-if="data && data.entries.length === 0" class="empty">未读到任何启动项来源。</p>
  </article>
</template>

<style scoped>
.card {
  background: var(--surface, #fff);
  border: 1px solid var(--border, #e7ebf2);
  border-radius: var(--r-card, 14px);
  box-shadow: var(--shadow-card, none);
  padding: 16px 18px;
}
.card-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 10px;
}
h2 {
  margin: 0;
  font-size: 1rem;
  color: var(--text, #1c2433);
}
.hint {
  margin: 2px 0 0;
  font-size: 0.78rem;
  color: var(--text-3, #8a97aa);
}
.reason {
  margin: 0 0 8px;
  font-size: 0.8rem;
  color: var(--text-2, #5d6b80);
  line-height: 1.6;
}
.stats {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}
.stat {
  padding: 3px 9px;
  border-radius: 999px;
  background: var(--accent-soft, #eaf1ff);
  color: var(--accent, #2563eb);
  font-size: 0.76rem;
}
.stat b {
  font-variant-numeric: tabular-nums;
}
.stat.zero {
  background: var(--surface-2, #f8fafc);
  color: var(--text-3, #8a97aa);
}
.failed {
  margin-top: 10px;
  padding: 8px 10px;
  border-radius: 10px;
  border: 1px solid rgba(217, 151, 28, 0.32);
  background: rgba(217, 151, 28, 0.07);
  font-size: 0.78rem;
  color: var(--text-2, #5d6b80);
}
.failed-title {
  margin: 0 0 4px;
  font-weight: 600;
  color: var(--text, #1c2433);
}
.failed ul {
  margin: 0;
  padding-left: 16px;
  line-height: 1.7;
}
.failed code {
  margin-left: 6px;
  font-size: 0.72rem;
  color: var(--text-3, #8a97aa);
}
.pointer {
  margin: 10px 0 0;
  font-size: 0.78rem;
  color: var(--text-2, #5d6b80);
}
.block {
  margin-top: 14px;
}
h3 {
  margin: 0 0 6px;
  font-size: 0.84rem;
  color: var(--text, #1c2433);
}
table {
  width: 100%;
  border-collapse: collapse;
  font-size: 0.78rem;
  table-layout: fixed;
}
th,
td {
  text-align: left;
  padding: 4px 6px;
  border-bottom: 1px solid var(--border, #e7ebf2);
  color: var(--text-2, #5d6b80);
}
th {
  color: var(--text-3, #8a97aa);
  font-weight: 500;
}
th:first-child,
td:first-child {
  width: 58%;
}
.num {
  text-align: right;
}
.ellipsis {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.dim {
  color: var(--text-3, #8a97aa);
}
.toggle {
  border: 1px solid var(--border, #e7ebf2);
  background: var(--surface-2, #f8fafc);
  color: var(--text-2, #5d6b80);
  border-radius: 8px;
  padding: 4px 10px;
  font-size: 0.78rem;
  cursor: pointer;
  margin-bottom: 8px;
  transition: color var(--dur-fast, 140ms) var(--ease-out, ease);
}
.toggle:hover {
  color: var(--accent, #2563eb);
  border-color: var(--accent-soft-2, #dbe7ff);
}
.empty {
  margin: 10px 0 0;
  font-size: 0.8rem;
  color: var(--text-3, #8a97aa);
}
@media (prefers-reduced-motion: reduce) {
  .toggle {
    transition: none;
  }
}
</style>
