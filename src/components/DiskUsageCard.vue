<script setup lang="ts">
import type { DiskUsageEntry } from "../mocks/data";
import type { DiskUsageInfo } from "../types/ipc";
import { useCountUp } from "../composables/useCountUp";

const props = defineProps<{ disk: DiskUsageEntry | DiskUsageInfo }>();

function formatBytes(bytes: number): string {
  const gb = bytes / (1024 * 1024 * 1024);
  if (gb >= 1024) return `${(gb / 1024).toFixed(1)} TB`;
  return `${gb.toFixed(0)} GB`;
}

function percentOf(d: DiskUsageEntry | DiskUsageInfo): number {
  if (d.totalBytes <= 0) return 0;
  return Math.min(100, Math.round(((d.totalBytes - d.freeBytes) / d.totalBytes) * 100));
}

const percent = useCountUp(() => percentOf(props.disk), { durationMs: 700 });
</script>

<template>
  <div class="disk-card">
    <div class="disk-top">
      <span class="drive-badge" :class="{ warn: percentOf(disk) >= 85 }">{{ disk.letter }}</span>
      <div class="disk-meta">
        <span class="disk-label">{{ disk.label }}</span>
        <span class="disk-nums">{{ formatBytes(disk.freeBytes) }} 可用 / {{ formatBytes(disk.totalBytes) }}</span>
      </div>
      <span class="disk-pct" :class="{ warn: percentOf(disk) >= 85 }">{{ percent }}%</span>
    </div>
    <div class="disk-bar">
      <div
        class="disk-bar-fill"
        :class="{ warn: percentOf(disk) >= 85, grow: true }"
        :style="{ '--target-w': percentOf(disk) + '%' }"
      ></div>
    </div>
  </div>
</template>

<style scoped>
.disk-card {
  background: var(--surface);
  border: 1px solid var(--glass-line);
  border-radius: var(--r-lg);
  box-shadow:
    var(--glass-sheen),
    var(--shadow-1);
  backdrop-filter: var(--glass-blur);
  -webkit-backdrop-filter: var(--glass-blur);
  padding: 15px 16px 14px;
  transition:
    transform var(--dur-base) var(--ease-out),
    box-shadow var(--dur-base) var(--ease-out);
}
.disk-card:hover {
  transform: translateY(-2px);
  box-shadow:
    var(--glass-sheen),
    var(--shadow-2);
}
.disk-top {
  display: flex;
  align-items: center;
  gap: 11px;
  margin-bottom: 12px;
}
.drive-badge {
  width: 36px;
  height: 36px;
  border-radius: 11px;
  background: linear-gradient(160deg, rgba(37, 99, 235, 0.16), rgba(37, 99, 235, 0.07));
  color: var(--accent);
  font-weight: 700;
  font-size: 0.88rem;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.7),
    inset 0 0 0 1px rgba(37, 99, 235, 0.1);
}
.drive-badge.warn {
  background: linear-gradient(160deg, rgba(201, 135, 8, 0.18), rgba(201, 135, 8, 0.08));
  color: var(--grade-yellow);
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.7),
    inset 0 0 0 1px rgba(178, 106, 0, 0.16);
}
.disk-meta {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
  flex: 1;
}
.disk-label {
  font-size: 0.86rem;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.disk-nums {
  font-size: 0.72rem;
  color: var(--text-3);
}
.disk-pct {
  font-size: 0.92rem;
  font-weight: 700;
  color: var(--accent);
  font-variant-numeric: tabular-nums;
}
.disk-pct.warn {
  color: var(--grade-yellow);
}
.disk-bar {
  height: 8px;
  border-radius: var(--r-pill);
  background: rgba(19, 32, 54, 0.06);
  box-shadow:
    inset 0 1px 2px rgba(16, 28, 52, 0.08),
    inset 0 0 0 1px rgba(19, 32, 54, 0.03);
  overflow: hidden;
}
.disk-bar-fill {
  height: 100%;
  border-radius: var(--r-pill);
  background: linear-gradient(90deg, #5b8cf5, var(--accent));
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.35);
  /* R29：宽度直接落到目标值，用 scaleX 做挂载生长（只动 transform，SPEC §7）*/
  width: var(--target-w);
  transform-origin: left center;
  animation: disk-grow var(--dur-slow) var(--ease-out) forwards;
}
.disk-bar-fill.warn {
  background: linear-gradient(90deg, #eab03c, #b87800);
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.35);
}
@keyframes disk-grow {
  from {
    transform: scaleX(0);
  }
  to {
    transform: scaleX(1);
  }
}
</style>
