<script setup lang="ts">
import { computed, ref } from "vue";
import { useRouter } from "vue-router";
import { useCleanStore } from "../stores/clean";
import { useScanStore } from "../stores/scan";
import { formatBytes, formatDay } from "./filesModel";
import {
  groupByCategory,
  guardHint,
  privacyItems,
  restorableNote,
  selectedTotalBytes,
} from "./privacyModel";

/** 屏：隐私资料（P3-05/R08）。逐项独立确认（默认全不勾），全部入隔离区可还原；
 *  浏览器类目受进程守卫（运行中整类阻止）。清理经 clean_execute → /executing。 */
const scan = useScanStore();
const clean = useCleanStore();
const router = useRouter();

const groups = computed(() => groupByCategory(privacyItems(scan.items)));
const hasScan = computed(() => scan.result !== null);

// 逐项独立确认：选中集合（默认空——隐私项不默认勾选）。
const selected = ref<Set<string>>(new Set());

function toggle(id: string): void {
  const next = new Set(selected.value);
  if (next.has(id)) {
    next.delete(id);
  } else {
    next.add(id);
  }
  selected.value = next;
}

const selectedCount = computed(() => selected.value.size);
const totalBytes = computed(() => selectedTotalBytes(scan.items, selected.value));

function cleanSelected(): void {
  if (selectedCount.value === 0 || clean.running) return;
  void clean.start([...selected.value]);
  void router.push("/executing");
}
</script>

<template>
  <section class="page anim-rise d1">
    <header class="head">
      <h1>隐私资料</h1>
      <p class="sub">浏览器历史与最近文档记录——逐项勾选确认后移入隔离区，保留期内可还原，不直接删除</p>
    </header>

    <div v-if="!hasScan" class="empty">
      <p>还没有扫描数据，先跑一次体检吧</p>
      <button class="cta" @click="router.push('/scan')">去体检</button>
    </div>

    <p v-else-if="groups.length === 0" class="empty-list">本次扫描未发现隐私痕迹</p>

    <template v-else>
      <div class="groups">
        <article
          v-for="(g, gi) in groups"
          :key="g.categoryId"
          class="group group-in"
          :style="{ animationDelay: Math.min(gi, 3) * 50 + 80 + 'ms' }"
        >
          <header class="group-head">
            <div class="group-title">
              <h2>{{ g.label }}</h2>
              <span class="count">{{ g.items.length }} 项</span>
            </div>
            <p class="reason">{{ g.reason }}</p>
            <p v-if="guardHint(g.categoryId)" class="guard">
              <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <path d="M12 3.6 21 19a1.4 1.4 0 0 1-1.2 2H4.2A1.4 1.4 0 0 1 3 19Z" />
                <path d="M12 9.6v4.1" />
                <circle cx="12" cy="17.2" r="0.9" fill="currentColor" stroke="none" />
              </svg>
              {{ guardHint(g.categoryId) }}
            </p>
          </header>
          <ul class="items">
            <li v-for="it in g.items" :key="it.id" :class="{ checked: selected.has(it.id) }">
              <label class="row">
                <input
                  type="checkbox"
                  :checked="selected.has(it.id)"
                  @change="toggle(it.id)"
                />
                <span class="row-main">
                  <span class="path" :title="it.path">{{ it.path }}</span>
                  <span class="row-meta">
                    {{ formatDay(it.mtime) }} · {{ restorableNote(it.disposition) }}
                  </span>
                </span>
                <span class="size">{{ formatBytes(it.sizeBytes) }}</span>
              </label>
            </li>
          </ul>
        </article>
      </div>

      <!-- 底部操作条：逐项独立确认汇总 -->
      <div class="actionbar">
        <span class="ab-stat">已选 {{ selectedCount }} 项</span>
        <span class="ab-stat">合计 {{ formatBytes(totalBytes) }}</span>
        <span class="note">全部入隔离区 · 保留期内可还原</span>
        <button class="btn" :disabled="selectedCount === 0 || clean.running" @click="cleanSelected">
          清理所选
        </button>
      </div>
    </template>
  </section>
</template>

