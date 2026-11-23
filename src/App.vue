<script setup lang="ts">
import { computed } from "vue";
import { useRoute } from "vue-router";
import AppIcon from "./components/AppIcon.vue";
import BrandMark from "./components/BrandMark.vue";

// 顶层分区（导航形态：顶部双层——主分区 + 分区内页面）。
// 只改信息架构与视觉，不改任何路由与页面行为。
const sections = [
  {
    key: "overview",
    label: "概览",
    to: "/",
    icon: "home",
    items: [{ to: "/", label: "首页", icon: "home" }],
  },
  {
    key: "clean",
    label: "清理",
    to: "/scan",
    icon: "scan",
    items: [
      { to: "/scan", label: "一键体检", icon: "scan" },
      { to: "/report", label: "扫描报告", icon: "report" },
      { to: "/files", label: "大文件", icon: "files" },
    ],
  },
  {
    key: "tune",
    label: "优化与安全",
    to: "/startup",
    icon: "startup",
    items: [
      { to: "/startup", label: "启动项", icon: "startup" },
      { to: "/privacy", label: "隐私清理", icon: "privacy" },
      { to: "/apps", label: "软件卸载", icon: "apps" },
    ],
  },
  {
    key: "records",
    label: "记录",
    to: "/quarantine",
    icon: "log",
    items: [
      { to: "/quarantine", label: "隔离区", icon: "quarantine" },
      { to: "/log", label: "操作日志", icon: "log" },
    ],
  },
] as const;
const setting = { to: "/settings", label: "设置", icon: "settings" } as const;

const route = useRoute();

/** 路由 → 所属分区（/executing 属于清理流程）。 */
function sectionOf(path: string) {
  if (/^\/(scan|report|executing|files)/.test(path)) return sections[1];
  if (/^\/(startup|privacy|apps)/.test(path)) return sections[2];
  if (/^\/(quarantine|log)/.test(path)) return sections[3];
  return sections[0];
}

const activeSection = computed(() => sectionOf(route.path));
const activeKey = computed(() => (route.path === setting.to ? "" : activeSection.value.key));
const isSetting = computed(() => route.path === setting.to);
const hasSubnav = computed(
  () => activeSection.value.key !== "overview" && activeSection.value.items.length > 1,
);
</script>

<template>
  <!-- 环境光背景：纯静态渐变，不做逐帧动画（backdrop-filter 每一帧都会重采样） -->
  <div class="app-backdrop" aria-hidden="true"></div>

  <div class="app">
    <header class="topbar">
      <router-link to="/" class="brand" aria-label="PureSlate 首页">
        <BrandMark :size="28" />
        <span class="brand-name">PureSlate</span>
      </router-link>

      <nav class="sections" aria-label="主导航">
        <router-link
          v-for="s in sections"
          :key="s.key"
          :to="s.to"
          class="section-pill"
          :class="{ on: s.key === activeKey }"
        >
          <AppIcon :name="s.icon" :size="16" class="sec-icon" />
          <span>{{ s.label }}</span>
        </router-link>
      </nav>

      <div class="topbar-right">
        <span class="readonly-chip" title="扫描过程只读，不会修改任何文件">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M12 3.5 19 6v5.2c0 4.6-3.1 7.6-7 9.3-3.9-1.7-7-4.7-7-9.3V6Z" />
            <path d="m8.8 12 2.2 2.2 4.2-4.4" />
          </svg>
          只读扫描
        </span>
        <router-link
          :to="setting.to"
          class="icon-btn"
          :class="{ on: isSetting }"
          :aria-label="setting.label"
          :title="setting.label"
        >
          <AppIcon :name="setting.icon" :size="18" />
        </router-link>
      </div>
    </header>

    <nav class="subbar" aria-label="分区页面">
      <template v-if="hasSubnav">
        <router-link
          v-for="it in activeSection.items"
          :key="it.to"
          :to="it.to"
          class="sub-pill"
          :class="{ on: route.path === it.to }"
        >
          <AppIcon :name="it.icon" :size="15" class="sub-icon" />
          <span>{{ it.label }}</span>
        </router-link>
      </template>
      <p v-else class="subbar-promise">本地清理 · 零广告 · 零遥测 · 不收集任何使用数据</p>
    </nav>

    <main class="stage">
      <router-view v-slot="{ Component }">
        <Transition name="page" mode="out-in">
          <component :is="Component" />
        </Transition>
      </router-view>
    </main>
  </div>
</template>

<style>
/* =========================================================================
   设计 token（全应用唯一来源）
   视觉世界：层叠玻璃 —— 近白环境光 + 半透明浮层 + 高光描边 + 分层投影。
   ========================================================================= */
