<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import GradeBadge from "../components/GradeBadge.vue";
import GradeIcon from "../components/GradeIcon.vue";
import ConfirmTokenModal from "../components/ConfirmTokenModal.vue";
import { commands } from "../api";
import { useScanStore } from "../stores/scan";
import { useCleanStore } from "../stores/clean";
import {
  buildReportOverview,
  safeCleanItems,
  type ReportCategory,
  type ReportPlan,
} from "./reportModel";
import {
  buildSuggestions,
  setSuggestionsEnabled,
  showSuggestions,
  summarizeSuggestions,
} from "./suggestModel";
import { useCountUp } from "../composables/useCountUp";

// P1-08 真数据联调：报告直接读全局扫描会话（mock 开关在 store 层切换）。
const store = useScanStore();
const cleanStore = useCleanStore();
const router = useRouter();
const route = useRoute();
const items = computed(() => store.items);

// R31-6：一键清理模式（/report?intent=clean）——默认只给"简化结果卡"；
// 展开完整报告后才显示总览/类目/方案 A/B。非 clean 模式仍直接是完整报告。
const cleanIntent = computed(() => route.query.intent === "clean");
const detailExpanded = ref(false);
/** F-2（P4-02）：白名单加载降级时清理前明示。 */
const whitelistDegraded = computed(() => store.result?.whitelistOk === false);
const loading = computed(() => store.running);

// 专家模式 & 二次确认：仅开启专家模式才允许纳入 🔴 危险项，且须弹 token 确认。
const expertMode = ref(false);
const modalOpen = ref(false);

onMounted(async () => {
  try {
    const s = await commands.settings_get();
    expertMode.value = s.expertMode;
  } catch {
    // 读取失败保持默认（专家模式关闭）。
  }
});

// 方案选择：默认 A（只清安全项）；B 仅在展开完整报告后可自定义。
const selectedPlan = ref<"A" | "B">("A");

// 大数字可能很小（如 mock 16MB），统一按 B/KB/MB/GB/TB 量级展示，避免 "0.0 GB"。
function formatBytes(bytes: number): string {
  if (bytes <= 0) return "0 B";
  const gb = bytes / (1024 * 1024 * 1024);
  if (gb >= 1) return gb >= 1024 ? `${(gb / 1024).toFixed(1)} TB` : `${gb.toFixed(2)} GB`;
  const mb = bytes / (1024 * 1024);
  if (mb >= 1) return `${mb.toFixed(1)} MB`;
  return `${Math.round(bytes / 1024)} KB`;
}

const overview = computed(() => buildReportOverview(items.value));

// ---- 清理建议 V0（纯前端、只影响文案与排序，不参与勾选/方案/灰禁） ----
const suggestions = computed(() =>
  showSuggestions.value
    ? buildSuggestions({
        categories: overview.value.categories,
        items: store.items,
        whitelistOk: store.result?.whitelistOk !== false,
      })
    : [],
);
const suggestionMap = computed(
  () => new Map(suggestions.value.map((s) => [s.categoryId, s])),
);
const suggestionSummary = computed(() =>
  summarizeSuggestions(suggestions.value, overview.value.categories),
);

/** 例外置顶：仅把「建议保留」的类目提到最前，其余顺序不变（勾选语义不受影响）。 */
const displayCategories = computed(() => {
  const cats = overview.value.categories;
  if (!showSuggestions.value) return cats;
  const keep = cats.filter((c) => suggestionMap.value.get(c.categoryId)?.state === "keep");
  if (keep.length === 0) return cats;
  return [...keep, ...cats.filter((c) => suggestionMap.value.get(c.categoryId)?.state !== "keep")];
});

function suggestionOf(cat: ReportCategory) {
  return suggestionMap.value.get(cat.categoryId);
}

function hideSuggestions(): void {
  setSuggestionsEnabled(false);
}

/** 某方案纳入的类目 id 集合（类目已按 R30 排除审阅型大文件）。 */
function categoryIdsForPlan(plan: ReportPlan): Set<string> {
  return new Set(
    overview.value.categories.filter((c) => plan.grades.includes(c.grade)).map((c) => c.categoryId),
  );
}

// 当前方案下被纳入的类目 id 集合。
const includedCategoryIds = computed(() =>
  categoryIdsForPlan(selectedPlan.value === "A" ? overview.value.planA : overview.value.planB),
);

/** R31-6 一键清理口径：只清 🟢 安全项（临时文件/可重建缓存），与专家模式无关；
 *  🟡/🔴 永远不进一键流程（走完整报告的人工确认）。 */
const safeItems = computed(() => safeCleanItems(store.items));

const planSummary = (p: ReportPlan) => `${p.items} 项 · ${formatBytes(p.bytes)}`;

