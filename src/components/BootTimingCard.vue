<script setup lang="ts">
import { computed } from "vue";
import type { BootSection, BootTimingData } from "../types/ipc";
import {
  formatMs,
  formatWhen,
  issueKindLabel,
  latestSample,
  phaseSegments,
  sortedIssues,
} from "../pages/bootdiagModel";
import BootStatusChip from "./BootStatusChip.vue";

const props = defineProps<{ section: BootSection<BootTimingData> }>();

const sample = computed(() => latestSample(props.section.data?.samples ?? []));
const segments = computed(() => phaseSegments(sample.value?.phases));
const issues = computed(() => sortedIssues(props.section.data?.issues ?? []));
const shownIssues = computed(() => issues.value.slice(0, 8));
const hiddenIssueCount = computed(() => Math.max(0, issues.value.length - shownIssues.value.length));
const recent = computed(() => (props.section.data?.samples ?? []).slice(0, 5));
</script>

<template>
  <article class="card">
    <header class="card-head">
      <div>
        <h2>开机耗时</h2>
        <p class="hint">来自系统「诊断-性能」事件，最近 7 天</p>
      </div>
      <BootStatusChip :status="section.status" />
    </header>

    <!-- 权限不足：说清为什么，不提供提权入口（评审决定） -->
    <div v-if="section.status === 'denied'" class="notice denied">
      <p class="notice-title">需要管理员权限读取系统日志</p>
      <p>
        「诊断-性能」通道的 ACL 默认不授予标准用户，因此普通权限下读不到任何开机记录。本页不提供提权入口。
      </p>
      <p class="path">
        手动查看：事件查看器 → 应用程序和服务日志 → Microsoft → Windows →
        Diagnostics-Performance → Operational
      </p>
      <p v-if="section.reason" class="reason">{{ section.reason }}</p>
    </div>

    <div v-else-if="section.status === 'unavailable'" class="notice">
      <p class="notice-title">没有可用的开机记录</p>
      <p>{{ section.reason || "事件通道未启用或尚无记录" }}</p>
    </div>

    <template v-else>
      <p v-if="section.reason" class="reason">{{ section.reason }}</p>

      <div v-if="sample" class="metrics">
        <div class="metric">
          <span class="metric-label">开机总耗时</span>
          <span class="metric-value">{{ formatMs(sample.bootTimeMs) }}</span>
        </div>
        <div class="metric">
          <span class="metric-label">主路径</span>
          <span class="metric-value">{{ formatMs(sample.mainPathBootTimeMs) }}</span>
        </div>
        <div class="metric">
          <span class="metric-label">启动后</span>
          <span class="metric-value">{{ formatMs(sample.postBootTimeMs) }}</span>
        </div>
        <div class="metric">
          <span class="metric-label">启动应用</span>
          <span class="metric-value">
            {{ sample.startupAppCount === undefined ? "—" : `${sample.startupAppCount} 个` }}
          </span>
        </div>
      </div>
      <p v-if="sample" class="when">记录时间：{{ formatWhen(sample.bootedAt) }}</p>

      <div v-if="segments.length > 0" class="phase-block">
        <div class="phase-bar" role="img" aria-label="可得阶段构成">
          <span
            v-for="s in segments"
            :key="s.key"
            class="phase-seg"
            :style="{ flexBasis: `${s.pct}%` }"
            :title="`${s.label} ${formatMs(s.ms)}（${s.pct.toFixed(0)}%）`"
          ></span>
        </div>
        <ul class="phase-legend">
          <li v-for="s in segments" :key="s.key">
            <span class="dot"></span>{{ s.label }} {{ formatMs(s.ms) }}
          </li>
        </ul>
        <p class="footnote">
          仅统计事件中可得的阶段，未列出的阶段可能缺失——各段之和不等同于开机总耗时。
        </p>
      </div>

      <div v-if="recent.length > 1" class="recent">
        <h3>最近 {{ recent.length }} 次</h3>
        <table>
          <thead>
            <tr>
              <th>时间（UTC）</th>
              <th class="num">总耗时</th>
              <th class="num">主路径</th>
              <th class="num">启动后</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="(s, i) in recent" :key="i">
              <td>{{ formatWhen(s.bootedAt) }}</td>
              <td class="num">{{ formatMs(s.bootTimeMs) }}</td>
              <td class="num">{{ formatMs(s.mainPathBootTimeMs) }}</td>
              <td class="num">{{ formatMs(s.postBootTimeMs) }}</td>
            </tr>
          </tbody>
        </table>
      </div>

      <div v-if="issues.length > 0" class="issues">
        <h3>拖慢开机的记录（{{ issues.length }} 条）</h3>
        <ul>
          <li v-for="(it, i) in shownIssues" :key="i">
            <span class="kind">{{ issueKindLabel(it.kind) }}</span>
            <span class="name">{{ it.friendlyName || it.name || "未命名项" }}</span>
            <span class="time">降级 {{ formatMs(it.degradationTimeMs) }}</span>
            <span v-if="it.totalTimeMs !== undefined" class="time dim">
              总 {{ formatMs(it.totalTimeMs) }}
            </span>
          </li>
        </ul>
        <p v-if="hiddenIssueCount > 0" class="footnote">另有 {{ hiddenIssueCount }} 条未展示。</p>
      </div>
    </template>
  </article>
