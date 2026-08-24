/**
 * invoke / listen 底层桥接。依赖 '@tauri-apps/api/core'。
 * 在纯浏览器（typecheck / vitest / mock 模式）下用适配层兜底，避免编译期依赖失效。
 */
import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen as tauriListen } from "@tauri-apps/api/event";
import { isMockMode } from "./mode";

/**
 * 调用 Tauri 命令。mock 模式下由上层（commands.ts）拦截，此函数仅承载真实桥接。
 */
export function invokeApp<T>(cmd: string, args?: object): Promise<T> {
  return tauriInvoke(cmd, args as never);
}

/**
 * 订阅 Tauri 事件。
 */
export function listenApp<T>(event: string, handler: (payload: T) => void): Promise<() => void> {
  return tauriListen<T>(event, (evt) => handler(evt.payload));
}

/** 暴露判断位，供 mock 分支使用，避免循环依赖。 */
export function __mockEnabled(): boolean {
  return isMockMode();
}