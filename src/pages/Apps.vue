<script setup lang="ts">
/**
 * 软件卸载页（R27 · SPEC §6.7）。
 * R27-1：只读列表；R27-2：卸载执行三态（确认 → 进度 → 结果）。
 * 边界：破坏性删除主体是第三方官方卸载器（其删除不进本应用 journal/隔离区），
 * 本页只做拉起/等待/事后核对；取消仅停止等待，不强杀进程。
 * 残留隔离清理在 R27-3 接入既有管线。
 */
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { commands, events, isMockMode } from "../api";
import type { InstalledApp, UninstallDoneEvent } from "../types/ipc";
import {
  canUninstall,
  filterApps,
  formatSize,
  installDateLabel,
  scopeLabel,
  summarizeApps,
  uninstallResultMeta,
} from "./appsModel";

type View = "list" | "confirm" | "running" | "done";

const apps = ref<InstalledApp[]>([]);
const loading = ref(true);
const error = ref("");
const query = ref("");

const view = ref<View>("list");
const target = ref<InstalledApp | null>(null);
const txId = ref("");
const message = ref("");
const done = ref<UninstallDoneEvent | null>(null);

const visible = computed(() => filterApps(apps.value, query.value));
const summary = computed(() => summarizeApps(apps.value));
const resultMeta = computed(() =>
  done.value ? uninstallResultMeta(done.value.result) : null,
);
const busy = computed(() => view.value === "confirm" || view.value === "running");

let unlistenFns: Array<() => void> = [];
const mockTimers: number[] = [];

async function loadList(): Promise<void> {
  loading.value = true;
  error.value = "";
  try {
    apps.value = await commands.uninstall_list();
  } catch (e) {
    error.value = String(e);
  } finally {
    loading.value = false;
  }
}

onMounted(loadList);

onBeforeUnmount(() => {
  teardownRun();
});

// ---- 确认态 ----
function askUninstall(app: InstalledApp): void {
  target.value = app;
  done.value = null;
  view.value = "confirm";
}

function backToList(): void {
  view.value = "list";
  target.value = null;
  done.value = null;
  txId.value = "";
  message.value = "";
}

// ---- 执行态 ----
async function confirmRun(): Promise<void> {
  const app = target.value;
  if (!app) return;
  view.value = "running";
  message.value = "正在定位程序…";
  txId.value = "";
  done.value = null;

  if (isMockMode()) {
    await runMock(app);
  } else {
    await runReal(app);
  }
}

/** 真实模式：订阅事件（竞态窗口采纳首帧 txId）→ uninstall_run。 */
async function runReal(app: InstalledApp): Promise<void> {
  unlistenFns.push(
    await events.onUninstallProgress((p) => {
      if (!txId.value) txId.value = p.txId;
      if (p.txId !== txId.value) return;
      message.value = p.message;
    }),
  );
  unlistenFns.push(
    await events.onUninstallDone((e) => {
      if (!txId.value) txId.value = e.txId;
      if (e.txId !== txId.value) return;
      finishDone(e);
    }),
  );
  try {
    txId.value = await commands.uninstall_run(app.id);
  } catch (e) {
    finishDone({
      txId: txId.value || "unknown",
      appId: app.id,
      name: app.name,
      result: "launch_failed",
      keyStillPresent: true,
      rebootPending: false,
      detail: String(e),
    });
  }
}

/** mock 模式：定时器模拟官方卸载器阶段（事件流形态与真实内核一致）。 */
async function runMock(app: InstalledApp): Promise<void> {
  let id: string;
  try {
    id = await commands.uninstall_run(app.id);
  } catch (e) {
    finishDone({
      txId: "mock",
      appId: app.id,
      name: app.name,
      result: "launch_failed",
      keyStillPresent: true,
      rebootPending: false,
      detail: String(e),
    });
    return;
  }
  txId.value = id;
  const phases: Array<[number, string]> = [
    [500, "已调起官方卸载器，请在弹出的窗口中完成卸载…"],
    [1300, "等待卸载器的 2 个子进程退出…"],
    [1900, "正在核对卸载结果…"],
  ];
  for (const [delay, msg] of phases) {
    const t = window.setTimeout(() => {
      message.value = msg;
    }, delay);
    mockTimers.push(t);
  }
  const t = window.setTimeout(() => {
    apps.value = apps.value.filter((a) => a.id !== app.id);
    finishDone({
      txId: id,
      appId: app.id,
      name: app.name,
      result: "removed",
      keyStillPresent: false,
      rebootPending: false,
      detail: "程序已卸载（mock 模拟）",
    });
  }, 2400);
  mockTimers.push(t);
}

/** 停止等待：仅通知内核取消等待，不强杀卸载器（SPEC §6.7）。 */
async function cancelWait(): Promise<void> {
  if (txId.value) await commands.uninstall_cancel(txId.value);
  if (isMockMode()) {
    for (const t of mockTimers.splice(0)) window.clearTimeout(t);
    finishDone({
      txId: txId.value || "mock",
      appId: target.value?.id ?? "",
      name: target.value?.name ?? "",
      result: "cancelled",
      keyStillPresent: true,
      rebootPending: false,
      detail: "已停止等待；官方卸载器可能仍在运行，请在其窗口内完成或取消",
    });
  }
  // 真实模式：done(cancelled) 事件由内核推送，这里不本地造结论。
}

