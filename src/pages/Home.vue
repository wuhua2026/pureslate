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

// 功能宫格（图标色为自定浅色块 + 深字色，纯本地配色无外部资源）。
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
  { to: "/scan", auto: true, intent: "clean", title: "一键清理", desc: "临时文件与缓存，安全项直接清", icon: "scan", c: "#2563eb", bg: "#eaf1ff" },
  { to: "/report", title: "扫描报告", desc: "查看分级结果并清理", icon: "report", c: "#4f46e5", bg: "#ecebff" },
  { to: "/files", title: "大文件", desc: "定位占用空间的大文件", icon: "files", c: "#0891b2", bg: "#e0f4fa" },
  { to: "/startup", title: "启动项", desc: "管理开机自启程序", icon: "startup", c: "#16a34a", bg: "#e4f7ec" },
  { to: "/privacy", title: "隐私清理", desc: "浏览痕迹与最近记录", icon: "privacy", c: "#7c3aed", bg: "#f0e8ff" },
  { to: "/apps", title: "软件卸载", desc: "调起官方卸载器并清残留", icon: "apps", c: "#ea580c", bg: "#ffeddc" },
  { to: "/quarantine", title: "隔离区", desc: "14 天内可一键还原", icon: "quarantine", c: "#475569", bg: "#e9eef4" },
  { to: "/log", title: "操作日志", desc: "每一步操作可审计", icon: "log", c: "#64748b", bg: "#eef2f7" },
];

const promises = [
  { t: "只读扫描", d: "确认前不删除任何文件" },
  { t: "安全分级", d: "绿/黄/红三级去向" },
  { t: "全程可还原", d: "默认隔离区保留 14 天" },
];
</script>

<template>
  <main class="home">
    <!-- 欢迎条 -->
    <header class="welcome">
      <h1>电脑清理</h1>
      <p>本地清理 · 零广告 · 零遥测，每一步都由你确认</p>
    </header>

    <!-- R24 启动恢复横幅 -->
    <section v-if="recoveryBanner" class="recovery-banner anim-rise d1">
      <span class="recovery-dot"></span>
      <p class="recovery-text">{{ recoveryBanner }}</p>
      <router-link to="/quarantine" class="recovery-link">查看隔离区 →</router-link>
    </section>

    <!-- 清理主卡（R31-6：默认入口改为"一键清理"两击流程） -->
    <section class="hero-card anim-rise d1">
      <div class="hero-left">
        <!-- 状态圆环：未扫描=中性待清态（不伪造健康分数） -->
        <div class="ring" aria-label="待清理">
          <div class="ring-inner">
            <span class="ring-title">待清理</span>
            <span class="ring-sub">未扫描</span>
          </div>
        </div>
        <div class="hero-text">
          <h2>一键清理</h2>
          <p class="hero-desc">
            只清理可安全重建的<strong>临时文件与缓存</strong>，不会动你的文档/照片；
            有风险的项目会让你确认。
          </p>
          <ul class="promise-row">
            <li v-for="p in promises" :key="p.t">
              <span class="promise-t">{{ p.t }}</span>
              <span class="promise-d">{{ p.d }}</span>
            </li>
          </ul>
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
        <router-link :to="{ path: '/scan', query: { auto: '1' } }" class="btn-ghost" @click="goScan(true)">深度体检</router-link>
        <span class="deep-hint">深度含大文件/重复文件，大容量盘较慢</span>
      </div>
    </section>

    <!-- 磁盘占用 -->
    <section class="block anim-rise d2">
      <div class="block-head">
        <h3>磁盘占用</h3>
      </div>
      <div v-if="disks.length" class="disk-grid">
        <DiskUsageCard v-for="d in disks" :key="d.letter" :disk="d" />
      </div>
      <p v-else class="empty">暂无磁盘数据</p>
    </section>

    <!-- 功能宫格 -->
    <section class="block anim-rise d3">
      <div class="block-head">
        <h3>清理工具</h3>
      </div>
      <div class="tile-grid">
        <router-link
          v-for="(t, i) in tiles"
          :key="t.to"
          :to="{
            path: t.to,
            query: t.intent === 'clean' ? { auto: '1', intent: 'clean' } : t.auto ? { auto: '1' } : {},
          }"
          class="tile tile-in"
          :style="{ animationDelay: Math.min(i, 6) * 40 + 120 + 'ms' }"
        >
          <span class="tile-icon" :style="{ background: t.bg, color: t.c }">
            <AppIcon :name="t.icon" :size="22" />
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
  gap: 20px;
  max-width: 1080px;
  margin: 0 auto;
}

