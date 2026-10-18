<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import GradeBadge from "../components/GradeBadge.vue";
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
    <div v-else-if="items.length === 0 && !store.result" class="state">
      暂无扫描数据，请先「{{ cleanIntent ? "一键清理" : "一键体检" }}」。
      <router-link
        :to="cleanIntent ? { path: '/scan', query: { auto: '1', intent: 'clean' } } : '/scan'"
        class="go-scan"
      >去{{ cleanIntent ? "清理" : "体检" }} →</router-link>
    </div>
    <div v-else-if="items.length === 0 && !cleanIntent" class="state">本次体检未发现可清理项</div>
    <template v-else>
      <!-- F-2：白名单降级明示（清理前必须可见） -->
      <div v-if="whitelistDegraded" class="whitelist-degraded">
        ⚠ 安全白名单配置加载失败，本次扫描的过滤保护已降级——建议重启应用或重新安装后再执行清理
      </div>

      <!-- ===== R31-6 一键清理：简化结果卡（clean 模式默认视图） ===== -->
      <template v-if="cleanIntent && !detailExpanded">
        <section v-if="overview.planA.items > 0" class="simple-card anim-rise d1">
          <p class="simple-eyebrow">一键清理 · 只清 🟢 安全项</p>
          <p class="simple-release">
            <span class="simple-label">可释放</span>
            <span class="simple-num">{{ formatBytes(animatedImmediate) }}</span>
          </p>
          <p class="simple-sub">
            共 <strong>{{ overview.planA.items }}</strong> 项安全项（临时文件 / 可重建缓存），
            清理后会自动重建，不影响你的文档与照片。
          </p>
          <p v-if="overview.confirmItems > 0" class="simple-confirm">
            另有 🟡/🔴 {{ overview.confirmItems }} 项需要你确认，不会进入一键清理。
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
              另有 🟡/🔴 {{ overview.confirmItems }} 项需要你确认，可查看完整报告后逐项处理。
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

        <!-- 总览 -->
        <section class="overview">
        <div class="metric anim-rise d1">
          <span class="metric-label">可立即释放</span>
          <span class="metric-val g">{{ formatBytes(animatedImmediate) }}</span>
          <span class="metric-note">🟢 直清（立即释放）</span>
        </div>
        <div class="metric anim-rise d2">
          <span class="metric-label">进隔离区（可还原）</span>
          <span class="metric-val y">{{ formatBytes(animatedQuarantine) }}</span>
          <span class="metric-note">🟡/🔴 14 天可还原</span>
        </div>
        <div class="metric anim-rise d3">
          <span class="metric-label">风险分布</span>
          <span class="metric-val risk">
            <span class="g-r">🟢×{{ overview.riskItems.green }}</span>
            <span class="y-r">🟡×{{ overview.riskItems.yellow }}</span>
            <span class="r-r">🔴×{{ overview.riskItems.red }}</span>
          </span>
          <span class="metric-note">清理项 / 件数</span>
        </div>
      </section>

      <!-- 逐项类目（可解释 + 去向预告） -->
      <section class="card anim-rise d2">
        <h2 class="card-title">清理项（可了解每项为什么删）</h2>
        <ul class="cat-list">
          <li
            v-for="(cat, idx) in overview.categories"
            :key="cat.categoryId"
            class="cat-row"
            :class="[
              { red: cat.grade === 'red' && !redExpanded },
              idx < 8 ? 'row-in' : '',
            ]"
            :style="idx < 8 ? { animationDelay: 120 + idx * 35 + 'ms' } : {}"
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
          🔴 危险项一律进<span class="em">隔离区（14 天可还原）</span>，默认灰禁；需在<span class="em">设置中开启专家模式</span>后才会纳入本次清理，并须<span class="em">二次确认 token</span>。
        </p>
      </section>

      <!-- 专家模式下的危险项纳入提示 -->
      <p v-if="hasRed" class="expert-note">
        已开启专家模式 → 本次将连带清理 <strong>{{ overview.redCategoryCount }}</strong> 个 🔴 危险项（进隔离区），确认时需输入一次性令牌。
      </p>

      <!-- 清理方案（默认 A；B 需用户主动选择） -->
      <section class="card anim-rise d3">
        <h2 class="card-title">清理方案</h2>
        <div class="plans">
          <button
            class="plan"
            :class="{ active: selectedPlan === 'A' }"
            @click="selectedPlan = 'A'"
          >
            <span class="plan-name">方案 A · {{ overview.planA.name }}</span>
            <span class="plan-desc">只清 🟢 安全项，立即释放空间</span>
            <span class="plan-nums">{{ planSummary(overview.planA) }}</span>
          </button>
          <button
            class="plan"
            :class="{ active: selectedPlan === 'B' }"
            @click="selectedPlan = 'B'"
          >
            <span class="plan-name">方案 B · {{ overview.planB.name }}</span>
            <span class="plan-desc">在安全项基础上加入 🟡 项（进隔离区，14 天可还原）</span>
            <span class="plan-nums">{{ planSummary(overview.planB) }}</span>
          </button>
        </div>
      </section>

      <footer class="foot">
        <p class="foot-note">
          你将释放 <strong>{{ formatBytes(animatedSelected) }}</strong>
          （🟢 立即释放 · 🟡/🔴 进隔离区）
          <span v-if="hasRed && !expertMode" class="red-warn">· 含 🔴 危险项，需先开启专家模式</span>
        </p>
        <button
          class="btn-primary"
          :disabled="selectedBytes <= 0 || (hasRed && !expertMode)"
          @click="beginClean"
        >
          确认清理 →
        </button>
      </footer>
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
}
.back {
  color: var(--accent);
  font-weight: 600;
}
.rep-title {
  margin: 0.5rem 0 0;
  font-size: 1.3rem;
}
.state {
  padding: 2rem;
  color: var(--text-2);
}
/* F-2：白名单降级横幅（清理前必须可见） */
.whitelist-degraded {
  margin-bottom: 1rem;
  padding: 0.75rem 1rem;
  border: 1px solid var(--grade-red);
  border-radius: 10px;
  background: var(--surface);
  color: var(--grade-red);
  font-size: 0.85rem;
  font-weight: 600;
}
.state.err {
  color: var(--grade-red);
}
.state.err .retry {
  margin: 0.75rem auto 0;
  display: block;
  padding: 0.4rem 1.5rem;
  border: 1px solid var(--grade-red);
  border-radius: 8px;
  background: var(--surface);
  color: var(--grade-red);
  font-size: 0.85rem;
  cursor: pointer;
}
.go-scan {
  margin-left: 0.75rem;
  color: var(--accent);
  font-weight: 600;
}

