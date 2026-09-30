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
  <section class="page">
    <header class="head">
      <h1>隐私资料</h1>
      <p class="sub">浏览器历史与最近文档记录——逐项勾选确认后移入隔离区，14 天内可还原，不直接删除</p>
    </header>

    <div v-if="!hasScan" class="empty">
      <p>还没有扫描数据，先跑一次体检吧</p>
      <button class="cta" @click="router.push('/scan')">去体检</button>
    </div>

    <p v-else-if="groups.length === 0" class="empty-list">本次扫描未发现隐私痕迹</p>

    <template v-else>
      <article v-for="g in groups" :key="g.categoryId" class="group">
        <header class="group-head">
          <h2>{{ g.label }}<span class="count">{{ g.items.length }} 项</span></h2>
          <p class="reason">{{ g.reason }}</p>
          <p v-if="guardHint(g.categoryId)" class="guard">⚠ {{ guardHint(g.categoryId) }}</p>
        </header>
        <ul class="items">
          <li v-for="it in g.items" :key="it.id" :class="{ checked: selected.has(it.id) }">
            <label class="row">
              <input
                type="checkbox"
                :checked="selected.has(it.id)"
                @change="toggle(it.id)"
              />
              <span class="path" :title="it.path">{{ it.path }}</span>
              <span class="meta">
                <span class="size">{{ formatBytes(it.sizeBytes) }}</span>
                <span class="date">{{ formatDay(it.mtime) }}</span>
                <span class="note">{{ restorableNote(it.disposition) }}</span>
              </span>
            </label>
          </li>
        </ul>
      </article>

      <!-- 底部操作条：逐项独立确认汇总 -->
      <div class="actionbar">
        <span>已选 {{ selectedCount }} 项</span>
        <span>合计 {{ formatBytes(totalBytes) }}</span>
        <span class="note">全部入隔离区 · 14 天内可还原</span>
        <button class="btn" :disabled="selectedCount === 0 || clean.running" @click="cleanSelected">
          清理所选
        </button>
      </div>
    </template>
  </section>
</template>

<style scoped>
.page {
  padding: 20px 24px 88px;
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
}
.empty-list {
  padding: 32px 0;
  text-align: center;
  color: var(--text-2, #5a6472);
}
.group {
  margin-bottom: 16px;
  background: var(--surface, #fff);
  border: 1px solid var(--border, #e4e7ec);
  border-radius: 10px;
  overflow: hidden;
}
.group-head {
  padding: 12px 16px;
  border-bottom: 1px solid var(--border, #e4e7ec);
  background: var(--bg, #fafafa);
}
.group-head h2 {
  margin: 0;
  font-size: 0.95rem;
  color: var(--text, #1f2430);
}
.count {
  margin-left: 8px;
  font-size: 0.8rem;
  font-weight: 400;
  color: var(--text-2, #5a6472);
}
.reason {
  margin: 6px 0 0;
  font-size: 0.8rem;
  color: var(--text-2, #5a6472);
}
.guard {
  margin: 4px 0 0;
  font-size: 0.8rem;
  color: var(--grade-yellow, #d9971c);
}
.items {
  list-style: none;
  margin: 0;
  padding: 0;
}
.items li {
  border-bottom: 1px solid var(--border, #e4e7ec);
}
.items li:last-child {
  border-bottom: none;
}
.items li.checked {
  background: rgba(59, 130, 246, 0.06);
}
.row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 8px 16px;
  cursor: pointer;
  font-size: 0.85rem;
}
.row input {
  flex: none;
}
.path {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: Consolas, monospace;
  font-size: 0.8rem;
  color: var(--text, #1f2430);
}
.meta {
  flex: none;
  display: flex;
  gap: 12px;
  align-items: baseline;
}
.size {
  font-weight: 600;
  text-align: right;
  min-width: 56px;
}
.date {
  color: var(--text-2, #5a6472);
  font-size: 0.78rem;
}
.note {
  color: var(--grade-green, #2e9e5b);
  font-size: 0.78rem;
  white-space: nowrap;
}
.actionbar {
  position: fixed;
  left: 50%;
  bottom: 16px;
  transform: translateX(-50%);
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 10px 20px;
  border: 1px solid var(--border, #e4e7ec);
  border-radius: 12px;
  background: var(--surface, #fff);
  box-shadow: 0 8px 24px rgba(15, 18, 25, 0.12);
  font-size: 0.85rem;
  color: var(--text, #1f2430);
}
.actionbar .note {
  color: var(--grade-green, #2e9e5b);
}
.btn {
  padding: 6px 20px;
  border: none;
  border-radius: 8px;
  background: var(--accent, #3b82f6);
  color: #fff;
  font-size: 0.88rem;
  font-weight: 600;
  cursor: pointer;
}
.btn:disabled {
  opacity: 0.45;
  cursor: default;
}
</style>