// 本次实际待清理项：plan 内 + （专家模式下额外纳入 🔴 危险项）。
const cleanItems = computed(() => {
  const base = store.items.filter((i) => includedCategoryIds.value.has(i.categoryId));
  if (!expertMode.value) return base;
  return [...base, ...store.items.filter((i) => i.grade === "red" && !includedCategoryIds.value.has(i.categoryId))];
});

const selectedBytes = computed(() => cleanItems.value.reduce((s, i) => s + i.sizeBytes, 0));
const hasRed = computed(() => cleanItems.value.some((i) => i.grade === "red"));

// R29：落定总量/已选量数字补间（汇总值，非高频实时数据）。
const animatedImmediate = useCountUp(() => overview.value.immediateBytes, { durationMs: 650 });
const animatedQuarantine = useCountUp(() => overview.value.quarantineBytes, { durationMs: 650 });
const animatedSelected = useCountUp(selectedBytes, { durationMs: 220 });

const redExpanded = ref(false);

function isIncluded(cat: ReportCategory): boolean {
  return includedCategoryIds.value.has(cat.categoryId);
}

// 「确认清理 →」：需二次确认（含 🔴 + 专家模式）或直接开始。
function beginClean() {
  if (selectedBytes.value <= 0) return;
  if (hasRed.value) {
    // 开启专家模式才允许；未开启则按钮已禁用（此处兜底）。
    if (!expertMode.value) return;
    modalOpen.value = true;
  } else {
    launchClean();
  }
}

function launchClean(token?: string) {
  void cleanStore.start(
    cleanItems.value.map((i) => i.id),
    token,
  );
  void router.push("/executing");
}

function onTokenConfirm(token: string) {
  modalOpen.value = false;
  launchClean(token);
}

// R31-6：「立即清理 N 项」（两击流程的第二击）——只提交 🟢 安全项（方案 A 口径），
// 不弹 token、不纳入专家模式危险项；🟡/🔴 永远不走这条路径。
function oneClickClean(): void {
  if (safeItems.value.length === 0) return;
  void cleanStore.start(safeItems.value.map((i) => i.id));
  void router.push("/executing");
}
</script>

