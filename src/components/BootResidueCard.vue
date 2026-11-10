<script setup lang="ts">
import { computed } from "vue";
import type { BootResidueData, BootSection } from "../types/ipc";
import { scopeLabel } from "../pages/bootdiagModel";
import BootStatusChip from "./BootStatusChip.vue";

const props = defineProps<{ section: BootSection<BootResidueData> }>();

const data = computed(() => props.section.data);
const entries = computed(() => data.value?.entries ?? []);
</script>

<template>
  <article class="card">
    <header class="card-head">
      <div>
        <h2>卸载残留</h2>
        <p class="hint">只读登记检查：卸载条目登记的目录与卸载程序是否还在</p>
      </div>
      <BootStatusChip :status="section.status" />
    </header>

    <p class="readonly">只读诊断，不提供清理入口。</p>
    <p v-if="section.reason" class="reason">{{ section.reason }}</p>

    <template v-if="data">
      <div class="summary">
        <span>疑似残留 <b>{{ entries.length }}</b> 条</span>
        <span class="dim">信息不足 <b>{{ data.unknownCount }}</b> 条</span>
      </div>
      <p class="footnote">
        「信息不足」指登记里没有安装位置或卸载命令、或指向网络路径，无法证实也无法证伪，不计入残留。
      </p>

      <table v-if="entries.length > 0">
        <thead>
          <tr>
            <th>名称</th>
            <th>发布者</th>
            <th>范围</th>
            <th>登记位置</th>
            <th>判定</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="e in entries" :key="e.id">
            <td class="ellipsis" :title="e.name">
              {{ e.name }}
              <span v-if="e.version" class="ver">{{ e.version }}</span>
            </td>
            <td class="ellipsis dim" :title="e.publisher">{{ e.publisher || "未知" }}</td>
            <td>{{ scopeLabel(e.scope) }}</td>
            <td class="ellipsis dim" :title="e.installLocation">{{ e.installLocation || "—" }}</td>
            <td class="reason-cell">{{ e.reason }}</td>
          </tr>
        </tbody>
      </table>
      <p v-else class="empty">没有发现登记层面的残留。</p>
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
  margin-bottom: 8px;
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
.readonly {
  margin: 0 0 8px;
  font-size: 0.76rem;
  color: var(--text-3, #8a97aa);
}
.reason {
  margin: 0 0 8px;
  font-size: 0.8rem;
  color: var(--text-2, #5d6b80);
  line-height: 1.6;
}
.summary {
  display: flex;
  gap: 14px;
  font-size: 0.8rem;
  color: var(--text-2, #5d6b80);
}
.summary b {
  font-variant-numeric: tabular-nums;
  color: var(--text, #1c2433);
}
.dim {
  color: var(--text-3, #8a97aa);
}
.footnote {
  margin: 6px 0 0;
  font-size: 0.73rem;
  color: var(--text-3, #8a97aa);
  line-height: 1.5;
}
table {
  width: 100%;
  border-collapse: collapse;
  font-size: 0.77rem;
  margin-top: 10px;
  table-layout: fixed;
}
th,
td {
  text-align: left;
  padding: 4px 6px;
  border-bottom: 1px solid var(--border, #e7ebf2);
  color: var(--text-2, #5d6b80);
  vertical-align: top;
}
th {
  color: var(--text-3, #8a97aa);
  font-weight: 500;
}
.ellipsis {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.ver {
  margin-left: 4px;
  font-size: 0.72rem;
  color: var(--text-3, #8a97aa);
}
.reason-cell {
  color: var(--grade-yellow, #d9971c);
}
.empty {
  margin: 10px 0 0;
  font-size: 0.8rem;
  color: var(--text-3, #8a97aa);
}
</style>