:root {
  /* 语义分级色（只用于安全分级，不做装饰；绿/黄均按白底 ≥4.5:1 选取） */
  --grade-green: #0e7f43;
  --grade-yellow: #b26a00;
  --grade-red: #d63a2f;

  /* 画布与表面 */
  --bg: #eef2f9;
  --surface: rgba(255, 255, 255, 0.78);
  --surface-2: rgba(244, 247, 252, 0.72);
  --surface-solid: #ffffff;
  --text: #131a26;
  --text-2: #4d5a6e;
  --text-3: #637080;

  --border: rgba(19, 32, 54, 0.08);
  --border-strong: rgba(19, 32, 54, 0.15);

  /* 品牌蓝 */
  --accent: #2563eb;
  --accent-hover: #1b4fd8;
  --accent-soft: rgba(37, 99, 235, 0.1);
  --accent-soft-2: rgba(37, 99, 235, 0.18);

  /* 玻璃材质 */
  --glass-blur: blur(22px) saturate(1.45);
  --glass-blur-strong: blur(30px) saturate(1.6);
  --glass-line: rgba(255, 255, 255, 0.72);
  --glass-sheen: inset 0 1px 0 rgba(255, 255, 255, 0.9);

  /* 圆角尺度 */
  --r-sm: 10px;
  --r-md: 14px;
  --r-lg: 18px;
  --r-xl: 24px;
  --r-pill: 999px;
  --r-card: var(--r-lg);
  --r-btn: var(--r-sm);
  --r-nav: 12px;

  /* 分层投影（偏移 + 软模糊，冷调而非纯黑） */
  --shadow-1:
    0 1px 1px rgba(16, 28, 52, 0.03),
    0 6px 18px -8px rgba(16, 28, 52, 0.12),
    0 20px 44px -28px rgba(16, 28, 52, 0.22);
  --shadow-2:
    0 2px 4px rgba(16, 28, 52, 0.04),
    0 14px 30px -12px rgba(16, 28, 52, 0.18),
    0 32px 60px -32px rgba(16, 28, 52, 0.26);
  --shadow-3:
    0 18px 50px -14px rgba(16, 28, 52, 0.28),
    0 4px 12px rgba(16, 28, 52, 0.08);
  --shadow-card: var(--shadow-1);
  --shadow-pop: var(--shadow-2);

  /* 动效 token（SPEC §7：全应用唯一来源，禁止散写时长/缓动） */
  --dur-fast: 140ms;
  --dur-base: 240ms;
  --dur-slow: 420ms;
  --ease-out: cubic-bezier(0.16, 1, 0.3, 1);
  --ease-inout: cubic-bezier(0.4, 0, 0.2, 1);

  --font: system-ui, "Segoe UI Variable Text", "Segoe UI", "Microsoft YaHei UI", "Microsoft YaHei",
    sans-serif;
}

* {
  box-sizing: border-box;
}

body {
  margin: 0;
  font-family: var(--font);
  background: var(--bg);
  color: var(--text);
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
  text-rendering: optimizeLegibility;
}

a {
  text-decoration: none;
  color: inherit;
}

button {
  font-family: inherit;
}

/* ---- 浏览器原生表面也属于设计的一部分 ---- */
::selection {
  background: rgba(37, 99, 235, 0.2);
}

input[type="checkbox"],
input[type="radio"] {
  accent-color: var(--accent);
}

::-webkit-scrollbar {
  width: 12px;
  height: 12px;
}
::-webkit-scrollbar-thumb {
  background: rgba(19, 32, 54, 0.18);
  border: 3px solid transparent;
  background-clip: padding-box;
  border-radius: var(--r-pill);
}
::-webkit-scrollbar-thumb:hover {
  background: rgba(19, 32, 54, 0.3);
  background-clip: padding-box;
}
::-webkit-scrollbar-track {
  background: transparent;
}

