<script setup lang="ts">
import { onMounted, ref } from "vue";
import { crash_recovery, disk_usage } from "../api/commands";
import { DiskUsageStub, type DiskUsageEntry } from "../mocks/data";
import { recoveryNotice } from "./crashModel";
import type { DiskUsageInfo } from "../types/ipc";

// GB/TB 易读格式化：>=1024GB 显示 TB，否则 GB。
function formatBytes(bytes: number): string {
  const gb = bytes / (1024 * 1024 * 1024);
  if (gb >= 1024) return `${(gb / 1024).toFixed(1)} TB`;
  return `${gb.toFixed(0)} GB`;
}

function usagePercent(d: DiskUsageEntry | DiskUsageInfo): number {
  if (d.totalBytes <= 0) return 0;
  return Math.min(100, Math.round(((d.totalBytes - d.freeBytes) / d.totalBytes) * 100));
}

// v0.1.2：磁盘占用接真实数据（disk_usage 命令枚举固定盘）；失败回退 mock stub。
const disks = ref<DiskUsageEntry[] | DiskUsageInfo[]>(DiskUsageStub);

// v0.1.2：体检分两档——快速（temp/cache/startup/privacy，秒级~分钟级）与
// 深度（加 large/dup，大容量盘 + 非提权时可能十分钟级）。选择经 store 传递。
import { useScanStore } from "../stores/scan";
const scanStore = useScanStore();
function goScan(deep: boolean): void {
  scanStore.setDeepScan(deep);
}

// R24：启动恢复通知（上次清理被崩溃中断时，横幅告知处理结果）。
const recoveryBanner = ref<string | null>(null);
onMounted(async () => {
  try {
    recoveryBanner.value = recoveryNotice(await crash_recovery());
  } catch {
    recoveryBanner.value = null;
  }
  try {
    const real = await disk_usage();
    if (real.length > 0) disks.value = real;
  } catch {
    disks.value = DiskUsageStub;
  }
});
</script>

<template>
  <main class="home">
    <header class="hero">
      <h1>PureSlate</h1>
      <p class="tag">本地磁盘清理 · 只读扫描 · 安全分级 · 全程可还原</p>
    </header>

    <!-- R24 启动恢复横幅 -->
    <section v-if="recoveryBanner" class="card recovery-card">
      <p class="recovery-text">{{ recoveryBanner }}</p>
      <router-link to="/quarantine" class="recovery-link">前往隔离区查看 →</router-link>
    </section>

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
          <p class="cta-desc">快速体检扫描临时/缓存/隐私/启动项（约几秒到几分钟），不删除任何文件，等你确认后再执行。需要定位大文件与重复文件时可选深度体检（大容量盘较慢）。</p>
        </div>
        <div class="cta-btns">
          <router-link to="/scan" class="btn-deep" @click="goScan(true)">深度体检</router-link>
          <router-link to="/scan" class="btn-primary" @click="goScan(false)">开始体检 →</router-link>
        </div>
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
.cta-btns {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  flex-shrink: 0;
}
.btn-deep {
  padding: 0.7rem 1.1rem;
  border-radius: 8px;
  border: 1px solid var(--border);
  color: var(--text-2);
  font-size: 0.85rem;
  white-space: nowrap;
}
.btn-deep:hover {
  border-color: var(--accent);
  color: var(--accent);
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

/* R24 启动恢复横幅 */
.recovery-card {
  border-color: var(--grade-yellow);
  background: #fdf8ee;
}
.recovery-text {
  margin: 0 0 6px;
  color: var(--text);
  font-size: 0.88rem;
  line-height: 1.6;
}
.recovery-link {
  font-size: 0.82rem;
  color: var(--accent);
}
</style>