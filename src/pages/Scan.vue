<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import { app_meta, disk_usage } from "../api/commands";
import { useScanStore } from "../stores/scan";
import GradeIcon from "../components/GradeIcon.vue";

const store = useScanStore();
const router = useRouter();
const route = useRoute();

// v0.1.2：非提权提示——便携/双击运行通常无管理员权限，MFT 快速引擎不启用，
// 深度维度（大文件全卷遍历/重复文件哈希）在大容量盘上会显著变慢。
const elevated = ref<boolean | null>(null);
const isDeep = computed(() => store.deepScan);

// R31-6：一键清理入口（/scan?auto=1&intent=clean）——强制快速维度、进页自动开扫，
// 正常完成（未取消）后自动进入简化结果页 /report?intent=clean；取消则停留不跳。
const cleanIntent = computed(() => route.query.intent === "clean");
const pageTitle = computed(() => {
  if (cleanIntent.value) return "一键清理";
  return isDeep.value ? "深度体检" : "一键体检";
});
const reportTarget = computed(() =>
  cleanIntent.value ? { path: "/report", query: { intent: "clean" } } : { path: "/report" },
);

// v0.1.6：进入体检页默认"待命"，需用户点开始；仅首页主按钮带 ?auto=1 才自动扫
//（侧栏"一键体检"是导航语义，不应一进页就开始扫描）。
// R31-6：clean 模式同步置 false，避免等待 app_meta/disk_usage 期间闪出"准备就绪"待命卡。
const idle = ref(route.query.intent !== "clean");

/** 由待命卡片/首页 auto 入口调用：选定档位并开始。 */
async function begin(deep: boolean): Promise<void> {
  idle.value = false;
  store.setDeepScan(deep);
  await store.start();
}

// v0.1.5 反馈 3：完成态显式出口——自动跳转 3s 未离开/被取消时显示大按钮。
const doneExit = ref(false);
let doneTimer: number | undefined;

// R31-6：一键清理模式的完成态文案（不出现"体检"措辞，延续用户视角的"清理"）。
const doneTitle = computed(() => {
  if (store.cancelled) return cleanIntent.value ? "清理扫描已取消" : "体检已取消";
  return cleanIntent.value ? "安全项扫描完成" : "体检完成";
});
const doneSub = computed(() =>
  store.cancelled
    ? "已收集的部分结果可查看（不写入任何文件）"
    : `正在进入${cleanIntent.value ? "清理结果" : "报告"}…`,
);
const doneButton = computed(() => {
  if (store.cancelled) return "查看已收集结果";
  return cleanIntent.value ? "查看清理结果" : "体检完成 → 查看报告";
});

// v0.1.5 反馈 2：多卷进度提示——固定盘总数（disk_usage）+ currentPath 盘符推进序号。
const totalVolumes = ref(0);
const seenVolumes = ref<string[]>([]);
const volumeHint = computed(() => {
  if (totalVolumes.value <= 1 || seenVolumes.value.length === 0) return "";
  return `第 ${Math.min(seenVolumes.value.length, totalVolumes.value)} / ${totalVolumes.value} 卷`;
});

watch(
  () => store.currentPath,
  (p) => {
    const v = p.slice(0, 2);
    if (/^[A-Za-z]:$/.test(v) && !seenVolumes.value.includes(v)) {
      seenVolumes.value = [...seenVolumes.value, v];
    }
  },
);

onMounted(async () => {
  try {
    const meta = await app_meta();
    elevated.value = meta.isElevated ?? null;
  } catch {
    elevated.value = null;
  }
  try {
    const disks = await disk_usage();
    totalVolumes.value = disks.length;
  } catch {
    totalVolumes.value = 0;
  }
  // R31-6：一键清理模式——默认快速维度并自动开扫（大文件/dup 不参与一键清理）。
  if (cleanIntent.value) {
    store.setDeepScan(false);
    idle.value = false;
    void store.start();
    return;
  }
  // 仅首页"开始体检/深度体检"主按钮（?auto=1）自动开扫；侧栏进入停留待命态。
  if (route.query.auto === "1") {
    idle.value = false;
    void store.start();
  }
});

onUnmounted(() => {
  if (doneTimer !== undefined) window.clearTimeout(doneTimer);
});

