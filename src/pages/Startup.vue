<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { startup_list, startup_toggle } from "../api/commands";
import type { StartupEntry } from "../types/ipc";
import {
  disableRiskText,
  impactLabel,
  publisherLabel,
  sortByImpactDesc,
  sourceLabel,
  splitByEnabled,
  toggleFailText,
} from "./startupModel";

/** 屏：启动项（P3-03/R07）。影响降序；禁用=备份后移除（弹风险提示确认），
 *  已禁用区提供恢复入口。全程不删用户程序。 */
const entries = ref<StartupEntry[]>([]);
const loaded = ref(false);
const pending = ref<StartupEntry | null>(null); // 待确认禁用项
const busyId = ref<string | null>(null);
const actionError = ref("");

async function refresh(): Promise<void> {
  entries.value = sortByImpactDesc(await startup_list());
}

onMounted(async () => {
  try {
    await refresh();
  } finally {
    loaded.value = true;
  }
});

const groups = computed(() => splitByEnabled(entries.value));
const highCount = computed(() => groups.value.enabled.filter((e) => e.impact === "high").length);

async function confirmDisable(): Promise<void> {
  const item = pending.value;
  if (!item) return;
  pending.value = null;
  busyId.value = item.id;
  try {
    const ok = await startup_toggle({ id: item.id, enabled: false });
    actionError.value = ok ? "" : toggleFailText(item);
    await refresh();
  } finally {
    busyId.value = null;
  }
}

async function restore(item: StartupEntry): Promise<void> {
  busyId.value = item.id;
  try {
    const ok = await startup_toggle({ id: item.id, enabled: true });
    actionError.value = ok ? "" : toggleFailText(item);
    await refresh();
  } finally {
    busyId.value = null;
  }
}
</script>

<template>
  <section class="page">
    <header class="head">
      <h1>启动项</h1>
      <p class="sub">开机自动运行的程序与任务——禁用只做备份后移除，不删除程序本身，可随时恢复</p>
    </header>

    <p v-if="!loaded" class="empty-list">加载中…</p>
    <p v-else-if="entries.length === 0" class="empty-list">未发现启动项</p>
    <template v-else>
      <div class="summary">
        <span>共 {{ entries.length }} 项</span>
        <span>启用 {{ groups.enabled.length }}</span>
        <span>已禁用 {{ groups.disabled.length }}</span>
        <span class="warn" v-if="highCount > 0">高影响 {{ highCount }} 项</span>
      </div>

      <p v-if="actionError" class="error">{{ actionError }}</p>

      <table class="list" v-if="groups.enabled.length > 0">
        <thead>
          <tr>
            <th>名称</th>
            <th>发布者</th>
            <th>影响</th>
            <th>来源</th>
            <th>命令</th>
            <th class="op">操作</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="e in groups.enabled" :key="e.id">
            <td class="name" :title="e.name">{{ e.name }}</td>
            <td class="pub" :class="{ unknown: publisherLabel(e.publisher) === '未知发布者' }">
              {{ publisherLabel(e.publisher) }}
            </td>
            <td><span class="impact" :class="e.impact">{{ impactLabel(e.impact) }}</span></td>
            <td>{{ sourceLabel(e.source) }}</td>
            <td class="cmd" :title="e.command">{{ e.command }}</td>
            <td class="op">
              <button
                class="btn"
                :disabled="busyId === e.id || pending !== null"
                @click="pending = e"
              >
                {{ busyId === e.id ? "处理中…" : "禁用" }}
              </button>
            </td>
          </tr>
        </tbody>
      </table>

      <template v-if="groups.disabled.length > 0">
        <h2 class="section">已禁用（备份在 PureSlate，可恢复）</h2>
        <table class="list muted">
          <thead>
            <tr>
              <th>名称</th>
              <th>来源</th>
              <th>命令</th>
              <th class="op">操作</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="e in groups.disabled" :key="e.id">
              <td class="name" :title="e.name">{{ e.name }}</td>
              <td>{{ sourceLabel(e.source) }}</td>
              <td class="cmd" :title="e.command">{{ e.command }}</td>
              <td class="op">
                <button
                  class="btn primary"
                  :disabled="busyId === e.id"
                  @click="restore(e)"
                >
                  {{ busyId === e.id ? "处理中…" : "恢复" }}
                </button>
              </td>
            </tr>
          </tbody>
        </table>
      </template>
    </template>

    <!-- 禁用风险确认（轻量面板，非 🔴 级不走 token 流） -->
    <div v-if="pending" class="mask" @click.self="pending = null">
      <div class="confirm" role="dialog" aria-label="禁用确认">
        <h3>禁用启动项</h3>
        <p class="risk">{{ disableRiskText(pending) }}</p>
        <p class="cmd-line" :title="pending.command">{{ pending.command }}</p>
        <div class="actions">
          <button class="btn" @click="pending = null">取消</button>
          <button class="btn danger" @click="confirmDisable">确认禁用</button>
        </div>
      </div>
    </div>
  </section>
