<script setup lang="ts">
import { computed } from "vue";
import { useRouter } from "vue-router";
import GradeBadge from "../components/GradeBadge.vue";
import { useScanStore } from "../stores/scan";
import { formatBytes, formatDay, largeItems, sortBySizeDesc } from "./filesModel";

/** 屏：大文件（P3-01/R05）。≥500MB 清单，大小降序；信息展示为主，
 *  清理走报告页勾选确认（🟡 → 隔离区 14 天可还原）。 */
const scan = useScanStore();
const router = useRouter();

const files = computed(() => sortBySizeDesc(largeItems(scan.items)));
const totalBytes = computed(() => files.value.reduce((acc, f) => acc + f.sizeBytes, 0));
const hasScan = computed(() => scan.result !== null);
</script>

<template>
  <section class="page">
    <header class="head">
      <h1>大文件</h1>
      <p class="sub">≥500MB 的文件清单，按大小降序——仅信息展示，清理请到报告页勾选确认</p>
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
          <span class="hint">全部为谨慎档 · 入隔离区（14 天可还原）</span>
        </div>
        <table class="list">
          <thead>
            <tr>
              <th>文件</th>
              <th class="num">大小</th>
              <th>修改时间</th>
              <th>访问时间</th>
              <th>级别</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="f in files" :key="f.id">
              <td class="path" :title="f.path">{{ f.path }}</td>
              <td class="num strong">{{ formatBytes(f.sizeBytes) }}</td>
              <td>{{ formatDay(f.mtime) }}</td>
              <td>{{ formatDay(f.atime) }}</td>
              <td><GradeBadge :grade="f.grade" /></td>
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
.path {
  max-width: 420px;
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
.strong {
  font-weight: 600;
}
</style>
