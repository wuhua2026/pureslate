import { createRouter, createWebHashHistory, type RouteRecordRaw } from "vue-router";

// SPEC §7 页面集合。除 Home 外均由后续 Phase 逐个构建，
// 未构建页面先落到 Placeholder（显示占位，保持外壳导航可用）。
const Placeholder = () => import("../pages/Placeholder.vue");

const routes: RouteRecordRaw[] = [
  { path: "/", name: "home", component: () => import("../pages/Home.vue"), meta: { title: "首页" } },
  { path: "/scan", name: "scan", component: () => import("../pages/Scan.vue"), meta: { title: "一键体检" } },
  { path: "/report", name: "report", component: () => import("../pages/Report.vue"), meta: { title: "扫描报告" } },
  { path: "/executing", name: "executing", component: () => import("../pages/Executing.vue"), meta: { title: "清理执行" } },
  { path: "/files", name: "files", component: Placeholder, meta: { title: "大文件" } },
  { path: "/startup", name: "startup", component: Placeholder, meta: { title: "启动项" } },
  { path: "/privacy", name: "privacy", component: Placeholder, meta: { title: "隐私资料" } },
  { path: "/quarantine", name: "quarantine", component: Placeholder, meta: { title: "隔离区" } },
  { path: "/log", name: "log", component: () => import("../pages/Log.vue"), meta: { title: "操作日志" } },
  { path: "/settings", name: "settings", component: Placeholder, meta: { title: "设置" } },
];

export const router = createRouter({
  history: createWebHashHistory(),
  routes,
});