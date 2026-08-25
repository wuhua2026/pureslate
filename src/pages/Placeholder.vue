<script setup lang="ts">
import { useRoute } from "vue-router";

const route = useRoute();
const title = (route.meta.title as string) ?? "页面";
</script>

<template>
  <main class="placeholder">
    <h2>{{ title }}</h2>
    <p class="desc">
      该页面将在后续 Phase 构建（报告页 P1-07 / 真数据联调 P1-08 / 后续清理链路）。
      当前为外壳占位，导航与整体结构已可用。
    </p>
  </main>
</template>

<style scoped>
.placeholder {
  padding: 2.5rem 2rem;
  color: var(--text);
  font-family: var(--font);
}
h2 {
  margin: 0 0 0.75rem;
  font-size: 1.5rem;
  color: var(--text);
}
.desc {
  margin: 0;
  color: var(--text-2);
  line-height: 1.7;
  max-width: 46rem;
}
</style>