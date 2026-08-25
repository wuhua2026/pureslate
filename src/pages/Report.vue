<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { commands } from "../api";
import GradeBadge from "../components/GradeBadge.vue";
import type { ScanItem } from "../types/ipc";
import { buildReportOverview, type ReportCategory, type ReportPlan } from "./reportModel";

// mock 体检的 scanId（P1-07 纯 mock；P1-08 真数据联调时改取真实 scanId）。
const MOCK_SCAN_ID = "mock-scan-0001";

const items = ref<ScanItem[]>([]);
const loading = ref(true);
const error = ref("");

// 方案选择：给选项不给结论（A 保守 / B 激进），默认 A。
const selectedPlan = ref<"A" | "B">("A");

function formatBytes(bytes: number): string {
  const gb = bytes / (1024 * 1024 * 1024);
  return gb >= 1024 ? `${(gb / 1024).toFixed(1)} TB` : `${gb.toFixed(1)} GB`;
}

const overview = computed(() => buildReportOverview(items.value));

// 当前方案下被纳入的类目 id 集合。
const includedCategoryIds = computed(() => {
  const plan = selectedPlan.value === "A" ? overview.value.planA : overview.value.planB;
  return new Set(overview.value.categories.filter((c) => plan.grades.includes(c.grade)).map((c) => c.categoryId));
});

const planSummary = (p: ReportPlan) => `${p.items} 项 · ${formatBytes(p.bytes)}`;

const selectedBytes = computed(() =>
  overview.value.categories
    .filter((c) => includedCategoryIds.value.has(c.categoryId))
    .reduce((s, c) => s + c.totalBytes, 0),
);

const redExpanded = ref(false);

function isIncluded(cat: ReportCategory): boolean {
  return includedCategoryIds.value.has(cat.categoryId);
}

onMounted(async () => {
  try {
    const res = await commands.scan_get_items({ scanId: MOCK_SCAN_ID, offset: 0, limit: 100000 });
    items.value = res;
  } catch (e) {
    error.value = String(e);
  } finally {
    loading.value = false;
  }
});
</script>

<template>
  <main class="report">
    <header class="rep-head">
      <router-link to="/" class="back">← 体检报告</router-link>
    </header>

    <div v-if="loading" class="state">加载中…</div>
    <div v-else-if="error" class="state err">加载失败：{{ error }}</div>

    <template v-else>
      <!-- 总览 -->
      <section class="overview">
        <div class="metric">
          <span class="metric-label">可立即释放</span>
          <span class="metric-val g">{{ formatBytes(overview.immediateBytes) }}</span>
          <span class="metric-note">🟢 直清 / 回收站</span>
        </div>
        <div class="metric">
          <span class="metric-label">进隔离区（可还原）</span>
          <span class="metric-val y">{{ formatBytes(overview.quarantineBytes) }}</span>
          <span class="metric-note">🟡/🔴 14 天可还原</span>
        </div>
        <div class="metric">
          <span class="metric-label">风险分布</span>
          <span class="metric-val risk">
            <span class="g-r">🟢×{{ overview.riskItems.green }}</span>
            <span class="y-r">🟡×{{ overview.riskItems.yellow }}</span>
            <span class="r-r">🔴×{{ overview.riskItems.red }}</span>
          </span>
          <span class="metric-note">清理项 / 发件数</span>
        </div>
      </section>

      <!-- 逐项类目（可解释 + 去向预告） -->
      <section class="card">
        <h2 class="card-title">清理项（可了解每项为什么删）</h2>
        <ul class="cat-list">
          <li
            v-for="cat in overview.categories"
            :key="cat.categoryId"
            class="cat-row"
            :class="{ red: cat.grade === 'red' && !redExpanded }"
          >
            <span v-if="cat.grade !== 'red'" class="check">{{ isIncluded(cat) ? "☑" : "☐" }}</span>
            <span v-else class="check lock">🔒</span>

            <span class="cat-main">
              <span class="cat-line">
                <GradeBadge :grade="cat.grade" />
                <span class="cat-label">{{ cat.label }}</span>
                <span class="cat-size">{{ formatBytes(cat.totalBytes) }} · {{ cat.itemCount }}项</span>
                <span class="cat-disp">→ {{ cat.dispositionLabel }}</span>
              </span>
              <span v-if="isIncluded(cat) || cat.grade === 'yellow'" class="cat-reason">{{ cat.reason }}</span>
              <span v-else-if="cat.grade === 'red'" class="cat-reason reddish">危险项默认禁用，需专家模式 + 二次确认</span>
            </span>
          </li>
        </ul>

        <!-- 🔴 折叠灰禁：危险项默认隐藏，仅展开查看，仍不可选 -->
        <button class="red-toggle" @click="redExpanded = !redExpanded">
          危险项（{{ overview.redCategoryCount }}）{{ redExpanded ? "收起" : "展开查看" }}
        </button>
        <p class="red-note">
          🔴 危险项一律进<span class="em">隔离区（14 天可还原）</span>，默认灰禁；需<span class="em">专家模式</span>开启后才能勾选，并须二次确认。
        </p>
      </section>

      <!-- A/B 方案对比：给选项不给结论 -->
      <section class="card">
        <h2 class="card-title">清理方案（给选项不给结论）</h2>
        <div class="plans">
          <button
            class="plan"
            :class="{ active: selectedPlan === 'A' }"
            @click="selectedPlan = 'A'"
          >
            <span class="plan-name">方案 A · 保守 <span class="tag">推荐</span></span>
            <span class="plan-desc">仅清理 ✓ 绿色项（立即释放）</span>
            <span class="plan-nums">{{ planSummary(overview.planA) }}</span>
          </button>
          <button
            class="plan"
            :class="{ active: selectedPlan === 'B' }"
            @click="selectedPlan = 'B'"
          >
            <span class="plan-name">方案 B · 激进</span>
            <span class="plan-desc">绿色 + 黄色项（黄色进隔离区可还原）</span>
            <span class="plan-nums">{{ planSummary(overview.planB) }}</span>
          </button>
        </div>
      </section>

      <footer class="foot">
        <p class="foot-note">
          你将释放 <strong>{{ formatBytes(selectedBytes) }}</strong>
          （🟢 立即释放 · 🟡/🔴 进隔离区）
        </p>
        <button class="btn-primary" :disabled="selectedBytes <= 0">确认清理 →</button>
      </footer>
    </template>
  </main>
