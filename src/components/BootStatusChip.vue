<script setup lang="ts">
import type { BootSectionStatus } from "../types/ipc";
import { statusLabel, statusTone } from "../pages/bootdiagModel";

defineProps<{ status: BootSectionStatus }>();
</script>

<template>
  <span class="chip" :class="statusTone(status)">{{ statusLabel(status) }}</span>
</template>

<style scoped>
.chip {
  display: inline-flex;
  align-items: center;
  padding: 0.1rem 0.5rem;
  border-radius: 999px;
  font-size: 0.75rem;
  line-height: 1.5;
  border: 1px solid transparent;
  white-space: nowrap;
}
.ok {
  color: #1f6e3c;
  background: rgba(22, 163, 74, 0.1);
  border-color: rgba(22, 163, 74, 0.28);
}
.warn {
  color: #8a5b06;
  background: rgba(217, 151, 28, 0.12);
  border-color: rgba(217, 151, 28, 0.35);
}
.muted {
  color: var(--text-3, #8a97aa);
  background: var(--surface-2, #f8fafc);
  border-color: var(--border, #e7ebf2);
}
.denied {
  color: #8f241c;
  background: rgba(220, 59, 48, 0.1);
  border-color: rgba(220, 59, 48, 0.3);
}
</style>