<template>
  <main class="report">
    <header class="rep-head">
      <router-link to="/" class="back">{{ cleanIntent ? "← 首页" : "← 体检报告" }}</router-link>
      <h1 class="rep-title">{{ cleanIntent ? "清理结果" : "扫描报告" }}</h1>
    </header>

    <div v-if="loading" class="state">加载中…</div>
    <!-- v0.1.5 反馈 3：明细拉取失败 → 错误分支 + 重试（不允许静默空报告） -->
    <div v-else-if="store.itemsError" class="state err">
      报告数据加载失败：{{ store.itemsError }}
      <button class="retry" @click="store.retryLoadItems()">重试</button>
    </div>
    <div v-else-if="items.length === 0 && !store.result" class="state empty-panel">
      <span class="empty-icon" aria-hidden="true">
        <svg width="26" height="26" viewBox="0 0 24 24" fill="none">
          <circle cx="10.5" cy="10.5" r="6.5" stroke="currentColor" stroke-width="1.7" />
          <path d="m15.5 15.5 4 4" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" />
        </svg>
      </span>
      <p class="empty-text">暂无扫描数据，请先「{{ cleanIntent ? "一键清理" : "一键体检" }}」。</p>
      <router-link
        :to="cleanIntent ? { path: '/scan', query: { auto: '1', intent: 'clean' } } : '/scan'"
        class="go-scan"
      >去{{ cleanIntent ? "清理" : "体检" }} →</router-link>
    </div>
    <div v-else-if="items.length === 0 && !cleanIntent" class="state empty-panel">
      <span class="empty-icon ok" aria-hidden="true">
        <svg width="26" height="26" viewBox="0 0 24 24" fill="none">
          <path d="m6 12.5 4 4 8-9" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" />
        </svg>
      </span>
      <p class="empty-text">本次体检未发现可清理项</p>
    </div>
    <template v-else>
      <!-- F-2：白名单降级明示（清理前必须可见） -->
      <div v-if="whitelistDegraded" class="whitelist-degraded">
        <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <path d="M12 3.6 21 19a1.4 1.4 0 0 1-1.2 2H4.2A1.4 1.4 0 0 1 3 19Z" />
          <path d="M12 9.6v4.1" />
          <circle cx="12" cy="17.2" r="0.9" fill="currentColor" stroke="none" />
        </svg>
        安全白名单配置加载失败，本次扫描的过滤保护已降级——建议重启应用或重新安装后再执行清理
      </div>

      <!-- ===== R31-6 一键清理：简化结果卡（clean 模式默认视图） ===== -->
      <template v-if="cleanIntent && !detailExpanded">
        <section v-if="overview.planA.items > 0" class="simple-card anim-rise d1">
          <p class="simple-eyebrow">
            <GradeIcon grade="green" :size="13" />
            一键清理 · 只清安全项
          </p>
          <p class="simple-release">
            <span class="simple-label">可释放</span>
            <span class="simple-num">{{ formatBytes(animatedImmediate) }}</span>
          </p>
          <p class="simple-sub">
            共 <strong>{{ overview.planA.items }}</strong> 项安全项（系统临时文件 / 可重建缓存），
            删除后由系统或应用自动重建；不会动你的文档与照片。
          </p>
          <p v-if="showSuggestions && suggestionSummary.clean.items > 0" class="simple-suggest">
            <GradeIcon grade="green" :size="13" />
            建议清理 <strong>{{ suggestionSummary.clean.items }}</strong> 项（可重建，删除后自动生成）
            <span v-if="suggestionSummary.keep.items > 0" class="simple-suggest-note">
              · {{ suggestionSummary.keep.items }} 项建议保留（建议仅作解释，不改变一键清理范围）
            </span>
          </p>
          <p v-if="overview.confirmItems > 0" class="simple-confirm">
            另有谨慎/危险 {{ overview.confirmItems }} 项需要你确认，不会进入一键清理。
          </p>
          <div class="simple-actions">
            <button class="btn-primary btn-big" :disabled="safeItems.length === 0" @click="oneClickClean">
              立即清理 {{ overview.planA.items }} 项
            </button>
            <button class="btn-link" @click="detailExpanded = true">查看明细并自定义</button>
          </div>
        </section>

        <!-- 无安全项：不给一键清理按钮，只引导看完整报告 -->
        <section v-else class="simple-card empty-safe anim-rise d1">
          <h2 class="empty-title">没有发现可安全清理的项目</h2>
          <p class="empty-desc">
            <template v-if="overview.confirmItems > 0">
              另有谨慎/危险 {{ overview.confirmItems }} 项需要你确认，可查看完整报告后逐项处理。
            </template>
            <template v-else>本次扫描没有发现可清理的项目，电脑已比较干净。</template>
          </p>
          <div class="simple-actions">
            <button class="btn-link strong" @click="detailExpanded = true">查看完整报告</button>
          </div>
        </section>
      </template>

      <!-- ===== 完整报告（非 clean 模式默认；clean 模式展开后） ===== -->
      <template v-else>
        <button v-if="cleanIntent" class="collapse-link" @click="detailExpanded = false">← 返回一键清理</button>

        <!-- 清理建议摘要（V0：只解释，不改变勾选/方案/灰禁） -->
        <section v-if="showSuggestions && suggestions.length > 0" class="suggest-bar anim-rise d1">
          <span class="suggest-bar-title">清理建议</span>
          <span class="suggest-bar-text">{{ suggestionSummary.headline }}</span>
          <button class="suggest-hide" @click="hideSuggestions">隐藏建议</button>
        </section>

        <div class="report-layout">
          <div class="report-main">
            <!-- 总览 -->
            <section class="overview">
              <div class="metric anim-rise d1">
                <span class="metric-label"><GradeIcon grade="green" :size="13" /> 可立即释放</span>
                <span class="metric-val g">{{ formatBytes(animatedImmediate) }}</span>
                <span class="metric-note">安全级直清，确认后立即释放</span>
              </div>
              <div class="metric anim-rise d2">
                <span class="metric-label"><GradeIcon grade="yellow" :size="13" /> 进隔离区（可还原）</span>
                <span class="metric-val y">{{ formatBytes(animatedQuarantine) }}</span>
                <span class="metric-note">谨慎/危险级，保留期内可还原</span>
              </div>
              <div class="metric anim-rise d3">
                <span class="metric-label"><GradeIcon grade="red" :size="13" /> 风险分布</span>
                <span class="metric-val risk">
                  <span class="g-r"><GradeIcon grade="green" :size="12" />×{{ overview.riskItems.green }}</span>
                  <span class="y-r"><GradeIcon grade="yellow" :size="12" />×{{ overview.riskItems.yellow }}</span>
                  <span class="r-r"><GradeIcon grade="red" :size="12" />×{{ overview.riskItems.red }}</span>
                </span>
                <span class="metric-note">清理项 / 件数</span>
              </div>
            </section>

      <!-- 逐项类目（可解释 + 去向预告） -->
      <section class="card anim-rise d2">
        <div class="cat-head">
          <h2 class="card-title">清理项</h2>
          <span class="cat-sub">每一类都写明去向；危险项默认灰禁，仅可展开查看</span>
        </div>
        <ul class="cat-list">
          <li
            v-for="(cat, idx) in displayCategories"
            :key="cat.categoryId"
            class="cat-row"
            :class="[
              { red: cat.grade === 'red' && !redExpanded },
              idx < 8 ? 'row-in' : '',
            ]"
            :style="idx < 8 ? { animationDelay: 120 + idx * 35 + 'ms' } : {}"
          >
            <span v-if="cat.grade !== 'red'" class="check" :class="{ on: isIncluded(cat) }" aria-hidden="true"></span>
            <span v-else class="check lock" aria-hidden="true">
              <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.1" stroke-linecap="round" stroke-linejoin="round">
                <rect x="4.8" y="10.2" width="14.4" height="10.4" rx="2.2" />
                <path d="M8.4 10.2V7.8a3.6 3.6 0 0 1 7.2 0v2.4" />
              </svg>
            </span>

            <span class="cat-main">
              <span class="cat-line">
                <GradeBadge :grade="cat.grade" />
                <span class="cat-label">{{ cat.label }}</span>
                <span class="cat-size">{{ formatBytes(cat.totalBytes) }} · {{ cat.itemCount }}项</span>
                <span class="cat-disp">→ {{ cat.dispositionLabel }}</span>
                <span
                  v-if="showSuggestions && suggestionOf(cat)"
                  class="cat-suggest"
                  :class="'sugg-' + suggestionOf(cat)?.state"
                >{{ suggestionOf(cat)?.label }}</span>
              </span>
              <span v-if="showSuggestions && suggestionOf(cat) && suggestionOf(cat)?.state !== 'unsure'" class="cat-suggest-basis">
                建议依据：{{ suggestionOf(cat)?.basis }}
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
          <GradeIcon grade="red" :size="13" />
          危险项一律进<span class="em">隔离区（保留期内可还原）</span>，默认灰禁；需在<span class="em">设置中开启专家模式</span>后才会纳入本次清理，并须<span class="em">二次确认 token</span>。
        </p>
      </section>

      <!-- 专家模式下的危险项纳入提示 -->
      <p v-if="hasRed" class="expert-note">
        已开启专家模式 → 本次将连带清理 <strong>{{ overview.redCategoryCount }}</strong> 个危险项（进隔离区），确认时需输入一次性令牌。
      </p>
          </div>

          <aside class="rail">
            <section class="rail-card anim-rise d3">
              <h2 class="rail-title">清理方案</h2>
              <div class="plans">
                <button
                  class="plan"
                  :class="{ active: selectedPlan === 'A' }"
                  @click="selectedPlan = 'A'"
                >
                  <span class="plan-name">方案 A · {{ overview.planA.name }}</span>
                  <span class="plan-desc">只清安全项，立即释放空间</span>
                  <span class="plan-nums">{{ planSummary(overview.planA) }}</span>
                </button>
                <button
                  class="plan"
                  :class="{ active: selectedPlan === 'B' }"
                  @click="selectedPlan = 'B'"
                >
                  <span class="plan-name">方案 B · {{ overview.planB.name }}</span>
                  <span class="plan-desc">安全项 + 谨慎项（进隔离区，保留期内可还原）</span>
                  <span class="plan-nums">{{ planSummary(overview.planB) }}</span>
                </button>
              </div>

              <div class="rail-total">
                <span class="rail-label">本次将处理</span>
                <span class="rail-num">{{ formatBytes(animatedSelected) }}</span>
                <span class="rail-sub">安全项立即释放 · 谨慎/危险项进隔离区</span>
                <span v-if="hasRed && !expertMode" class="red-warn">含危险项，需先开启专家模式</span>
              </div>

              <button
                class="btn-primary rail-cta"
                :disabled="selectedBytes <= 0 || (hasRed && !expertMode)"
                @click="beginClean"
              >
                确认清理 →
              </button>
              <p class="rail-hint">确认后先写操作日志再执行；谨慎/危险项进入隔离区，保留期内可一键还原。</p>
            </section>
            <p class="rail-lock">
              <GradeIcon grade="red" :size="14" />
              危险项默认灰禁：需在设置中开启专家模式，并输入二次确认 token。
            </p>
          </aside>
        </div>
      </template>
    </template>

    <!-- 🔴 二次确认（含危险项时弹窗） -->
    <ConfirmTokenModal v-model="modalOpen" :has-red="hasRed" @confirm="onTokenConfirm" />
  </main>
