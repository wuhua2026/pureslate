<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import ConfirmTokenModal from "../components/ConfirmTokenModal.vue";
import GradeBadge from "../components/GradeBadge.vue";
import { quarantine_list, quarantine_purge, quarantine_restore, quarantine_status } from "../api/commands";
import { events } from "../api";
import type { QuarantineEntry } from "../types/ipc";
import { formatBytes, formatDay } from "./filesModel";
import {
  expiringSoon,
  semanticHeadline,
  sortByExpiryAsc,
  totalBytes,
} from "./quarantineModel";

/** 屏 8：隔离区管理（P3-06/R25）。语义首行（已隔离≠已释放，SAFETY §4.5）+
 *  剩余天数/到期提醒/还原/容量上限（超限建议释放最早批次）/确认清空（token）。 */
const entries = ref<QuarantineEntry[]>([]);
const overQuota = ref(false);
const quotaText = ref("");
const earliestBatchIds = ref<string[]>([]);
const selected = ref<Set<string>>(new Set());
const busy = ref(false);
const notice = ref("");
const tokenOpen = ref(false);
/** token 确认的目标动作（确认清空 / 释放最早批次）。 */
let tokenAction: "clear-all" | "earliest" = "clear-all";

const sorted = computed(() => sortByExpiryAsc(entries.value));
const headline = computed(() => semanticHeadline(formatBytes(totalBytes(entries.value))));
const expiring = computed(() => expiringSoon(sorted.value));
const selectedCount = computed(() => selected.value.size);

let unlisten: (() => void) | undefined;

async function refresh(): Promise<void> {
  const [list, status] = await Promise.all([quarantine_list(), quarantine_status()]);
  entries.value = list;
  overQuota.value = status.overQuota;
  quotaText.value = `${formatBytes(status.usedBytes)} / ${formatBytes(status.quotaBytes)}`;
  earliestBatchIds.value = status.earliestBatchIds;
  selected.value = new Set();
}

function toggle(id: string): void {
  const next = new Set(selected.value);
  if (next.has(id)) next.delete(id);
  else next.add(id);
  selected.value = next;
}

async function restoreSelected(): Promise<void> {
  const ids = [...selected.value];
  if (ids.length === 0 || busy.value) return;
  busy.value = true;
  try {
    const report = await quarantine_restore({ ids });
    notice.value =
      report.failures.length === 0
        ? `已还原 ${report.restored} 项${report.conflict > 0 ? `（${report.conflict} 项原路径被占用，已存入还原冲突目录）` : ""}`
        : `还原 ${report.restored} 项，${report.failures.length} 项失败`;
    await refresh();
  } finally {
    busy.value = false;
  }
}

function askToken(action: "clear-all" | "earliest"): void {
  if (busy.value) return;
  tokenAction = action;
  tokenOpen.value = true;
}

async function onTokenConfirm(token: string): Promise<void> {
  tokenOpen.value = false;
  const ids =
    tokenAction === "clear-all" ? entries.value.map((e) => e.id) : earliestBatchIds.value;
  if (ids.length === 0) return;
  busy.value = true;
  try {
    const report = await quarantine_purge({ ids, confirmToken: token });
    notice.value =
      report.failures.length === 0
        ? `已清除 ${report.purged} 项（硬删，不可还原）`
        : `清除 ${report.purged} 项，${report.failures.length} 项失败`;
    await refresh();
  } finally {
    busy.value = false;
  }
}

onMounted(async () => {
  unlisten = await events.onQuarantineExpiryWarning((p) => {
    notice.value = `${p.ids.length} 项将在 ${p.daysLeft} 天内到期自动清除，如需保留请及时还原`;
  });
  await refresh();
});

onUnmounted(() => {
  unlisten?.();
});
</script>

