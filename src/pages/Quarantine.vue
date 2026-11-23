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

const quotaUsed = ref(0);
const quotaCap = ref(0);
const sorted = computed(() => sortByExpiryAsc(entries.value));
const quotaPercent = computed(() =>
  quotaCap.value > 0 ? Math.min(100, Math.round((quotaUsed.value / quotaCap.value) * 100)) : 0,
);
const headline = computed(() => semanticHeadline(formatBytes(totalBytes(entries.value))));
const expiring = computed(() => expiringSoon(sorted.value));
const selectedCount = computed(() => selected.value.size);

let unlisten: (() => void) | undefined;

async function refresh(): Promise<void> {
  const [list, status] = await Promise.all([quarantine_list(), quarantine_status()]);
  entries.value = list;
  overQuota.value = status.overQuota;
  quotaUsed.value = status.usedBytes;
  quotaCap.value = status.quotaBytes;
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
    notice.value = `${p.ids.length} 项将在 ${p.daysLeft} 天内到期，如需保留请及时还原`;
  });
  await refresh();
});

onUnmounted(() => {
  unlisten?.();
});
</script>

<template>
  <section class="page anim-rise d1">
    <header class="head">
      <div class="head-main">
        <h1>隔离区</h1>
        <!-- 语义首行（SAFETY §4.5）：已隔离 ≠ 已释放 -->
        <p class="headline">{{ headline }}</p>
      </div>
      <div class="quota-meter" :class="{ over: overQuota }">
        <div class="qm-top">
          <span class="qm-label">容量占用</span>
          <strong class="qm-val">{{ quotaText || "—" }}</strong>
        </div>
        <div class="qm-bar">
          <span class="qm-fill" :style="{ width: quotaPercent + '%' }"></span>
        </div>
      </div>
    </header>

    <p v-if="notice" class="notice">{{ notice }}</p>

    <!-- 容量上限（SAFETY §4.4：超限不静默丢弃，提示显式确认释放最早批次） -->
    <div v-if="overQuota" class="quota">
      <span>隔离区占用 {{ quotaText }} 超过容量上限</span>
      <button class="btn warn" :disabled="busy" @click="askToken('earliest')">
        释放最早批次…
      </button>
    </div>

    <div v-if="entries.length === 0" class="empty-list">
      <div class="empty-icon" aria-hidden="true">
        <svg width="34" height="34" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
          <path d="M12 3.5 19 6v5.2c0 4.6-3.1 7.6-7 9.3-3.9-1.7-7-4.7-7-9.3V6Z" />
          <rect x="9.4" y="10.6" width="5.2" height="4.6" rx="1.1" />
        </svg>
      </div>
      <strong>隔离区为空</strong>
      <span>被隔离的文件会出现在这里，保留期内可随时一键还原。</span>
    </div>
    <template v-else>
      <div class="toolbar">
        <div class="toolbar-info">
          <span class="chip">共 {{ entries.length }} 项</span>
          <span v-if="expiring.length > 0" class="chip danger">{{ expiring.length }} 项即将到期</span>
          <span v-if="selectedCount > 0" class="chip accent">已选 {{ selectedCount }}</span>
        </div>
        <div class="toolbar-actions">
          <button class="btn" :disabled="selectedCount === 0 || busy" @click="restoreSelected">
            还原所选（{{ selectedCount }}）
          </button>
          <button class="btn danger" :disabled="busy" @click="askToken('clear-all')">
            确认清空…
          </button>
        </div>
      </div>

      <div class="list-card">
        <div class="list-head">
          <span></span>
          <span>原路径 / 移入时间</span>
          <span class="num">大小</span>
          <span>级别</span>
          <span class="num">剩余</span>
        </div>
        <div v-for="e in sorted" :key="e.id" class="row" :class="{ checked: selected.has(e.id) }">
          <input
            type="checkbox"
            :checked="selected.has(e.id)"
            :aria-label="`选择 ${e.originalPath}`"
            @change="toggle(e.id)"
          />
          <div class="cell-main">
            <span class="path" :title="e.originalPath">{{ e.originalPath }}</span>
            <span class="moved">{{ formatDay(e.movedAt) }} 移入</span>
          </div>
          <span class="size">{{ formatBytes(e.sizeBytes) }}</span>
          <GradeBadge :grade="e.grade" />
          <span class="days" :class="{ danger: e.daysLeft <= 3 }">{{ e.daysLeft }} 天</span>
        </div>
      </div>

      <p class="hint">还原 = 移回原路径；清空 = 硬删不可还原（需二次确认 token）。</p>
    </template>

    <!-- 硬删（🔴 语义）二次确认 token -->
    <ConfirmTokenModal v-model="tokenOpen" :has-red="true" @confirm="onTokenConfirm" />
  </section>
