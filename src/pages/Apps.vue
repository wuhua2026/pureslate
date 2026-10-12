<script setup lang="ts">
/**
 * 软件卸载页（R27-1 · SPEC §6.7）。
 * 本阶段只做只读列表；执行（拉起官方卸载器/进程树等待）在 R27-2 落地，
 * 残留清理在 R27-3 接入既有隔离区管线。
 */
import { computed, onMounted, ref } from "vue";
import { uninstall_list } from "../api/commands";
import type { InstalledApp } from "../types/ipc";
import {
  filterApps,
  formatSize,
  installDateLabel,
  scopeLabel,
  summarizeApps,
} from "./appsModel";

const apps = ref<InstalledApp[]>([]);
const loading = ref(true);
const error = ref("");
const query = ref("");

const visible = computed(() => filterApps(apps.value, query.value));
const summary = computed(() => summarizeApps(apps.value));

onMounted(async () => {
  try {
    apps.value = await uninstall_list();
  } catch (e) {
    error.value = String(e);
  } finally {
    loading.value = false;
  }
});
</script>

<template>
  <section class="page">
    <header class="head">
      <h1>软件卸载</h1>
      <p class="sub">
        共 {{ summary.total }} 个程序（本机 {{ summary.machine }} · 当前用户 {{ summary.user }} ·
        MSI {{ summary.msi }}）——卸载将调用程序<strong>自带的卸载器</strong>执行，
        卸载残留由本应用扫描并隔离处理（可还原）。
      </p>
    </header>

    <div v-if="loading" class="state">加载中…</div>
    <div v-else-if="error" class="state err">加载失败：{{ error }}</div>
    <template v-else>
      <div class="toolbar">
        <input v-model="query" class="search" type="search" placeholder="搜索程序名/发布者/版本…" />
        <span class="count">{{ visible.length }} / {{ apps.length }}</span>
      </div>

      <div v-if="visible.length === 0" class="state">没有匹配的程序</div>

      <table v-else class="list">
        <thead>
          <tr>
            <th>名称</th>
            <th>发布者</th>
            <th>版本</th>
            <th>范围</th>
            <th>大小</th>
            <th>安装日期</th>
            <th class="op">操作</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="a in visible" :key="a.id">
            <td class="name">
              {{ a.name }}
              <span v-if="a.isMsi" class="badge msi" title="Windows Installer 产品">MSI</span>
              <span class="badge arch">{{ a.arch }}</span>
            </td>
            <td class="pub">{{ a.publisher ?? "未知发布者" }}</td>
            <td>{{ a.version ?? "—" }}</td>
            <td>{{ scopeLabel(a.scope) }}</td>
            <td>{{ formatSize(a.estimatedSizeBytes) || "—" }}</td>
            <td>{{ installDateLabel(a.installDate) || "—" }}</td>
            <td class="op">
              <button class="btn" disabled title="卸载执行将在下一版本开放（R27-2）">
                卸载
              </button>
            </td>
          </tr>
        </tbody>
      </table>

      <p class="footnote">
        安全说明：本应用不替代系统"应用和功能"；不会强杀进程，不处理 UWP 应用与系统组件。
      </p>
    </template>
  </section>
</template>

<style scoped>
.page {
  padding: 20px 24px;
  max-width: 1000px;
  margin: 0 auto;
}
.head h1 {
  margin: 0;
  font-size: 1.35rem;
}
.sub {
  margin: 4px 0 16px;
  font-size: 0.85rem;
  color: var(--text-2, #5a6472);
}
.state {
  padding: 32px 0;
  text-align: center;
  color: var(--text-2, #5a6472);
}
.state.err {
  color: var(--grade-red, #c4382e);
}
.toolbar {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-bottom: 12px;
}
.search {
  flex: 1;
  padding: 8px 12px;
  border: 1px solid var(--border, #e4e7ec);
  border-radius: 8px;
  font-size: 0.9rem;
  background: var(--surface, #fff);
  color: var(--text, #1f2430);
}
.count {
  font-size: 0.8rem;
  color: var(--text-2, #5a6472);
  white-space: nowrap;
}
.list {
  width: 100%;
  border-collapse: collapse;
  background: var(--surface, #fff);
  border: 1px solid var(--border, #e4e7ec);
  border-radius: 10px;
  overflow: hidden;
  font-size: 0.85rem;
}
.list th,
.list td {
  padding: 9px 12px;
  text-align: left;
  border-bottom: 1px solid var(--border, #e4e7ec);
}
.list th {
  background: #f6f8fb;
  color: var(--text-2, #5a6472);
  font-weight: 600;
  font-size: 0.78rem;
}
.list tbody tr:last-child td {
  border-bottom: none;
}
.name {
  font-weight: 600;
}
.pub {
  color: var(--text-2, #5a6472);
  max-width: 180px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.badge {
  display: inline-block;
  margin-left: 6px;
  padding: 0 6px;
  border-radius: 8px;
  font-size: 0.68rem;
  font-weight: 600;
  vertical-align: middle;
}
.badge.msi {
  background: #eef2ff;
  color: #1a56db;
}
.badge.arch {
  background: #f0f3f7;
  color: var(--text-2, #5a6472);
}
.op {
  text-align: right;
  white-space: nowrap;
}
.btn {
  padding: 5px 16px;
  border: 1px solid var(--border, #e4e7ec);
  border-radius: 8px;
  background: var(--surface, #fff);
  color: var(--text-2, #5a6472);
  font-size: 0.82rem;
  cursor: not-allowed;
  opacity: 0.55;
}
.footnote {
  margin-top: 12px;
  font-size: 0.75rem;
  color: var(--text-2, #5a6472);
}
</style>
