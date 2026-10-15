<script setup lang="ts">
/**
 * 屏：大文件审阅（P3-01/R05；R30 定位调整）。
 * ≥500MB 清单，大小降序。R30 起为**纯审阅页**：大文件绝大多数是用户主动存放的
 * 个人资料（视频/数据集/虚拟机镜像），不是垃圾——本应用不提供大文件删除入口，
 * 也不纳入任何清理方案；仅支持"在资源管理器中显示"，由用户自行确认处理。
 */
import { computed, ref } from "vue";
import { useRouter } from "vue-router";
import { commands, isMockMode } from "../api";
import { useScanStore } from "../stores/scan";
import type { ScanItem } from "../types/ipc";
import { formatBytes, formatDay, largeItems, sortBySizeDesc } from "./filesModel";

const scan = useScanStore();
const router = useRouter();

const files = computed(() => sortBySizeDesc(largeItems(scan.items)));
const totalBytes = computed(() => files.value.reduce((acc, f) => acc + f.sizeBytes, 0));
const hasScan = computed(() => scan.result !== null);

// "在资源管理器中显示"交互态（逐行）。
const revealingId = ref("");
const revealError = ref("");

async function revealInExplorer(f: ScanItem): Promise<void> {
  if (revealingId.value) return;
  revealingId.value = f.id;
  revealError.value = "";
  try {
    await commands.shell_reveal(f.path);
  } catch (e) {
    // 路径失效（文件已被移动/删除）等：就地提示，不打扰整页。
    revealError.value = `无法定位：${String(e)}`;
  } finally {
    revealingId.value = "";
  }
}

const mockHint = isMockMode();
</script>

<template>
  <section class="page anim-rise d1">
    <header class="head">
      <h1>大文件</h1>
      <p class="sub">
        ≥500MB 的文件清单，按大小降序。这些通常是你主动存放的个人资料，
        <strong>不会进入任何清理方案</strong>；如需处理，请逐项在资源管理器中自行确认。
      </p>
    </header>

    <div v-if="!hasScan" class="empty">
      <p>还没有扫描数据，先跑一次体检吧</p>
      <button class="cta" @click="router.push('/scan')">去体检</button>
    </div>

    <template v-else>
      <p v-if="files.length === 0" class="empty-list">本次扫描未发现 ≥500MB 的大文件</p>
      <template v-else>
        <div class="summary">
          <span>共 {{ files.length }} 个</span>
          <span>合计 {{ formatBytes(totalBytes) }}</span>
          <span class="hint">个人文件 · 仅供查阅，应用内不提供删除</span>
          <span v-if="mockHint" class="mock-tag">mock</span>
        </div>
        <p v-if="revealError" class="reveal-err">{{ revealError }}</p>
        <table class="list">
          <colgroup>
            <col class="col-path" />
            <col class="col-size" />
            <col class="col-date" />
            <col class="col-date" />
            <col class="col-op" />
          </colgroup>
          <thead>
            <tr>
              <th>文件</th>
              <th class="num">大小</th>
              <th>修改时间</th>
              <th>访问时间</th>
              <th class="op">操作</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="f in files" :key="f.id">
              <td class="path" :title="f.path">{{ f.path }}</td>
              <td class="num strong">{{ formatBytes(f.sizeBytes) }}</td>
              <td>{{ formatDay(f.mtime) }}</td>
              <td>{{ formatDay(f.atime) }}</td>
              <td class="op">
                <button
                  class="reveal-btn"
                  :disabled="revealingId !== ''"
                  :title="`在资源管理器中打开并选中：${f.path}`"
                  @click="revealInExplorer(f)"
                >
                  {{ revealingId === f.id ? "打开中…" : "在资源管理器中显示" }}
                </button>
              </td>
            </tr>
          </tbody>
        </table>
      </template>
    </template>
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
  line-height: 1.6;
}
.empty {
  padding: 48px 0;
  text-align: center;
  color: var(--text-2, #5a6472);
}
.cta {
  margin-top: 8px;
  padding: 6px 20px;
  border: none;
  border-radius: 8px;
  background: var(--accent, #3b82f6);
  color: #fff;
  font-size: 0.9rem;
  cursor: pointer;
  transition:
    transform var(--dur-fast) var(--ease-inout),
    background var(--dur-fast) var(--ease-inout);
}
.cta:hover {
  background: var(--accent-hover, #1d4ed8);
}
.cta:active {
  transform: scale(0.98);
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
.summary .hint {
  color: var(--text-2, #5a6472);
}
.mock-tag {
  padding: 0 8px;
  border-radius: 6px;
  background: var(--accent-soft);
  color: var(--accent);
  font-size: 0.72rem;
}
.reveal-err {
  margin: 0 0 8px;
  padding: 6px 10px;
  font-size: 0.8rem;
  color: var(--grade-red, #c4382e);
  background: rgba(220, 59, 48, 0.07);
  border: 1px solid rgba(220, 59, 48, 0.35);
  border-radius: 8px;
}
/* R30-3：fixed 布局 + 列宽，长路径省略号截断，杜绝横向溢出。 */
.list {
  width: 100%;
  table-layout: fixed;
  border-collapse: collapse;
  background: var(--surface, #fff);
  border: 1px solid var(--border, #e4e7ec);
  border-radius: 10px;
  overflow: hidden;
  font-size: 0.85rem;
}
.col-path {
  width: auto;
}
.col-size {
  width: 84px;
}
.col-date {
  width: 104px;
}
.col-op {
  width: 148px;
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
.list tbody tr {
  transition: background var(--dur-fast) var(--ease-inout);
}
.list tbody tr:hover {
  background: var(--surface-2, #f8fafc);
}
.list tbody tr:last-child td {
  border-bottom: none;
}
.path {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: Consolas, monospace;
  font-size: 0.8rem;
}
.num {
  text-align: right;
  white-space: nowrap;
  font-variant-numeric: tabular-nums;
}
.strong {
  font-weight: 600;
}
.op {
  text-align: right;
  white-space: nowrap;
}
.reveal-btn {
  padding: 4px 10px;
  border: 1px solid var(--border, #e4e7ec);
  border-radius: 7px;
  background: var(--surface, #fff);
  color: var(--accent, #2563eb);
  font-size: 0.76rem;
  cursor: pointer;
  transition:
    transform var(--dur-fast) var(--ease-inout),
    border-color var(--dur-fast) var(--ease-inout),
    background var(--dur-fast) var(--ease-inout),
    opacity var(--dur-fast) var(--ease-inout);
}
.reveal-btn:hover {
  border-color: var(--accent, #2563eb);
  background: var(--accent-soft, #eaf1ff);
}
.reveal-btn:active {
  transform: scale(0.97);
}
.reveal-btn:disabled {
  cursor: default;
  opacity: 0.55;
}
</style>
