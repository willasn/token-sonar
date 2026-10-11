/* TokenSonar · 统一后端调用层
 *
 * 这是 React 与 Tauri 之间唯一的接缝：视图只从这里取数，不直接碰 window.__TAURI__。
 * 运行在 Tauri 容器里走真实命令；直接用浏览器打开（vite dev / 静态预览）走 Mock。
 */
import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen as tauriListen } from "@tauri-apps/api/event";
import { mockInvoke } from "./mock";
import type {
  AppInfo,
  AppSettings,
  LedgerEntry,
  ProviderConfig,
  RemoteBackup,
  SyncConfig,
  SyncResult,
  UsageSnapshot,
} from "../types";

export function isTauri(): boolean {
  return !!(window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
}

/** 原始命令调用：Tauri 容器走真实 IPC，否则回落到演示 Mock */
export async function invoke<T = unknown>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauri()) return tauriInvoke<T>(cmd, args);
  return mockInvoke(cmd, args || {}) as Promise<T>;
}

/** 订阅后端推送事件；演示模式下没有事件通道，返回 null，调用方需容忍 */
export async function listen<T = unknown>(
  event: string,
  handler: (payload: T) => void
): Promise<(() => void) | null> {
  if (!isTauri()) return null;
  try {
    return await tauriListen<T>(event, (e) => handler(e.payload));
  } catch {
    return null;
  }
}

/** 用系统默认浏览器打开外部链接（演示模式退化为新标签页） */
export async function openExternal(url: string): Promise<void> {
  const target = String(url || "").trim();
  if (!target) return;
  if (isTauri()) {
    await tauriInvoke("open_external_url", { url: target });
    return;
  }
  window.open(target, "_blank", "noopener");
}

/* ============================ 类型化命令封装 ============================ */

export const api = {
  listProviders: () => invoke<ProviderConfig[]>("list_providers"),
  saveProvider: (provider: ProviderConfig) => invoke<ProviderConfig>("save_provider", { provider }),
  removeProvider: (id: string) => invoke<null>("remove_provider", { id }),
  reorderProviders: (ids: string[]) => invoke<ProviderConfig[]>("reorder_providers", { ids }),
  testProvider: (provider: ProviderConfig) => invoke<string>("test_provider", { provider }),
  refreshProvider: (id: string) => invoke<UsageSnapshot>("refresh_provider", { id }),
  refreshAll: () => invoke<UsageSnapshot[]>("refresh_all"),
  getSnapshots: () => invoke<UsageSnapshot[]>("get_snapshots"),

  listLedger: (providerId: string | null = null) =>
    invoke<LedgerEntry[]>("list_ledger", { providerId }),
  addLedgerEntry: (entry: LedgerEntry) => invoke<LedgerEntry>("add_ledger_entry", { entry }),
  removeLedgerEntry: (id: string) => invoke<null>("remove_ledger_entry", { id }),

  getSettings: () => invoke<AppSettings>("get_settings"),
  saveSettings: (settings: AppSettings) => invoke<null>("save_settings", { settings }),
  getAppInfo: () => invoke<AppInfo>("get_app_info"),
  openDataDir: () => invoke<null>("open_data_dir"),

  getSyncConfig: () => invoke<SyncConfig>("get_sync_config"),
  saveSyncConfig: (config: SyncConfig) => invoke<SyncConfig>("save_sync_config", { config }),
  testSyncConnection: (config: SyncConfig) => invoke<string>("test_sync_connection", { config }),
  pullRemoteBackup: () => invoke<RemoteBackup>("pull_remote_backup"),
  syncToRepo: (providers: ProviderConfig[], includeCredentials: boolean) =>
    invoke<SyncResult>("sync_to_repo", { providers, includeCredentials }),

  startDeepseekLogin: () => invoke<string>("start_deepseek_login"),
  deepseekLoginToken: () => invoke<string | null>("deepseek_login_token"),
};
