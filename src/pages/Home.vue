<script setup lang="ts">
import { onMounted, ref } from "vue";
import { crash_recovery, disk_usage } from "../api/commands";
import { DiskUsageStub, type DiskUsageEntry } from "../mocks/data";
import { recoveryNotice } from "./crashModel";
import type { DiskUsageInfo } from "../types/ipc";
import AppIcon from "../components/AppIcon.vue";
import DiskUsageCard from "../components/DiskUsageCard.vue";

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

// 功能入口（图标色为自定浅色块 + 深字色，纯本地配色无外部资源）。
const tiles: Array<{
  to: string;
  auto?: boolean;
  /** R31-6：一键清理入口标记——跳 /scan 时带 intent=clean，自动快速扫描。 */
  intent?: "clean";
  title: string;
  desc: string;
  icon:
    | "scan"
    | "report"
    | "files"
    | "startup"
    | "privacy"
    | "apps"
    | "quarantine"
    | "log";
  c: string;
  bg: string;
}> = [
  { to: "/scan", auto: true, intent: "clean", title: "一键清理", desc: "系统临时文件与缓存，安全项直接清", icon: "scan", c: "#2563eb", bg: "#e9f0ff" },
  { to: "/report", title: "扫描报告", desc: "查看分级结果并清理", icon: "report", c: "#4f46e5", bg: "#eeeaff" },
  { to: "/files", title: "大文件", desc: "定位占用空间的大文件", icon: "files", c: "#0b7f9c", bg: "#e4f5f9" },
  { to: "/startup", title: "启动项", desc: "管理开机自启程序", icon: "startup", c: "#12884a", bg: "#e6f6ec" },
  { to: "/privacy", title: "隐私清理", desc: "浏览痕迹与最近记录", icon: "privacy", c: "#7c3aed", bg: "#f2ecff" },
  { to: "/apps", title: "软件卸载", desc: "调起官方卸载器并清残留", icon: "apps", c: "#d9540a", bg: "#fdefe3" },
  { to: "/quarantine", title: "隔离区", desc: "保留期内可一键还原", icon: "quarantine", c: "#475569", bg: "#edf1f6" },
  { to: "/log", title: "操作日志", desc: "每一步操作可审计", icon: "log", c: "#5b6b80", bg: "#eff3f8" },
];

const promises = [
  { t: "只读扫描", d: "确认前不删除任何文件" },
  { t: "安全分级", d: "绿/黄/红三级去向" },
  { t: "全程可还原", d: "默认隔离区保留 14 天（设置中可调 7–30 天）" },
];
</script>

<template>
  <main class="home">
    <!-- 欢迎条 -->
    <header class="welcome anim-rise">
      <h1>电脑清理</h1>
      <p>本地清理 · 零广告 · 零遥测，每一步都由你确认</p>
    </header>

    <!-- R24 启动恢复横幅 -->
    <section v-if="recoveryBanner" class="recovery-banner anim-rise d1">
      <span class="recovery-dot"></span>
      <p class="recovery-text">{{ recoveryBanner }}</p>
      <router-link to="/quarantine" class="recovery-link">查看隔离区 →</router-link>
    </section>

    <!-- 仪表盘：左侧主行动 + 右侧磁盘/还原入口 -->
    <div class="board">
      <section class="hero-card anim-rise d1">
        <div class="hero-main">
          <!-- 状态圆环：未扫描=中性待清态（不伪造健康分数） -->
          <div class="ring" aria-label="待清理，尚未扫描">
            <div class="ring-inner">
              <span class="ring-title">待清理</span>
              <span class="ring-sub">未扫描</span>
            </div>
          </div>
          <div class="hero-text">
            <span class="eyebrow">本机状态</span>
            <h2>先看清，再清理</h2>
            <p class="hero-desc">
              只清理系统临时目录（%TEMP%）与可重建缓存（缩略图/浏览器），不会动你的文档、照片和其它目录里的文件；
              有风险的项目会让你确认。
            </p>
          </div>
        </div>

        <div class="hero-actions">
          <router-link
            :to="{ path: '/scan', query: { auto: '1', intent: 'clean' } }"
            class="btn-main"
            @click="goScan(false)"
          >
            <AppIcon name="scan" :size="19" />
            一键清理
          </router-link>
          <router-link :to="{ path: '/scan', query: { auto: '1' } }" class="btn-ghost" @click="goScan(true)">
            深度体检
          </router-link>
          <span class="deep-hint">深度含大文件/重复文件，大容量盘较慢</span>
        </div>

        <ul class="promise-row">
          <li v-for="p in promises" :key="p.t">
            <span class="promise-t">{{ p.t }}</span>
            <span class="promise-d">{{ p.d }}</span>
          </li>
        </ul>
      </section>

      <aside class="side-col">
        <section class="disk-panel anim-rise d2">
          <div class="side-head">
            <h3>磁盘占用</h3>
          </div>
          <div v-if="disks.length" class="disk-stack">
            <DiskUsageCard v-for="d in disks" :key="d.letter" :disk="d" />
          </div>
          <p v-else class="empty">暂无磁盘数据</p>
        </section>

        <router-link to="/quarantine" class="side-link anim-rise d3">
          <span class="side-link-icon"><AppIcon name="quarantine" :size="18" /></span>
          <span class="side-link-body">
            <strong>隔离区</strong>
            <span>保留期内随时一键还原</span>
          </span>
          <span class="side-link-arrow" aria-hidden="true">›</span>
        </router-link>
      </aside>
    </div>

    <!-- 工具入口：清单行而非宫格 -->
    <section class="tools anim-rise d4">
      <div class="block-head">
        <h3>清理工具</h3>
        <span class="block-hint">按需进入，不会自动执行</span>
      </div>
      <div class="tool-grid">
        <router-link
          v-for="(t, i) in tiles"
          :key="t.to"
          :to="{
            path: t.to,
            query: t.intent === 'clean' ? { auto: '1', intent: 'clean' } : t.auto ? { auto: '1' } : {},
          }"
          class="tile tile-in"
          :style="{ animationDelay: Math.min(i, 7) * 34 + 90 + 'ms' }"
        >
          <span class="tile-icon" :style="{ background: t.bg, color: t.c }">
            <AppIcon :name="t.icon" :size="21" />
          </span>
          <span class="tile-body">
            <span class="tile-title">{{ t.title }}</span>
            <span class="tile-desc">{{ t.desc }}</span>
          </span>
          <span class="tile-arrow" aria-hidden="true">›</span>
        </router-link>
      </div>
    </section>
  </main>