</template>

<style scoped>
.report {
  display: flex;
  flex-direction: column;
  gap: 1.25rem;
  max-width: 1120px;
  margin: 0 auto;
}
.report-layout {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 322px;
  gap: 1.1rem;
  align-items: start;
}
.report-main {
  display: flex;
  flex-direction: column;
  gap: 1.25rem;
  min-width: 0;
}
.rail {
  position: sticky;
  top: 12px;
  display: flex;
  flex-direction: column;
  gap: 0.8rem;
}
.rail-card {
  background: var(--surface);
  border: 1px solid var(--glass-line);
  border-radius: var(--r-lg);
  box-shadow:
    var(--glass-sheen),
    var(--shadow-2);
  backdrop-filter: var(--glass-blur-strong);
  -webkit-backdrop-filter: var(--glass-blur-strong);
  padding: 1.2rem 1.2rem 1.15rem;
  display: flex;
  flex-direction: column;
  gap: 0.85rem;
}
.rail-title {
  margin: 0;
  font-size: 1rem;
  font-weight: 700;
  letter-spacing: -0.01em;
}
.rail-total {
  display: flex;
  flex-direction: column;
  gap: 0.15rem;
  padding: 0.8rem 0.9rem;
  border-radius: 12px;
  background: rgba(37, 99, 235, 0.05);
  border: 1px solid rgba(37, 99, 235, 0.12);
}
.rail-label {
  font-size: 0.76rem;
  color: var(--text-2);
}
.rail-num {
  font-size: 1.8rem;
  font-weight: 760;
  letter-spacing: -0.03em;
  font-variant-numeric: tabular-nums;
  color: var(--text);
}
.rail-sub {
  font-size: 0.76rem;
  line-height: 1.5;
  color: var(--text-2);
}
.rail-cta {
  width: 100%;
  padding: 0.8rem 1rem;
  font-size: 0.95rem;
}
.rail-hint {
  margin: 0;
  font-size: 0.74rem;
  line-height: 1.55;
  color: var(--text-2);
}
.rail-lock {
  margin: 0;
  display: flex;
  align-items: flex-start;
  gap: 7px;
  padding: 0.65rem 0.8rem;
  border-radius: 12px;
  background: rgba(255, 244, 242, 0.72);
  border: 1px solid rgba(214, 58, 47, 0.18);
  font-size: 0.75rem;
  line-height: 1.55;
  color: var(--text-2);
}
.back {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  color: var(--text-2);
  font-weight: 600;
  font-size: 0.82rem;
  padding: 4px 10px 4px 8px;
  margin-left: -8px;
  border-radius: var(--r-pill);
  transition:
    color var(--dur-fast) var(--ease-inout),
    background var(--dur-fast) var(--ease-inout);
}
.back:hover {
  color: var(--accent);
  background: rgba(37, 99, 235, 0.08);
}
.rep-title {
  margin: 0.5rem 0 0;
  font-size: 1.6rem;
  font-weight: 720;
  letter-spacing: -0.02em;
}
.state {
  padding: 2rem;
  color: var(--text-2);
}
/* 空状态：与内容卡片同一材质，避免文字直接漂在环境光上 */
.empty-panel {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 0.7rem;
  padding: 3rem 2rem;
  text-align: center;
  background: var(--surface);
  border: 1px solid var(--glass-line);
  border-radius: var(--r-xl);
  backdrop-filter: var(--glass-blur);
  -webkit-backdrop-filter: var(--glass-blur);
  box-shadow:
    var(--glass-sheen),
    var(--shadow-1);
  animation: ps-rise var(--dur-slow) var(--ease-out) both;
}
.empty-icon {
  width: 52px;
  height: 52px;
  border-radius: 16px;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--accent);
  background: rgba(37, 99, 235, 0.09);
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.9),
    inset 0 0 0 1px rgba(37, 99, 235, 0.16),
    0 4px 12px -8px rgba(16, 28, 52, 0.4);
}
.empty-icon.ok {
  color: var(--grade-green);
  background: rgba(18, 161, 80, 0.1);
  box-shadow:
    inset 0 0 0 1px rgba(18, 161, 80, 0.22),
    0 4px 12px -8px rgba(18, 161, 80, 0.4);
}
.empty-text {
  margin: 0;
  font-size: 0.92rem;
  color: var(--text-2);
}
/* F-2：白名单降级横幅（清理前必须可见） */
.whitelist-degraded {
  margin-bottom: 1rem;
  display: flex;
  align-items: flex-start;
  gap: 7px;
  padding: 0.8rem 1.1rem;
  border: 1px solid rgba(214, 58, 47, 0.4);
  border-radius: var(--r-md);
  background: rgba(255, 244, 242, 0.9);
  color: var(--grade-red);
  font-size: 0.85rem;
  font-weight: 600;
  box-shadow: var(--shadow-1);
}
.whitelist-degraded svg {
  flex: none;
  margin-top: 1px;
}
.state.err {
  color: var(--grade-red);
}
.state.err .retry {
  margin: 0.75rem auto 0;
  display: block;
  padding: 0.4rem 1.5rem;
  border: 1px solid rgba(214, 58, 47, 0.5);
  border-radius: 9px;
  background: var(--surface-solid);
  color: var(--grade-red);
  font-size: 0.85rem;
  cursor: pointer;
}
.go-scan {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  margin-top: 0.15rem;
  padding: 7px 18px;
  border-radius: var(--r-pill);
  color: var(--accent);
  font-weight: 650;
  font-size: 0.86rem;
  background: rgba(255, 255, 255, 0.9);
  border: 1px solid rgba(37, 99, 235, 0.28);
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.8),
    0 3px 10px -6px rgba(37, 99, 235, 0.6);
  transition:
    background var(--dur-fast) var(--ease-inout),
    border-color var(--dur-fast) var(--ease-inout),
    transform var(--dur-fast) var(--ease-out);
}
.go-scan:hover {
  background: var(--accent-soft);
  border-color: rgba(37, 99, 235, 0.45);
}
.go-scan:active {
  transform: scale(0.98);
}