</template>

<style scoped>
.page {
  max-width: 1080px;
  margin: 0 auto;
}
.head {
  display: flex;
  align-items: flex-end;
  justify-content: space-between;
  gap: 1.5rem;
  flex-wrap: wrap;
  margin-bottom: 18px;
}
.head-main h1 {
  margin: 0;
  font-size: 1.6rem;
  font-weight: 720;
  letter-spacing: -0.02em;
  color: var(--text, #1f2430);
}
.headline {
  margin: 6px 0 0;
  font-size: 0.9rem;
  font-weight: 600;
  color: var(--grade-yellow, #b26a00);
}
.quota-meter {
  min-width: 220px;
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 10px 14px;
  border-radius: var(--r-md);
  border: 1px solid var(--glass-line);
  background: rgba(255, 255, 255, 0.62);
  box-shadow: var(--glass-sheen);
}
.qm-top {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 12px;
}
.qm-label {
  font-size: 0.75rem;
  color: var(--text-2, #5a6472);
}
.qm-val {
  font-size: 0.82rem;
  font-weight: 650;
  font-variant-numeric: tabular-nums;
}
.qm-bar {
  height: 6px;
  border-radius: var(--r-pill);
  background: rgba(19, 32, 54, 0.08);
  overflow: hidden;
}
.qm-fill {
  display: block;
  height: 100%;
  border-radius: inherit;
  background: linear-gradient(90deg, #5b8cf5, #2563eb);
  transition: width var(--dur-base) var(--ease-out);
}
.quota-meter.over .qm-fill {
  background: linear-gradient(90deg, #e2544a, #c4382e);
}
.quota-meter.over .qm-val {
  color: var(--grade-red, #c4382e);
}
.notice {
  margin: 0 0 12px;
  padding: 9px 13px;
  border: 1px solid rgba(202, 158, 74, 0.4);
  border-radius: var(--r-md);
  background: rgba(253, 247, 235, 0.85);
  color: var(--grade-yellow, #b26a00);
  font-size: 0.85rem;
  box-shadow: var(--shadow-1);
  animation: ps-banner-in var(--dur-base) var(--ease-out);
}
.quota {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-bottom: 12px;
  padding: 9px 13px;
  border: 1px solid rgba(214, 58, 47, 0.4);
  border-radius: var(--r-md);
  background: rgba(255, 244, 242, 0.88);
  color: var(--grade-red, #c4382e);
  font-size: 0.85rem;
  box-shadow: var(--shadow-1);
  animation: ps-banner-in var(--dur-base) var(--ease-out);
}
@keyframes ps-banner-in {
  from {
    opacity: 0;
    transform: translateY(-4px);
  }
}
.empty-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 64px 24px;
  text-align: center;
  color: var(--text-2, #5a6472);
  background: var(--surface);
  border: 1px solid var(--glass-line);
  border-radius: var(--r-xl);
  backdrop-filter: var(--glass-blur);
  -webkit-backdrop-filter: var(--glass-blur);
  box-shadow:
    var(--glass-sheen),
    var(--shadow-1);
  animation: ps-rise var(--dur-slow) var(--ease-out) both;
}
.empty-list strong {
  font-size: 1rem;
  font-weight: 650;
  color: var(--text, #1f2430);
}
.empty-list span {
  font-size: 0.85rem;
  color: var(--text-2, #5a6472);
}
.empty-icon {
  width: 56px;
  height: 56px;
  margin: 0 auto 4px;
  border-radius: 18px;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-3, #8792a3);
  background: rgba(19, 32, 54, 0.05);
}
.toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  flex-wrap: wrap;
  margin-bottom: 12px;
}
.toolbar-info {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}
.chip {
  font-size: 0.78rem;
  font-weight: 600;
  color: var(--text-2, #5a6472);
  background: rgba(255, 255, 255, 0.7);
  border: 1px solid var(--glass-line);
  border-radius: var(--r-pill);
  padding: 4px 10px;
}
.chip.danger {
  color: var(--grade-red, #c4382e);
  background: rgba(255, 244, 242, 0.85);
  border-color: rgba(214, 58, 47, 0.28);
}
.chip.accent {
  color: var(--accent);
  background: rgba(37, 99, 235, 0.08);
  border-color: rgba(37, 99, 235, 0.2);
}
.toolbar-actions {
  display: flex;
  gap: 10px;
}
.list-card {
  background: var(--surface);
  border: 1px solid var(--glass-line);
  border-radius: var(--r-lg);
  overflow: hidden;
  backdrop-filter: var(--glass-blur);
  -webkit-backdrop-filter: var(--glass-blur);
  box-shadow:
    var(--glass-sheen),
    var(--shadow-1);
}
.list-head,
.row {
  display: grid;
  grid-template-columns: 26px minmax(0, 1fr) 92px 96px 64px;
  align-items: center;
  gap: 12px;
  padding: 10px 16px;
}
.list-head {
  background: rgba(244, 247, 252, 0.85);
  border-bottom: 1px solid var(--border, #e4e7ec);
  font-size: 0.75rem;
  font-weight: 600;
  color: var(--text-2, #5a6472);
}
.row {
  border-bottom: 1px solid var(--border, #e4e7ec);
  font-size: 0.85rem;
  transition: background var(--dur-fast) var(--ease-inout);
}
.row:last-child {
  border-bottom: none;
}
.row:hover {
  background: rgba(255, 255, 255, 0.75);
}
.row.checked {
  background: rgba(37, 99, 235, 0.07);
}
.row.checked:hover {
  background: rgba(37, 99, 235, 0.1);
}
.row input[type="checkbox"] {
  width: 15px;
  height: 15px;
  accent-color: var(--accent);
  cursor: pointer;
}
.cell-main {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}
.path {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: Consolas, monospace;
  font-size: 0.8rem;
}
.moved {
  font-size: 0.72rem;
  color: var(--text-2, #5a6472);
}
.size,
.num {
  text-align: right;
  white-space: nowrap;
  font-variant-numeric: tabular-nums;
}
.days {
  justify-self: end;
  padding: 2px 8px;
  border-radius: var(--r-pill);
  font-size: 0.78rem;
  font-weight: 600;
  color: var(--text-2, #5a6472);
  background: rgba(19, 32, 54, 0.05);
  font-variant-numeric: tabular-nums;
}
.days.danger {
  color: var(--grade-red, #c4382e);
  background: rgba(255, 244, 242, 0.9);
}
.hint {
  margin: 12px 0 0;
  font-size: 0.78rem;
  color: var(--text-2, #5a6472);
}
@media (max-width: 760px) {
  .list-head {
    display: none;
  }
  .row {
    grid-template-columns: 24px minmax(0, 1fr) auto auto;
    row-gap: 6px;
  }
  .size {
    display: none;
  }
  .list-head,
  .row {
    padding: 10px 12px;
  }
}
.btn {
  padding: 8px 18px;
  border: 1px solid var(--border-strong, #d7deea);
  border-radius: 10px;
  background: rgba(255, 255, 255, 0.75);
  color: var(--text, #1f2430);
  font-size: 0.85rem;
  font-weight: 600;
  cursor: pointer;
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.8),
    0 2px 6px -3px rgba(16, 28, 52, 0.2);
  transition:
    transform var(--dur-fast) var(--ease-out),
    border-color var(--dur-fast) var(--ease-inout),
    color var(--dur-fast) var(--ease-inout),
    background var(--dur-fast) var(--ease-inout),
    box-shadow var(--dur-fast) var(--ease-out),
    opacity var(--dur-fast) var(--ease-inout);
}
.btn:hover:not(:disabled) {
  border-color: rgba(37, 99, 235, 0.4);
  color: var(--accent);
  background: #fff;
}
.btn:active {
  transform: scale(0.97);
}
.btn:disabled {
  opacity: 0.45;
  box-shadow: none;
  cursor: default;
}
.btn.warn {
  border-color: rgba(214, 58, 47, 0.45);
  color: var(--grade-red, #c4382e);
  background: rgba(255, 244, 242, 0.85);
}
.btn.danger {
  border-color: transparent;
  background: linear-gradient(180deg, #e2544a 0%, #d63a2f 55%, #b92a20 100%);
  color: #fff;
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.28),
    0 6px 16px -8px rgba(214, 58, 47, 0.6);
}
.btn.danger:hover:not(:disabled) {
  color: #fff;
  filter: brightness(1.05);
}
.hint {
  font-size: 0.78rem;
  color: var(--text-2, #5a6472);
}
</style>