</template>

<style scoped>
.home {
  display: flex;
  flex-direction: column;
  gap: 18px;
  max-width: 1120px;
  margin: 0 auto;
}

/* 欢迎条 */
.welcome h1 {
  margin: 0 0 4px;
  font-size: 1.62rem;
  font-weight: 740;
  letter-spacing: -0.024em;
}
.welcome p {
  margin: 0;
  font-size: 0.86rem;
  color: var(--text-2);
}

/* 恢复横幅 */
.recovery-banner {
  display: flex;
  align-items: center;
  gap: 10px;
  background: rgba(253, 247, 235, 0.8);
  border: 1px solid rgba(202, 158, 74, 0.28);
  border-radius: var(--r-md);
  padding: 10px 16px;
  backdrop-filter: var(--glass-blur);
  -webkit-backdrop-filter: var(--glass-blur);
  box-shadow:
    var(--glass-sheen),
    0 6px 18px -12px rgba(120, 84, 20, 0.35);
}
.recovery-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--grade-yellow);
  flex-shrink: 0;
}
.recovery-text {
  margin: 0;
  font-size: 0.84rem;
  line-height: 1.55;
  flex: 1;
}
.recovery-link {
  font-size: 0.82rem;
  color: var(--accent);
  white-space: nowrap;
  font-weight: 600;
}

/* ===== 仪表盘栅格 ===== */
.board {
  display: grid;
  grid-template-columns: minmax(0, 1.62fr) minmax(300px, 0.95fr);
  gap: 16px;
  align-items: start;
}

/* 主行动卡 */
.hero-card {
  position: relative;
  display: flex;
  flex-direction: column;
  gap: 18px;
  padding: 24px 26px 20px;
  border-radius: var(--r-xl);
  border: 1px solid var(--glass-line);
  background:
    radial-gradient(520px 220px at 92% -30%, rgba(37, 99, 235, 0.14), rgba(37, 99, 235, 0) 70%),
    linear-gradient(135deg, rgba(255, 255, 255, 0.94) 0%, rgba(240, 246, 255, 0.78) 100%);
  box-shadow:
    var(--glass-sheen),
    var(--shadow-2);
  backdrop-filter: var(--glass-blur-strong);
  -webkit-backdrop-filter: var(--glass-blur-strong);
  overflow: hidden;
}
.hero-main {
  display: flex;
  align-items: center;
  gap: 22px;
  min-width: 0;
}

