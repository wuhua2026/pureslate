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
        <div class="toolbar">
          <span class="chip">共 {{ files.length }} 个</span>
          <span class="chip strong">合计 {{ formatBytes(totalBytes) }}</span>
          <span class="hint">个人文件 · 仅供查阅，应用内不提供删除</span>
          <span v-if="mockHint" class="mock-tag">mock</span>
        </div>
        <p v-if="revealError" class="reveal-err">{{ revealError }}</p>

        <section class="data" aria-label="大文件清单">
          <article v-for="f in files" :key="f.id" class="file-row">
            <span class="f-ico" aria-hidden="true">
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                <path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8Z" />
                <path d="M14 3v5h5" />
              </svg>
            </span>
            <div class="f-main">
              <p class="f-path" :title="f.path">{{ f.path }}</p>
              <p class="f-meta">
                修改 {{ formatDay(f.mtime) }} · 访问 {{ formatDay(f.atime) }}
              </p>
            </div>
            <span class="f-size">{{ formatBytes(f.sizeBytes) }}</span>
            <button
              class="reveal-btn"
              :disabled="revealingId !== ''"
              :title="`在资源管理器中打开并选中：${f.path}`"
              @click="revealInExplorer(f)"
            >
              {{ revealingId === f.id ? "打开中…" : "在资源管理器中显示" }}
            </button>
          </article>
        </section>
      </template>
    </template>
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
.empty {
  padding: 56px 0;
  text-align: center;
  color: var(--text-2, #5a6472);
  background: var(--surface);
  border: 1px solid var(--glass-line);
  border-radius: var(--r-xl);
  box-shadow:
    var(--glass-sheen),
    var(--shadow-1);
  backdrop-filter: var(--glass-blur);
  -webkit-backdrop-filter: var(--glass-blur);
}
.cta {
  margin-top: 12px;
  padding: 8px 22px;
  border: none;
  border-radius: 11px;
  background: linear-gradient(180deg, #3b7bf5 0%, #2563eb 55%, #1d51d8 100%);
  color: #fff;
  font-size: 0.9rem;
  font-weight: 650;
  cursor: pointer;
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.3),
    0 8px 18px -8px rgba(37, 99, 235, 0.55);
  transition:
    transform var(--dur-fast) var(--ease-out),
    filter var(--dur-fast) var(--ease-inout);
}
.cta:hover {
  filter: brightness(1.05);
}
.cta:active {
  transform: scale(0.98);
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
.chip.strong {
  color: var(--text, #1f2430);
}
.toolbar .hint {
  color: var(--text-2, #5a6472);
  font-size: 0.78rem;
}
.mock-tag {
  padding: 1px 8px;
  border-radius: var(--r-pill);
  background: var(--accent-soft);
  color: var(--accent);
  font-size: 0.72rem;
  font-weight: 600;
}
.reveal-err {
  margin: 0 0 8px;
  padding: 7px 11px;
  font-size: 0.8rem;
  color: var(--grade-red, #c4382e);
  background: rgba(255, 244, 242, 0.85);
  border: 1px solid rgba(214, 58, 47, 0.3);
  border-radius: var(--r-md);
}
/* 数据区：行卡片（长路径省略号截断，杜绝横向溢出） */
.data {
  display: flex;
  flex-direction: column;
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
.file-row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 11px 16px;
  border-bottom: 1px solid var(--border, #e4e7ec);
  transition: background var(--dur-fast) var(--ease-inout);
}
.file-row:last-child {
  border-bottom: none;
}
.file-row:hover {
  background: rgba(255, 255, 255, 0.72);
}
.f-ico {
  flex: none;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 32px;
  height: 32px;
  border-radius: 9px;
  background: rgba(244, 247, 252, 0.9);
  color: var(--text-3, #8a97aa);
  box-shadow: inset 0 0 0 1px var(--border);
}
.f-main {
  flex: 1;
  min-width: 0;
}
.f-path {
  margin: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: Consolas, monospace;
  font-size: 0.79rem;
  color: var(--text, #1f2430);
}
.f-meta {
  margin: 2px 0 0;
  font-size: 0.72rem;
  color: var(--text-2, #5a6472);
  font-variant-numeric: tabular-nums;
}
.f-size {
  flex: none;
  font-size: 0.85rem;
  font-weight: 700;
  color: var(--text, #1f2430);
  font-variant-numeric: tabular-nums;
  text-align: right;
  min-width: 72px;
}
.reveal-btn {
  padding: 5px 11px;
  border: 1px solid var(--border-strong, #d7deea);
  border-radius: 9px;
  background: rgba(255, 255, 255, 0.75);
  color: var(--accent, #2563eb);
  font-size: 0.76rem;
  font-weight: 600;
  cursor: pointer;
  transition:
    transform var(--dur-fast) var(--ease-out),
    border-color var(--dur-fast) var(--ease-inout),
    background var(--dur-fast) var(--ease-inout),
    opacity var(--dur-fast) var(--ease-inout);
}
.reveal-btn:hover:not(:disabled) {
  border-color: rgba(37, 99, 235, 0.5);
  background: #fff;
}
.reveal-btn:active {
  transform: scale(0.97);
}
.reveal-btn:disabled {
  cursor: default;
  opacity: 0.55;
}
</style>
