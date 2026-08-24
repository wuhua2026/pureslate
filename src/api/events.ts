/**
 * Tauri 事件订阅封装。
 */
import { IPCEvents } from "../types/ipc";
import type {
  CleanProgressEvent,
  QuarantineExpiryWarningEvent,
  ScanProgressEvent,
  ScanResult,
  UpdateStatus,
} from "../types/ipc";
import { listenApp } from "./client";

export function onScanProgress(handler: (p: ScanProgressEvent) => void): Promise<() => void> {
  return listenApp<ScanProgressEvent>(IPCEvents.scanProgress, handler);
}

export function onScanDone(handler: (p: ScanResult) => void): Promise<() => void> {
  return listenApp<ScanResult>(IPCEvents.scanDone, handler);
}

export function onCleanProgress(handler: (p: CleanProgressEvent) => void): Promise<() => void> {
  return listenApp<CleanProgressEvent>(IPCEvents.cleanProgress, handler);
}

export function onQuarantineExpiryWarning(
  handler: (p: QuarantineExpiryWarningEvent) => void,
): Promise<() => void> {
  return listenApp<QuarantineExpiryWarningEvent>(IPCEvents.quarantineExpiryWarning, handler);
}

export function onUpdateAvailable(handler: (p: UpdateStatus) => void): Promise<() => void> {
  return listenApp<UpdateStatus>(IPCEvents.updateAvailable, handler);
}