/* ===== R31-6 一键清理简化结果（clean 模式默认视图） ===== */
.simple-card {
  display: flex;
  flex-direction: column;
  gap: 0.55rem;
  background:
    radial-gradient(560px 240px at 96% -20%, rgba(18, 161, 80, 0.12), rgba(18, 161, 80, 0) 68%),
    linear-gradient(135deg, rgba(255, 255, 255, 0.94) 0%, rgba(243, 251, 246, 0.78) 100%);
  border: 1px solid var(--glass-line);
  border-radius: var(--r-xl);
  box-shadow:
    var(--glass-sheen),
    var(--shadow-2);
  backdrop-filter: var(--glass-blur-strong);
  -webkit-backdrop-filter: var(--glass-blur-strong);
  padding: 1.7rem 2rem;
}
.simple-eyebrow {
  margin: 0;
  display: inline-flex;
  align-items: center;
  gap: 5px;
  font-size: 0.76rem;
  font-weight: 700;
  color: var(--text-3);
  letter-spacing: 0.07em;
  text-transform: uppercase;
}
.simple-release {
  margin: 0;
  display: flex;
  align-items: baseline;
  gap: 0.7rem;
}
.simple-label {
  font-size: 0.95rem;
  font-weight: 600;
  color: var(--text-2);
}
.simple-num {
  font-size: 2.9rem;
  line-height: 1.08;
  font-weight: 780;
  letter-spacing: -0.035em;
  color: var(--grade-green);
  font-variant-numeric: tabular-nums;
}
.simple-sub {
  margin: 0;
  font-size: 0.92rem;
  line-height: 1.65;
  color: var(--text-2);
}
.simple-sub strong {
  color: var(--text);
  font-variant-numeric: tabular-nums;
}
.simple-confirm {
  margin: 0;
  font-size: 0.85rem;
  color: var(--grade-yellow);
}
/* 清理建议 V0：一键清理卡摘要（只解释，不改变按钮提交范围） */
.simple-suggest {
  margin: 0;
  display: inline-flex;
  align-items: center;
  gap: 5px;
  flex-wrap: wrap;
  font-size: 0.86rem;
  color: var(--text-2);
}
.simple-suggest strong {
  color: var(--grade-green);
  font-variant-numeric: tabular-nums;
}
.simple-suggest-note {
  color: var(--text-3);
}
/* 完整报告顶部摘要条 */
.suggest-bar {
  display: flex;
  align-items: center;
  gap: 0.7rem;
  flex-wrap: wrap;
  padding: 0.68rem 1rem;
  border: 1px solid rgba(37, 99, 235, 0.14);
  border-radius: var(--r-md);
  background: rgba(37, 99, 235, 0.05);
  font-size: 0.84rem;
}
.suggest-bar-title {
  font-weight: 700;
  color: var(--accent);
}
.suggest-bar-text {
  color: var(--text-2);
}
.suggest-hide {
  margin-left: auto;
  background: none;
  border: none;
  padding: 0;
  color: var(--text-2);
  font-size: 0.78rem;
  cursor: pointer;
}
.suggest-hide:hover {
  color: var(--accent);
  text-decoration: underline;
}
/* 类目行建议徽标（固定词表 + 分级色） */
.cat-suggest {
  font-size: 0.72rem;
  font-weight: 650;
  line-height: 1.5;
  padding: 1px 8px;
  border-radius: var(--r-pill);
  border: 1px solid transparent;
  white-space: nowrap;
}
.sugg-clean {
  color: var(--grade-green);
  background: rgba(18, 161, 80, 0.1);
  border-color: rgba(18, 161, 80, 0.22);
}
.sugg-caveat {
  color: var(--grade-yellow);
  background: rgba(217, 151, 28, 0.12);
  border-color: rgba(217, 151, 28, 0.25);
}
.sugg-keep {
  color: var(--text-2);
  background: rgba(19, 32, 54, 0.06);
  border-color: rgba(19, 32, 54, 0.14);
}
.sugg-unsure {
  color: var(--grade-red);
  background: rgba(214, 58, 47, 0.08);
  border-color: rgba(214, 58, 47, 0.2);
}
.cat-suggest-basis {
  font-size: 0.76rem;
  color: var(--text-3, var(--text-2));
}
.simple-actions {
  display: flex;
  align-items: center;
  gap: 1.1rem;
  flex-wrap: wrap;
  margin-top: 0.85rem;
}
.btn-link {
  background: none;
  border: none;
  padding: 0.35rem 0.2rem;
  color: var(--accent);
  font-size: 0.9rem;
  font-weight: 600;
  cursor: pointer;
}
.btn-link:hover {
  text-decoration: underline;
}
.btn-link.strong {
  border: 1px solid var(--accent);
  border-radius: 8px;
  padding: 0.6rem 1.4rem;
}
.btn-link.strong:hover {
  background: var(--accent-soft, #eaf1ff);
  text-decoration: none;
}
.empty-safe {
  background: var(--surface);
}
.empty-title {
  margin: 0;
  font-size: 1.15rem;
}
.empty-desc {
  margin: 0;
  font-size: 0.9rem;
  line-height: 1.65;
  color: var(--text-2);
}
.collapse-link {
  align-self: flex-start;
  background: none;
  border: none;
  padding: 0;
  color: var(--accent);
  font-size: 0.88rem;
  font-weight: 600;
  cursor: pointer;
}
.collapse-link:hover {
  text-decoration: underline;
}

.overview {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 0.9rem;
}
.metric {
  background: var(--surface);
  border: 1px solid var(--glass-line);
  border-radius: var(--r-lg);
  padding: 1.05rem 1.25rem;
  display: flex;
  flex-direction: column;
  gap: 0.35rem;
  box-shadow:
    var(--glass-sheen),
    var(--shadow-1);
  backdrop-filter: var(--glass-blur);
  -webkit-backdrop-filter: var(--glass-blur);
}
.metric-label {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  font-size: 0.82rem;
  color: var(--text-2);
}
.metric-val {
  font-size: 1.5rem;
  font-weight: 740;
  letter-spacing: -0.025em;
  font-variant-numeric: tabular-nums;
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
  gap: 0.7rem;
}
.metric-val.risk > span {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  font-variant-numeric: tabular-nums;
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
  border: 1px solid var(--glass-line);
  border-radius: var(--r-xl);
  box-shadow:
    var(--glass-sheen),
    var(--shadow-1);
  backdrop-filter: var(--glass-blur);
  -webkit-backdrop-filter: var(--glass-blur);
  padding: 1.4rem 1.6rem;
}
.card-title {
  margin: 0 0 0.85rem;
  font-size: 1.06rem;
  font-weight: 700;
  letter-spacing: -0.01em;
}

.cat-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 0.75rem;
  flex-wrap: wrap;
  margin-bottom: 0.85rem;
}
.cat-head .card-title {
  margin: 0;
}
.cat-sub {
  font-size: 0.76rem;
  color: var(--text-2);
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
  gap: 0.65rem;
  align-items: flex-start;
  padding: 0.6rem 0.75rem;
  border: 1px solid var(--border);
  border-radius: var(--r-md);
  background: rgba(255, 255, 255, 0.5);
  /* R29：灰禁展开/收起只淡 opacity，不改变布局；锁定语义仍由文案+图标承担 */
  transition:
    opacity var(--dur-base) var(--ease-inout),
    border-color var(--dur-fast) var(--ease-inout),
    background var(--dur-fast) var(--ease-inout),
    box-shadow var(--dur-fast) var(--ease-inout);
}
.cat-row:hover {
  background: rgba(255, 255, 255, 0.86);
  border-color: var(--border-strong);
  box-shadow: 0 4px 14px -10px rgba(16, 28, 52, 0.4);
}
/* R29：前 8 行首屏 stagger rise（之后的行不挂 delay，SPEC §7）。
   终态 opacity 走元素变量 --row-final，使 🔴 灰禁行（0.55）动画结束后不被 forwards 覆盖。*/