</template>

<style scoped>
.card {
  background: var(--surface, #fff);
  border: 1px solid var(--border, #e7ebf2);
  border-radius: var(--r-card, 14px);
  box-shadow: var(--shadow-card, none);
  padding: 16px 18px;
}
.card-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 10px;
}
h2 {
  margin: 0;
  font-size: 1rem;
  color: var(--text, #1c2433);
}
.hint {
  margin: 2px 0 0;
  font-size: 0.78rem;
  color: var(--text-3, #8a97aa);
}
.notice {
  border: 1px solid var(--border, #e7ebf2);
  background: var(--surface-2, #f8fafc);
  border-radius: 10px;
  padding: 10px 12px;
  font-size: 0.84rem;
  color: var(--text-2, #5d6b80);
  line-height: 1.6;
}
.notice.denied {
  border-color: rgba(220, 59, 48, 0.3);
  background: rgba(220, 59, 48, 0.05);
}
.notice-title {
  margin: 0 0 4px;
  font-weight: 600;
  color: var(--text, #1c2433);
}
.notice p {
  margin: 0;
}
.path {
  margin-top: 6px !important;
  color: var(--text-3, #8a97aa);
}
.reason {
  margin: 0 0 8px;
  font-size: 0.8rem;
  color: var(--text-2, #5d6b80);
  line-height: 1.6;
}
.metrics {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 10px;
}
.metric {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 8px 10px;
  background: var(--surface-2, #f8fafc);
  border-radius: 10px;
}
.metric-label {
  font-size: 0.74rem;
  color: var(--text-3, #8a97aa);
}
.metric-value {
  font-size: 1.02rem;
  font-weight: 600;
  color: var(--text, #1c2433);
  font-variant-numeric: tabular-nums;
}
.when {
  margin: 8px 0 0;
  font-size: 0.75rem;
  color: var(--text-3, #8a97aa);
}
.phase-block {
  margin-top: 12px;
}
.phase-bar {
  display: flex;
  gap: 2px;
  height: 12px;
  border-radius: 6px;
  overflow: hidden;
  background: var(--surface-2, #f8fafc);
}
.phase-seg {
  display: block;
  height: 100%;
  background: var(--accent, #2563eb);
  opacity: 0.85;
}
.phase-seg:nth-child(2n) {
  opacity: 0.6;
}
.phase-seg:nth-child(3n) {
  opacity: 0.42;
}
.phase-legend {
  display: flex;
  flex-wrap: wrap;
  gap: 4px 14px;
  margin: 8px 0 0;
  padding: 0;
  list-style: none;
  font-size: 0.76rem;
  color: var(--text-2, #5d6b80);
}
.phase-legend li {
  display: inline-flex;
  align-items: center;
  gap: 5px;
}
.dot {
  width: 7px;
  height: 7px;
  border-radius: 2px;
  background: var(--accent, #2563eb);
  opacity: 0.75;
}
.footnote {
  margin: 6px 0 0;
  font-size: 0.74rem;
  color: var(--text-3, #8a97aa);
  line-height: 1.5;
}
.recent,
.issues {
  margin-top: 14px;
}
h3 {
  margin: 0 0 6px;
  font-size: 0.84rem;
  color: var(--text, #1c2433);
}
table {
  width: 100%;
  border-collapse: collapse;
  font-size: 0.78rem;
}
th,
td {
  text-align: left;
  padding: 4px 6px;
  border-bottom: 1px solid var(--border, #e7ebf2);
  color: var(--text-2, #5d6b80);
}
th {
  color: var(--text-3, #8a97aa);
  font-weight: 500;
}
.num {
  text-align: right;
  font-variant-numeric: tabular-nums;
}
.issues ul {
  margin: 0;
  padding: 0;
  list-style: none;
}
.issues li {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 5px 0;
  border-bottom: 1px dashed var(--border, #e7ebf2);
  font-size: 0.79rem;
}
.kind {
  flex: 0 0 auto;
  padding: 0 6px;
  border-radius: 5px;
  background: var(--accent-soft, #eaf1ff);
  color: var(--accent, #2563eb);
  font-size: 0.72rem;
}
.name {
  flex: 1 1 auto;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--text, #1c2433);
}
.time {
  flex: 0 0 auto;
  color: var(--grade-yellow, #d9971c);
  font-variant-numeric: tabular-nums;
}
.time.dim {
  color: var(--text-3, #8a97aa);
}
@media (prefers-reduced-motion: reduce) {
  .phase-seg {
    transition: none;
  }
}
</style>
