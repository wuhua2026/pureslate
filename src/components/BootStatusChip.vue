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
  padding: 0.12rem 0.55rem;
  border-radius: 999px;
  font-size: 0.75rem;
  font-weight: 600;
  line-height: 1.5;
  border: 1px solid transparent;
  white-space: nowrap;
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.55);
}
.ok {
  color: #14663a;
  background: rgba(18, 161, 80, 0.12);
  border-color: rgba(18, 161, 80, 0.3);
}
.warn {
  color: #7a4a00;
  background: rgba(178, 106, 0, 0.13);
  border-color: rgba(178, 106, 0, 0.32);
}
.muted {
  color: var(--text-3, #8a97aa);
  background: rgba(244, 247, 252, 0.8);
  border-color: var(--border, #e7ebf2);
}
.denied {
  color: #9a2019;
  background: rgba(214, 58, 47, 0.11);
  border-color: rgba(214, 58, 47, 0.32);
}
</style>
