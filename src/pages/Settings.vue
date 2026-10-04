<script setup lang="ts">
import { onMounted, ref } from "vue";
import {
  crash_list,
  crash_preview,
  crash_upload,
  settings_get,
  settings_set,
  update_check,
} from "../api/commands";
import type { AppSettings, CrashDumpInfo, CrashDumpPreview, UpdateStatus } from "../types/ipc";
import { formatBytes } from "./filesModel";
import { moduleLine, previewSummary } from "./crashModel";

/** 设置页（P4-01 起步）：R22 更新（optIn 周查/镜像优先/手动检查）+ 专家模式。
 *  P4-03 扩展：R24 崩溃数据（opt-in 上传 + 本地转储预览）。 */
const settings = ref<AppSettings | null>(null);
const checking = ref(false);
const status = ref<UpdateStatus | null>(null);
const checkError = ref("");

// 崩溃转储（P4-03）：列表 + 展开的预览 + 上传状态。
const dumps = ref<CrashDumpInfo[]>([]);
const previews = ref<Record<string, CrashDumpPreview>>({});
const openName = ref<string | null>(null);
const uploading = ref<string | null>(null);
const uploadMsgs = ref<Record<string, { text: string; ok: boolean }>>({});

async function load(): Promise<void> {
  settings.value = await settings_get();
  try {
    dumps.value = await crash_list();
  } catch {
    dumps.value = [];
  }
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

async function togglePreview(name: string): Promise<void> {
  if (openName.value === name) {
    openName.value = null;
    return;
  }
  openName.value = name;
  try {
    previews.value[name] = await crash_preview({ fileName: name });
  } catch (e) {
    previews.value[name] = {
      fileName: name,
      sizeBytes: 0,
      ts: 0,
      error: String(e),
    };
  }
}

async function upload(name: string): Promise<void> {
  if (uploading.value) return;
  uploading.value = name;
  try {
    const r = await crash_upload({ fileName: name });
    uploadMsgs.value[name] = r.uploaded
      ? { text: "已上传，感谢反馈", ok: true }
      : { text: r.detail ?? "上传失败", ok: false };
  } catch (e) {
    uploadMsgs.value[name] = { text: String(e), ok: false };
  } finally {
    uploading.value = null;
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

      <section class="card">
        <h2>崩溃数据（默认仅本地保存）</h2>
        <label class="row">
          <input
            type="checkbox"
            :checked="settings.crashUploadOptIn"
            @change="toggle('crashUploadOptIn')"
          />
          <span>允许上传崩溃转储帮助修复问题（默认关闭；上传前可预览内容）</span>
        </label>
        <p class="warn">
          上传服务尚未上线：当前版本转储仅保存在本机，开启开关暂无实际联网行为。
        </p>
        <p v-if="dumps.length === 0" class="empty-list">没有本地崩溃转储</p>
        <ul v-else class="dump-list">
          <li v-for="d in dumps" :key="d.fileName" class="dump-row">
            <div class="dump-head">
              <span class="dump-name">{{ d.fileName }}</span>
              <span class="dump-size">{{ formatBytes(d.sizeBytes) }}</span>
              <span class="dump-actions">
                <button class="btn" @click="togglePreview(d.fileName)">
                  {{ openName === d.fileName ? "收起" : "预览" }}
                </button>
                <button
                  class="btn"
                  :disabled="!settings.crashUploadOptIn || uploading === d.fileName"
                  @click="upload(d.fileName)"
                >
                  {{ uploading === d.fileName ? "上传中…" : "上传" }}
                </button>
              </span>
            </div>
            <div v-if="openName === d.fileName" class="dump-preview">
              <template v-if="previews[d.fileName]">
                <p class="preview-line">
                  {{ previewSummary(previews[d.fileName]) }}
                </p>
                <code class="mod-list">{{ moduleLine(previews[d.fileName]) }}</code>
              </template>
              <p v-else class="preview-line">解析中…</p>
            </div>
            <p
              v-if="uploadMsgs[d.fileName]"
              class="upload-msg"
              :class="{ ok: uploadMsgs[d.fileName].ok }"
            >
              {{ uploadMsgs[d.fileName].text }}
            </p>
          </li>
        </ul>
        <p class="hint">
          崩溃转储仅在本程序意外退出时写入本机，包含加载模块列表与异常代码摘要，
          不自动上传；本地最多保留 5 份。
        </p>
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
.dump-list {
  list-style: none;
  margin: 8px 0 0;
  padding: 0;
}
.dump-row {
  padding: 8px 0;
  border-top: 1px solid var(--border, #e4e7ec);
  font-size: 0.82rem;
}
.dump-head {
  display: flex;
  align-items: center;
  gap: 10px;
}
.dump-name {
  flex: 1;
  color: var(--text, #1f2430);
  word-break: break-all;
}
.dump-size {
  color: var(--text-2, #5a6472);
  white-space: nowrap;
}
.dump-actions {
  display: flex;
  gap: 6px;
}
.dump-preview {
  margin-top: 6px;
  padding: 8px 10px;
  background: var(--bg, #fafafa);
  border: 1px solid var(--border, #e4e7ec);
  border-radius: 8px;
}
.preview-line {
  margin: 0 0 4px;
  color: var(--text, #1f2430);
}
.mod-list {
  display: block;
  font-size: 0.75rem;
  color: var(--text-2, #5a6472);
  word-break: break-all;
}
.upload-msg {
  margin: 4px 0 0;
  color: var(--grade-yellow, #d9971c);
  font-size: 0.8rem;
}
.upload-msg.ok {
  color: var(--grade-green, #2e9e5b);
}
.hint {
  margin: 10px 0 0;
  font-size: 0.75rem;
  color: var(--text-2, #5a6472);
}
</style>
