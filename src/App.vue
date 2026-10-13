<script setup lang="ts">
import AppIcon from "./components/AppIcon.vue";

// 侧栏分组（视觉组织，不改路由）。as const 让 icon 字段推导为字面量联合，
// 直接满足 AppIcon 的 name 类型。
const groups = [
  {
    label: "概览",
    items: [{ to: "/", label: "首页", icon: "home" }],
  },
  {
    label: "清理",
    items: [
      { to: "/scan", label: "一键体检", icon: "scan" },
      { to: "/report", label: "扫描报告", icon: "report" },
      { to: "/files", label: "大文件", icon: "files" },
    ],
  },
  {
    label: "优化与安全",
    items: [
      { to: "/startup", label: "启动项", icon: "startup" },
      { to: "/privacy", label: "隐私清理", icon: "privacy" },
      { to: "/apps", label: "软件卸载", icon: "apps" },
    ],
  },
  {
    label: "记录",
    items: [
      { to: "/quarantine", label: "隔离区", icon: "quarantine" },
      { to: "/log", label: "操作日志", icon: "log" },
    ],
  },
] as const;
const setting = { to: "/settings", label: "设置", icon: "settings" } as const;
</script>

<template>
  <div class="shell">
    <aside class="sidebar">
      <div class="brand">
        <span class="brand-mark" aria-hidden="true">
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none">
            <path
              d="M12 3 19 5.6v5c0 4.4-2.9 7.4-7 9-4.1-1.6-7-4.6-7-9v-5Z"
              fill="#fff"
              fill-opacity="0.18"
              stroke="#fff"
              stroke-width="1.7"
              stroke-linejoin="round"
            />
            <path d="m8.6 12 2.3 2.3 4.5-4.7" stroke="#fff" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" />
          </svg>
        </span>
        <span class="brand-name">PureSlate</span>
      </div>

      <nav class="nav">
        <div v-for="g in groups" :key="g.label" class="nav-group">
          <div class="nav-group-label">{{ g.label }}</div>
          <router-link
            v-for="it in g.items"
            :key="it.to"
            :to="it.to"
            class="nav-item"
          >
            <AppIcon :name="it.icon" :size="19" class="nav-icon" />
            <span>{{ it.label }}</span>
          </router-link>
        </div>
      </nav>

      <div class="sidebar-foot">
        <router-link :to="setting.to" class="nav-item nav-setting">
          <AppIcon :name="'settings'" :size="19" class="nav-icon" />
          <span>{{ setting.label }}</span>
        </router-link>
        <div class="slogan">本地清理 · 零广告 · 零遥测</div>
      </div>
    </aside>

    <main class="content">
      <router-view />
    </main>
  </div>
</template>

<style>
/* ===== 全局设计 token（联想电脑管家风格：蓝白 / 大圆角 / 卡片阴影）===== */
:root {
  --grade-green: #16a34a;
  --grade-yellow: #d9971c;
  --grade-red: #dc3b30;

  --bg: #f3f5f9;
  --surface: #ffffff;
  --surface-2: #f8fafc;
  --text: #1c2433;
  --text-2: #5d6b80;
  --text-3: #8a97aa;
  --border: #e7ebf2;
  --border-strong: #d7deea;

  /* 主品牌蓝（联想系管家蓝，比旧值略沉稳） */
  --accent: #2563eb;
  --accent-hover: #1d4ed8;
  --accent-soft: #eaf1ff;
  --accent-soft-2: #dbe7ff;

  --r-card: 14px;
  --r-btn: 10px;
  --r-nav: 10px;
  --shadow-card: 0 1px 2px rgba(16, 24, 40, 0.04), 0 4px 14px rgba(16, 24, 40, 0.05);
  --shadow-pop: 0 8px 28px rgba(16, 24, 40, 0.12);

  --font: system-ui, "Microsoft YaHei UI", "Segoe UI", sans-serif;
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
}

a {
  text-decoration: none;
  color: inherit;
}

button {
  font-family: inherit;
}

/* 细滚动条（桌面管家感） */
::-webkit-scrollbar {
  width: 10px;
  height: 10px;
}
::-webkit-scrollbar-thumb {
  background: #cdd5e1;
  border: 3px solid var(--bg);
  border-radius: 8px;
}
::-webkit-scrollbar-thumb:hover {
  background: #b3bdcd;
}
::-webkit-scrollbar-track {
  background: transparent;
}

.shell {
  display: grid;
  grid-template-columns: 212px 1fr;
  min-height: 100vh;
}

/* ===== 侧边栏 ===== */
.sidebar {
  background: var(--surface);
  border-right: 1px solid var(--border);
  display: flex;
  flex-direction: column;
  padding: 18px 12px 14px;
  gap: 10px;
  position: sticky;
  top: 0;
  height: 100vh;
}

.brand {
  display: flex;
  align-items: center;
  gap: 9px;
  padding: 4px 8px 14px;
}
.brand-mark {
  width: 32px;
  height: 32px;
  border-radius: 9px;
  background: linear-gradient(145deg, #3b78f5, #1d4ed8);
  display: flex;
  align-items: center;
  justify-content: center;
  box-shadow: 0 3px 8px rgba(37, 99, 235, 0.32);
  flex-shrink: 0;
}
.brand-name {
  font-size: 1.08rem;
  font-weight: 700;
  letter-spacing: 0.2px;
  color: var(--text);
}

.nav {
  display: flex;
  flex-direction: column;
  gap: 14px;
  overflow-y: auto;
  flex: 1;
  min-height: 0;
}
.nav-group {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.nav-group-label {
  font-size: 0.68rem;
  font-weight: 600;
  letter-spacing: 0.08em;
  color: var(--text-3);
  padding: 0 10px 5px;
}

.nav-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 10px;
  border-radius: var(--r-nav);
  color: var(--text-2);
  font-size: 0.9rem;
  font-weight: 500;
  white-space: nowrap;
  transition:
    background 0.14s ease,
    color 0.14s ease;
}
.nav-icon {
  color: var(--text-3);
  transition: color 0.14s ease;
  flex-shrink: 0;
}
.nav-item:hover {
  background: var(--surface-2);
  color: var(--text);
}
.nav-item:hover .nav-icon {
  color: var(--accent);
}
.nav-item.router-link-active {
  background: var(--accent-soft);
  color: var(--accent);
  font-weight: 600;
}
.nav-item.router-link-active .nav-icon {
  color: var(--accent);
}

.sidebar-foot {
  border-top: 1px solid var(--border);
  padding-top: 10px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.nav-setting {
  padding-top: 4px;
}
.slogan {
  font-size: 0.68rem;
  color: var(--text-3);
  padding: 0 10px 2px;
  letter-spacing: 0.02em;
}

/* ===== 内容区 ===== */
.content {
  padding: 22px 28px 30px;
  min-width: 0;
}

/* 焦点可见性（无障碍） */
a:focus-visible,
button:focus-visible,
input:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}
</style>