/* 状态圆环（纯 CSS conic-gradient；初始态轨道灰） */
.ring {
  width: 132px;
  height: 132px;
  border-radius: 50%;
  background: conic-gradient(var(--border-strong) 0deg, var(--border) 0deg);
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  position: relative;
}
/* R29：待体检态的极弱呼吸（只动 opacity，不伪造健康进度）*/
.ring::after {
  content: "";
  position: absolute;
  inset: -4px;
  border-radius: 50%;
  border: 2px solid var(--accent-soft-2);
  opacity: 0;
  animation: ring-breath 2.8s var(--ease-inout) infinite;
  pointer-events: none;
}
@keyframes ring-breath {
  0%,
  100% {
    opacity: 0;
    transform: scale(0.96);
  }
  50% {
    opacity: 0.7;
    transform: scale(1.02);
  }
}
.ring::before {
  content: "";
  position: absolute;
  inset: 10px;
  border-radius: 50%;
  background: rgba(255, 255, 255, 0.9);
  box-shadow:
    inset 0 0 0 1px var(--border),
    inset 0 2px 8px rgba(16, 28, 52, 0.04),
    0 2px 8px rgba(16, 28, 52, 0.04);
}
.ring-inner {
  position: relative;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
}
.ring-title {
  font-size: 1.04rem;
  font-weight: 720;
  color: var(--text);
}
.ring-sub {
  font-size: 0.7rem;
  color: var(--text-3);
}

.hero-text {
  min-width: 0;
}
.eyebrow {
  display: inline-block;
  margin-bottom: 6px;
  font-size: 0.7rem;
  font-weight: 700;
  letter-spacing: 0.1em;
  color: var(--accent);
}
.hero-text h2 {
  margin: 0 0 7px;
  font-size: 1.42rem;
  font-weight: 730;
  letter-spacing: -0.02em;
}
.hero-desc {
  margin: 0;
  font-size: 0.86rem;
  line-height: 1.68;
  color: var(--text-2);
  max-width: 46rem;
}

.hero-actions {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
}
.btn-main {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 7px;
  background: linear-gradient(180deg, #3b7bf5 0%, #2563eb 55%, #1d51d8 100%);
  color: #fff;
  padding: 11px 20px;
  border-radius: 12px;
  font-weight: 700;
  font-size: 0.94rem;
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.32),
    inset 0 -1px 0 rgba(0, 0, 0, 0.12),
    0 8px 20px -6px rgba(37, 99, 235, 0.55);
  transition:
    transform var(--dur-fast) var(--ease-out),
    box-shadow var(--dur-fast) var(--ease-out),
    filter var(--dur-fast) var(--ease-inout);
}
.btn-main:hover {
  filter: brightness(1.05);
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.4),
    inset 0 -1px 0 rgba(0, 0, 0, 0.12),
    0 12px 26px -8px rgba(37, 99, 235, 0.6);
}
.btn-main:active {
  transform: translateY(1px) scale(0.99);
  filter: brightness(0.97);
}
.btn-ghost {
  text-align: center;
  padding: 10px 18px;
  border-radius: 12px;
  border: 1px solid var(--border-strong);
  color: var(--text);
  font-size: 0.86rem;
  font-weight: 600;
  background: rgba(255, 255, 255, 0.72);
  transition:
    border-color var(--dur-fast) var(--ease-inout),
    color var(--dur-fast) var(--ease-inout),
    background var(--dur-fast) var(--ease-inout),
    transform var(--dur-fast) var(--ease-out);
}
.btn-ghost:hover {
  border-color: rgba(37, 99, 235, 0.45);
  color: var(--accent);
  background: rgba(255, 255, 255, 0.95);
}
.btn-ghost:active {
  transform: scale(0.99);
}
.deep-hint {
  font-size: 0.72rem;
  color: var(--text-3);
}

.promise-row {
  list-style: none;
  margin: 0;
  padding: 15px 0 0;
  border-top: 1px solid var(--border);
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 12px;
}
.promise-row li {
  position: relative;
  padding-left: 14px;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.promise-row li::before {
  content: "";
  position: absolute;
  left: 0;
  top: 6px;
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--accent);
  box-shadow: 0 0 0 3px rgba(37, 99, 235, 0.1);
}
.promise-t {
  font-size: 0.8rem;
  font-weight: 700;
}
.promise-d {
  font-size: 0.71rem;
  color: var(--text-3);
  line-height: 1.5;
}

