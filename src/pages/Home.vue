<script setup lang="ts">
import { onMounted, ref } from "vue";
import { commands } from "../api";
import type { AppMeta } from "../types/ipc";

const meta = ref<AppMeta | null>(null);
const error = ref("");

onMounted(async () => {
  try {
    meta.value = await commands.app_meta();
  } catch (e) {
    error.value = String(e);
  }
});
</script>

<template>
  <main class="home">
    <h1>PureSlate</h1>
    <p v-if="meta" class="meta">
      v{{ meta.version }} · 规则 {{ meta.rulesVersion }} · {{ meta.channel }}
    </p>
    <p v-if="error" class="err">app_meta 调用失败: {{ error }}</p>
    <p class="hint">Phase 0 地基已就绪。UI 将在后续 Phase 构建。</p>
  </main>
</template>

<style scoped>
.home {
  padding: 4rem 2rem;
  font-family: system-ui, "Microsoft YaHei UI", sans-serif;
  color: var(--text, #1f2430);
  background: var(--bg, #fafafa);
  min-height: 100vh;
}
h1 {
  color: var(--accent, #3b82f6);
}
.meta {
  color: var(--text-2, #5a6472);
}
.err {
  color: var(--grade-red, #c4382e);
}
</style>