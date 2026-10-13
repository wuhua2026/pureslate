<!--
  自绘几何图标（24×24 描边、currentColor）。
  不使用第三方图标包（净室 + 零依赖 + 包体 <20MB 红线）；
  全部为基础几何路径，非任何图标集的拷贝。
-->
<script setup lang="ts">
const props = withDefaults(
  defineProps<{
    name:
      | "home"
      | "scan"
      | "report"
      | "files"
      | "startup"
      | "privacy"
      | "apps"
      | "quarantine"
      | "log"
      | "settings";
    size?: number;
  }>(),
  { size: 20 },
);
</script>

<template>
  <svg
    :width="props.size"
    :height="props.size"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="1.8"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
  >
    <!-- 首页：房子 -->
    <g v-if="name === 'home'">
      <path d="M3.5 10.5 12 4l8.5 6.5" />
      <path d="M5.5 9.5V20h13V9.5" />
      <path d="M10 20v-5h4v5" />
    </g>
    <!-- 体检：盾牌勾 -->
    <g v-else-if="name === 'scan'">
      <path d="M12 3.5 19 6v5.2c0 4.6-3.1 7.6-7 9.3-3.9-1.7-7-4.7-7-9.3V6Z" />
      <path d="m8.8 12 2.2 2.2 4.2-4.4" />
    </g>
    <!-- 报告：文档+横线 -->
    <g v-else-if="name === 'report'">
      <rect x="4.5" y="3.5" width="15" height="17" rx="2.2" />
      <path d="M8 8.5h8M8 12h8M8 15.5h5" />
    </g>
    <!-- 大文件：文件夹 -->
    <g v-else-if="name === 'files'">
      <path d="M3.5 7.5A1.5 1.5 0 0 1 5 6h3.6l1.9 1.9H19a1.5 1.5 0 0 1 1.5 1.5V18a1.5 1.5 0 0 1-1.5 1.5H5A1.5 1.5 0 0 1 3.5 18Z" />
    </g>
    <!-- 启动项：电源 -->
    <g v-else-if="name === 'startup'">
      <path d="M12 4v7" />
      <path d="M7.2 7.2a7 7 0 1 0 9.6 0" />
    </g>
    <!-- 隐私：挂锁 -->
    <g v-else-if="name === 'privacy'">
      <rect x="5.5" y="10.5" width="13" height="9.5" rx="2" />
      <path d="M8.5 10.5V8a3.5 3.5 0 0 1 7 0v2.5" />
      <circle cx="12" cy="15.2" r="1.2" />
    </g>
    <!-- 软件卸载：四宫格 -->
    <g v-else-if="name === 'apps'">
      <rect x="4.5" y="4.5" width="6" height="6" rx="1.4" />
      <rect x="13.5" y="4.5" width="6" height="6" rx="1.4" />
      <rect x="4.5" y="13.5" width="6" height="6" rx="1.4" />
      <rect x="13.5" y="13.5" width="6" height="6" rx="1.4" />
    </g>
    <!-- 隔离区：归档盒 -->
    <g v-else-if="name === 'quarantine'">
      <path d="M3.5 8 12 4l8.5 4-8.5 4Z" />
      <path d="M3.5 8v8L12 20l8.5-4V8" />
      <path d="M12 12v8" />
    </g>
    <!-- 日志：列表 -->
    <g v-else-if="name === 'log'">
      <path d="M6 6h12M6 12h12M6 18h9" />
    </g>
    <!-- 设置：齿轮（中心圆+辐条） -->
    <g v-else-if="name === 'settings'">
      <circle cx="12" cy="12" r="3.1" />
      <path
        d="M12 2.8v3M12 18.2v3M2.8 12h3M18.2 12h3M5.2 5.2l2.1 2.1M16.7 16.7l2.1 2.1M18.8 5.2l-2.1 2.1M7.3 16.7l-2.1 2.1"
      />
    </g>
  </svg>
</template>
