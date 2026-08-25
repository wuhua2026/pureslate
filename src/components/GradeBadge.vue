<script setup lang="ts">
import { computed } from "vue";
import type { Grade } from "../types/ipc";

const props = defineProps<{ grade: Grade }>();

// 硬性要求：🔴 除颜色外必须同时有图标+文案。
const meta = computed<{ label: string; icon?: string }>(() => {
  switch (props.grade) {
    case "green":
      return { label: "安全" };
    case "yellow":
      return { label: "谨慎" };
    case "red":
      return { label: "危险", icon: "🔒" };
  }
});
</script>

<template>
  <span class="badge" :class="`g-${grade}`">
    <span v-if="meta.icon" class="icon">{{ meta.icon }}</span>
    <span v-else class="dot"></span>
    {{ meta.label }}
  </span>
</template>

<style scoped>
.badge {
  display: inline-flex;
  align-items: center;
  gap: 0.35rem;
  padding: 0.15rem 0.55rem;
  border-radius: 999px;
  font-size: 0.78rem;
  line-height: 1.4;
  border: 1px solid transparent;
  white-space: nowrap;
}
.g-green {
  color: #1f6e3c;
  background: rgba(46, 158, 91, 0.12);
  border-color: rgba(46, 158, 91, 0.35);
}
.g-yellow {
  color: #8a5b06;
  background: rgba(217, 151, 28, 0.14);
  border-color: rgba(217, 151, 28, 0.4);
}
.g-red {
  color: #8f241c;
  background: rgba(196, 56, 46, 0.12);
  border-color: rgba(196, 56, 46, 0.4);
}
.icon {
  font-size: 0.7rem;
}
.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--grade-green);
}
.g-yellow .dot {
  background: var(--grade-yellow);
}
.g-red .dot {
  background: var(--grade-red);
}
</style>