<style scoped>
.page {
  padding-bottom: 40px;
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
  box-shadow: var(--shadow-1);
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
/* R29：分组卡片入场（≤3 组受控 stagger）*/
.group-in {
  opacity: 0;
  animation: ps-rise var(--dur-slow) var(--ease-out) forwards;
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
.groups {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 14px;
  align-items: start;
}
.group {
  background: var(--surface);
  border: 1px solid var(--glass-line);
  border-radius: var(--r-lg);
  overflow: hidden;
  box-shadow:
    var(--glass-sheen),
    var(--shadow-1);
  backdrop-filter: var(--glass-blur);
  -webkit-backdrop-filter: var(--glass-blur);
}
.group-head {
  padding: 13px 16px;
  border-bottom: 1px solid var(--border, #e4e7ec);
  background: rgba(244, 247, 252, 0.55);
}
.group-title {
  display: flex;
  align-items: baseline;
  gap: 8px;
  flex-wrap: wrap;
}
.group-head h2 {
  margin: 0;
  font-size: 1rem;
  font-weight: 700;
  letter-spacing: -0.01em;
  color: var(--text, #1f2430);
}
.count {
  font-size: 0.78rem;
  font-weight: 600;
  color: var(--text-2, #5a6472);
  background: rgba(19, 32, 54, 0.05);
  border-radius: var(--r-pill);
  padding: 1px 8px;
}
.reason {
  margin: 6px 0 0;
  font-size: 0.79rem;
  line-height: 1.55;
  color: var(--text-2, #5a6472);
}
.guard {
  margin: 6px 0 0;
  display: flex;
  align-items: flex-start;
  gap: 5px;
  font-size: 0.78rem;
  color: var(--grade-yellow, #d9971c);
  line-height: 1.5;
}
.guard svg {
  flex: none;
  margin-top: 2px;
}
.items {
  list-style: none;
  margin: 0;
  padding: 0;
}
.items li {
  border-bottom: 1px solid var(--border, #e4e7ec);
  transition: background var(--dur-fast) var(--ease-inout);
}
.items li:hover {
  background: rgba(255, 255, 255, 0.7);
}
.items li:last-child {
  border-bottom: none;
}
.items li.checked {
  background: rgba(37, 99, 235, 0.07);
}
.items li.checked:hover {
  background: rgba(37, 99, 235, 0.1);
}
.row {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  padding: 8px 14px;
  cursor: pointer;
  font-size: 0.85rem;
}
.row input {
  flex: none;
  margin-top: 3px;
  accent-color: var(--accent);
}
.row-main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.path {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: Consolas, monospace;
  font-size: 0.78rem;
  color: var(--text, #1f2430);
}
.row-meta {
  font-size: 0.72rem;
  color: var(--text-2, #5a6472);
  line-height: 1.4;
}
.size {
  flex: none;
  font-weight: 600;
  text-align: right;
  min-width: 56px;
  font-variant-numeric: tabular-nums;
}
.actionbar {
  position: sticky;
  bottom: 12px;
  z-index: 5;
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 11px 20px;
  border: 1px solid var(--glass-line);
  border-radius: var(--r-lg);
  background: rgba(255, 255, 255, 0.84);
  backdrop-filter: var(--glass-blur-strong);
  -webkit-backdrop-filter: var(--glass-blur-strong);
  box-shadow:
    var(--glass-sheen),
    var(--shadow-2);
  font-size: 0.85rem;
  color: var(--text, #1f2430);
  animation: ps-actionbar-up var(--dur-slow) var(--ease-out) both;
}
@keyframes ps-actionbar-up {
  from {
    opacity: 0;
    transform: translateY(10px);
  }
}
.actionbar .note {
  color: var(--grade-green, #12a150);
  font-size: 0.78rem;
  white-space: nowrap;
}
.ab-stat {
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}
.btn {
  padding: 8px 20px;
  border: none;
  border-radius: 10px;
  background: linear-gradient(180deg, #3b7bf5 0%, #2563eb 55%, #1d51d8 100%);
  color: #fff;
  font-size: 0.88rem;
  font-weight: 650;
  cursor: pointer;
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.3),
    0 6px 16px -8px rgba(37, 99, 235, 0.55);
  transition:
    transform var(--dur-fast) var(--ease-out),
    filter var(--dur-fast) var(--ease-inout),
    opacity var(--dur-fast) var(--ease-inout);
}
.btn:hover:not(:disabled) {
  filter: brightness(1.05);
}
.btn:active {
  transform: scale(0.98);
}
.btn:disabled {
  opacity: 0.45;
  box-shadow: none;
  cursor: default;
}
@media (max-width: 880px) {
  .groups {
    grid-template-columns: minmax(0, 1fr);
  }
}
@media (max-width: 640px) {
  .actionbar {
    flex-wrap: wrap;
    gap: 8px 14px;
  }
}
</style>