/* ===== 右栏：磁盘 + 隔离区入口 ===== */
.side-col {
  display: flex;
  flex-direction: column;
  gap: 12px;
  min-width: 0;
}
.side-head h3 {
  margin: 0 0 10px;
  font-size: 0.94rem;
  font-weight: 700;
  letter-spacing: -0.01em;
  display: flex;
  align-items: center;
  gap: 8px;
}
.side-head h3::before {
  content: "";
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: linear-gradient(140deg, #5b93ff, var(--accent));
  box-shadow: 0 0 0 3px rgba(37, 99, 235, 0.1);
}
.disk-stack {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.side-link {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 14px 16px;
  border-radius: var(--r-lg);
  background: var(--surface);
  border: 1px solid var(--glass-line);
  backdrop-filter: var(--glass-blur);
  -webkit-backdrop-filter: var(--glass-blur);
  box-shadow:
    var(--glass-sheen),
    var(--shadow-1);
  transition:
    transform var(--dur-base) var(--ease-out),
    box-shadow var(--dur-base) var(--ease-out),
    border-color var(--dur-base) var(--ease-inout);
}
.side-link:hover {
  transform: translateY(-2px);
  border-color: rgba(37, 99, 235, 0.26);
  box-shadow:
    var(--glass-sheen),
    var(--shadow-2);
}
.side-link:active {
  transform: translateY(0) scale(0.995);
}
.side-link-icon {
  width: 36px;
  height: 36px;
  border-radius: 11px;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  color: var(--accent);
  background: rgba(37, 99, 235, 0.09);
  box-shadow: inset 0 0 0 1px rgba(37, 99, 235, 0.14);
}
.side-link-body {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
  flex: 1;
}
.side-link-body strong {
  font-size: 0.88rem;
  font-weight: 700;
}
.side-link-body span {
  font-size: 0.74rem;
  color: var(--text-3);
}
.side-link-arrow {
  color: #c2cbd8;
  font-size: 1.2rem;
  line-height: 1;
  transition:
    transform var(--dur-fast) var(--ease-inout),
    color var(--dur-fast) var(--ease-inout);
}
.side-link:hover .side-link-arrow {
  color: var(--accent);
  transform: translateX(2px);
}

/* ===== 工具入口 ===== */
.tools {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.block-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 12px;
}
.block-head h3 {
  margin: 0;
  font-size: 1rem;
  font-weight: 700;
  letter-spacing: -0.01em;
  display: flex;
  align-items: center;
  gap: 9px;
}
.block-head h3::before {
  content: "";
  display: inline-block;
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: linear-gradient(140deg, #5b93ff, var(--accent));
  box-shadow: 0 0 0 4px rgba(37, 99, 235, 0.1);
}
.block-hint {
  font-size: 0.74rem;
  color: var(--text-3);
}

.tool-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 10px;
}
.tile {
  position: relative;
  display: flex;
  align-items: center;
  gap: 12px;
  background: var(--surface);
  border: 1px solid var(--glass-line);
  border-radius: var(--r-lg);
  box-shadow:
    var(--glass-sheen),
    var(--shadow-1);
  backdrop-filter: var(--glass-blur);
  -webkit-backdrop-filter: var(--glass-blur);
  padding: 13px 15px;
  transition:
    transform var(--dur-base) var(--ease-out),
    box-shadow var(--dur-base) var(--ease-out),
    border-color var(--dur-base) var(--ease-inout);
}
.tile:hover {
  transform: translateY(-2px);
  box-shadow:
    var(--glass-sheen),
    var(--shadow-2);
  border-color: rgba(37, 99, 235, 0.28);
}
.tile:active {
  transform: translateY(0) scale(0.995);
  transition-duration: var(--dur-fast);
}
/* R29：清单行首屏入场（级差受控 stagger）*/
.tile-in {
  opacity: 0;
  animation: ps-rise var(--dur-slow) var(--ease-out) forwards;
}
.tile-icon {
  width: 40px;
  height: 40px;
  border-radius: 12px;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.85),
    inset 0 0 0 1px rgba(19, 32, 54, 0.05),
    0 2px 6px -2px rgba(16, 28, 52, 0.12);
  transition: transform var(--dur-base) var(--ease-out);
}
.tile:hover .tile-icon {
  transform: scale(1.06) rotate(-2deg);
}
.tile-body {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
  flex: 1;
}
.tile-title {
  font-size: 0.9rem;
  font-weight: 700;
}
.tile-desc {
  font-size: 0.73rem;
  color: var(--text-3);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.tile-arrow {
  color: #c2cbd8;
  font-size: 1.3rem;
  line-height: 1;
  transition:
    transform var(--dur-fast) var(--ease-inout),
    color var(--dur-fast) var(--ease-inout);
}
.tile:hover .tile-arrow {
  color: var(--accent);
  transform: translateX(2px);
}

.empty {
  color: var(--text-2);
  font-size: 0.86rem;
  padding: 18px 0;
}

/* 窄窗口：仪表盘收成单列 */
@media (max-width: 900px) {
  .board {
    grid-template-columns: 1fr;
  }
  .side-col {
    flex-direction: row;
    flex-wrap: wrap;
  }
  .side-col > * {
    flex: 1 1 320px;
  }
}
@media (max-width: 760px) {
  .hero-main {
    flex-direction: column;
    align-items: flex-start;
  }
  .promise-row {
    grid-template-columns: 1fr;
  }
  .tool-grid {
    grid-template-columns: 1fr;
  }
}
</style>