<template>
  <section class="page">
    <header class="head">
      <h1>隔离区</h1>
      <!-- 语义首行（SAFETY §4.5）：已隔离 ≠ 已释放 -->
      <p class="headline">{{ headline }}</p>
    </header>

    <p v-if="notice" class="notice">{{ notice }}</p>

    <!-- 容量上限（SAFETY §4.4：超限不静默丢弃，提示显式确认释放最早批次） -->
    <div v-if="overQuota" class="quota">
      <span>隔离区占用 {{ quotaText }} 超过容量上限</span>
      <button class="btn warn" :disabled="busy" @click="askToken('earliest')">
        释放最早批次…
      </button>
    </div>

    <p v-if="entries.length === 0" class="empty-list">隔离区为空</p>
    <template v-else>
      <div class="summary">
        <span>共 {{ entries.length }} 项</span>
        <span class="danger" v-if="expiring.length > 0">{{ expiring.length }} 项即将到期</span>
      </div>

      <table class="list">
        <thead>
          <tr>
            <th class="sel"></th>
            <th>原路径</th>
            <th class="num">大小</th>
            <th>级别</th>
            <th>移入时间</th>
            <th class="num">剩余天数</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="e in sorted" :key="e.id" :class="{ checked: selected.has(e.id) }">
            <td class="sel">
              <input
                type="checkbox"
                :checked="selected.has(e.id)"
                @change="toggle(e.id)"
              />
            </td>
            <td class="path" :title="e.originalPath">{{ e.originalPath }}</td>
            <td class="num">{{ formatBytes(e.sizeBytes) }}</td>
            <td><GradeBadge :grade="e.grade" /></td>
            <td>{{ formatDay(e.movedAt) }}</td>
            <td class="num days" :class="{ danger: e.daysLeft <= 3 }">
              {{ e.daysLeft }} 天
            </td>
          </tr>
        </tbody>
      </table>

      <div class="actions">
        <button class="btn" :disabled="selectedCount === 0 || busy" @click="restoreSelected">
          还原所选（{{ selectedCount }}）
        </button>
        <button class="btn danger" :disabled="busy" @click="askToken('clear-all')">
          确认清空…
        </button>
        <span class="hint">还原 = 移回原路径；清空 = 硬删不可还原（需二次确认）</span>
      </div>
    </template>

    <!-- 硬删（🔴 语义）二次确认 token -->
    <ConfirmTokenModal v-model="tokenOpen" :has-red="true" @confirm="onTokenConfirm" />
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
.headline {
  margin: 6px 0 16px;
  font-size: 0.9rem;
  font-weight: 600;
  color: var(--grade-yellow, #d9971c);
}
.notice {
  margin: 0 0 12px;
  padding: 8px 12px;
  border: 1px solid var(--grade-yellow, #d9971c);
  border-radius: 8px;
  background: var(--surface, #fff);
  color: var(--grade-yellow, #d9971c);
  font-size: 0.85rem;
}
.quota {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-bottom: 12px;
  padding: 8px 12px;
  border: 1px solid var(--grade-red, #c4382e);
  border-radius: 8px;
  background: var(--surface, #fff);
  color: var(--grade-red, #c4382e);
  font-size: 0.85rem;
}
.empty-list {
  padding: 32px 0;
  text-align: center;
  color: var(--text-2, #5a6472);
}
.summary {
  display: flex;
  gap: 16px;
  margin-bottom: 12px;
  font-size: 0.85rem;
  color: var(--text, #1f2430);
}
.summary .danger {
  color: var(--grade-red, #c4382e);
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
.list tbody tr.checked {
  background: rgba(59, 130, 246, 0.06);
}
.sel {
  width: 32px;
}
.path {
  max-width: 380px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: Consolas, monospace;
  font-size: 0.8rem;
}
.num {
  text-align: right;
  white-space: nowrap;
}
.days.danger {
  color: var(--grade-red, #c4382e);
  font-weight: 600;
}
.actions {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-top: 14px;
}
.btn {
  padding: 6px 18px;
  border: 1px solid var(--border, #e4e7ec);
  border-radius: 8px;
  background: var(--surface, #fff);
  color: var(--text, #1f2430);
  font-size: 0.85rem;
  cursor: pointer;
}
.btn:disabled {
  opacity: 0.45;
  cursor: default;
}
.btn.warn {
  border-color: var(--grade-red, #c4382e);
  color: var(--grade-red, #c4382e);
}
.btn.danger {
  border-color: var(--grade-red, #c4382e);
  background: var(--grade-red, #c4382e);
  color: #fff;
}
.hint {
  font-size: 0.78rem;
  color: var(--text-2, #5a6472);
}
</style>