/* ===== R31-6 一键清理简化结果（clean 模式默认视图） ===== */
.simple-card {
  display: flex;
  flex-direction: column;
  gap: 0.55rem;
  background: linear-gradient(120deg, #ffffff 0%, #f4f9f5 100%);
  border: 1px solid var(--border);
  border-radius: var(--r-card, 14px);
  box-shadow: var(--shadow-card, 0 4px 14px rgba(16, 24, 40, 0.05));
  padding: 1.6rem 1.9rem;
}
.simple-eyebrow {
  margin: 0;
  font-size: 0.8rem;
  font-weight: 600;
  color: var(--text-3);
  letter-spacing: 0.02em;
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
  font-size: 2.7rem;
  line-height: 1.1;
  font-weight: 800;
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
  /* R29：灰禁展开/收起只淡 opacity，不改变布局；锁定语义仍由文案+图标承担 */
  transition:
    opacity var(--dur-base) var(--ease-inout),
    border-color var(--dur-fast) var(--ease-inout),
    background var(--dur-fast) var(--ease-inout);
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
  --row-final: 0.55;
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
.expert-note {
  margin: 0;
  padding: 0.6rem 0.9rem;
  font-size: 0.82rem;
  color: var(--grade-red);
  background: rgba(239, 68, 68, 0.08);
  border: 1px solid var(--grade-red);
  border-radius: 8px;
  line-height: 1.5;
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
  grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
  gap: 0.75rem;
}
.plan {
  position: relative;
  text-align: left;
  background: var(--bg);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 0.85rem 1rem;
  display: flex;
  flex-direction: column;
  gap: 0.3rem;
  cursor: pointer;
  transition:
    border-color var(--dur-fast) var(--ease-inout),
    background var(--dur-fast) var(--ease-inout),
    transform var(--dur-fast) var(--ease-inout);
}
.plan:active {
  transform: scale(0.995);
}
.plan.active {
  border-color: var(--accent);
  background: rgba(37, 99, 235, 0.07);
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
  font-variant-numeric: tabular-nums;
}
.btn-primary {
  background: var(--accent);
  color: #fff;
  border: none;
  padding: 0.7rem 1.5rem;
  border-radius: 8px;
  font-weight: 600;
  cursor: pointer;
  transition:
    transform var(--dur-fast) var(--ease-inout),
    opacity var(--dur-fast) var(--ease-inout),
    background var(--dur-fast) var(--ease-inout);
}
.btn-primary:hover {
  background: var(--accent-hover, #1d4ed8);
}
.btn-primary:active {
  transform: scale(0.98);
}
.btn-primary:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
/* R31-6：简化结果卡主按钮加大（双类选择器确保覆盖上方 .btn-primary 的尺寸） */
.btn-primary.btn-big {
  padding: 0.9rem 2.1rem;
  font-size: 1.02rem;
  border-radius: var(--r-btn, 10px);
}
</style>