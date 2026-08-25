<script setup lang="ts">
import { computed } from "vue";
import { DiskUsageStub, type DiskUsageEntry } from "../mocks/data";

// GB/TB 易读格式化：>=1024GB 显示 TB，否则 GB。
function formatBytes(bytes: number): string {
  const gb = bytes / (1024 * 1024 * 1024);
  if (gb >= 1024) return `${(gb / 1024).toFixed(1)} TB`;
  return `${gb.toFixed(0)} GB`;
}

function usagePercent(d: DiskUsageEntry): number {
  if (d.totalBytes <= 0) return 0;
  return Math.min(100, Math.round(((d.totalBytes - d.freeBytes) / d.totalBytes) * 100));
}

const disks = computed(() => DiskUsageStub);
</script>

<template>
  <main class="home">
    <header class="hero">
      <h1>PureSlate</h1>
      <p class="tag">本地磁盘清理 · 只读扫描 · 安全分级 · 全程可还原</p>
    </header>

    <!-- 磁盘占用条 -->
    <section class="card">
      <h2 class="card-title">磁盘占用</h2>
      <ul class="disk-list" v-if="disks.length">
        <li v-for="d in disks" :key="d.letter" class="disk-row">
          <div class="disk-head">
            <span class="disk-name">{{ d.letter }}盘 · {{ d.label }}</span>
            <span class="disk-nums">{{ formatBytes(d.totalBytes - d.freeBytes) }} 已用 / {{ formatBytes(d.freeBytes) }} 可用</span>
          </div>
          <div class="disk-bar">
            <div
              class="disk-bar-fill"
              :style="{ width: usagePercent(d) + '%' }"
              :class="{ warn: usagePercent(d) >= 85 }"
            ></div>
          </div>
        </li>
      </ul>
      <p v-else class="empty">暂无磁盘数据</p>
    </section>

    <!-- 一键体检 -->
    <section class="card">
      <div class="cta-row">
        <div>
          <h2 class="card-title">一键体检</h2>
          <p class="cta-desc">扫描各类可清理项并做安全分级，不删除任何文件，等你确认后再执行。</p>
        </div>
        <router-link to="/scan" class="btn-primary">开始体检 →</router-link>
      </div>
    </section>

    <!-- 快速建议区 -->
    <section class="card">
      <h2 class="card-title">快速建议</h2>
      <div class="suggest-grid">
        <router-link to="/scan" class="sugg">
          <span class="sugg-dot" style="background: var(--grade-green)"></span>
          <span class="sugg-label">临时文件</span>
          <span class="sugg-desc">缓存与临时残留，可安全清理</span>
        </router-link>
        <router-link to="/files" class="sugg">
          <span class="sugg-dot" style="background: var(--grade-green)"></span>
          <span class="sugg-label">大文件</span>
          <span class="sugg-desc">大体积文件定位，确认后再回收站</span>
        </router-link>
        <router-link to="/report" class="sugg">
          <span class="sugg-dot" style="background: var(--grade-yellow)"></span>
          <span class="sugg-label">重复文件</span>
          <span class="sugg-desc">内容相同副本，保留最早一份</span>
        </router-link>
        <router-link to="/startup" class="sugg">
          <span class="sugg-dot" style="background: var(--grade-yellow)"></span>
          <span class="sugg-label">启动项</span>
          <span class="sugg-desc">管理开机自启，按需禁用</span>
        </router-link>
      </div>
    </section>
  </main>
</template>

<style scoped>
.home {
  display: flex;
  flex-direction: column;
  gap: 1.25rem;
}

.hero h1 {
  margin: 0 0 0.25rem;
  font-size: 2rem;
  color: var(--accent);
}
.tag {
  margin: 0;
  color: var(--text-2);
}

.card {
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 1.25rem 1.5rem;
}
.card-title {
  margin: 0 0 0.75rem;
  font-size: 1.1rem;
  color: var(--text);
}

/* 磁盘占用条 */
.disk-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 1rem;
}
.disk-head {
  display: flex;
  justify-content: space-between;
  font-size: 0.95rem;
  margin-bottom: 0.35rem;
}
.disk-name {
  font-weight: 600;
}
.disk-nums {
  color: var(--text-2);
}
.disk-bar {
  height: 12px;
  border-radius: 6px;
  background: var(--bg);
  border: 1px solid var(--border);
  overflow: hidden;
}
.disk-bar-fill {
  height: 100%;
  background: var(--accent);
  border-radius: 6px;
  transition: width 0.3s ease;
}
.disk-bar-fill.warn {
  background: var(--grade-yellow);
}
.empty {
  color: var(--text-2);
}

/* 一键体检 */
.cta-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 1rem;
}
.cta-desc {
  margin: 0;
  color: var(--text-2);
  max-width: 40rem;
}
.btn-primary {
  flex-shrink: 0;
  background: var(--accent);
  color: #fff;
  padding: 0.7rem 1.5rem;
  border-radius: 8px;
  font-weight: 600;
  transition: filter 0.15s ease;
}
.btn-primary:hover {
  filter: brightness(1.05);
}

/* 快速建议 */
.suggest-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(210px, 1fr));
  gap: 0.75rem;
}
.sugg {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 0.85rem 1rem;
  transition: border-color 0.15s ease, background 0.15s ease;
}
.sugg:hover {
  border-color: var(--accent);
  background: var(--bg);
}
.sugg-dot {
  width: 10px;
  height: 10px;
  border-radius: 50%;
}
.sugg-label {
  font-weight: 600;
}
.sugg-desc {
  font-size: 0.8rem;
  color: var(--text-2);
  line-height: 1.5;
}
</style>