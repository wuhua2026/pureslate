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
import GradeIcon from "../components/GradeIcon.vue";
import { formatBytes } from "./filesModel";
import { moduleLine, previewSummary } from "./crashModel";
import { setSuggestionsEnabled, showSuggestions } from "./suggestModel";

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

/** 隔离区保留期：入口即夹到 7–30 天（后端 `settings_set` 会再夹一次）。 */
function onRetentionChange(e: Event): void {
  if (!settings.value) return;
  const raw = Number((e.target as HTMLInputElement).value);
  const days = Math.min(30, Math.max(7, Number.isFinite(raw) ? Math.round(raw) : 14));
  void save({ ...settings.value, quarantineRetentionDays: days });
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
      <p class="sub">所有联网功能默认关闭，仅在你明确开启后才会访问网络；改动即时保存</p>
    </header>

    <div v-if="!settings" class="empty-list">加载中…</div>
    <div v-else class="sections">
      <!-- 更新 -->
      <section class="card anim-rise d1">
        <header class="card-head">
          <span class="c-ico" aria-hidden="true">
            <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <path d="M20 11a8 8 0 1 0-2.3 6.3" />
              <path d="M20 4v7h-7" />
            </svg>
          </span>
          <div class="c-title">
            <h2>更新</h2>
            <p>默认关闭 · 仅检查版本与规则包</p>
          </div>
        </header>
        <div class="body">
          <label class="opt">
            <input
              class="sw"
              type="checkbox"
              :checked="settings.updateOptIn"
              @change="toggle('updateOptIn')"
            />
            <span class="opt-text">
              <strong>每周自动检查更新</strong>
              <span>仅发送版本请求，无任何标识</span>
            </span>
          </label>
          <label class="opt">
            <input
              class="sw"
              type="checkbox"
              :checked="settings.mirrorFirst"
              @change="toggle('mirrorFirst')"
            />
            <span class="opt-text">
              <strong>国内镜像优先</strong>
              <span>jsDelivr → ghproxy → GitHub 直连</span>
            </span>
          </label>
          <div class="opt-actions">
            <button class="btn" :disabled="checking" @click="checkNow">
              {{ checking ? "检查中…" : "立即检查更新" }}
            </button>
            <span v-if="status" class="status" :class="{ ok: !status.hasUpdate }">
              {{ statusText(status) }}
            </span>
          </div>
          <p v-if="status?.rulesPackHashOk === false" class="warn">
            规则包校验失败已丢弃（不会安装未通过校验的内容）
          </p>
          <p v-if="checkError" class="warn">{{ checkError }}</p>
        </div>
      </section>

      <!-- 专家模式 -->
      <section class="card anim-rise d2">
        <header class="card-head">
          <span class="c-ico warn" aria-hidden="true">
            <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <path d="M12 3 4.5 6v5.5c0 4.4 3.2 8.4 7.5 9.5 4.3-1.1 7.5-5.1 7.5-9.5V6Z" />
              <path d="M12 8v4" />
              <circle cx="12" cy="15.6" r="0.9" fill="currentColor" stroke="none" />
            </svg>
          </span>
          <div class="c-title">
            <h2>专家模式</h2>
            <p>高风险操作的额外解锁</p>
          </div>
        </header>
        <div class="body">
          <label class="opt">
            <input
              class="sw"
              type="checkbox"
              :checked="settings.expertMode"
              @change="toggle('expertMode')"
            />
            <span class="opt-text">
              <strong>
                解锁高风险（<span class="red-ico"><GradeIcon grade="red" :size="12" /></span>）项
              </strong>
              <span>灰禁解除，清理前仍需二次确认令牌</span>
            </span>
          </label>
          <p class="hint">开启不代表自动执行：每一项高风险操作仍需逐次确认。</p>
        </div>
      </section>

      <!-- 清理建议（前端偏好，不入 IPC 契约） -->
      <section class="card anim-rise d3">
        <header class="card-head">
          <span class="c-ico" aria-hidden="true">
            <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <path d="M9 18h6" />
              <path d="M10 21h4" />
              <path d="M12 3a6 6 0 0 0-3.6 10.8c.6.5 1 1.2 1 2V17h5.2v-1.2c0-.8.4-1.5 1-2A6 6 0 0 0 12 3Z" />
            </svg>
          </span>
          <div class="c-title">
            <h2>清理建议</h2>
            <p>解释与建议 · 默认开启</p>
          </div>
        </header>
        <div class="body">
          <label class="opt">
            <input
              class="sw"
              type="checkbox"
              :checked="showSuggestions"
              @change="setSuggestionsEnabled(!showSuggestions)"
            />
            <span class="opt-text">
              <strong>在报告中显示清理建议</strong>
              <span>按分级给每个类目一句建议与依据</span>
            </span>
          </label>
          <p class="hint">
            建议只作解释：不改变勾选范围、方案 A/B 与危险项灰禁；
            关闭后报告页恢复原样（不显示建议徽标与摘要）。
          </p>
        </div>
      </section>

      <!-- 隔离区 -->
      <section class="card anim-rise d3">
        <header class="card-head">
          <span class="c-ico" aria-hidden="true">
            <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <path d="M3 7h18v12a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z" />
              <path d="M3 7V5a2 2 0 0 1 2-2h5l2 2h7a2 2 0 0 1 2 2" />
              <path d="M9.5 12h5" />
            </svg>
          </span>
          <div class="c-title">
            <h2>隔离区</h2>
            <p>保留期与到期处理</p>
          </div>
        </header>
        <div class="body">
          <label class="opt">
            <input
              class="sw"
              type="checkbox"
              :checked="settings.quarantineAutoPurge"
              @change="toggle('quarantineAutoPurge')"
            />
            <span class="opt-text">
              <strong>到期后自动清除</strong>
              <span>关闭后到期文件保留，需在隔离区手动清理</span>
            </span>
          </label>
          <div class="opt static">
            <span class="opt-text">
              <strong>保留期</strong>
              <span>可设 7–30 天，默认 14；到期前 3 天提醒</span>
            </span>
            <span class="days">
              <input
                class="days-input"
                type="number"
                min="7"
                max="30"
                step="1"
                :value="settings.quarantineRetentionDays"
                @change="onRetentionChange"
              />
              <span>天</span>
            </span>
          </div>
          <p class="hint">
            关闭「到期自动清除」不会释放磁盘空间，只是把“到期即删”改为手动确认；
            保留期内可随时在隔离区还原。
          </p>
        </div>
      </section>

      <!-- 崩溃数据 -->
      <section class="card anim-rise d3">
        <header class="card-head">
          <span class="c-ico" aria-hidden="true">
            <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <path d="M8 6V5a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v1" />
              <path d="M6 8h12l1 13H5Z" />
              <path d="M10 12v5M14 12v5" />
            </svg>
          </span>
          <div class="c-title">
            <h2>崩溃数据</h2>
            <p>默认仅本地保存</p>
          </div>
        </header>
        <div class="body">
          <label class="opt">
            <input
              class="sw"
              type="checkbox"
              :checked="settings.crashUploadOptIn"
              @change="toggle('crashUploadOptIn')"
            />
            <span class="opt-text">
              <strong>允许上传崩溃转储帮助修复问题</strong>
              <span>默认关闭；上传前可预览内容</span>
            </span>
          </label>
          <p class="warn">
            上传服务尚未上线：当前版本转储仅保存在本机，开启开关暂无实际联网行为。
          </p>
          <p v-if="dumps.length === 0" class="dump-empty">没有本地崩溃转储</p>
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
        </div>
      </section>
    </div>
  </section>
</template>

<style scoped>
.page {
  max-width: 1080px;
  margin: 0 auto;
}
.head h1 {
  margin: 0;
  font-size: 1.6rem;
  font-weight: 720;
  letter-spacing: -0.02em;
  color: var(--text, #1f2430);
}
.sub {
  margin: 4px 0 18px;
  font-size: 0.85rem;
  color: var(--text-2, #5a6472);
  line-height: 1.65;
}
.empty-list {
  padding: 48px 0;
  text-align: center;
  color: var(--text-2, #5a6472);
}
.sections {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 14px;
  align-items: start;
}
.card {
  padding: 16px 18px;
  background: var(--surface);
  border: 1px solid var(--glass-line);
  border-radius: var(--r-lg);
  box-shadow:
    var(--glass-sheen),
    var(--shadow-1);
  backdrop-filter: var(--glass-blur);
  -webkit-backdrop-filter: var(--glass-blur);
}
.card-head {
  display: flex;
  align-items: center;
  gap: 11px;
  padding-bottom: 12px;
  border-bottom: 1px solid var(--border, #e4e7ec);
}
.c-ico {
  flex: none;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 34px;
  height: 34px;
  border-radius: 10px;
  background: rgba(37, 99, 235, 0.09);
  color: var(--accent);
  box-shadow: inset 0 0 0 1px rgba(37, 99, 235, 0.14);
}
.c-ico.warn {
  background: rgba(217, 151, 28, 0.12);
  color: var(--grade-yellow, #b26a00);
  box-shadow: inset 0 0 0 1px rgba(217, 151, 28, 0.2);
}
.c-title {
  min-width: 0;
}
.c-title h2 {
  margin: 0;
  font-size: 0.96rem;
  font-weight: 700;
  letter-spacing: -0.01em;
  color: var(--text, #1f2430);
}
.c-title p {
  margin: 2px 0 0;
  font-size: 0.74rem;
  color: var(--text-2, #5a6472);
}
.body {
  padding-top: 10px;
}
.opt {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 8px 0;
  cursor: pointer;
}
.opt.static {
  cursor: default;
}
.opt-text {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.opt-text strong {
  font-size: 0.85rem;
  font-weight: 600;
  color: var(--text, #1f2430);
}
.opt-text > span {
  font-size: 0.74rem;
  color: var(--text-2, #5a6472);
  line-height: 1.5;
}
/* 开关：纯 CSS，行为仍是原生 checkbox（change 事件不变） */
.sw {
  appearance: none;
  -webkit-appearance: none;
  position: relative;
  flex: none;
  width: 36px;
  height: 20px;
  margin: 0;
  border-radius: 999px;
  background: rgba(19, 32, 54, 0.16);
  cursor: pointer;
  transition: background var(--dur-fast) var(--ease-inout);
}
.sw::after {
  content: "";
  position: absolute;
  top: 2px;
  left: 2px;
  width: 16px;
  height: 16px;
  border-radius: 50%;
  background: #fff;
  box-shadow: 0 1px 3px rgba(16, 28, 52, 0.3);
  transition: transform var(--dur-fast) var(--ease-out);
}
.sw:checked {
  background: var(--accent);
}
.sw:checked::after {
  transform: translateX(16px);
}
.sw:focus-visible {
  outline: none;
  box-shadow: 0 0 0 4px rgba(37, 99, 235, 0.18);
}
.opt-actions {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 10px;
  padding: 10px 0 2px;
}
.days {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 0.8rem;
  color: var(--text-2, #5a6472);
}
.red-ico {
  display: inline-flex;
  vertical-align: -0.1em;
  color: var(--grade-red, #c4382e);
}
.dump-empty {
  margin: 12px 0 0;
  padding: 16px;
  text-align: center;
  font-size: 0.8rem;
  color: var(--text-2, #5a6472);
  border: 1px dashed var(--border-strong, #d7deea);
  border-radius: var(--r-md);
}
.days-input {
  width: 72px;
  padding: 6px 8px;
  border: 1px solid var(--border-strong, #d7deea);
  border-radius: 8px;
  background: rgba(255, 255, 255, 0.8);
  color: var(--text, #1f2430);
  font-size: 0.85rem;
  font-variant-numeric: tabular-nums;
  outline: none;
  transition:
    border-color var(--dur-fast) var(--ease-inout),
    box-shadow var(--dur-fast) var(--ease-inout);
}
.days-input:focus {
  border-color: rgba(37, 99, 235, 0.6);
  box-shadow: 0 0 0 4px rgba(37, 99, 235, 0.13);
}
.btn {
  padding: 7px 16px;
  border: 1px solid var(--border-strong, #d7deea);
  border-radius: 10px;
  background: rgba(255, 255, 255, 0.75);
  color: var(--text, #1f2430);
  font-size: 0.85rem;
  font-weight: 600;
  cursor: pointer;
  box-shadow: 0 2px 6px -3px rgba(16, 28, 52, 0.2);
  transition:
    transform var(--dur-fast) var(--ease-out),
    border-color var(--dur-fast) var(--ease-inout),
    color var(--dur-fast) var(--ease-inout),
    background var(--dur-fast) var(--ease-inout),
    opacity var(--dur-fast) var(--ease-inout);
}
.btn:hover:not(:disabled) {
  border-color: rgba(37, 99, 235, 0.4);
  color: var(--accent);
  background: #fff;
}
.btn:active {
  transform: scale(0.97);
}
.btn:disabled {
  opacity: 0.45;
  box-shadow: none;
  cursor: default;
}
.status {
  color: var(--text-2, #5a6472);
}
.status.ok {
  color: var(--grade-green, #12a150);
  font-weight: 600;
}
.warn {
  color: var(--grade-yellow, #b26a00);
  font-size: 0.8rem;
  margin: 4px 0 0;
}
.dump-list {
  list-style: none;
  margin: 8px 0 0;
  padding: 0;
}
.dump-row {
  padding: 9px 8px;
  border-top: 1px solid var(--border, #e4e7ec);
  font-size: 0.82rem;
  transition: background var(--dur-fast) var(--ease-inout);
  border-radius: 8px;
}
.dump-row:hover {
  background: rgba(255, 255, 255, 0.6);
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
  font-variant-numeric: tabular-nums;
}
.dump-actions {
  display: flex;
  gap: 6px;
}
.dump-preview {
  margin-top: 8px;
  padding: 9px 11px;
  background: rgba(244, 247, 252, 0.7);
  border: 1px solid var(--border, #e4e7ec);
  border-radius: var(--r-md);
  animation: ps-preview-in var(--dur-base) var(--ease-out);
}
@keyframes ps-preview-in {
  from {
    opacity: 0;
    transform: translateY(-4px);
  }
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
  line-height: 1.6;
}
@media (max-width: 880px) {
  .sections {
    grid-template-columns: minmax(0, 1fr);
  }
}
</style>
