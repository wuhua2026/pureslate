<script setup lang="ts"></script>

<template>
  <div class="shell">
    <aside class="sidebar">
      <div class="brand">PureSlate</div>
      <nav class="nav">
        <router-link to="/" class="nav-item">首页</router-link>
        <router-link to="/scan" class="nav-item">一键体检</router-link>
        <router-link to="/report" class="nav-item">扫描报告</router-link>
        <router-link to="/files" class="nav-item">大文件</router-link>
        <router-link to="/startup" class="nav-item">启动项</router-link>
        <router-link to="/privacy" class="nav-item">隐私资料</router-link>
        <router-link to="/quarantine" class="nav-item">隔离区</router-link>
        <router-link to="/log" class="nav-item">操作日志</router-link>
        <router-link to="/settings" class="nav-item">设置</router-link>
      </nav>
      <div class="foot">本地清理 · 零广告 · 零外传</div>
    </aside>
    <main class="content">
      <router-view />
    </main>
  </div>
</template>

<style>
:root {
  --grade-green: #2e9e5b;
  --grade-yellow: #d9971c;
  --grade-red: #c4382e;
  --bg: #fafafa;
  --surface: #ffffff;
  --text: #1f2430;
  --text-2: #5a6472;
  --border: #e4e7ec;
  --accent: #3b82f6;
  --font: system-ui, "Microsoft YaHei UI", sans-serif;
}

* {
  box-sizing: border-box;
}

body {
  margin: 0;
  font-family: var(--font);
  background: var(--bg);
  color: var(--text);
}

a {
  text-decoration: none;
  color: inherit;
}

.shell {
  display: grid;
  grid-template-columns: 220px 1fr;
  min-height: 100vh;
}

.sidebar {
  background: var(--surface);
  border-right: 1px solid var(--border);
  display: flex;
  flex-direction: column;
  padding: 1.25rem 0.75rem;
  gap: 1.5rem;
}

.brand {
  font-size: 1.25rem;
  font-weight: 700;
  color: var(--accent);
  padding: 0 0.75rem;
}

.nav {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
}

.nav-item {
  padding: 0.55rem 0.75rem;
  border-radius: 6px;
  color: var(--text-2);
  font-size: 0.95rem;
  transition: background 0.15s ease, color 0.15s ease;
}

.nav-item:hover {
  background: var(--bg);
  color: var(--text);
}

.nav-item.router-link-active {
  background: rgba(59, 130, 246, 0.1);
  color: var(--accent);
  font-weight: 600;
}

.foot {
  margin-top: auto;
  padding: 0 0.75rem;
  font-size: 0.75rem;
  color: var(--text-2);
}

.content {
  padding: 1.5rem 2rem;
  min-width: 0;
}
</style>