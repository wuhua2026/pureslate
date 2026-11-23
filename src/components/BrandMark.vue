<script setup lang="ts">
/** 与安装包/exe 同源的品牌标（见 src-tauri/icons/source.svg）：
 *  深石板靛蓝圆角底 + 单一白色对勾，缩小到 16px 仍保持轮廓。 */
withDefaults(defineProps<{ size?: number }>(), { size: 28 });
</script>

<template>
  <svg
    :width="size"
    :height="size"
    viewBox="0 0 1024 1024"
    class="brand-mark"
    aria-hidden="true"
    focusable="false"
  >
    <defs>
      <linearGradient id="ps-slate" x1="0" y1="0" x2="1" y2="1">
        <stop offset="0" stop-color="#22355F" />
        <stop offset=".55" stop-color="#16264A" />
        <stop offset="1" stop-color="#0F1C39" />
      </linearGradient>
      <linearGradient id="ps-sheen" x1="0" y1="0" x2="0.4" y2="1">
        <stop offset="0" stop-color="#FFFFFF" stop-opacity=".07" />
        <stop offset=".55" stop-color="#FFFFFF" stop-opacity="0" />
      </linearGradient>
    </defs>
    <rect width="1024" height="1024" rx="228" fill="url(#ps-slate)" />
    <rect width="1024" height="1024" rx="228" fill="url(#ps-sheen)" />
    <path
      d="M286 522l150 150 306-316"
      fill="none"
      stroke="#FFFFFF"
      stroke-width="124"
      stroke-linecap="round"
      stroke-linejoin="round"
    />
  </svg>
</template>

<style scoped>
.brand-mark {
  display: block;
  flex: none;
  border-radius: 22.3%;
  box-shadow:
    0 2px 6px -2px rgba(16, 28, 52, 0.45),
    0 8px 18px -10px rgba(16, 28, 52, 0.5);
}
</style>
