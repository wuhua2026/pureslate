<script setup lang="ts">
import { ref, watch } from "vue";
import { generateToken, verifyToken } from "./confirmToken";

// 🔴 二次确认（SAFETY §8）：展示一次性 token，须用户逐字输入一致才放行清理。
const props = defineProps<{
  modelValue: boolean;
  /** 是否因含 🔴 项触发（true 时文案强调风险）。 */
  hasRed?: boolean;
}>();

const emit = defineEmits<{
  (e: "update:modelValue", v: boolean): void;
  (e: "confirm", token: string): void;
}>();

const displayToken = ref("");
const inputToken = ref("");
// 已输错次数（仅提示，不锁定）。
const mismatched = ref(false);

// 每次打开生成新 token 并清空输入。
watch(
  () => props.modelValue,
  (open) => {
    if (!open) return;
    displayToken.value = generateToken();
    inputToken.value = "";
    mismatched.value = false;
  },
);

const canConfirm = ref(false);

function onInput(v: string) {
  inputToken.value = v;
  canConfirm.value = verifyToken(v, displayToken.value);
  mismatched.value = v.length > 0 && !canConfirm.value;
}

function doConfirm() {
  if (!canConfirm.value) return;
  emit("confirm", inputToken.value.trim().toUpperCase());
}

function close() {
  emit("update:modelValue", false);
}
</script>

<template>
  <teleport to="body">
    <div v-if="modelValue" class="mask" @click.self="close">
      <div class="modal" role="dialog" aria-modal="true" aria-label="二次确认">
        <h2 class="m-title">🔴 二次确认</h2>
        <p class="m-desc" :class="{ red: hasRed }">
          {{ hasRed ? "本次清理包含高风险（🔴）项。请核对下方一次性令牌并逐字输入以确认。" : "请核对下方一次性令牌并逐字输入以确认。" }}
        </p>

        <div class="token-box">
          <span class="tok">{{ displayToken }}</span>
          <button class="gen" @click="displayToken = generateToken(); inputToken = ''; canConfirm = false; mismatched = false">
            换一个
          </button>
        </div>

        <input
          :value="inputToken"
          class="pin"
          :class="{ mismatch: mismatched }"
          :placeholder="hasRed ? '输入上方令牌确认清理' : '输入上方令牌确认'"
          autocomplete="one-time-code"
          spellcheck="false"
          @input="onInput(($event.target as HTMLInputElement).value)"
        />
        <p v-if="mismatched" class="hint mismatch">
          令牌不一致，请重新核对输入。
        </p>

        <div class="row">
          <button class="btn ghost" @click="close">取消</button>
          <button class="btn primary" :disabled="!canConfirm" @click="doConfirm">确认清理</button>
        </div>
      </div>
    </div>
  </teleport>
</template>

<style scoped>
.mask {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.45);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 50;
}
.modal {
  width: min(420px, 90vw);
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 12px;
  padding: 1.5rem;
  display: flex;
  flex-direction: column;
  gap: 1rem;
  box-shadow: 0 12px 40px rgba(0, 0, 0, 0.2);
}
.m-title {
  margin: 0;
  font-size: 1.15rem;
}
.m-desc {
  margin: 0;
  font-size: 0.85rem;
  color: var(--text-2);
  line-height: 1.5;
}
.m-desc.red {
  color: var(--grade-red);
}
.token-box {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.75rem;
  background: var(--bg);
  border: 1px dashed var(--border);
  border-radius: 8px;
  padding: 0.75rem 1rem;
}
.tok {
  font-family: ui-monospace, monospace;
  font-size: 1.1rem;
  font-weight: 700;
  letter-spacing: 1px;
  user-select: all;
}
.gen {
  background: none;
  border: 1px solid var(--border);
  color: var(--text-2);
  border-radius: 5px;
  padding: 0.25rem 0.6rem;
  font-size: 0.75rem;
  cursor: pointer;
}
.pin {
  width: 100%;
  box-sizing: border-box;
  font-family: ui-monospace, monospace;
  font-size: 1rem;
  letter-spacing: 2px;
  text-transform: uppercase;
  padding: 0.6rem 0.9rem;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--bg);
  color: var(--text);
  outline: none;
}
.pin:focus {
  border-color: var(--accent);
}
.pin.mismatch {
  border-color: var(--grade-red);
}
.hint {
  margin: 0;
  font-size: 0.78rem;
  color: var(--text-2);
}
.hint.mismatch {
  color: var(--grade-red);
}
.row {
  display: flex;
  justify-content: flex-end;
  gap: 0.75rem;
}
.btn {
  border: none;
  border-radius: 8px;
  padding: 0.6rem 1.25rem;
  font-weight: 600;
  cursor: pointer;
}
.btn.ghost {
  background: none;
  border: 1px solid var(--border);
  color: var(--text);
}
.btn.primary {
  background: var(--grade-red);
  color: #fff;
}
.btn.primary:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}
</style>