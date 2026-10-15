<script setup lang="ts">
/**
 * 软件卸载页（R27 · SPEC §6.7）。
 * R27-1：只读列表；R27-2：卸载执行三态（确认 → 进度 → 结果）；
 * R27-3：卸载后残留扫描与清理（隔离区默认可还原 / 直接删除需专家令牌）。
 * 边界：第三方官方卸载器是删除主体；残留删除复用 cleaner 事务（journal/审计/隔离区）。
 */
import { computed, onBeforeUnmount, onMounted, reactive, ref } from "vue";
import { commands, events, isMockMode } from "../api";
import type {
  CleanDoneEvent,
  InstalledApp,
  ResidueCandidate,
  UninstallDoneEvent,
} from "../types/ipc";
import AppIcon from "../components/AppIcon.vue";
import ConfirmTokenModal from "../components/ConfirmTokenModal.vue";
import {
  canUninstall,
  filterApps,
  formatSize,
  installDateLabel,
  scopeLabel,
  summarizeApps,
  uninstallResultMeta,
} from "./appsModel";
import {
  buildSelections,
  initialRowState,
  kindLabel,
  rootTail,
  summarize,
  toggleAll,
  type DispositionChoice,
  type ResidueRowState,
} from "./residueModel";

type View = "list" | "confirm" | "running" | "done";
type ResidueView = "none" | "loading" | "list" | "running" | "result";

const apps = ref<InstalledApp[]>([]);
const loading = ref(true);
const error = ref("");
const query = ref("");

const view = ref<View>("list");
const target = ref<InstalledApp | null>(null);
const txId = ref("");
const message = ref("");
const done = ref<UninstallDoneEvent | null>(null);

// ---- R27-3 残留 ----
const residueView = ref<ResidueView>("none");
const candidates = ref<ResidueCandidate[]>([]);
const rowState = reactive(new Map<string, ResidueRowState>());
const residueError = ref("");
const tokenModalOpen = ref(false);
const cleanProgressMsg = ref("");
const cleanDone = ref<CleanDoneEvent | null>(null);
const cleanTxId = ref("");

const visible = computed(() => filterApps(apps.value, query.value));
const summary = computed(() => summarizeApps(apps.value));
const resultMeta = computed(() =>
  done.value ? uninstallResultMeta(done.value.result) : null,
);
const residueSummary = computed(() => summarize(candidates.value, rowState));
const allChecked = computed(
  () => candidates.value.length > 0 && residueSummary.value.selected === candidates.value.length,
);
/** 卸载成功/待重启后才允许扫描残留（其他结论不提供入口）。 */
const canScanResidue = computed(
  () =>
    done.value !== null &&
    (done.value.result === "removed" || done.value.result === "reboot_pending"),
);

let unlistenFns: Array<() => void> = [];
const mockTimers: number[] = [];

async function loadList(): Promise<void> {
  loading.value = true;
  error.value = "";
  try {
    apps.value = await commands.uninstall_list();
    void loadIcons();
  } catch (e) {
    error.value = String(e);
  } finally {
    loading.value = false;
  }
}

// ---- R30 程序图标（批量懒取一次；空串=已尝试但无图标，不再重复请求） ----
const iconUrls = reactive(new Map<string, string>());
const iconsLoading = ref(false);

async function loadIcons(): Promise<void> {
  const ids = apps.value.map((a) => a.id).filter((id) => !iconUrls.has(id));
  if (ids.length === 0 || iconsLoading.value) return;
  iconsLoading.value = true;
  try {
    const map = await commands.uninstall_app_icons(ids);
    for (const id of ids) iconUrls.set(id, map[id] ?? "");
  } catch {
    // 整批失败（IPC 异常）：全部回退字形，不阻断列表功能。
    for (const id of ids) iconUrls.set(id, "");
  } finally {
    iconsLoading.value = false;
  }
}

onMounted(loadList);

onBeforeUnmount(() => {
  teardownRun();
});