function finishDone(e: UninstallDoneEvent): void {
  done.value = e;
  view.value = "done";
  teardownRun();
  // removed/reboot_pending 后内核侧列表已变；真实模式后台刷新一次列表供返回时展示。
  if (!isMockMode() && (e.result === "removed" || e.result === "reboot_pending")) {
    void commands.uninstall_list().then((list) => {
      apps.value = list;
    });
  }
}

function teardownRun(): void {
  for (const t of mockTimers.splice(0)) window.clearTimeout(t);
  for (const fn of unlistenFns.splice(0)) fn();
}
</script>

<template>
  <section class="page">
    <header class="head">
      <h1>软件卸载</h1>
      <p class="sub">
        共 {{ summary.total }} 个程序（本机 {{ summary.machine }} · 当前用户 {{ summary.user }} ·
        MSI {{ summary.msi }}）——卸载将调用程序<strong>自带的卸载器</strong>执行，
        卸载残留由本应用扫描并隔离处理（可还原）。
      </p>
    </header>

    <div v-if="loading" class="state">加载中…</div>
    <div v-else-if="error" class="state err">加载失败：{{ error }}</div>

    <!-- 确认面板 -->
    <div v-else-if="view === 'confirm' && target" class="panel">
      <h2>确认卸载</h2>
      <div class="target">
        <strong>{{ target.name }}</strong>
        <span class="muted">{{ target.publisher ?? "未知发布者" }} · {{ target.version ?? "—" }}</span>
      </div>
      <ul class="notice">
        <li>将调起该程序<strong>自带的官方卸载器</strong>，请在它弹出的窗口中手动完成卸载。</li>
        <li>
          卸载器自身删除的文件<strong>不经过</strong>本应用的隔离区与操作日志，无法由本应用还原；
          请按官方卸载器的提示谨慎选择是否保留个人数据。
        </li>
        <li>本应用只负责拉起、等待与结果核对，<strong>不会强制结束任何进程</strong>。</li>
      </ul>
      <div class="actions">
        <button class="btn ghost" :disabled="busy" @click="backToList">取消</button>
        <button class="btn danger" @click="confirmRun">我已了解，调起官方卸载器</button>
      </div>
    </div>

    <!-- 进度面板 -->
    <div v-else-if="view === 'running'" class="panel">
      <h2>正在卸载{{ target ? `：${target.name}` : "" }}</h2>
      <div class="spin" aria-hidden="true"></div>
      <p class="phase">{{ message || "准备中…" }}</p>
      <p class="muted small">
        请在官方卸载器窗口中完成操作；停止等待不会关闭卸载器，也不会强制结束其进程。
      </p>
      <div class="actions">
        <button class="btn ghost" @click="cancelWait">停止等待</button>
      </div>
    </div>

    <!-- 结果面板 -->
    <div v-else-if="view === 'done' && done && resultMeta" class="panel">
      <h2 :class="['result-title', resultMeta.tone]">{{ resultMeta.title }}</h2>
      <p class="detail">{{ done.detail }}</p>
      <ul v-if="done.rebootPending" class="tag-row">
        <li class="tag info">系统已登记待重启删除</li>
      </ul>
      <ul v-if="done.installLocationStillPresent" class="tag-row">
        <li class="tag warn">安装目录仍存在</li>
      </ul>
      <p v-if="done.result === 'reboot_pending'" class="muted small">重启电脑后卸载才会最终完成。</p>
      <div class="actions">
        <button class="btn primary" @click="backToList">返回列表</button>
      </div>
    </div>

    <!-- 列表 -->
    <template v-else>
      <div class="toolbar">
        <input v-model="query" class="search" type="search" placeholder="搜索程序名/发布者/版本…" />
        <span class="count">{{ visible.length }} / {{ apps.length }}</span>
      </div>

      <div v-if="visible.length === 0" class="state">没有匹配的程序</div>

      <table v-else class="list">
        <thead>
          <tr>
            <th>名称</th>
            <th>发布者</th>
            <th>版本</th>
            <th>范围</th>
            <th>大小</th>
            <th>安装日期</th>
            <th class="op">操作</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="a in visible" :key="a.id">
            <td class="name">
              {{ a.name }}
              <span v-if="a.isMsi" class="badge msi" title="Windows Installer 产品">MSI</span>
              <span class="badge arch">{{ a.arch }}</span>
            </td>
            <td class="pub">{{ a.publisher ?? "未知发布者" }}</td>
            <td>{{ a.version ?? "—" }}</td>
            <td>{{ scopeLabel(a.scope) }}</td>
            <td>{{ formatSize(a.estimatedSizeBytes) || "—" }}</td>
            <td>{{ installDateLabel(a.installDate) || "—" }}</td>
            <td class="op">
              <button
                class="btn"
                :disabled="!canUninstall(a)"
                :title="canUninstall(a) ? '调起官方卸载器' : '该程序未登记可用的卸载命令'"
                @click="askUninstall(a)"
              >
                卸载
              </button>
            </td>
          </tr>
        </tbody>
      </table>

      <p class="footnote">
        安全说明：本应用不替代系统"应用和功能"；不会强杀进程，不处理 UWP 应用与系统组件。
      </p>
    </template>
  </section>
