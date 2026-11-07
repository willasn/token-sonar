/* TokenSonar · 应用内更新（唯一更新接缝）
 *
 * 检查与安装在 Rust 侧完成：并行探测 R2 镜像与 GitHub Release（单源 5 秒超时），
 * 选版本更高的源下载带 minisign 签名的更新包，安装成功后自动重启。
 * 浏览器演示模式下没有更新通道：检查一律返回 null，调用方需容忍。
 */
import { invoke, isTauri, listen } from "./api";

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
  /** 下载并安装；Rust 侧安装成功后会自动重启应用 */
  install: (onProgress?: (p: UpdateProgress) => void) => Promise<void>;
}

interface UpdateInfo {
  hasUpdate: boolean;
  current: string;
  latest?: string | null;
  notes?: string | null;
  publishedAt?: number | null;
}

interface ProgressPayload {
  chunk: number;
  total: number | null;
}

/** 检查是否有新版本；无更新或演示模式返回 null */
export async function checkForUpdate(): Promise<PendingUpdate | null> {
  if (!isTauri()) return null;
  const info = await invoke<UpdateInfo>("auto_update_check");
  if (!info.hasUpdate || !info.latest) return null;
  return {
    version: info.latest,
    currentVersion: info.current,
    notes: info.notes ?? undefined,
    install: async (onProgress) => {
      let downloaded = 0;
      const unlisten = onProgress
        ? await listen<ProgressPayload>("update:progress", (p) => {
            downloaded += p.chunk;
            onProgress({ downloaded, total: p.total ?? 0 });
          })
        : null;
      try {
        await invoke("auto_update_install");
      } finally {
        unlisten?.();
      }
    },
  };
}

/** 重启应用以运行新版本（Windows 由安装器自动重启；macOS / Linux 手动调用） */
export async function relaunchApp(): Promise<void> {
  if (!isTauri()) return;
  const { relaunch } = await import("@tauri-apps/plugin-process");
  await relaunch();
}