.row-in {
  opacity: 0;
  animation: row-rise var(--dur-base) var(--ease-out) forwards;
}
@keyframes row-rise {
  from {
    opacity: 0;
    transform: translateY(8px);
  }
  to {
    opacity: var(--row-final, 1);
    transform: translateY(0);
  }
}
.check {
  width: 18px;
  height: 18px;
  margin-top: 2px;
  flex: none;
  position: relative;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 5px;
  border: 1.6px solid rgba(19, 32, 54, 0.22);
  background: rgba(255, 255, 255, 0.85);
}
.check.on {
  border-color: #2563eb;
  background: linear-gradient(180deg, #3b7bf5, #2563eb);
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.35),
    0 2px 6px -3px rgba(37, 99, 235, 0.6);
}
.check.on::after {
  content: "";
  width: 9px;
  height: 5px;
  border-left: 2px solid #fff;
  border-bottom: 2px solid #fff;
  transform: rotate(-45deg) translateY(-1px);
}
.check.lock {
  width: 20px;
  height: 20px;
  border: none;
  background: transparent;
  color: var(--grade-red);
  opacity: 0.6;
}
.check.lock svg {
  display: block;
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
  --row-final: 0.55;
}
.red-toggle {
  margin-top: 0.95rem;
  background: rgba(255, 255, 255, 0.6);
  border: 1px solid rgba(214, 58, 47, 0.28);
  color: var(--grade-red);
  padding: 0.42rem 0.95rem;
  border-radius: var(--r-pill);
  cursor: pointer;
  font-size: 0.84rem;
  font-weight: 600;
  transition:
    background var(--dur-fast) var(--ease-inout),
    border-color var(--dur-fast) var(--ease-inout);
}
.red-toggle:hover {
  background: rgba(255, 244, 242, 0.95);
  border-color: rgba(214, 58, 47, 0.45);
}
.red-note {
  margin: 0.75rem 0 0;
  display: flex;
  align-items: flex-start;
  gap: 6px;
  font-size: 0.8rem;
  color: var(--text-2);
  line-height: 1.65;
}
.red-note svg {
  flex: none;
  margin-top: 2px;
}
.expert-note {
  margin: 0;
  padding: 0.65rem 1rem;
  font-size: 0.82rem;
  color: var(--grade-red);
  background: rgba(255, 244, 242, 0.85);
  border: 1px solid rgba(214, 58, 47, 0.3);
  border-radius: var(--r-md);
  line-height: 1.55;
}
.red-warn {
  color: var(--grade-red);
  font-weight: 600;
}
.em {
  font-weight: 600;
  color: var(--grade-red);
}

