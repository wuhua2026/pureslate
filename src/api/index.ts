/**
 * IPC invoke 薄封装。
 * 组件禁止直接 import @tauri-apps/api/core，一律经由本模块。
 * typecheck 时无法确定运行环境，故 keep builds at runtime via platform detection。
 */
export { invokeApp, listenApp } from "./client";
export { isMockMode } from "./mode";

// 业务命令统一从 commands.ts 导入
export * as commands from "./commands";
export * as events from "./events";