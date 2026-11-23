<script setup lang="ts">
import type { Grade } from "../types/ipc";

/** 分级图标：形状编码（圆/三角/八边形），不只靠颜色区分。
 *  与 GradeBadge 配套使用；16px 下仍保持轮廓可辨。 */
withDefaults(defineProps<{ grade: Grade; size?: number }>(), { size: 16 });
</script>

<template>
  <svg
    class="grade-icon"
    :width="size"
    :height="size"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="2.1"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
    focusable="false"
  >
    <template v-if="grade === 'green'">
      <circle cx="12" cy="12" r="9" />
      <path d="m8.2 12.4 2.6 2.6 5-5.3" />
    </template>
    <template v-else-if="grade === 'yellow'">
      <path d="M12 3.6 21 19a1.4 1.4 0 0 1-1.2 2H4.2A1.4 1.4 0 0 1 3 19Z" />
      <path d="M12 9.6v4.1" />
      <circle cx="12" cy="17.2" r=".9" fill="currentColor" stroke="none" />
    </template>
    <template v-else>
      <path d="M8.1 2.9h7.8l5.2 5.2v7.8l-5.2 5.2H8.1L2.9 16V8.1Z" />
      <path d="m9.2 9.2 5.6 5.6M14.8 9.2l-5.6 5.6" />
    </template>
  </svg>
</template>

<style scoped>
.grade-icon {
  display: inline-block;
  flex: none;
  vertical-align: -0.16em;
}
</style>
