<script setup lang="ts">
import { onMounted, ref } from "vue";
import { settings_get, settings_set, update_check } from "../api/commands";
import type { AppSettings, UpdateStatus } from "../types/ipc";

/** 设置页（P4-01 起步）：R22 更新（optIn 周查/镜像优先/手动检查）+ 专家模式。
 *  后续任务按需扩展（隔离保留期等）。 */
const settings = ref<AppSettings | null>(null);
const checking = ref(false);
const status = ref<UpdateStatus | null>(null);
const checkError = ref("");

async function load(): Promise<void> {
  settings.value = await settings_get();
}

async function save(next: AppSettings): Promise<void> {
  settings.value = await settings_set(next);
}

function toggle<K extends keyof AppSettings>(key: K): void {
  if (!settings.value) return;
  void save({ ...settings.value, [key]: !settings.value[key] });
}

async function checkNow(): Promise<void> {
  if (checking.value) return;
  checking.value = true;
  checkError.value = "";
  try {
    status.value = await update_check(true);
  } catch (e) {
    checkError.value = String(e);
  } finally {
    checking.value = false;
  }
}

function statusText(s: UpdateStatus): string {
  if (s.hasUpdate) {
    return `发现新版本 ${s.latestVersion ?? ""}（当前 ${s.currentVersion}）`;
  }
  if (s.checkedAt === 0) return "未检查";
  return "已是最新版本";
}

onMounted(() => {
  void load();
});
</script>

<template>
  <section class="page">
    <header class="head">
      <h1>设置</h1>
      <p class="sub">所有联网功能默认关闭，仅在你明确开启后才会访问网络</p>
    </header>

    <div v-if="!settings" class="empty-list">加载中…</div>
    <template v-else>
      <section class="card">
        <h2>更新（默认关闭 · 仅检查版本与规则包）</h2>
        <label class="row">
          <input
            type="checkbox"
            :checked="settings.updateOptIn"
            @change="toggle('updateOptIn')"
          />
          <span>每周自动检查更新（仅发送版本请求，无任何标识）</span>
        </label>
        <label class="row">
          <input
            type="checkbox"
            :checked="settings.mirrorFirst"
            @change="toggle('mirrorFirst')"
          />
          <span>国内镜像优先（jsDelivr → ghproxy → GitHub 直连）</span>
        </label>
        <div class="row actions">
          <button class="btn" :disabled="checking" @click="checkNow">
            {{ checking ? "检查中…" : "立即检查更新" }}
          </button>
          <span v-if="status" class="status" :class="{ ok: !status.hasUpdate }">
            {{ statusText(status) }}
          </span>
          <span v-if="status?.rulesPackHashOk === false" class="warn">
            规则包校验失败已丢弃（不会安装未通过校验的内容）
          </span>
        </div>
        <p v-if="checkError" class="warn">{{ checkError }}</p>
      </section>

      <section class="card">
        <h2>专家模式</h2>
        <label class="row">
          <input
            type="checkbox"
            :checked="settings.expertMode"
            @change="toggle('expertMode')"
          />
          <span>解锁高风险（🔴）项：灰禁解除，清理前仍需二次确认令牌</span>
        </label>
      </section>
    </template>
  </section>
</template>

<style scoped>
.page {
  padding: 20px 24px;
  max-width: 760px;
  margin: 0 auto;
}
.head h1 {
  margin: 0;
  font-size: 1.35rem;
  color: var(--text, #1f2430);
}
.sub {
  margin: 4px 0 16px;
  font-size: 0.85rem;
  color: var(--text-2, #5a6472);
}
.empty-list {
  padding: 32px 0;
  text-align: center;
  color: var(--text-2, #5a6472);
}
.card {
  margin-bottom: 16px;
  padding: 16px;
  background: var(--surface, #fff);
  border: 1px solid var(--border, #e4e7ec);
  border-radius: 10px;
}
.card h2 {
  margin: 0 0 12px;
  font-size: 0.95rem;
  color: var(--text, #1f2430);
}
.row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 6px 0;
  font-size: 0.85rem;
  color: var(--text, #1f2430);
  cursor: pointer;
}
.row.actions {
  cursor: default;
}
.btn {
  padding: 6px 18px;
  border: 1px solid var(--border, #e4e7ec);
  border-radius: 8px;
  background: var(--surface, #fff);
  color: var(--text, #1f2430);
  font-size: 0.85rem;
  cursor: pointer;
}
.btn:disabled {
  opacity: 0.45;
  cursor: default;
}
.status {
  color: var(--text-2, #5a6472);
}
.status.ok {
  color: var(--grade-green, #2e9e5b);
}
.warn {
  color: var(--grade-yellow, #d9971c);
  font-size: 0.8rem;
  margin: 4px 0 0;
}
</style>