</template>

<style scoped>
.page {
  padding: 20px 24px;
  max-width: 1080px;
  margin: 0 auto;
}
.head h1 {
  margin: 0;
  font-size: 1.35rem;
  color: var(--text, #1f2430);
}
.sub {
  margin: 4px 0 16px;
  font-size: 0.85rem;
  color: var(--text-2, #5a6472);
}
.empty-list {
  padding: 32px 0;
  text-align: center;
  color: var(--text-2, #5a6472);
}
.summary {
  display: flex;
  flex-wrap: wrap;
  gap: 16px;
  align-items: center;
  margin-bottom: 12px;
  font-size: 0.85rem;
  color: var(--text, #1f2430);
}
.summary .warn {
  color: var(--grade-red, #c4382e);
}
.error {
  margin: 0 0 12px;
  padding: 8px 12px;
  border: 1px solid var(--grade-red, #c4382e);
  border-radius: 8px;
  background: var(--surface, #fff);
  color: var(--grade-red, #c4382e);
  font-size: 0.85rem;
}
.section {
  margin: 20px 0 10px;
  font-size: 0.95rem;
  color: var(--text, #1f2430);
}
.list {
  width: 100%;
  border-collapse: collapse;
  background: var(--surface, #fff);
  border: 1px solid var(--border, #e4e7ec);
  border-radius: 10px;
  overflow: hidden;
  font-size: 0.85rem;
}
.list.muted {
  opacity: 0.85;
}
.list th,
.list td {
  padding: 8px 12px;
  text-align: left;
  border-bottom: 1px solid var(--border, #e4e7ec);
}
.list thead th {
  background: var(--bg, #fafafa);
  color: var(--text-2, #5a6472);
  font-weight: 500;
}
.list tbody tr:last-child td {
  border-bottom: none;
}
.name {
  max-width: 180px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-weight: 500;
}
.pub {
  max-width: 140px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.pub.unknown {
  color: var(--grade-yellow, #d9971c);
}
.impact {
  white-space: nowrap;
}
.impact.high {
  color: var(--grade-red, #c4382e);
}
.impact.medium {
  color: var(--grade-yellow, #d9971c);
}
.impact.low {
  color: var(--grade-green, #2e9e5b);
}
.cmd {
  max-width: 320px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: Consolas, monospace;
  font-size: 0.8rem;
}
.op {
  text-align: right;
  white-space: nowrap;
}
.btn {
  padding: 4px 14px;
  border: 1px solid var(--border, #e4e7ec);
  border-radius: 8px;
  background: var(--surface, #fff);
  color: var(--text, #1f2430);
  font-size: 0.82rem;
  cursor: pointer;
}
.btn:disabled {
  opacity: 0.5;
  cursor: default;
}
.btn.primary {
  border-color: var(--accent, #3b82f6);
  color: var(--accent, #3b82f6);
}
.btn.danger {
  border-color: var(--grade-red, #c4382e);
  background: var(--grade-red, #c4382e);
  color: #fff;
}
.mask {
  position: fixed;
  inset: 0;
  background: rgba(15, 18, 25, 0.45);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 10;
}
.confirm {
  width: min(480px, calc(100vw - 48px));
  padding: 20px;
  border-radius: 12px;
  background: var(--surface, #fff);
  box-shadow: 0 12px 32px rgba(15, 18, 25, 0.2);
}
.confirm h3 {
  margin: 0 0 10px;
  font-size: 1.05rem;
  color: var(--text, #1f2430);
}
.risk {
  margin: 0 0 10px;
  font-size: 0.88rem;
  line-height: 1.6;
  color: var(--text, #1f2430);
}
.cmd-line {
  margin: 0 0 14px;
  padding: 8px 10px;
  border-radius: 8px;
  background: var(--bg, #fafafa);
  font-family: Consolas, monospace;
  font-size: 0.78rem;
  color: var(--text-2, #5a6472);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.actions {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
}
</style>