const PHASE_CN: Record<string, string> = {
  walking: "遍历扫描中",
  hashing: "哈希比对中",
  aggregating: "结果聚合中",
};

function formatBytes(b: number): string {
  if (b <= 0) return "0 B";
  const gb = b / (1024 * 1024 * 1024);
  if (gb >= 1) return gb >= 1024 ? `${(gb / 1024).toFixed(1)} TB` : `${gb.toFixed(2)} GB`;
  const mb = b / (1024 * 1024);
  if (mb >= 1) return `${mb.toFixed(1)} MB`;
  return `${Math.round(b / 1024)} KB`;
}

function fmtElapsed(ms: number): string {
  const s = Math.max(0, Math.round(ms / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

// 完成 → 跳转报告页（v0.1.5 反馈 3：取消后不自动跳，给"查看已收集结果"入口；
// 正常完成 3s 未离开也显示显式出口按钮兜底）。
watch(
  () => store.isDone,
  (done) => {
    if (!done) return;
    if (!store.cancelled) void router.push(reportTarget.value);
    const delay = store.cancelled ? 0 : 3000;
    doneTimer = window.setTimeout(() => {
      doneExit.value = true;
    }, delay);
  },
);
</script>

<template>
  <main class="scan">
    <header class="scan-head">
      <router-link to="/" class="back">← 首页</router-link>
      <div class="head-line">
        <h1>{{ pageTitle }}</h1>
        <span class="ro-chip">
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M12 3.5 19 6v5.2c0 4.6-3.1 7.6-7 9.3-3.9-1.7-7-4.7-7-9.3V6Z" />
            <path d="m8.8 12 2.2 2.2 4.2-4.4" />
          </svg>
          只读扫描
        </span>
      </div>
      <p class="sub">
        <template v-if="cleanIntent">
          只清理系统临时目录（%TEMP%）与可重建缓存（缩略图/浏览器），不会动你的文档、照片和其它目录里的文件；有风险的项目会让你确认。
        </template>
        <template v-else>不删除任何文件 · 扫描完成后按安全/谨慎/危险分级，处置逐项确认</template>
      </p>
    </header>

    <div v-if="store.error" class="error">扫描失败：{{ store.error }}</div>

    <!-- 待命态：双档模式卡（先选档，再开始） -->
    <section v-else-if="idle && !store.isDone" class="idle-stage anim-rise d1">
      <div class="intro">
        <div class="intro-icon" aria-hidden="true">
          <svg width="34" height="34" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
            <path d="M12 3.5 19 6v5.2c0 4.6-3.1 7.6-7 9.3-3.9-1.7-7-4.7-7-9.3V6Z" />
            <path d="m8.8 12 2.2 2.2 4.2-4.4" />
          </svg>
        </div>
        <div class="intro-text">
          <h2>准备就绪，先选一档</h2>
          <p>
            体检全程<strong>只读</strong>：不删除、不移动、不写入用户目录。扫描结束后按
            安全/谨慎/危险分级列出结果，清理什么、是否隔离或删除，全部由你逐项确认。
          </p>
        </div>
      </div>

      <div class="mode-grid">
        <article class="mode-card is-primary">
          <div class="mode-top">
            <h3>快速体检</h3>
            <span class="mode-tag">推荐</span>
          </div>
          <p class="mode-desc">临时文件 · 缓存 · 隐私痕迹 · 启动项</p>
          <ul class="mode-list">
            <li>几秒到几分钟完成</li>
            <li>覆盖最常见、最安全的可清理项</li>
            <li>不需要管理员权限</li>
          </ul>
          <button class="mode-btn primary" @click="begin(false)">开始快速体检</button>
        </article>

        <article class="mode-card">
          <div class="mode-top">
            <h3>深度体检</h3>
          </div>
          <p class="mode-desc">在快速基础上增加大文件与重复文件定位</p>
          <ul class="mode-list">
            <li>全卷遍历，大容量盘耗时较长</li>
            <li>重复文件按哈希比对确认</li>
            <li>未提权时遍历速度明显变慢</li>
          </ul>
          <button class="mode-btn" @click="begin(true)">深度体检</button>
        </article>
      </div>

      <p v-if="elevated === false" class="idle-warn">
        当前未以管理员运行：深度体检的大文件/重复文件维度会明显变慢，快速体检不受影响。
      </p>
    </section>

    <p v-else-if="isDeep && elevated === false" class="slow-hint">
      当前未以管理员运行：大文件/重复文件维度使用慢速遍历，大容量盘耗时明显变长。
      可取消后右键"以管理员身份运行"本程序再次体检（快速体检不受影响）。
    </p>

    <!-- v0.1.5 反馈 3：取消后的部分结果入口 / 完成态兜底出口 -->
    <section v-else-if="store.isDone && doneExit" class="card done-card anim-rise d1">
      <p class="done-text">{{ doneTitle }}</p>
      <p class="done-sub">{{ doneSub }}</p>
      <div class="done-btns">
        <button class="btn-done" @click="router.push(reportTarget)">{{ doneButton }}</button>
        <button class="btn-done-ghost" @click="begin(store.deepScan)">再扫一次</button>
      </div>
    </section>

    <!-- 运行态：左侧扫描舞台 + 右侧进度/统计/路径 -->
    <section class="card scan-card anim-rise d1" v-else>
      <p v-if="cleanIntent" class="clean-hint">正在扫描可安全清理的项目…</p>
      <div class="scan-grid">
        <div class="scan-visual">
          <div class="radar" :class="{ paused: !store.running }">
            <div class="radar-core">
              <Transition name="phase-pop">
                <span :key="store.phase" class="phase-glyph">
                  <svg v-if="store.phase === 'hashing'" width="38" height="38" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                    <circle cx="11" cy="11" r="6.4" />
                    <path d="m16 16 4.2 4.2" />
                  </svg>
                  <svg v-else-if="store.phase === 'aggregating'" width="38" height="38" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                    <path d="M4 7.5 12 3l8 4.5-8 4.5Z" />
                    <path d="M4 12.2 12 16.7l8-4.5" />
                    <path d="M4 16.8 12 21.3l8-4.5" />
                  </svg>
                  <svg v-else width="38" height="38" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                    <path d="M3.5 7.2c0-1 .8-1.7 1.7-1.7h3.6l2 2.2h8c1 0 1.7.8 1.7 1.7v7.4c0 1-.8 1.7-1.7 1.7H5.2c-1 0-1.7-.8-1.7-1.7Z" />
                  </svg>
                </span>
              </Transition>
            </div>
          </div>
          <div class="phase-col">
            <span class="pct">{{ store.percent }}<small>%</small></span>
            <span class="phase">{{ PHASE_CN[store.phase] ?? store.phase }}</span>
            <span v-if="volumeHint" class="vol">{{ volumeHint }}</span>
          </div>
        </div>

        <div class="scan-body">
          <div class="bar">
            <div
              class="bar-fill"
              :style="{ transform: `scaleX(${store.percent / 100})` }"
              :class="{ paused: !store.running }"
            >
              <span class="bar-sheen" aria-hidden="true"></span>
            </div>
          </div>

          <!-- 实时统计 -->
          <div class="metrics">
            <div class="metric">
              <span class="m-label">实时累计</span>
              <span class="m-val">{{ formatBytes(store.foundBytesTotal) }}</span>
            </div>
            <div class="metric">
              <span class="m-label">已用时</span>
              <span class="m-val">{{ fmtElapsed(store.elapsedMs) }}</span>
            </div>
            <div class="metric wide">
              <span class="m-label">按分级</span>
              <span class="grade-row">
                <span class="m-sub g"><GradeIcon grade="green" :size="13" /> {{ formatBytes(store.found.green) }}</span>
                <span class="m-sub y"><GradeIcon grade="yellow" :size="13" /> {{ formatBytes(store.found.yellow) }}</span>
                <span class="m-sub r"><GradeIcon grade="red" :size="13" /> {{ formatBytes(store.found.red) }}</span>
              </span>
            </div>
          </div>

          <!-- 当前路径（R29：路径切换淡入淡出，快速连刷不堆积）-->
          <div class="path-block">
            <span class="m-label">当前扫描</span>
            <span class="path-window">
              <Transition name="path-fade" mode="out-in">
                <span :key="store.currentPath" class="path" :title="store.currentPath">{{
                  store.currentPath || "初始化…"
                }}</span>
              </Transition>
            </span>
          </div>

          <!-- 取消 -->
          <div class="actions">
            <button class="btn-cancel" :disabled="!store.running" @click="store.cancel()">
              {{ cleanIntent ? "取消扫描" : "取消体检" }}
            </button>
            <p class="tip">
              {{
                cleanIntent
                  ? "可随时取消，已扫描部分不写入任何文件。完成自动进入清理结果。"
                  : "可随时取消，已扫描部分不写入任何文件。完成自动进入报告。"
              }}
            </p>
          </div>
        </div>
      </div>
    </section>
  </main>
</template>

<style scoped>
.scan {
  display: flex;
  flex-direction: column;
  gap: 1.25rem;
  max-width: 1080px;
  margin: 0 auto;
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
.head-line {
  display: flex;
  align-items: center;
  gap: 10px;
  margin: 0.5rem 0 0.3rem;
}
.head-line h1 {
  margin: 0;
  font-size: 1.6rem;
  font-weight: 720;
  letter-spacing: -0.02em;
}
.ro-chip {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 3px 9px 3px 7px;
  border-radius: var(--r-pill);
  font-size: 0.72rem;
  font-weight: 650;
  color: #1d51d8;
  background: rgba(37, 99, 235, 0.09);
  border: 1px solid rgba(37, 99, 235, 0.16);
  white-space: nowrap;
}
.sub {
  margin: 0;
  color: var(--text-2);
  font-size: 0.9rem;
  line-height: 1.65;
}
.error {
  color: var(--grade-red);
  background: rgba(255, 244, 242, 0.85);
  border: 1px solid rgba(214, 58, 47, 0.24);
  border-radius: var(--r-md);
  padding: 1rem 1.25rem;
  box-shadow: var(--shadow-1);
}
.card {
  background: var(--surface);
  border: 1px solid var(--glass-line);
  border-radius: var(--r-xl);
  box-shadow:
    var(--glass-sheen),
    var(--shadow-2);
  backdrop-filter: var(--glass-blur-strong);
  -webkit-backdrop-filter: var(--glass-blur-strong);
  padding: 1.6rem;
  display: flex;
  flex-direction: column;
  gap: 1.25rem;
}
/* 待命态：介绍 + 双档模式卡 */
.idle-stage {
  background: var(--surface);
  border: 1px solid var(--glass-line);
  border-radius: var(--r-xl);
  box-shadow:
    var(--glass-sheen),
    var(--shadow-2);
  backdrop-filter: var(--glass-blur-strong);
  -webkit-backdrop-filter: var(--glass-blur-strong);
  padding: 1.55rem;
  display: flex;
  flex-direction: column;
  gap: 1.15rem;
}
.intro {
  display: flex;
  align-items: flex-start;
  gap: 14px;
}
.intro-icon {
  width: 48px;
  height: 48px;
  border-radius: 14px;
  background: linear-gradient(155deg, rgba(37, 99, 235, 0.16), rgba(37, 99, 235, 0.05));
  color: var(--accent);
  display: flex;
  align-items: center;
  justify-content: center;
  flex: none;
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.8),
    inset 0 0 0 1px rgba(37, 99, 235, 0.1);
}
.intro-text h2 {
  margin: 0 0 4px;
  font-size: 1.1rem;
  letter-spacing: -0.015em;
}
.intro-text p {
  margin: 0;
  font-size: 0.86rem;
  line-height: 1.7;
  color: var(--text-2);
  max-width: 46rem;
}
.intro-text strong {
  color: var(--text);
}
.mode-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 12px;
}
.mode-card {
  background: rgba(255, 255, 255, 0.62);
  border: 1px solid var(--glass-line);
  border-radius: var(--r-lg);
  padding: 1.15rem 1.2rem 1.2rem;
  display: flex;
  flex-direction: column;
  gap: 9px;
}
.mode-card.is-primary {
  background: linear-gradient(180deg, rgba(239, 245, 255, 0.95), rgba(255, 255, 255, 0.7));
  border-color: rgba(37, 99, 235, 0.22);
  box-shadow: 0 12px 26px -20px rgba(37, 99, 235, 0.6);
}
.mode-top {
  display: flex;
  align-items: center;
  gap: 8px;
}
.mode-top h3 {
  margin: 0;
  font-size: 1.02rem;
  letter-spacing: -0.01em;
}
.mode-tag {
  font-size: 0.66rem;
  font-weight: 700;
  letter-spacing: 0.04em;
  color: #fff;
  background: linear-gradient(180deg, #3b7bf5, #2563eb);
  padding: 2px 7px;
  border-radius: var(--r-pill);
}
.mode-desc {
  margin: 0;
  font-size: 0.82rem;
  color: var(--text-2);
  line-height: 1.6;
}
.mode-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 5px;
  font-size: 0.79rem;
  color: var(--text-2);
}
.mode-list li {
  position: relative;
  padding-left: 16px;
  line-height: 1.55;
}
.mode-list li::before {
  content: "";
  position: absolute;
  left: 2px;
  top: 0.52em;
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: rgba(37, 99, 235, 0.5);
}
.mode-btn {
  margin-top: auto;
  padding: 10px 16px;
  border-radius: 11px;
  border: 1px solid var(--border-strong, #d7deea);
  background: rgba(255, 255, 255, 0.85);
  color: var(--text);
  font-size: 0.88rem;
  font-weight: 650;
  cursor: pointer;
  transition:
    border-color var(--dur-fast) var(--ease-inout),
    color var(--dur-fast) var(--ease-inout),
    background var(--dur-fast) var(--ease-inout),
    transform var(--dur-fast) var(--ease-out);
}
.mode-btn:hover {
  border-color: rgba(37, 99, 235, 0.45);
  color: var(--accent);
  background: #fff;
}
.mode-btn:active {
  transform: translateY(1px) scale(0.99);
}
.mode-btn.primary {
  border: none;
  background: linear-gradient(180deg, #3b7bf5 0%, #2563eb 55%, #1d51d8 100%);
  color: #fff;
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.32),
    inset 0 -1px 0 rgba(0, 0, 0, 0.12),
    0 8px 18px -8px rgba(37, 99, 235, 0.55);
}
.mode-btn.primary:hover {
  color: #fff;
  filter: brightness(1.06);
}
.idle-warn {
  margin: 0;
  font-size: 0.78rem;
  color: var(--grade-yellow);
}
/* R31-6：一键清理模式的进行中提示（标题已说明只清安全项，这里只报进行态） */
.clean-hint {
  margin: 0;
  font-size: 0.9rem;
  font-weight: 600;
  color: var(--accent);
}
.done-btns {
  margin-top: 10px;
  display: flex;
  gap: 10px;
  align-items: center;
}
.btn-done-ghost {
  padding: 0.7rem 1.4rem;
  border: 1px solid var(--border-strong, #d7deea);
  border-radius: 11px;
  background: rgba(255, 255, 255, 0.75);
  color: var(--text-2);
  font-size: 0.9rem;
  font-weight: 600;
  cursor: pointer;
  transition:
    border-color var(--dur-fast) var(--ease-inout),
    color var(--dur-fast) var(--ease-inout);
}
.btn-done-ghost:hover {
  border-color: rgba(37, 99, 235, 0.45);
  color: var(--accent);
}
.btn-done {
  margin-top: 0;
}

.done-card {
  align-items: center;
  text-align: center;
}
.done-text {
  margin: 0;
  font-size: 1.28rem;
  font-weight: 720;
  letter-spacing: -0.015em;
  color: var(--text);
}
.done-sub {
  margin: 0.25rem 0 0;
  color: var(--text-2);
  font-size: 0.85rem;
}
.btn-done {
  margin-top: 0.75rem;
  padding: 0.72rem 2.2rem;
  border: none;
  border-radius: 12px;
  background: linear-gradient(180deg, #3b7bf5 0%, #2563eb 55%, #1d51d8 100%);
  color: #fff;
  font-size: 1rem;
  font-weight: 650;
  cursor: pointer;
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.32),
    0 10px 22px -8px rgba(37, 99, 235, 0.6);
  transition:
    transform var(--dur-fast) var(--ease-out),
    filter var(--dur-fast) var(--ease-inout);
}
.btn-done:hover {
  filter: brightness(1.05);
}
.btn-done:active {
  transform: translateY(1px) scale(0.99);
}
/* 运行态：左侧扫描舞台 + 右侧数据 */
.scan-grid {
  display: grid;
  grid-template-columns: 216px minmax(0, 1fr);
  gap: 1.6rem;
  align-items: center;
}
.scan-visual {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 0.85rem;
}
.radar {
  position: relative;
  width: 176px;
  height: 176px;
  flex: none;
  border-radius: 50%;
  background: rgba(255, 255, 255, 0.5);
  box-shadow:
    inset 0 0 0 1px rgba(19, 32, 54, 0.06),
    inset 0 1px 0 rgba(255, 255, 255, 0.8);
  display: flex;
  align-items: center;
  justify-content: center;
}
.radar::before {
  content: "";
  position: absolute;
  inset: 0;
  border-radius: 50%;
  background: conic-gradient(
    from 0deg,
    rgba(37, 99, 235, 0) 0deg,
    rgba(37, 99, 235, 0.1) 42deg,
    rgba(37, 99, 235, 0.85) 72deg,
    rgba(37, 99, 235, 0) 78deg
  );
  animation: radar-spin 1.5s linear infinite;
}
.radar.paused::before {
  animation-play-state: paused;
  opacity: 0.35;
}
.radar::after {
  content: "";
  position: absolute;
  inset: 11px;
  border-radius: 50%;
  border: 1px dashed rgba(37, 99, 235, 0.18);
}
.radar-core {
  position: relative;
  z-index: 1;
  width: 112px;
  height: 112px;
  border-radius: 50%;
  background: rgba(255, 255, 255, 0.97);
  border: 1px solid var(--glass-line);
  color: var(--accent);
  display: flex;
  align-items: center;
  justify-content: center;
  box-shadow:
    inset 0 1px 0 #fff,
    0 12px 28px -16px rgba(16, 28, 52, 0.55);
}
.phase-glyph {
  display: inline-flex;
}
/* R29：阶段图标切换 pop（≤base 时长，只动 transform/opacity）*/
.phase-pop-enter-active {
  transition:
    transform var(--dur-base) var(--ease-out),
    opacity var(--dur-base) var(--ease-out);
}
.phase-pop-enter-from {
  opacity: 0;
  transform: scale(0.6);
}
.phase-pop-leave-active {
  transition:
    transform var(--dur-fast) var(--ease-inout),
    opacity var(--dur-fast) var(--ease-inout);
  position: absolute;
}
.phase-pop-leave-to {
  opacity: 0;
  transform: scale(1.25);
}
@keyframes radar-spin {
  to {
    transform: rotate(360deg);
  }
}
.phase-col {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 0.15rem;
  font-weight: 600;
  text-align: center;
}
.pct {
  font-size: 2rem;
  font-weight: 720;
  letter-spacing: -0.03em;
  line-height: 1.1;
  font-variant-numeric: tabular-nums;
}
.pct small {
  font-size: 0.95rem;
  font-weight: 650;
  margin-left: 1px;
  color: var(--text-2);
}
.phase {
  font-size: 0.82rem;
  font-weight: 600;
  color: var(--text-2);
}
.vol {
  font-size: 0.74rem;
  color: var(--accent);
}
.bar {
  position: relative;
  height: 12px;
  border-radius: var(--r-pill);
  background: rgba(19, 32, 54, 0.07);
  box-shadow:
    inset 0 1px 2px rgba(16, 28, 52, 0.09),
    inset 0 0 0 1px rgba(19, 32, 54, 0.03);
  overflow: hidden;
}
.bar-fill {
  position: relative;
  height: 100%;
  width: 100%;
  border-radius: var(--r-pill);
  background: linear-gradient(90deg, #5b8cf5 0%, #2563eb 60%, #1d51d8 100%);
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.4);
  /* R29：进度用 scaleX（只动 transform，SPEC §7），origin 左对齐 */
  transform-origin: left center;
  transition: transform var(--dur-base) var(--ease-out);
  overflow: hidden;
}
.bar-fill.paused .bar-sheen {
  animation-play-state: paused;
  opacity: 0.35;
}
/* R29：进度流光（纯 transform 位移的伪元素，不触发布局）*/
.bar-sheen {
  position: absolute;
  top: 0;
  bottom: 0;
  width: 40%;
  background: linear-gradient(
    90deg,
    rgba(255, 255, 255, 0) 0%,
    rgba(255, 255, 255, 0.45) 50%,
    rgba(255, 255, 255, 0) 100%
  );
  animation: bar-sheen-slide 1.6s linear infinite;
}
@keyframes bar-sheen-slide {
  from {
    transform: translateX(-120%);
  }
  to {
    transform: translateX(320%);
  }
}
.scan-body {
  display: flex;
  flex-direction: column;
  gap: 1rem;
  min-width: 0;
}
.metrics {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr)) minmax(0, 1.45fr);
  gap: 0.7rem;
}
.metric {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
  padding: 0.55rem 0.75rem;
  border-radius: 12px;
  background: rgba(255, 255, 255, 0.52);
  border: 1px solid rgba(19, 32, 54, 0.06);
}
.grade-row {
  display: flex;
  flex-wrap: wrap;
  gap: 0.2rem 0.7rem;
}
.m-label {
  font-size: 0.76rem;
  color: var(--text-2);
}
.m-val {
  font-size: 1.18rem;
  font-weight: 720;
  letter-spacing: -0.02em;
  font-variant-numeric: tabular-nums;
}
.m-sub {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 0.82rem;
  font-weight: 600;
  white-space: nowrap;
  font-variant-numeric: tabular-nums;
}
.g {
  color: var(--grade-green);
}
.y {
  color: var(--grade-yellow);
}
.r {
  color: var(--grade-red);
}
.path-block {
  display: flex;
  flex-direction: column;
  gap: 0.35rem;
  min-width: 0;
  padding: 0.6rem 0.8rem;
  border-radius: 12px;
  background: rgba(15, 23, 42, 0.035);
  border: 1px solid rgba(19, 32, 54, 0.05);
}
.path-window {
  position: relative;
  min-height: 1.2em;
}
.path {
  display: block;
  font-size: 0.85rem;
  color: var(--text-2);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  font-family: ui-monospace, monospace;
}
/* R29：当前路径轮换淡入淡出（fast，mode=out-in 保证快速连刷不堆积）*/
.path-fade-enter-active {
  transition:
    opacity var(--dur-fast) var(--ease-out),
    transform var(--dur-fast) var(--ease-out);
}
.path-fade-leave-active {
  transition:
    opacity var(--dur-fast) var(--ease-inout),
    transform var(--dur-fast) var(--ease-inout);
  position: absolute;
  inset-inline: 0;
}
.path-fade-enter-from {
  opacity: 0;
  transform: translateY(4px);
}
.path-fade-leave-to {
  opacity: 0;
  transform: translateY(-4px);
}
.actions {
  display: flex;
  align-items: center;
  gap: 1rem;
}
.btn-cancel {
  background: linear-gradient(180deg, #c98800 0%, #b26a00 100%);
  color: #fff;
  border: none;
  padding: 0.62rem 1.3rem;
  border-radius: 11px;
  font-weight: 650;
  cursor: pointer;
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.3),
    0 6px 16px -8px rgba(140, 84, 0, 0.65);
  transition:
    transform var(--dur-fast) var(--ease-out),
    filter var(--dur-fast) var(--ease-inout),
    opacity var(--dur-fast) var(--ease-inout);
}
.btn-cancel:hover:not(:disabled) {
  filter: brightness(1.05);
}
.btn-cancel:active {
  transform: scale(0.98);
}
.btn-cancel:disabled {
  opacity: 0.45;
  box-shadow: none;
  cursor: not-allowed;
}
.tip {
  margin: 0;
  font-size: 0.8rem;
  color: var(--text-2);
}
.slow-hint {
  color: var(--grade-yellow);
  background: rgba(253, 247, 235, 0.82);
  border: 1px solid rgba(202, 158, 74, 0.28);
  border-radius: var(--r-md);
  padding: 1rem 1.25rem;
  margin: 0;
  box-shadow: var(--shadow-1);
  line-height: 1.65;
}
@media (max-width: 900px) {
  .scan-grid {
    grid-template-columns: minmax(0, 1fr);
    gap: 1.2rem;
  }
  .scan-visual {
    flex-direction: row;
    justify-content: flex-start;
    gap: 1.2rem;
  }
  .radar {
    width: 132px;
    height: 132px;
  }
  .radar-core {
    width: 84px;
    height: 84px;
  }
  .phase-glyph svg {
    width: 30px;
    height: 30px;
  }
  .phase-col {
    align-items: flex-start;
    text-align: left;
  }
}
@media (max-width: 760px) {
  .mode-grid {
    grid-template-columns: minmax(0, 1fr);
  }
  .metrics {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
  .metric:last-child {
    grid-column: 1 / -1;
  }
}
</style>