// ---- 卸载确认/执行（R27-2） ----
function askUninstall(app: InstalledApp): void {
  target.value = app;
  done.value = null;
  view.value = "confirm";
}

function backToList(): void {
  view.value = "list";
  residueView.value = "none";
  target.value = null;
  done.value = null;
  txId.value = "";
  message.value = "";
}

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
    mockTimers.push(window.setTimeout(() => (message.value = msg), delay));
  }
  mockTimers.push(
    window.setTimeout(() => {
      apps.value = apps.value.filter((a) => a.id !== app.id);
      finishDone({
        txId: id,
        appId: app.id,
        name: app.name,
        result: "removed",
        keyStillPresent: false,
        rebootPending: false,
        detail: "程序已卸载（mock 模拟）",
        installLocation: app.installLocation,
      });
    }, 2400),
  );
}

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
}

function finishDone(e: UninstallDoneEvent): void {
  done.value = e;
  view.value = "done";
  teardownRun();
  if (!isMockMode() && (e.result === "removed" || e.result === "reboot_pending")) {
    void commands.uninstall_list().then((list) => {
      apps.value = list;
      void loadIcons();
    });
  }
}

function teardownRun(): void {
  for (const t of mockTimers.splice(0)) window.clearTimeout(t);
  for (const fn of unlistenFns.splice(0)) fn();
}

// ---- R27-3 残留扫描 ----
async function scanResidue(): Promise<void> {
  residueView.value = "loading";
  residueError.value = "";
  candidates.value = [];
  rowState.clear();
  try {
    candidates.value = await commands.uninstall_residue_scan();
    const init = initialRowState(candidates.value);
    init.forEach((v, k) => rowState.set(k, v));
    residueView.value = "list";
  } catch (e) {
    residueError.value = String(e);
    residueView.value = "list";
  }
}

function setChecked(id: string, checked: boolean): void {
  const cur = rowState.get(id);
  if (cur) rowState.set(id, { ...cur, checked });
}

function setDisposition(id: string, disposition: DispositionChoice): void {
  const cur = rowState.get(id);
  if (cur) rowState.set(id, { ...cur, disposition });
}

function onToggleAll(e: Event): void {
  const checked = (e.target as HTMLInputElement).checked;
  const next = toggleAll(rowState, candidates.value, checked);
  rowState.clear();
  next.forEach((v, k) => rowState.set(k, v));
}

// ---- R27-3 残留清理 ----
function askClean(): void {
  if (residueSummary.value.selected === 0) return;
  if (residueSummary.value.needsToken) {
    tokenModalOpen.value = true; // 🔴 永久删除：专家模式一次性令牌
  } else {
    void runClean(undefined);
  }
}

async function onTokenConfirm(token: string): Promise<void> {
  tokenModalOpen.value = false;
  await runClean(token);
}

async function runClean(confirmToken: string | undefined): Promise<void> {
  residueView.value = "running";
  cleanDone.value = null;
  cleanProgressMsg.value = "正在准备残留清理…";
  cleanTxId.value = "";
  const params = {
    selections: buildSelections(rowState),
    ...(confirmToken ? { confirmToken } : {}),
  };

  if (isMockMode()) {
    await runCleanMock(params);
    return;
  }

  unlistenFns.push(
    await events.onCleanProgress((p) => {
      if (!cleanTxId.value) cleanTxId.value = p.txId;
      if (p.txId !== cleanTxId.value) return;
      cleanProgressMsg.value = p.itemPath;
    }),
  );
  unlistenFns.push(
    await events.onCleanDone((r) => {
      if (!cleanTxId.value) cleanTxId.value = r.txId;
      if (r.txId !== cleanTxId.value) return;
      cleanDone.value = r;
      residueView.value = "result";
      teardownRun();
    }),
  );
  try {
    cleanTxId.value = await commands.uninstall_residue_clean(params);
  } catch (e) {
    residueError.value = String(e);
    residueView.value = "list";
    teardownRun();
  }
}

