<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { startup_list_report, startup_toggle } from "../api/commands";
import type { StartupEntry } from "../types/ipc";
import BootDiagnosisPanel from "../components/BootDiagnosisPanel.vue";
import {
  disableRiskText,
  failedSourcesNotice,
  impactLabel,
  isSystemDisabled,
  publisherLabel,
  sortByImpactDesc,
  sourceLabel,
  splitByEnabled,
  toggleFailText,
} from "./startupModel";

/** 屏：启动项（P3-03/R07）。影响降序；禁用=备份后移除（弹风险提示确认），
 *  已禁用区提供恢复入口。全程不删用户程序。 */
const entries = ref<StartupEntry[]>([]);
const failedSources = ref<string[]>([]);
const loaded = ref(false);
const pending = ref<StartupEntry | null>(null); // 待确认禁用项
const busyId = ref<string | null>(null);
const actionError = ref("");

async function refresh(): Promise<void> {
  const report = await startup_list_report();
  entries.value = sortByImpactDesc(report.entries);
  failedSources.value = report.failedSources;
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
  <section class="page anim-rise d1">
    <header class="head">
      <h1>启动项</h1>
      <p class="sub">开机自动运行的程序与任务——禁用只做备份后移除，不删除程序本身，可随时恢复</p>
    </header>

    <!-- 开机诊断（只读）：放在启动项列表之上，列表仍是唯一可操作入口 -->
    <BootDiagnosisPanel />

    <p v-if="failedSourcesNotice(failedSources)" class="source-warn" role="status">
      {{ failedSourcesNotice(failedSources) }}
    </p>

    <p v-if="!loaded" class="empty-list">加载中…</p>
    <p v-else-if="entries.length === 0" class="empty-list">
      {{ failedSources.length ? "未读取到启动项（有来源读取失败，见上方说明）" : "未发现启动项" }}
    </p>
    <template v-else>
      <div class="summary">
        <span class="chip">共 {{ entries.length }} 项</span>
        <span class="chip ok">启用 {{ groups.enabled.length }}</span>
        <span class="chip">已禁用 {{ groups.disabled.length }}</span>
        <span class="chip danger" v-if="highCount > 0">高影响 {{ highCount }} 项</span>
      </div>

      <p v-if="actionError" class="error">{{ actionError }}</p>

      <div class="board">
        <section class="panel">
          <div class="panel-head">
            <h2>已启用</h2>
            <span class="panel-count">{{ groups.enabled.length }}</span>
          </div>
          <p class="panel-hint">禁用 = 备份后移除，不删除程序本身，可随时恢复</p>
          <div class="panel-list">
            <p v-if="groups.enabled.length === 0" class="panel-empty">没有启用的启动项</p>
            <article
              v-for="e in groups.enabled"
              :key="e.id"
              class="item"
              :class="{ muted: isSystemDisabled(e) }"
            >
              <div class="item-main">
                <div class="item-top">
                  <span class="item-name" :title="e.name">{{ e.name }}</span>
                  <span class="impact" :class="e.impact">{{ impactLabel(e.impact) }}</span>
                </div>
                <div class="item-meta">
                  <span :class="{ unknown: publisherLabel(e.publisher) === '未知发布者' }">
                    {{ publisherLabel(e.publisher) }}
                  </span>
                  <span class="dot">·</span>
                  <span>{{ sourceLabel(e.source) }}</span>
                </div>
                <p class="item-cmd" :title="e.command">{{ e.command }}</p>
              </div>
              <div class="item-op">
                <span v-if="isSystemDisabled(e)" class="sys-disabled">系统已禁用</span>
                <button
                  v-else
                  class="btn"
                  :disabled="busyId === e.id || pending !== null"
                  @click="pending = e"
                >
                  {{ busyId === e.id ? "处理中…" : "禁用" }}
                </button>
              </div>
            </article>
          </div>
        </section>

        <section class="panel">
          <div class="panel-head">
            <h2>已禁用</h2>
            <span class="panel-count">{{ groups.disabled.length }}</span>
          </div>
          <p class="panel-hint">备份在 PureSlate，随时可恢复</p>
          <div class="panel-list">
            <p v-if="groups.disabled.length === 0" class="panel-empty">还没有禁用项</p>
            <article v-for="e in groups.disabled" :key="e.id" class="item">
              <div class="item-main">
                <div class="item-top">
                  <span class="item-name" :title="e.name">{{ e.name }}</span>
                </div>
                <div class="item-meta">
                  <span>{{ sourceLabel(e.source) }}</span>
                </div>
                <p class="item-cmd" :title="e.command">{{ e.command }}</p>
              </div>
              <button class="btn primary" :disabled="busyId === e.id" @click="restore(e)">
                {{ busyId === e.id ? "处理中…" : "恢复" }}
              </button>
            </article>
          </div>
        </section>
      </div>
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
  font-size: 0.85rem;
  color: var(--text-2, #5a6472);
  line-height: 1.65;
}
.empty-list {
  padding: 48px 0;
  text-align: center;
  color: var(--text-2, #5a6472);
  background: var(--surface);
  border: 1px solid var(--glass-line);
  border-radius: var(--r-xl);
  box-shadow: var(--shadow-1);
}
.summary {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  align-items: center;
  margin-bottom: 14px;
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
.chip.ok {
  color: var(--grade-green, #12a150);
  background: rgba(230, 246, 236, 0.85);
  border-color: rgba(18, 136, 74, 0.22);
}
.chip.danger {
  color: var(--grade-red, #c4382e);
  background: rgba(255, 244, 242, 0.85);
  border-color: rgba(214, 58, 47, 0.28);
}
.board {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 1rem;
  align-items: start;
}
.panel {
  display: flex;
  flex-direction: column;
  gap: 0.7rem;
  padding: 1.1rem 1.15rem 1.2rem;
  background: var(--surface);
  border: 1px solid var(--glass-line);
  border-radius: var(--r-xl);
  backdrop-filter: var(--glass-blur);
  -webkit-backdrop-filter: var(--glass-blur);
  box-shadow:
    var(--glass-sheen),
    var(--shadow-1);
}
.panel-head {
  display: flex;
  align-items: center;
  gap: 8px;
}
.panel-head h2 {
  margin: 0;
  font-size: 1rem;
  font-weight: 700;
  letter-spacing: -0.01em;
}
.panel-count {
  font-size: 0.72rem;
  font-weight: 700;
  color: var(--text-2, #5a6472);
  background: rgba(19, 32, 54, 0.06);
  border-radius: var(--r-pill);
  padding: 1px 8px;
}
.panel-hint {
  margin: 0;
  font-size: 0.76rem;
  line-height: 1.5;
  color: var(--text-2, #5a6472);
}
.panel-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-top: 2px;
}
.panel-empty {
  margin: 0;
  padding: 28px 0;
  text-align: center;
  font-size: 0.82rem;
  color: var(--text-2, #5a6472);
  border: 1px dashed var(--border, #e4e7ec);
  border-radius: var(--r-md);
}
.item {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 10px;
  padding: 0.7rem 0.8rem;
  border: 1px solid var(--border, #e4e7ec);
  border-radius: var(--r-md);
  background: rgba(255, 255, 255, 0.55);
  transition: background var(--dur-fast) var(--ease-inout);
}
.item:hover {
  background: rgba(255, 255, 255, 0.85);
}
.item.muted {
  opacity: 0.72;
}
.item-main {
  display: flex;
  flex-direction: column;
  gap: 3px;
  min-width: 0;
}
.item-top {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}
.item-name {
  font-size: 0.86rem;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.item-meta {
  display: flex;
  flex-wrap: wrap;
  gap: 5px;
  font-size: 0.73rem;
  color: var(--text-2, #5a6472);
  min-width: 0;
}
.item-meta .unknown {
  color: var(--grade-yellow, #b26a00);
}
.dot {
  color: var(--border-strong, #d7deea);
}
.item-cmd {
  margin: 0;
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: Consolas, monospace;
  font-size: 0.72rem;
  color: var(--text-2, #5a6472);
}
.item-op {
  flex: none;
  padding-top: 2px;
}
.impact {
  flex: none;
  padding: 1px 6px;
  border-radius: var(--r-pill);
  font-size: 0.7rem;
  font-weight: 700;
  background: rgba(19, 32, 54, 0.05);
}
.impact.high {
  color: var(--grade-red, #c4382e);
  background: rgba(255, 244, 242, 0.95);
}
.impact.medium {
  color: var(--grade-yellow, #b26a00);
  background: rgba(253, 247, 235, 0.95);
}
.impact.low {
  color: var(--grade-green, #12a150);
  background: rgba(230, 246, 236, 0.95);
}
.sys-disabled {
  color: var(--text-2, #5a6472);
  font-size: 0.78rem;
  white-space: nowrap;
}
@media (max-width: 900px) {
  .board {
    grid-template-columns: minmax(0, 1fr);
  }
}
.error {
  margin: 0 0 12px;
  padding: 9px 13px;
  border: 1px solid rgba(214, 58, 47, 0.35);
  border-radius: var(--r-md);
  background: rgba(255, 244, 242, 0.88);
  color: var(--grade-red, #c4382e);
  font-size: 0.85rem;
  box-shadow: var(--shadow-1);
}

/* 来源读取不完整：黄色警示，明确"空列表 ≠ 没有启动项"。 */
.source-warn {
  margin: 0 0 12px;
  padding: 9px 13px;
  border: 1px solid rgba(202, 158, 74, 0.4);
  border-radius: var(--r-md);
  background: rgba(253, 247, 235, 0.88);
  color: var(--text, #1f2328);
  font-size: 0.85rem;
  box-shadow: var(--shadow-1);
}
.btn {
  padding: 5px 14px;
  border: 1px solid var(--border-strong, #d7deea);
  border-radius: 9px;
  background: rgba(255, 255, 255, 0.75);
  color: var(--text, #1f2430);
  font-size: 0.82rem;
  font-weight: 600;
  cursor: pointer;
  transition:
    transform var(--dur-fast) var(--ease-out),
    border-color var(--dur-fast) var(--ease-inout),
    color var(--dur-fast) var(--ease-inout),
    background var(--dur-fast) var(--ease-inout),
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
  opacity: 0.5;
  cursor: default;
}
.btn.primary {
  border-color: rgba(37, 99, 235, 0.4);
  color: var(--accent, #2563eb);
  background: rgba(255, 255, 255, 0.88);
}
.btn.primary:hover {
  background: rgba(37, 99, 235, 0.08);
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
.mask {
  position: fixed;
  inset: 0;
  background: rgba(14, 22, 38, 0.34);
  backdrop-filter: blur(6px);
  -webkit-backdrop-filter: blur(6px);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 10;
  animation: ps-mask-in var(--dur-base) var(--ease-out);
}
.confirm {
  width: min(480px, calc(100vw - 48px));
  padding: 22px;
  border-radius: var(--r-xl);
  background: rgba(255, 255, 255, 0.94);
  border: 1px solid var(--glass-line);
  backdrop-filter: var(--glass-blur-strong);
  -webkit-backdrop-filter: var(--glass-blur-strong);
  box-shadow:
    var(--glass-sheen),
    0 30px 70px -20px rgba(14, 22, 38, 0.4);
  animation: ps-modal-pop var(--dur-base) var(--ease-out);
}
.confirm h3 {
  margin: 0 0 10px;
  font-size: 1.1rem;
  font-weight: 720;
  letter-spacing: -0.01em;
  color: var(--text, #1f2430);
}
.risk {
  margin: 0 0 10px;
  font-size: 0.88rem;
  line-height: 1.65;
  color: var(--text, #1f2430);
}
.cmd-line {
  margin: 0 0 16px;
  padding: 9px 11px;
  border-radius: var(--r-md);
  background: rgba(244, 247, 252, 0.8);
  border: 1px solid var(--border);
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
