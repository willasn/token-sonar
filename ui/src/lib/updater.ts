/* TokenSonar · 应用内更新（唯一更新接缝）
 *
 * 走 Tauri 官方 updater 插件：从 GitHub Releases 的 latest.json 检查新版本，
 * 下载并安装带 minisign 签名的更新包，最后重启应用生效。
 * 浏览器演示模式下插件不可用：检查一律返回 null，安装 / 重启为空操作，调用方需容忍。
 * 插件按需动态载入，确保非 Tauri 环境不加载任何 Tauri 运行时。
 */
import type { DownloadEvent } from "@tauri-apps/plugin-updater";
import { isTauri } from "./api";

export interface UpdateProgress {
  /** 已下载字节 */
  downloaded: number;
  /** 总字节；为 0 表示服务端未给出大小 */
  total: number;
}

export interface PendingUpdate {
  /** 服务端公布的新版本号 */
  version: string;
  /** 当前运行版本 */
  currentVersion: string;
  /** 更新说明（可能缺失） */
  notes?: string;
  /** 下载并安装；安装完成后需调用 relaunchApp 才会生效（Windows 会自动重启） */
  install: (onProgress?: (p: UpdateProgress) => void) => Promise<void>;
}

/** 检查是否有新版本；无更新或演示模式返回 null */
export async function checkForUpdate(): Promise<PendingUpdate | null> {
  if (!isTauri()) return null;
  const { check } = await import("@tauri-apps/plugin-updater");
  const update = await check();
  if (!update) return null;
  return {
    version: update.version,
    currentVersion: update.currentVersion,
    notes: update.body,
    install: async (onProgress) => {
      let downloaded = 0;
      let total = 0;
      await update.downloadAndInstall((event: DownloadEvent) => {
        if (event.event === "Started") {
          total = event.data.contentLength ?? 0;
        } else if (event.event === "Progress") {
          downloaded += event.data.chunkLength;
        }
        onProgress?.({ downloaded, total });
      });
    },
  };
}

/** 重启应用以运行新版本（macOS / Linux 需要；Windows 由安装器自动重启） */
export async function relaunchApp(): Promise<void> {
  if (!isTauri()) return;
  const { relaunch } = await import("@tauri-apps/plugin-process");
  await relaunch();
}