async function runCleanMock(params: {
  selections: ReturnType<typeof buildSelections>;
}): Promise<void> {
  cleanTxId.value = "residue-mock";
  const total = params.selections.length;
  params.selections.forEach((_s, i) => {
    mockTimers.push(
      window.setTimeout(() => {
        cleanProgressMsg.value = `正在清理残留（${i + 1}/${total}）…`;
      }, 300 + i * 400),
    );
  });
  mockTimers.push(
    window.setTimeout(
      () => {
        cleanDone.value = {
          txId: cleanTxId.value,
          total,
          ok: total,
          fail: 0,
          skip: 0,
        };
        residueView.value = "result";
      },
      400 + total * 400,
    ),
  );
}

async function cancelClean(): Promise<void> {
  if (cleanTxId.value && !isMockMode()) {
    await commands.clean_cancel(cleanTxId.value);
  }
  for (const t of mockTimers.splice(0)) window.clearTimeout(t);
  residueView.value = "list";
}

/** 清理后重新扫描（看还剩什么）。 */
async function rescanAfterClean(): Promise<void> {
  await scanResidue();
}
</script>

<template>
  <section class="page anim-rise d1">
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

    <!-- R27-3 残留视图（覆盖列表/卸载面板） -->
    <template v-else-if="residueView !== 'none'">
      <div class="res-head">
        <button class="btn ghost small" @click="residueView = 'none'">← 返回卸载结果</button>
        <h2>卸载残留清理{{ done ? `：${done.name}` : "" }}</h2>
        <p class="muted small">
          仅扫描<strong>本次卸载</strong>的残留（安装目录 / 开始菜单 / AppData），不做全盘孤儿扫描；
          每项均给出判定理由，请逐项核对。
        </p>
      </div>

      <div v-if="residueView === 'loading'" class="state">
        <div class="spin"></div>正在扫描残留…
      </div>

      <template v-else-if="residueView === 'list'">
        <div v-if="residueError" class="state err">{{ residueError }}</div>
        <div v-else-if="candidates.length === 0" class="state">
          未发现残留——官方卸载器清理得很干净。
          <div class="res-actions">
            <button class="btn primary" @click="backToList">完成</button>
          </div>
        </div>

        <template v-else>
          <div class="res-toolbar">
            <label class="checkall">
              <input type="checkbox" :checked="allChecked" @change="onToggleAll" />
              全选
            </label>
            <span class="muted small">
              已选 {{ residueSummary.selected }}/{{ residueSummary.total }} 项 ·
              隔离 {{ residueSummary.quarantineCount }} · 直删
              {{ residueSummary.directCount }}
            </span>
          </div>

          <div
            v-for="c in candidates"
            :key="c.id"
            class="res-card"
            :class="{ off: !rowState.get(c.id)?.checked }"
          >
            <div class="res-card-head">
              <label class="res-check">
                <input
                  type="checkbox"
                  :checked="rowState.get(c.id)?.checked ?? false"
                  @change="(e) => setChecked(c.id, (e.target as HTMLInputElement).checked)"
                />
                <span class="badge kind">{{ kindLabel(c.kind) }}</span>
                <strong :title="c.rootPath">{{ rootTail(c.rootPath) }}</strong>
              </label>
              <span class="res-meta">{{ c.fileCount }} 个文件 · {{ formatSize(c.sizeBytes) }}</span>
            </div>
            <p class="res-reason">判定理由：{{ c.reason }}</p>
            <p class="res-path muted small" :title="c.rootPath">{{ c.rootPath }}</p>
            <div class="res-choice">
              <label :class="{ active: rowState.get(c.id)?.disposition === 'quarantine' }">
                <input
                  type="radio"
                  :name="`disp-${c.id}`"
                  value="quarantine"
                  :checked="rowState.get(c.id)?.disposition === 'quarantine'"
                  @change="setDisposition(c.id, 'quarantine')"
                />
                移入隔离区（14 天可还原，推荐）
              </label>
              <label :class="{ active: rowState.get(c.id)?.disposition === 'direct' }">
                <input
                  type="radio"
                  :name="`disp-${c.id}`"
                  value="direct"
                  :checked="rowState.get(c.id)?.disposition === 'direct'"
                  @change="setDisposition(c.id, 'direct')"
                />
                <span class="direct-text">直接删除（永久不可还原）</span>
              </label>
            </div>
          </div>

          <div class="res-actions">
            <button class="btn ghost" @click="residueView = 'none'">取消</button>
            <button class="btn primary" :disabled="residueSummary.selected === 0" @click="askClean">
              清理所选（{{ residueSummary.selected }}）
            </button>
          </div>
          <p v-if="residueSummary.needsToken" class="muted small token-hint">
            含"直接删除"项：执行时需要专家模式下的一次性确认令牌。
          </p>
        </template>
      </template>

      <!-- 清理进度 -->
      <div v-else-if="residueView === 'running'" class="panel">
        <div class="spin"></div>
        <p class="phase">正在清理残留…</p>
        <p class="muted small path-ellipsis" :title="cleanProgressMsg">{{ cleanProgressMsg }}</p>
        <div class="actions">
          <button class="btn ghost" @click="cancelClean">取消</button>
        </div>
      </div>

      <!-- 清理结果 -->
      <div v-else-if="residueView === 'result' && cleanDone" class="panel">
        <h2 :class="['result-title', cleanDone.fail > 0 ? 'warn' : 'ok']">
          {{ cleanDone.fail > 0 ? "部分完成" : "清理完成" }}
        </h2>
        <p class="detail">
          共 {{ cleanDone.total }} 项：成功 {{ cleanDone.ok }} · 失败 {{ cleanDone.fail }} ·
          跳过 {{ cleanDone.skip }}。移入隔离区的内容可在"隔离区"页面 14 天内还原。
        </p>
        <div class="actions">
          <button class="btn ghost" @click="rescanAfterClean">再扫一次</button>
          <button class="btn primary" @click="backToList">返回列表</button>
        </div>
      </div>
    </template>

    <!-- 卸载确认面板 -->
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
        <button class="btn ghost" @click="backToList">取消</button>
        <button class="btn danger" @click="confirmRun">我已了解，调起官方卸载器</button>
      </div>
    </div>

    <!-- 卸载进度面板 -->
    <div v-else-if="view === 'running'" class="panel">
      <h2>正在卸载{{ target ? `：${target.name}` : "" }}</h2>
      <div class="spin"></div>
      <p class="phase">{{ message || "准备中…" }}</p>
      <p class="muted small">
        请在官方卸载器窗口中完成操作；停止等待不会关闭卸载器，也不会强制结束其进程。
      </p>
      <div class="actions">
        <button class="btn ghost" @click="cancelWait">停止等待</button>
      </div>
    </div>

    <!-- 卸载结果面板 -->
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
        <button v-if="canScanResidue" class="btn primary" @click="scanResidue">
          扫描卸载残留
        </button>
        <button class="btn ghost" @click="backToList">返回列表</button>
      </div>
    </div>

    <!-- 程序列表 -->
    <template v-else>
      <div class="toolbar">
        <input v-model="query" class="search" type="search" placeholder="搜索程序名/发布者/版本…" />
        <span class="count">{{ visible.length }} / {{ apps.length }}</span>
      </div>

      <div v-if="visible.length === 0" class="state">没有匹配的程序</div>

      <table v-else class="list">
        <colgroup>
          <col class="col-icon" />
          <col class="col-name" />
          <col class="col-pub" />
          <col class="col-ver" />
          <col class="col-scope" />
          <col class="col-size" />
          <col class="col-date" />
          <col class="col-op" />
        </colgroup>
        <thead>
          <tr>
            <th class="ico" aria-label="图标"></th>
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
            <td class="ico">
              <img
                v-if="iconUrls.get(a.id)"
                :src="iconUrls.get(a.id)"
                class="app-icon"
                alt=""
                draggable="false"
                @error="iconUrls.set(a.id, '')"
              />
              <span v-else class="app-icon-fallback" aria-hidden="true">
                <AppIcon name="apps" :size="14" />
              </span>
            </td>
            <td class="name">
              <div class="name-cell">
                <span class="name-text" :title="a.name">{{ a.name }}</span>
                <span v-if="a.isMsi" class="badge msi" title="Windows Installer 产品">MSI</span>
                <span class="badge arch">{{ a.arch }}</span>
              </div>
            </td>
            <td class="pub" :title="a.publisher ?? ''">{{ a.publisher ?? "未知发布者" }}</td>
            <td class="ver" :title="a.version ?? ''">{{ a.version ?? "—" }}</td>
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

    <ConfirmTokenModal v-model="tokenModalOpen" :has-red="true" @confirm="onTokenConfirm" />
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
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
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
  transition:
    border-color var(--dur-fast) var(--ease-inout),
    box-shadow var(--dur-fast) var(--ease-inout);
}
.search:focus {
  outline: none;
  border-color: var(--accent);
  box-shadow: 0 0 0 3px rgba(37, 99, 235, 0.12);
}
.count {
  font-size: 0.8rem;
  color: var(--text-2, #5a6472);
  white-space: nowrap;
}
.list {
  width: 100%;
  /* R30-3：fixed 布局——超长程序名/发布者由列宽+省略号截断，杜绝横向溢出。 */
  table-layout: fixed;
  border-collapse: collapse;
  background: var(--surface, #fff);
  border: 1px solid var(--border, #e4e7ec);
  border-radius: 10px;
  overflow: hidden;
  font-size: 0.85rem;
}
/* R30-3/R30-4：百分比列宽（合计 100%）——任意窗口宽度下列区间严格不重叠；
   像素列宽在窄窗会超约束把 auto 列挤成 0，导致列重叠。 */
.col-icon {
  width: 7%;
}
.col-name {
  width: 32%;
}
.col-pub {
  width: 15%;
}
.col-ver {
  width: 12%;
}
.col-scope {
  width: 6%;
}
.col-size {
  width: 9%;
}
.col-date {
  width: 10%;
}
.col-op {
  width: 9%;
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
.list td {
  font-variant-numeric: tabular-nums;
}
.list tbody tr {
  transition: background var(--dur-fast) var(--ease-inout);
}
.list tbody tr:hover {
  background: var(--surface-2, #f8fafc);
}
.list tbody tr:last-child td {
  border-bottom: none;
}
/* R30-4：软件图标（22px，与系统程序列表的观感对齐；缺失走同款灰底字形）。 */
.ico {
  text-align: center;
  vertical-align: middle;
}
.app-icon,
.app-icon-fallback {
  width: 22px;
  height: 22px;
  border-radius: 5px;
  vertical-align: middle;
}
.app-icon {
  object-fit: contain;
  /* 图标加载瞬间不撑动行高 */
  background: transparent;
}
.app-icon-fallback {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background: var(--surface-2, #f1f4f9);
  color: var(--text-3, #8a97aa);
}

/* R30-3：td 保持 table-cell（display:flex 直接放 td 会破坏固定列表格），
   内层 .name-cell 做 flex——文本截断省略，徽章固定不被压缩/截掉。 */
.name-cell {
  display: flex;
  align-items: center;
  gap: 5px;
  min-width: 0;
}
.name {
  font-weight: 600;
}
.name-text {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.pub,
.ver {
  color: var(--text-2, #5a6472);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.badge {
  display: inline-block;
  /* R30-3：名称列改用 flex gap 间距；徽章固定不缩 */
  margin-left: 0;
  flex: none;
  padding: 0 6px;
  border-radius: 8px;
  font-size: 0.68rem;
  font-weight: 600;
  vertical-align: middle;
}
.badge.msi {
  background: var(--accent-soft);
  color: var(--accent);
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
  transition:
    transform var(--dur-fast) var(--ease-inout),
    border-color var(--dur-fast) var(--ease-inout),
    color var(--dur-fast) var(--ease-inout),
    background var(--dur-fast) var(--ease-inout),
    opacity var(--dur-fast) var(--ease-inout);
}
.btn:active {
  transform: scale(0.97);
}
.btn:disabled {
  cursor: not-allowed;
  opacity: 0.55;
}
.btn.small {
  padding: 3px 10px;
  font-size: 0.76rem;
}
.btn.ghost {
  background: var(--surface, #fff);
}
.btn.primary {
  background: var(--accent);
  border-color: var(--accent);
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

/* 面板（确认/进度/结果） */
.panel {
  background: var(--surface, #fff);
  border: 1px solid var(--border, #e4e7ec);
  border-radius: 10px;
  padding: 22px 24px;
  animation: ps-rise var(--dur-base) var(--ease-out) both;
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
.actions,
.res-actions {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
}
.res-actions {
  margin-top: 16px;
}
.spin {
  width: 30px;
  height: 30px;
  border: 3px solid #e4e7ec;
  border-top-color: var(--accent);
  border-radius: 50%;
  animation: ps-spin 0.8s linear infinite;
  margin: 6px auto 12px;
}
@keyframes ps-spin {
  to {
    transform: rotate(360deg);
  }
}
.phase {
  margin: 0 0 8px;
  font-size: 0.92rem;
  text-align: center;
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
  background: var(--accent-soft);
  color: var(--accent);
}
.tag.warn {
  background: #fdf3e7;
  color: #9a5b00;
}
.result-title.ok {
  color: #15803d;
}
.result-title.info {
  color: var(--accent);
}
.result-title.warn {
  color: #9a5b00;
}
.result-title.err {
  color: #c4382e;
}

/* R27-3 残留 */
.res-head {
  margin-bottom: 14px;
}
.res-head h2 {
  margin: 8px 0 4px;
  font-size: 1.1rem;
}
.res-head .small {
  margin: 0;
}
.res-toolbar {
  display: flex;
  align-items: center;
  gap: 14px;
  margin-bottom: 12px;
  font-size: 0.82rem;
}
.checkall {
  display: flex;
  align-items: center;
  gap: 6px;
  font-weight: 600;
}
.res-card {
  background: var(--surface, #fff);
  border: 1px solid var(--border, #e4e7ec);
  border-radius: 10px;
  padding: 12px 14px;
  margin-bottom: 10px;
  transition:
    border-color var(--dur-fast) var(--ease-inout),
    background var(--dur-fast) var(--ease-inout),
    opacity var(--dur-fast) var(--ease-inout);
}
.res-card:hover {
  border-color: var(--accent-soft-2, #dbe7ff);
  background: var(--surface-2, #f8fafc);
}
.res-card.off {
  opacity: 0.55;
}
.res-card.off:hover {
  background: var(--surface, #fff);
}
.res-card-head {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 10px;
}
.res-check {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}
.res-check strong {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.res-meta {
  font-size: 0.76rem;
  color: var(--text-2, #5a6472);
  white-space: nowrap;
}
.badge.kind {
  margin-left: 0;
  background: var(--accent-soft);
  color: var(--accent);
  padding: 1px 8px;
}
.res-reason {
  margin: 8px 0 2px;
  font-size: 0.83rem;
}
.res-path {
  margin: 0 0 8px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.res-choice {
  display: flex;
  gap: 18px;
  font-size: 0.82rem;
  padding-top: 6px;
  border-top: 1px dashed var(--border, #e4e7ec);
}
.res-choice label {
  display: flex;
  align-items: center;
  gap: 6px;
  cursor: pointer;
}
.res-choice label.active {
  font-weight: 600;
}
.direct-text {
  color: #c4382e;
}
.token-hint {
  margin-top: 8px;
  text-align: right;
}
.path-ellipsis {
  text-align: center;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 560px;
  margin: 0 auto;
}
</style>
