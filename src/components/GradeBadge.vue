<script setup lang="ts">
import { computed } from "vue";
import type { Grade } from "../types/ipc";
import GradeIcon from "./GradeIcon.vue";

const props = defineProps<{ grade: Grade }>();

// 硬性要求：🔴 除颜色外必须同时有图标+文案（图标为形状编码，不只靠色相）。
const meta = computed<{ label: string }>(() => {
  switch (props.grade) {
    case "green":
      return { label: "安全" };
    case "yellow":
      return { label: "谨慎" };
    case "red":
      return { label: "危险" };
  }
});
</script>

<template>
  <span class="badge" :class="`g-${grade}`">
    <GradeIcon :grade="grade" :size="14" />
    {{ meta.label }}
  </span>
</template>

<style scoped>
.badge {
  display: inline-flex;
  align-items: center;
  gap: 0.3rem;
  padding: 0.16rem 0.6rem 0.16rem 0.5rem;
  border-radius: 999px;
  font-size: 0.76rem;
  font-weight: 650;
  letter-spacing: 0.01em;
  line-height: 1.45;
  border: 1px solid transparent;
  white-space: nowrap;
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.55);
}
.g-green {
  color: #0e7f43;
  background: rgba(14, 127, 67, 0.11);
  border-color: rgba(14, 127, 67, 0.3);
}
.g-yellow {
  color: #8a5200;
  background: rgba(178, 106, 0, 0.12);
  border-color: rgba(178, 106, 0, 0.32);
}
.g-red {
  color: #b3271d;
  background: rgba(214, 58, 47, 0.1);
  border-color: rgba(214, 58, 47, 0.32);
}
</style>