/* ---- 环境光：近白画布上三团极低饱和度的光晕，为玻璃提供折射内容 ---- */
.app-backdrop {
  position: fixed;
  inset: 0;
  z-index: 0;
  pointer-events: none;
  background:
    radial-gradient(1100px 720px at 8% -12%, rgba(206, 224, 255, 0.9) 0%, rgba(206, 224, 255, 0) 62%),
    radial-gradient(900px 640px at 102% -4%, rgba(205, 241, 233, 0.85) 0%, rgba(205, 241, 233, 0) 58%),
    radial-gradient(1000px 760px at 58% 118%, rgba(232, 222, 255, 0.8) 0%, rgba(232, 222, 255, 0) 62%),
    linear-gradient(180deg, #f8fafd 0%, #eef2f9 55%, #eef1f8 100%);
}

/* ===== 顶部双层导航：第一层主分区，第二层分区内页面 ===== */
.app {
  position: relative;
  z-index: 1;
  display: flex;
  flex-direction: column;
  min-height: 100vh;
}

.topbar {
  position: sticky;
  top: 0;
  z-index: 30;
  display: flex;
  align-items: center;
  gap: 20px;
  min-height: 60px;
  padding: 9px 24px;
  background: linear-gradient(180deg, rgba(255, 255, 255, 0.88), rgba(255, 255, 255, 0.7));
  border-bottom: 1px solid rgba(255, 255, 255, 0.72);
  backdrop-filter: var(--glass-blur-strong);
  -webkit-backdrop-filter: var(--glass-blur-strong);
  box-shadow: 0 10px 26px -24px rgba(16, 28, 52, 0.55);
}

.brand {
  display: flex;
  align-items: center;
  gap: 10px;
  flex: none;
  padding: 2px 4px 2px 0;
}
.brand-name {
  font-size: 1rem;
  font-weight: 720;
  letter-spacing: -0.012em;
  color: var(--text);
}

.sections {
  display: flex;
  gap: 2px;
  padding: 3px;
  border-radius: var(--r-pill);
  background: rgba(19, 32, 54, 0.045);
  box-shadow: inset 0 1px 2px rgba(16, 28, 52, 0.05);
}
.section-pill {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 7px 13px;
  border-radius: var(--r-pill);
  font-size: 0.86rem;
  font-weight: 550;
  color: var(--text-2);
  white-space: nowrap;
  transition:
    color var(--dur-fast) var(--ease-inout),
    background var(--dur-fast) var(--ease-inout),
    box-shadow var(--dur-fast) var(--ease-out),
    transform var(--dur-fast) var(--ease-out);
}
.sec-icon {
  color: var(--text-3);
  transition: color var(--dur-fast) var(--ease-inout);
}
.section-pill:hover {
  color: var(--text);
  background: rgba(255, 255, 255, 0.72);
}
.section-pill:active {
  transform: scale(0.98);
}
.section-pill.on {
  color: var(--accent);
  background: rgba(255, 255, 255, 0.96);
  font-weight: 650;
  box-shadow:
    inset 0 0 0 1px rgba(37, 99, 235, 0.14),
    0 2px 8px -4px rgba(16, 28, 52, 0.3);
}
.section-pill.on .sec-icon {
  color: var(--accent);
}

.topbar-right {
  margin-left: auto;
  display: flex;
  align-items: center;
  gap: 10px;
}
.readonly-chip {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 5px 11px 5px 9px;
  border-radius: var(--r-pill);
  font-size: 0.76rem;
  font-weight: 650;
  letter-spacing: 0.01em;
  color: #0e7f43;
  background: rgba(14, 127, 67, 0.09);
  border: 1px solid rgba(14, 127, 67, 0.22);
  white-space: nowrap;
}
.icon-btn {
  display: flex;
  width: 34px;
  height: 34px;
  align-items: center;
  justify-content: center;
  border-radius: 10px;
  color: var(--text-2);
  border: 1px solid transparent;
  transition:
    color var(--dur-fast) var(--ease-inout),
    background var(--dur-fast) var(--ease-inout),
    border-color var(--dur-fast) var(--ease-inout);
}
.icon-btn:hover {
  color: var(--text);
  background: rgba(255, 255, 255, 0.88);
  border-color: var(--border);
}
.icon-btn.on {
  color: var(--accent);
  background: rgba(37, 99, 235, 0.1);
  border-color: rgba(37, 99, 235, 0.18);
}

.subbar {
  position: relative;
  z-index: 1;
  display: flex;
  align-items: center;
  gap: 6px;
  min-height: 44px;
  padding: 8px 26px;
}
.sub-pill {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 6px 12px;
  border-radius: var(--r-pill);
  font-size: 0.84rem;
  font-weight: 550;
  color: var(--text-2);
  white-space: nowrap;
  transition:
    color var(--dur-fast) var(--ease-inout),
    background var(--dur-fast) var(--ease-inout),
    box-shadow var(--dur-fast) var(--ease-out);
}
.sub-icon {
  color: var(--text-3);
  transition: color var(--dur-fast) var(--ease-inout);
}
.sub-pill:hover {
  color: var(--text);
  background: rgba(255, 255, 255, 0.8);
}
.sub-pill.on {
  color: var(--accent);
  background: rgba(37, 99, 235, 0.1);
  box-shadow: inset 0 0 0 1px rgba(37, 99, 235, 0.14);
}
.sub-pill.on .sub-icon {
  color: var(--accent);
}
.subbar-promise {
  margin: 0;
  font-size: 0.76rem;
  color: var(--text-3);
  letter-spacing: 0.01em;
}

/* ===== 内容舞台 ===== */
.stage {
  flex: 1;
  padding: 20px 30px 44px;
  min-width: 0;
  /* R30-3：全局横向溢出兜底——超长单元格由各表 table-layout:fixed 截断，
     任何遗漏都不允许带出窗口级横向滚动条。 */
  overflow-x: hidden;
}

/* 焦点可见性（无障碍） */
a:focus-visible,
button:focus-visible,
input:focus-visible,
select:focus-visible,
textarea:focus-visible {
  outline: 2px solid rgba(37, 99, 235, 0.62);
  outline-offset: 2px;
  border-radius: 6px;
}

/* =========================================================================
   共享玻璃表面：各页面自有 scoped 规则负责底色/圆角，这里补齐折射与高光，
   让所有卡片属于同一套材质，而不是各写各的。
   ========================================================================= */
.card,
.tile,
.disk-card,
.group,
.panel,
.simple-card,
.done-card,
.res-card,
.actionbar,
.confirm,
.modal {
  backdrop-filter: var(--glass-blur);
  -webkit-backdrop-filter: var(--glass-blur);
}
.card,
.tile,
.disk-card,
.group,
.panel,
.simple-card,
.done-card,
.res-card,
.actionbar {
  box-shadow:
    var(--glass-sheen),
    var(--shadow-1);
}
.card,
.panel,
.group {
  border-color: var(--glass-line);
}

/* ===== R29 动效（SPEC §7）===== */

/* 数字类文本统一等宽数字，补间/高频刷新时不跳动 */
.metric-val,
.m-val,
.disk-pct,
.pct,
.promise-t {
  font-variant-numeric: tabular-nums;
}

/* 路由切换：唯一主动效（淡入 + 上移 + 轻微散焦后聚焦）*/
.page-enter-active {
  transition:
    opacity var(--dur-base) var(--ease-out),
    transform var(--dur-base) var(--ease-out),
    filter var(--dur-base) var(--ease-out);
}
.page-leave-active {
  transition:
    opacity var(--dur-fast) var(--ease-inout),
    transform var(--dur-fast) var(--ease-inout),
    filter var(--dur-fast) var(--ease-inout);
}
.page-enter-from {
  opacity: 0;
  transform: translateY(8px);
  filter: blur(5px);
}
.page-leave-to {
  opacity: 0;
  transform: translateY(-4px);
  filter: blur(4px);
}

/* 首屏入场工具类：挂载时 rise，配合 d1..d6 做受控 stagger（≤6 级/页，SPEC §7）*/
@keyframes ps-rise {
  from {
    opacity: 0;
    transform: translateY(10px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}
.anim-rise {
  opacity: 0;
  animation: ps-rise var(--dur-slow) var(--ease-out) forwards;
}
.anim-rise.d1 { animation-delay: 30ms; }
.anim-rise.d2 { animation-delay: 75ms; }
.anim-rise.d3 { animation-delay: 120ms; }
.anim-rise.d4 { animation-delay: 165ms; }
.anim-rise.d5 { animation-delay: 210ms; }
.anim-rise.d6 { animation-delay: 255ms; }

/* 弹窗/遮罩入场（各页 mask/dialog 复用；只动 opacity/transform）*/
@keyframes ps-mask-in {
  from {
    opacity: 0;
  }
}
@keyframes ps-modal-pop {
  from {
    opacity: 0;
    transform: translateY(10px) scale(0.97);
  }
}

/* 系统「减少动态效果」：token 层统一归零，页面不各自判断 */
@media (prefers-reduced-motion: reduce) {
  *,
  *::before,
  *::after {
    animation-duration: 0.01ms !important;
    animation-delay: 0ms !important;
    animation-iteration-count: 1 !important;
    transition-duration: 0.01ms !important;
    transition-delay: 0ms !important;
    scroll-behavior: auto !important;
  }
  .anim-rise {
    opacity: 1;
  }
  .page-enter-from,
  .page-leave-to {
    opacity: 1;
    transform: none;
    filter: none;
  }
}

/* 高对比度偏好：移除毛玻璃，回到实底，保证可读性 */
@media (prefers-contrast: more) {
  .topbar,
  .readonly-chip,
  .card,
  .tile,
  .disk-card,
  .group,
  .panel,
  .simple-card,
  .done-card,
  .res-card,
  .actionbar,
  .confirm,
  .modal,
  .list,
  .empty-list,
  .empty-panel,
  .hero-card,
  .notice,
  .quota {
    backdrop-filter: none;
    -webkit-backdrop-filter: none;
    background: var(--surface-solid);
    border-color: var(--border-strong);
  }
}

/* 窄窗：收紧顶栏间距（窗口最小宽度 900，此处主要服务网页预览） */
@media (max-width: 980px) {
  .topbar {
    gap: 12px;
    padding: 8px 16px;
  }
  .brand-name {
    display: none;
  }
  .sections {
    gap: 0;
  }
  .section-pill {
    padding: 7px 11px;
    font-size: 0.82rem;
  }
  .subbar {
    padding: 7px 18px;
  }
  .stage {
    padding: 16px 18px 32px;
  }
}
</style>
