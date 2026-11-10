<script setup lang="ts">
import { computed, onBeforeUnmount, ref } from "vue";
import { boot_diagnose } from "../api/commands";
import type { BootDiagnosisReport } from "../types/ipc";
import { formatWhen } from "../pages/bootdiagModel";
import BootTimingCard from "./BootTimingCard.vue";
import BootStartupSourcesCard from "./BootStartupSourcesCard.vue";
import BootResidueCard from "./BootResidueCard.vue";
import BootPowerCard from "./BootPowerCard.vue";

/** 开机诊断（只读，B1-08）：默认不自动执行，结果只留在组件内存。 */
const report = ref<BootDiagnosisReport | null>(null);
const loading = ref(false);
const slow = ref(false);
const error = ref("");
let slowTimer: number | undefined;

async function run(): Promise<void> {
  loading.value = true;
  slow.value = false;
  error.value = "";
  slowTimer = window.setTimeout(() => {
    slow.value = true;
  }, 3000);
  try {
    report.value = await boot_diagnose();
  } catch (e) {
    report.value = null;
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    window.clearTimeout(slowTimer);
    loading.value = false;
  }
}

onBeforeUnmount(() => window.clearTimeout(slowTimer));

const meta = computed(() => {
  const r = report.value;
  if (!r) return "";
  return `采集于 ${formatWhen(r.collectedAt)} · ${r.isElevated ? "管理员权限" : "标准权限"}`;
});
</script>

<template>
  <section class="panel" aria-live="polite">
    <header class="head">
      <div>
        <h2>开机诊断</h2>
        <p class="sub">
          只读查看开机耗时、启动项来源、卸载残留与快速启动状态；不修改系统，不提供清理或禁用入口。
        </p>
      </div>
      <button class="btn" :disabled="loading" @click="run">
        {{ loading ? "诊断中…" : report ? "重新诊断" : "运行开机诊断（只读，约 1–2 秒）" }}
      </button>
    </header>

    <p v-if="error" class="error">诊断失败：{{ error }}</p>

    <div v-if="loading" class="loading">
      <div class="skel" v-for="i in 4" :key="i"></div>
      <p class="loading-text">{{ slow ? "仍在读取…" : "正在读取系统只读数据…" }}</p>
    </div>

    <template v-else-if="report">
      <p class="meta">{{ meta }}</p>
      <div class="grid">
        <BootTimingCard class="span-2" :section="report.timing" />
        <BootStartupSourcesCard class="span-2" :section="report.startup" />
        <BootResidueCard :section="report.residue" />
        <BootPowerCard :section="report.power" />
      </div>
    </template>

    <p v-else class="idle">尚未运行。诊断只读取系统数据，不写盘、不改注册表。</p>
  </section>
</template>

<style scoped>
.panel {
  background: var(--surface, #fff);
  border: 1px solid var(--border, #e7ebf2);
  border-radius: var(--r-card, 14px);
  box-shadow: var(--shadow-card, none);
  padding: 16px 18px;
  margin-bottom: 18px;
}
.head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
}
h2 {
  margin: 0;
  font-size: 1.05rem;
  color: var(--text, #1c2433);
}
.sub {
  margin: 4px 0 0;
  font-size: 0.8rem;
  line-height: 1.6;
  color: var(--text-2, #5d6b80);
  max-width: 60ch;
}
.btn {
  flex: 0 0 auto;
  border: 1px solid var(--accent, #2563eb);
  background: var(--accent, #2563eb);
  color: #fff;
  border-radius: 9px;
  padding: 7px 14px;
  font-size: 0.82rem;
  cursor: pointer;
  transition:
    background var(--dur-fast, 140ms) var(--ease-out, ease),
    opacity var(--dur-fast, 140ms) var(--ease-out, ease);
}
.btn:hover:not(:disabled) {
  background: var(--accent-hover, #1d4ed8);
}
.btn:disabled {
  opacity: 0.65;
  cursor: default;
}
.error {
  margin: 12px 0 0;
  font-size: 0.82rem;
  color: var(--grade-red, #dc3b30);
}
.meta {
  margin: 12px 0 10px;
  font-size: 0.75rem;
  color: var(--text-3, #8a97aa);
}
.grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 14px;
}
.span-2 {
  grid-column: 1 / -1;
}
.loading {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 10px;
  margin-top: 14px;
}
.skel {
  height: 72px;
  border-radius: 10px;
  background: linear-gradient(
    90deg,
    var(--surface-2, #f8fafc) 25%,
    var(--border, #e7ebf2) 37%,
    var(--surface-2, #f8fafc) 63%
  );
  background-size: 400% 100%;
  animation: shimmer 1.4s var(--ease-inout, ease) infinite;
}
.loading-text {
  grid-column: 1 / -1;
  margin: 2px 0 0;
  font-size: 0.78rem;
  color: var(--text-3, #8a97aa);
}
.idle {
  margin: 12px 0 0;
  font-size: 0.8rem;
  color: var(--text-3, #8a97aa);
}
@keyframes shimmer {
  0% {
    background-position: 100% 50%;
  }
  100% {
    background-position: 0 50%;
  }
}
@media (max-width: 900px) {
  .grid,
  .loading {
    grid-template-columns: minmax(0, 1fr);
  }
}
@media (prefers-reduced-motion: reduce) {
  .skel {
    animation: none;
  }
  .btn {
    transition: none;
  }
}
</style>