</template>

<style scoped>
.report {
  display: flex;
  flex-direction: column;
  gap: 1.25rem;
}
.back {
  color: var(--accent);
  font-weight: 600;
}
.state {
  padding: 2rem;
  color: var(--text-2);
}
.state.err {
  color: var(--grade-red);
}

.overview {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
  gap: 1rem;
}
.metric {
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 1rem 1.25rem;
  display: flex;
  flex-direction: column;
  gap: 0.35rem;
}
.metric-label {
  font-size: 0.85rem;
  color: var(--text-2);
}
.metric-val {
  font-size: 1.4rem;
  font-weight: 700;
}
.metric-val.g {
  color: var(--grade-green);
}
.metric-val.y {
  color: var(--grade-yellow);
}
.metric-val.risk {
  font-size: 1.05rem;
  display: flex;
  gap: 0.6rem;
}
.g-r {
  color: var(--grade-green);
}
.y-r {
  color: var(--grade-yellow);
}
.r-r {
  color: var(--grade-red);
}
.metric-note {
  font-size: 0.78rem;
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
  font-size: 1.05rem;
}

.cat-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}
.cat-row {
  display: flex;
  gap: 0.6rem;
  align-items: flex-start;
  padding: 0.55rem 0.6rem;
  border: 1px solid var(--border);
  border-radius: 8px;
}
.check {
  width: 1.3rem;
  text-align: center;
  color: var(--accent);
}
.check.lock {
  color: var(--grade-red);
  opacity: 0.6;
}
.cat-main {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
}
.cat-line {
  display: flex;
  align-items: center;
  gap: 0.6rem;
  flex-wrap: wrap;
}
.cat-label {
  font-weight: 600;
}
.cat-size {
  font-size: 0.85rem;
  color: var(--text-2);
}
.cat-disp {
  font-size: 0.85rem;
  color: var(--accent);
}
.cat-reason {
  font-size: 0.8rem;
  color: var(--text-2);
  line-height: 1.5;
}
.cat-reason.reddish {
  color: var(--grade-red);
}
.cat-row.red {
  opacity: 0.55;
}
.red-toggle {
  margin-top: 0.9rem;
  background: none;
  border: 1px solid var(--border);
  color: var(--grade-red);
  padding: 0.4rem 0.9rem;
  border-radius: 6px;
  cursor: pointer;
  font-size: 0.85rem;
}
.red-note {
  margin: 0.75rem 0 0;
  font-size: 0.8rem;
  color: var(--text-2);
}
.em {
  font-weight: 600;
  color: var(--grade-red);
}

.plans {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
  gap: 0.75rem;
}
.plan {
  text-align: left;
  background: var(--bg);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 0.85rem 1rem;
  display: flex;
  flex-direction: column;
  gap: 0.3rem;
  cursor: pointer;
  transition: border-color 0.15s ease, background 0.15s ease;
}
.plan.active {
  border-color: var(--accent);
  background: rgba(59, 130, 246, 0.06);
}
.plan-name {
  font-weight: 600;
}
.tag {
  font-size: 0.7rem;
  color: var(--accent);
  border: 1px solid var(--accent);
  border-radius: 4px;
  padding: 0 0.3rem;
}
.plan-desc {
  font-size: 0.8rem;
  color: var(--text-2);
}
.plan-nums {
  font-size: 0.9rem;
  color: var(--text);
  font-weight: 600;
}

.foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 1rem;
  border-top: 1px solid var(--border);
  padding-top: 1rem;
}
.foot-note {
  margin: 0;
  color: var(--text-2);
}
.foot-note strong {
  color: var(--text);
}
.btn-primary {
  background: var(--accent);
  color: #fff;
  border: none;
  padding: 0.7rem 1.5rem;
  border-radius: 8px;
  font-weight: 600;
  cursor: pointer;
}
.btn-primary:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
</style>