/* TokenSonar · 应用更新弹窗
 *
 * 展示新版本号与更新说明，负责下载进度、安装与重启。
 * 由设置页「检查更新」按钮与启动时的静默自动检查共用。
 */
import { useState } from "react";
import { Icon } from "@/lib/icons";
import { t, useT } from "@/lib/i18n";
import { relaunchApp, type PendingUpdate } from "@/lib/updater";
import type { useUi } from "@/components/ui";

type Ui = ReturnType<typeof useUi>;

/** 弹窗主体：说明 + 进度 + 操作（按钮随阶段变化，故直接内置于 body） */
function UpdateBody({ update, onClose }: { update: PendingUpdate; onClose: () => void }) {
  const tr = useT();
  const [phase, setPhase] = useState<"idle" | "downloading" | "ready">("idle");
  const [percent, setPercent] = useState(0); // 负数表示服务端未给出总大小
  const [error, setError] = useState<string | null>(null);

  const install = async () => {
    setError(null);
    setPhase("downloading");
    setPercent(0);
    try {
      await update.install(({ downloaded, total }) =>
        setPercent(total > 0 ? Math.min(100, Math.round((downloaded / total) * 100)) : -1)
      );
      setPhase("ready");
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setPhase("idle");
    }
  };

  const restart = async () => {
    try {
      await relaunchApp();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <>
      <div className="kv">
        <span>{tr("settings.updateCurrent", { v: update.currentVersion })}</span>
        <span>{tr("settings.updateLatest", { v: update.version })}</span>
      </div>

      {update.notes ? (
        <div className="update-notes">
          <div className="update-notes-title">{tr("settings.updateNotes")}</div>
          <pre>{update.notes}</pre>
        </div>
      ) : null}

      {phase === "downloading" ? (
        <div className="update-progress">
          <div className="update-bar">
            <i style={{ width: percent >= 0 ? percent + "%" : "100%" }} />
          </div>
          <span className="update-progress-text">
            {percent >= 0
              ? tr("settings.updateDownloading", { p: percent })
              : tr("settings.updateDownloadingUnknown")}
          </span>
        </div>
      ) : null}

      {phase === "ready" ? (
        <div className="update-ok">
          <Icon name="check" />
          {tr("settings.updateReady")}
        </div>
      ) : null}

      {error ? <div className="update-error">{tr("settings.updateFailed", { msg: error })}</div> : null}

      <div className="update-actions">
        {phase === "ready" ? (
          <button className="btn btn-primary" onClick={restart}>
            <Icon name="power" />
            {tr("settings.updateRestart")}
          </button>
        ) : (
          <button className="btn btn-primary" disabled={phase === "downloading"} onClick={install}>
            <Icon name="refresh" />
            {phase === "downloading" ? tr("settings.checking") : tr("settings.updateDownload")}
          </button>
        )}
        <button className="btn" disabled={phase === "downloading"} onClick={onClose}>
          {tr("common.close")}
        </button>
      </div>
    </>
  );
}

/** 打开「发现新版本」弹窗；供设置页与启动自动检查复用 */
export function showUpdateModal(ui: Ui, update: PendingUpdate) {
  ui.openModal({
    title: t("settings.updateAvailable", { v: update.version }),
    subtitle: t("settings.update"),
    body: <UpdateBody update={update} onClose={ui.closeModal} />,
    footNote: t("settings.updateHint"),
    buttons: [],
  });
}
