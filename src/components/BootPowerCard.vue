<script setup lang="ts">
import { computed } from "vue";
import type { BootPowerData, BootSection } from "../types/ipc";
import { powerContradiction } from "../pages/bootdiagModel";
import BootStatusChip from "./BootStatusChip.vue";

const props = defineProps<{ section: BootSection<BootPowerData> }>();

const data = computed(() => props.section.data);
const contradiction = computed(() => powerContradiction(data.value));

function tri(v?: boolean): string {
  return v === undefined ? "—" : v ? "是" : "否";
}

const rows = computed<Array<{ label: string; value: string; note?: string }>>(() => {
  const d = data.value;
  if (!d) return [];
  return [
    {
      label: "快速启动（注册表）",
      value: !d.fastStartupRegistryPresent
        ? "无此设置"
        : d.fastStartupEnabled === undefined
          ? "无法判断"
          : d.fastStartupEnabled
            ? "已开启"
            : "已关闭",
      note: "HKEY_LOCAL_MACHINE\\SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Power",
    },
    { label: "快速启动（能力）", value: tri(d.hiberbootCapability) },
    { label: "休眠能力", value: tri(d.hibernateSupported) },
    { label: "休眠文件", value: tri(d.hiberFilePresent) },
    { label: "新式待机（Modern Standby）", value: tri(d.modernStandby) },
  ];
});
</script>

<template>
  <article class="card">
    <header class="card-head">
      <div>
        <h2>快速启动与休眠</h2>
        <p class="hint">配置意图与系统能力分开呈现，不自动修改</p>
      </div>
      <BootStatusChip :status="section.status" />
    </header>

    <p v-if="section.reason" class="reason">{{ section.reason }}</p>

    <p v-if="contradiction" class="warn">{{ contradiction }}</p>

    <dl v-if="rows.length > 0" class="rows">
      <template v-for="r in rows" :key="r.label">
        <dt>{{ r.label }}</dt>
        <dd :class="{ dim: r.value === '—' }">{{ r.value }}</dd>
        <dd v-if="r.note" class="note">{{ r.note }}</dd>
      </template>
    </dl>

    <p v-else class="empty">电源状态不可读。</p>
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
.reason {
  margin: 0 0 8px;
  font-size: 0.8rem;
  color: var(--text-2, #5d6b80);
  line-height: 1.6;
}
.warn {
  margin: 0 0 10px;
  padding: 8px 10px;
  border-radius: 10px;
  border: 1px solid rgba(217, 151, 28, 0.32);
  background: rgba(217, 151, 28, 0.07);
  font-size: 0.79rem;
  line-height: 1.6;
  color: #8a5b06;
}
.rows {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  gap: 6px 12px;
  margin: 0;
  font-size: 0.8rem;
}
dt {
  color: var(--text-2, #5d6b80);
}
dd {
  margin: 0;
  text-align: right;
  color: var(--text, #1c2433);
  font-variant-numeric: tabular-nums;
}
dd.dim {
  color: var(--text-3, #8a97aa);
}
dd.note {
  grid-column: 1 / -1;
  text-align: left;
  margin-top: -4px;
  font-size: 0.7rem;
  color: var(--text-3, #8a97aa);
  word-break: break-all;
}
.empty {
  margin: 10px 0 0;
  font-size: 0.8rem;
  color: var(--text-3, #8a97aa);
}
</style>