</template>

<style scoped>
.page {
  padding: 20px 24px;
  max-width: 1000px;
  margin: 0 auto;
}
.head h1 {
  margin: 0;
  font-size: 1.35rem;
}
.sub {
  margin: 4px 0 16px;
  font-size: 0.85rem;
  color: var(--text-2, #5a6472);
}
.state {
  padding: 32px 0;
  text-align: center;
  color: var(--text-2, #5a6472);
}
.state.err {
  color: var(--grade-red, #c4382e);
}
.toolbar {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-bottom: 12px;
}
.search {
  flex: 1;
  padding: 8px 12px;
  border: 1px solid var(--border, #e4e7ec);
  border-radius: 8px;
  font-size: 0.9rem;
  background: var(--surface, #fff);
  color: var(--text, #1f2430);
}
.count {
  font-size: 0.8rem;
  color: var(--text-2, #5a6472);
  white-space: nowrap;
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
  padding: 9px 12px;
  text-align: left;
  border-bottom: 1px solid var(--border, #e4e7ec);
}
.list th {
  background: #f6f8fb;
  color: var(--text-2, #5a6472);
  font-weight: 600;
  font-size: 0.78rem;
}
.list tbody tr:last-child td {
  border-bottom: none;
}
.name {
  font-weight: 600;
}
.pub {
  color: var(--text-2, #5a6472);
  max-width: 180px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.badge {
  display: inline-block;
  margin-left: 6px;
  padding: 0 6px;
  border-radius: 8px;
  font-size: 0.68rem;
  font-weight: 600;
  vertical-align: middle;
}
.badge.msi {
  background: #eef2ff;
  color: #1a56db;
}
.badge.arch {
  background: #f0f3f7;
  color: var(--text-2, #5a6472);
}
.op {
  text-align: right;
  white-space: nowrap;
}
.btn {
  padding: 5px 16px;
  border: 1px solid var(--border, #e4e7ec);
  border-radius: 8px;
  background: var(--surface, #fff);
  color: var(--text-2, #5a6472);
  font-size: 0.82rem;
  cursor: pointer;
}
.btn:disabled {
  cursor: not-allowed;
  opacity: 0.55;
}
.btn.ghost {
  background: var(--surface, #fff);
}
.btn.primary {
  background: #1a56db;
  border-color: #1a56db;
  color: #fff;
}
.btn.danger {
  background: #c4382e;
  border-color: #c4382e;
  color: #fff;
}
.footnote {
  margin-top: 12px;
  font-size: 0.75rem;
  color: var(--text-2, #5a6472);
}

/* 执行三态面板 */
.panel {
  background: var(--surface, #fff);
  border: 1px solid var(--border, #e4e7ec);
  border-radius: 10px;
  padding: 22px 24px;
}
.panel h2 {
  margin: 0 0 14px;
  font-size: 1.08rem;
}
.target {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 10px 12px;
  background: #f6f8fb;
  border-radius: 8px;
  margin-bottom: 14px;
}
.muted {
  color: var(--text-2, #5a6472);
}
.small {
  font-size: 0.78rem;
}
.notice {
  margin: 0 0 18px;
  padding-left: 20px;
  font-size: 0.86rem;
  line-height: 1.7;
  color: var(--text, #1f2430);
}
.actions {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
}
.spin {
  width: 30px;
  height: 30px;
  border: 3px solid #e4e7ec;
  border-top-color: #1a56db;
  border-radius: 50%;
  animation: ps-spin 0.8s linear infinite;
  margin: 6px 0 12px;
}
@keyframes ps-spin {
  to {
    transform: rotate(360deg);
  }
}
.phase {
  margin: 0 0 8px;
  font-size: 0.92rem;
}
.detail {
  margin: 0 0 12px;
  font-size: 0.9rem;
  line-height: 1.6;
}
.tag-row {
  list-style: none;
  margin: 0 0 12px;
  padding: 0;
  display: flex;
  gap: 8px;
}
.tag {
  padding: 2px 10px;
  border-radius: 10px;
  font-size: 0.75rem;
}
.tag.info {
  background: #eef2ff;
  color: #1a56db;
}
.tag.warn {
  background: #fdf3e7;
  color: #9a5b00;
}
.result-title.ok {
  color: #15803d;
}
.result-title.info {
  color: #1a56db;
}
.result-title.warn {
  color: #9a5b00;
}
.result-title.err {
  color: #c4382e;
}
</style>