/* 欢迎条 */
.welcome h1 {
  margin: 0 0 4px;
  font-size: 1.45rem;
  font-weight: 700;
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
  background: #fdf6e9;
  border: 1px solid #f2ddb4;
  border-radius: var(--r-card);
  padding: 10px 16px;
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

/* ===== 体检主卡 ===== */
.hero-card {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 28px;
  background: linear-gradient(120deg, #ffffff 0%, #f4f8ff 100%);
  border: 1px solid var(--border);
  border-radius: var(--r-card);
  box-shadow: var(--shadow-card);
  padding: 26px 30px;
}
.hero-left {
  display: flex;
  align-items: center;
  gap: 24px;
  min-width: 0;
}

/* 状态圆环（纯 CSS conic-gradient；初始态轨道灰） */
.ring {
  width: 118px;
  height: 118px;
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
  inset: 9px;
  border-radius: 50%;
  background: var(--surface);
  box-shadow: inset 0 0 0 1px var(--border);
}
.ring-inner {
  position: relative;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
}
.ring-title {
  font-size: 1rem;
  font-weight: 700;
  color: var(--text);
}
.ring-sub {
  font-size: 0.7rem;
  color: var(--text-3);
}

.hero-text {
  min-width: 0;
}
.hero-text h2 {
  margin: 0 0 8px;
  font-size: 1.22rem;
}
.hero-desc {
  margin: 0 0 14px;
  font-size: 0.86rem;
  line-height: 1.7;
  color: var(--text-2);
  max-width: 34rem;
}
.promise-row {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  gap: 22px;
  flex-wrap: wrap;
}
.promise-row li {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding-left: 13px;
  position: relative;
}
.promise-row li::before {
  content: "";
  position: absolute;
  left: 0;
  top: 5px;
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--accent);
}
.promise-t {
  font-size: 0.82rem;
  font-weight: 700;
}
.promise-d {
  font-size: 0.72rem;
  color: var(--text-3);
}

.hero-actions {
  display: flex;
  flex-direction: column;
  align-items: stretch;
  gap: 9px;
  flex-shrink: 0;
  width: 168px;
}
.btn-main {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 7px;
  background: var(--accent);
  color: #fff;
  padding: 12px 18px;
  border-radius: var(--r-btn);
  font-weight: 700;
  font-size: 0.95rem;
  box-shadow: 0 6px 16px rgba(37, 99, 235, 0.28);
  transition:
    background var(--dur-fast) var(--ease-inout),
    transform var(--dur-fast) var(--ease-inout);
}
.btn-main:hover {
  background: var(--accent-hover);
}
.btn-main:active {
  transform: translateY(1px);
}
.btn-ghost {
  text-align: center;
  padding: 9px 18px;
  border-radius: var(--r-btn);
  border: 1px solid var(--border-strong);
  color: var(--text);
  font-size: 0.86rem;
  font-weight: 600;
  background: var(--surface);
  transition:
    border-color var(--dur-fast) var(--ease-inout),
    color var(--dur-fast) var(--ease-inout);
}
.btn-ghost:hover {
  border-color: var(--accent);
  color: var(--accent);
}
.deep-hint {
  font-size: 0.68rem;
  color: var(--text-3);
  text-align: center;
  line-height: 1.4;
}

/* ===== 区块 ===== */
.block-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 12px;
}
.block-head h3 {
  margin: 0;
  font-size: 1rem;
  font-weight: 700;
}
.block-head h3::before {
  content: "";
  display: inline-block;
  width: 4px;
  height: 15px;
  border-radius: 2px;
  background: var(--accent);
  margin-right: 9px;
  vertical-align: -2px;
}

/* 磁盘网格布局（卡片本体在 components/DiskUsageCard.vue）*/
.disk-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
  gap: 12px;
}

/* 功能宫格 */
.tile-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
  gap: 12px;
}
.tile {
  display: flex;
  align-items: center;
  gap: 13px;
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--r-card);
  box-shadow: var(--shadow-card);
  padding: 15px 16px;
  transition:
    transform var(--dur-fast) var(--ease-inout),
    box-shadow var(--dur-fast) var(--ease-inout),
    border-color var(--dur-fast) var(--ease-inout);
}
.tile:hover {
  transform: translateY(-2px);
  box-shadow: var(--shadow-pop);
  border-color: var(--accent-soft-2);
}
.tile:active {
  transform: translateY(0) scale(0.99);
}
/* R29：宫格卡片首屏入场（级差 ≤6，受控 stagger）*/
.tile-in {
  opacity: 0;
  animation: ps-rise var(--dur-slow) var(--ease-out) forwards;
}
.tile-icon {
  width: 44px;
  height: 44px;
  border-radius: 12px;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
}
.tile-body {
  display: flex;
  flex-direction: column;
  gap: 3px;
  min-width: 0;
  flex: 1;
}
.tile-title {
  font-size: 0.92rem;
  font-weight: 700;
}
.tile-desc {
  font-size: 0.74rem;
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

/* 窄窗口：主卡改为纵向 */
@media (max-width: 820px) {
  .hero-card {
    flex-direction: column;
    align-items: stretch;
  }
  .hero-actions {
    width: 100%;
  }
  .hero-left {
    flex-direction: column;
    text-align: center;
  }
  .promise-row {
    justify-content: center;
  }
}
</style>
