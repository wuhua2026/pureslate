<script setup lang="ts">
</script>

<template>
  <router-view />
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
</style>