.plans {
  display: grid;
  grid-template-columns: minmax(0, 1fr);
  gap: 0.6rem;
}
.plan {
  position: relative;
  text-align: left;
  background: rgba(255, 255, 255, 0.5);
  border: 1px solid var(--border);
  border-radius: var(--r-md);
  padding: 0.9rem 1.05rem;
  display: flex;
  flex-direction: column;
  gap: 0.3rem;
  cursor: pointer;
  transition:
    border-color var(--dur-fast) var(--ease-inout),
    background var(--dur-fast) var(--ease-inout),
    box-shadow var(--dur-fast) var(--ease-out),
    transform var(--dur-fast) var(--ease-out);
}
.plan:hover {
  background: rgba(255, 255, 255, 0.82);
  border-color: var(--border-strong);
}
.plan:active {
  transform: scale(0.995);
}
.plan.active {
  border-color: rgba(37, 99, 235, 0.55);
  background: linear-gradient(140deg, rgba(37, 99, 235, 0.1), rgba(37, 99, 235, 0.04));
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.7),
    0 6px 16px -10px rgba(37, 99, 235, 0.55);
}
/* R29：选中勾——伪元素 pop 出现（scale+opacity），不引图标库 */
.plan.active::after {
  content: "";
  position: absolute;
  top: 10px;
  right: 12px;
  width: 14px;
  height: 8px;
  border-left: 2px solid var(--accent);
  border-bottom: 2px solid var(--accent);
  transform: rotate(-45deg) scale(1);
  animation: plan-check var(--dur-base) var(--ease-out);
}
@keyframes plan-check {
  from {
    opacity: 0;
    transform: rotate(-45deg) scale(0.4) translateY(-2px);
  }
  to {
    opacity: 1;
    transform: rotate(-45deg) scale(1) translateY(0);
  }
}
.plan-name {
  font-weight: 600;
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

@media (max-width: 980px) {
  .report-layout {
    grid-template-columns: minmax(0, 1fr);
  }
  .rail {
    position: static;
  }
}
@media (max-width: 720px) {
  .overview {
    grid-template-columns: minmax(0, 1fr);
  }
}
.btn-primary {
  background: linear-gradient(180deg, #3b7bf5 0%, #2563eb 55%, #1d51d8 100%);
  color: #fff;
  border: none;
  padding: 0.72rem 1.55rem;
  border-radius: 11px;
  font-weight: 650;
  cursor: pointer;
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.3),
    0 8px 20px -8px rgba(37, 99, 235, 0.55);
  transition:
    transform var(--dur-fast) var(--ease-out),
    filter var(--dur-fast) var(--ease-inout),
    opacity var(--dur-fast) var(--ease-inout);
}
.btn-primary:hover:not(:disabled) {
  filter: brightness(1.05);
}
.btn-primary:active {
  transform: scale(0.98);
}
.btn-primary:disabled {
  opacity: 0.45;
  box-shadow: none;
  cursor: not-allowed;
}
/* R31-6：简化结果卡主按钮加大（双类选择器确保覆盖上方 .btn-primary 的尺寸） */
.btn-primary.btn-big {
  padding: 0.95rem 2.2rem;
  font-size: 1.02rem;
  border-radius: 13px;
}
</style>