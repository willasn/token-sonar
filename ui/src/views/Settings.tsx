/* TokenSonar · 设置视图（React 版）
 *
 * 由旧 ui/assets/js/views/settings.js 平移：外观主题、界面语言、默认货币、
 * 自动刷新与间隔、数据目录、Git 仓库同步（范围选择 / 冲突处理 / 拉取推送）、
 * 关于版本与数据说明。
 *
 * 数据一律走 store / api hook，不再直接操作 DOM 或 window.TL。
 * 同步配置里的仓库地址与账号是用户运行时输入，源码中一律用空串占位。
 */
import { useEffect, useState, type ReactNode } from "react";
import { BrandMark, Icon } from "@/lib/icons";
import { useT, setLocale, getLocale, available } from "@/lib/i18n";
import { api, isTauri } from "@/lib/api";
import { mockReset } from "@/lib/mock";
import { useStore } from "@/state/store";
import { useUi } from "@/components/ui";
import type {
  AppSettings,
  ProviderConfig,
  RemoteBackup,
  SyncConfig,
  SyncResult,
} from "@/types";

/* ------------------------------ 纯工具函数 ------------------------------ */

function errText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/** 只比对有意义的字段，忽略两端各自更新的 createdAt / updatedAt */
function providerSignature(p: ProviderConfig): string {
  const clone = { ...p } as unknown as Record<string, unknown>;
  delete clone.createdAt;
  delete clone.updatedAt;
  return JSON.stringify(clone);
}

/** 去掉账号凭证的副本，用于「不含凭证」模式的比对与合并 */
function stripCred(p: ProviderConfig): ProviderConfig {
  const c: ProviderConfig = {
    ...p,
    token: null,
    accessKeySecret: null,
    sessionToken: null,
    password: null,
    platformToken: null,
  };
  if (Array.isArray(c.apiKeys)) {
    c.apiKeys = c.apiKeys.map((k) => ({ ...k, secret: null }));
  }
  return c;
}

/** 远端覆盖时保留本机已有凭证，避免「不含凭证」同步把本机凭证清空 */
function keepLocalCreds(remote: ProviderConfig, local: ProviderConfig): ProviderConfig {
  return {
    ...remote,
    token: local.token || remote.token,
    accessKeySecret: local.accessKeySecret || remote.accessKeySecret,
    sessionToken: local.sessionToken || remote.sessionToken,
    password: local.password || remote.password,
    platformToken: local.platformToken || remote.platformToken,
  };
}

interface SyncConflict {
  local: ProviderConfig;
  remote: ProviderConfig;
}

/* ------------------------------ 展示型组件 ------------------------------ */

/** 设置行：左侧标签 + 说明，右侧控件（沿用旧 .setting-row 结构） */
function SettingRow({ label, desc, children }: { label: string; desc: string; children: ReactNode }) {
  return (
    <div className="setting-row">
      <div>
        <div className="label">{label}</div>
        <div className="desc">{desc}</div>
      </div>
      <div className="control">{children}</div>
    </div>
  );
}

/** 同步表单字段（沿用旧 .field 结构） */
function SyncField({ label, wide, children }: { label: string; wide?: boolean; children: ReactNode }) {
  return (
    <div className={"field" + (wide ? " span-2" : "")}>
      <label>{label}</label>
      {children}
    </div>
  );
}

interface SyncFormState {
  repoUrl: string;
  username: string;
  password: string;
  branch: string;
}

interface SyncStatusState {
  text: string;
  level: "ok" | "error" | null;
}

/* ------------------------------ 视图主体 ------------------------------ */

export function Settings() {
  const { state, saveSettings, loadSync, saveSyncConfig, pullRemote, pushMerged } = useStore();
  const { toast, openModal, closeModal } = useUi();
  const tr = useT();

  const st = state.settings;
  const lang = (st && st.language) || getLocale();

  /* ---------------------------- 基础设置 ---------------------------- */

  const persist = (next: AppSettings) => {
    saveSettings(next).catch((e) => toast(errText(e), "error"));
  };

  const onLang = (code: string) => {
    if (!st) return;
    if (code === (st.language || getLocale())) return;
    // 切换语言后通知全局重绘（本视图随语言订阅一起刷新）
    setLocale(code);
    persist({ ...st, language: code });
  };

  const onTheme = (theme: string) => {
    if (!st) return;
    // 主题由 App 依据 settings.theme 统一应用
    persist({ ...st, theme });
  };

  const onCurrency = (currency: string) => {
    if (!st) return;
    persist({ ...st, currency });
  };

  const onAuto = () => {
    if (!st) return;
    const next = !st.autoRefresh;
    persist({ ...st, autoRefresh: next });
    toast(tr(next ? "settings.autoOn" : "settings.autoOff"));
  };

  const onInterval = (min: number) => {
    if (!st) return;
    persist({ ...st, refreshIntervalMin: min });
    toast(tr("settings.intervalUpdated"));
  };

  const onOpenDir = async () => {
    try {
      await api.openDataDir();
    } catch (e) {
      toast(errText(e), "error");
    }
  };

  const onReset = () => {
    mockReset();
    location.reload();
  };

  /* ------------------------------ Git 同步 ------------------------------ */

  const [syncCfg, setSyncCfg] = useState<SyncConfig | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [form, setForm] = useState<SyncFormState>({
    repoUrl: "",
    username: "",
    password: "",
    branch: "main",
  });
  const [busy, setBusy] = useState(false);
  const [statusOverride, setStatusOverride] = useState<SyncStatusState | null>(null);

  useEffect(() => {
    let cancelled = false;
    loadSync()
      .then((cfg) => {
        if (cancelled) return;
        setSyncCfg(cfg);
        setForm({
          repoUrl: cfg.repoUrl || "",
          username: cfg.username || "",
          password: cfg.password || "",
          branch: cfg.branch || "main",
        });
      })
      .catch((e) => {
        if (!cancelled) setLoadError(errText(e));
      });
    return () => {
      cancelled = true;
    };
  }, [loadSync]);

  const draftFromForm = (): SyncConfig => ({
    repoUrl: form.repoUrl.trim(),
    username: form.username.trim(),
    password: form.password,
    branch: form.branch.trim() || "main",
    lastSyncAt: syncCfg?.lastSyncAt || "",
    lastMessage: syncCfg?.lastMessage || "",
  });

  /** 返回错误文案，null 表示校验通过 */
  const validateSync = (cfg: SyncConfig): string | null => {
    const url = cfg.repoUrl;
    if (!url) return tr("sync.needUrl");
    const http = url.indexOf("https://") === 0 || url.indexOf("http://") === 0;
    const ssh = url.indexOf("git@") === 0 || url.indexOf("ssh://") === 0;
    if (!http && !ssh) return tr("sync.badUrl");
    if (http && (!cfg.username || !cfg.password)) return tr("sync.needCred");
    return null;
  };

  /** 同步范围弹窗：返回 true（含凭证）/ false（不含凭证），取消返回 null */
  const pickSyncMode = (): Promise<boolean | null> =>
    new Promise((resolve) => {
      let done = false;
      const finish = (v: boolean | null) => {
        if (done) return;
        done = true;
        closeModal();
        resolve(v);
      };
      openModal({
        title: tr("sync.modeTitle"),
        subtitle: tr("sync.modeSubtitle"),
        body: (
          <ul className="conflict-list">
            <li>
              <strong>{tr("sync.modeAll")}</strong>
              <span>{tr("sync.modeAllHint")}</span>
            </li>
            <li>
              <strong>{tr("sync.modeNoCred")}</strong>
              <span>{tr("sync.modeNoCredHint")}</span>
            </li>
          </ul>
        ),
        onClose: () => finish(null),
        buttons: [
          { label: tr("sync.modeNoCred"), onClick: () => finish(false) },
          { label: tr("sync.modeAll"), kind: "btn-primary", onClick: () => finish(true) },
        ],
      });
    });

  /** 冲突弹窗：返回 "local" | "remote" | "both"，取消 / 关闭返回 null */
  const pickConflict = (conflicts: SyncConflict[]): Promise<"local" | "remote" | "both" | null> =>
    new Promise((resolve) => {
      let done = false;
      const finish = (v: "local" | "remote" | "both" | null) => {
        if (done) return;
        done = true;
        closeModal();
        resolve(v);
      };
      openModal({
        title: tr("sync.conflictTitle"),
        subtitle: tr("sync.conflictSubtitle", { n: conflicts.length }),
        body: (
          <>
            <ul className="conflict-list">
              {conflicts.map((c) => (
                <li key={c.local.id}>
                  <strong>{c.local.name || c.local.id}</strong>
                  <span>{c.local.baseUrl || c.local.kind || ""}</span>
                </li>
              ))}
            </ul>
            <div className="hint">{tr("sync.conflictHint")}</div>
          </>
        ),
        onClose: () => finish(null),
        buttons: [
          { label: tr("sync.conflictKeepRemote"), onClick: () => finish("remote") },
          { label: tr("sync.conflictKeepLocal"), onClick: () => finish("local") },
          { label: tr("sync.conflictKeepBoth"), kind: "btn-primary", onClick: () => finish("both") },
        ],
      });
    });

  /**
   * 合并本机与远端账号：
   * - 远端有、本机没有 → 直接补进来（新电脑可自动恢复）
   * - 同 id 且内容一致 → 无需处理
   * - 同 id 但内容不同 → 冲突，弹窗让用户选择远端覆盖 / 本地覆盖 / 两者保留
   * 返回合并后的账号数组；用户取消返回 null。
   */
  const mergeWithRemote = async (
    remote: RemoteBackup,
    includeCredentials: boolean
  ): Promise<ProviderConfig[] | null> => {
    const local = state.providers.slice();
    const localById: Record<string, ProviderConfig> = {};
    local.forEach((p) => {
      localById[p.id] = p;
    });

    const sig = includeCredentials
      ? providerSignature
      : (p: ProviderConfig) => providerSignature(stripCred(p));

    const added: ProviderConfig[] = [];
    const conflicts: SyncConflict[] = [];
    ((remote && remote.providers) || []).forEach((r) => {
      const l = localById[r.id];
      if (!l) added.push(r);
      else if (sig(l) !== sig(r)) conflicts.push({ local: l, remote: r });
    });

    if (!conflicts.length) return local.concat(added);

    const choice = await pickConflict(conflicts);
    if (!choice) return null;

    const byId: Record<string, SyncConflict> = {};
    conflicts.forEach((c) => {
      byId[c.local.id] = c;
    });

    // 本地覆盖 / 两者保留时，本机账号原样；远端覆盖时替换为远端版本（保留本机凭证）
    const merged = local.map((p) => {
      const c = byId[p.id];
      if (c && choice === "remote") {
        return includeCredentials ? c.remote : keepLocalCreds(c.remote, c.local);
      }
      return p;
    });

    // 「都保留」：远端版本以新 id 追加，避免与本地同 id 相互覆盖
    if (choice === "both") {
      conflicts.forEach((c) => {
        merged.push({
          ...c.remote,
          id: "pv_remote_" + Date.now().toString(16) + "_" + Math.random().toString(16).slice(2, 6),
          name: (c.remote.name || "") + tr("sync.conflictDupSuffix"),
        });
      });
    }

    return merged.concat(added);
  };

  const runSync = async () => {
    const cfg = draftFromForm();
    const err = validateSync(cfg);
    if (err) {
      toast(err, "error");
      return;
    }
    // 1) 选同步范围：全部 / 不含账号密码与 API Key
    const includeCredentials = await pickSyncMode();
    if (includeCredentials === null) {
      setStatusOverride({ text: tr("sync.cancelled"), level: null });
      return;
    }

    setBusy(true);
    setStatusOverride({ text: tr("sync.running"), level: null });
    try {
      // 先把草稿落盘，确保同步用的是当前填写的仓库
      const saved = await saveSyncConfig(cfg);
      setSyncCfg(saved);
      // 2) 拉取远端备份（只读，不写本机）
      const remote = await pullRemote();
      // 3) 设置以远端为准（远端还没有备份时保持本机设置）。
      // 直接落盘不走 store.saveSettings，避免同步中途触发全局重绘；同步结束后 reload 会统一刷新
      if (remote && remote.settings) {
        await api.saveSettings(remote.settings);
      }
      // 4) 合并本机与远端账号；同 id 冲突时弹窗让用户选择，取消则中止
      const merged = await mergeWithRemote(remote, includeCredentials);
      if (!merged) {
        setStatusOverride({ text: tr("sync.cancelled"), level: null });
        return;
      }
      // 5) 推送合并结果并回读本机数据
      const res = (await pushMerged(merged, includeCredentials)) as SyncResult | null;
      const cfg2 = await loadSync();
      setSyncCfg(cfg2);
      const msg = (res && res.message) || tr("sync.saved");
      setStatusOverride({ text: msg, level: "ok" });
      toast(msg, "ok");
    } catch (e) {
      const msg = errText(e);
      setStatusOverride({ text: msg, level: "error" });
      toast(msg, "error");
    } finally {
      setBusy(false);
    }
  };

  const defaultStatus: SyncStatusState = syncCfg?.lastSyncAt
    ? {
        text:
          tr("sync.lastAt", { t: syncCfg.lastSyncAt }) +
          (syncCfg.lastMessage ? " · " + syncCfg.lastMessage : ""),
        level: "ok",
      }
    : { text: tr("sync.never"), level: null };
  const status = statusOverride ?? defaultStatus;

  /* ------------------------------ 渲染 ------------------------------ */

  return (
    <>
      <div className="card">
        <div className="about-hero">
          <div className="detail-icon" style={{ background: "linear-gradient(140deg,#4d6bfe,#00d3a7)" }}>
            <BrandMark />
          </div>
          <div>
            <h2 style={{ margin: 0, fontSize: 18 }}>TokenSonar</h2>
            <div style={{ fontSize: 12, color: "var(--text-mute)", marginTop: 4 }}>
              {tr("settings.aboutDesc")}
            </div>
            <div className="kv">
              <span>{tr("settings.version", { v: (state.appInfo && state.appInfo.version) || "0.1.0" })}</span>
              {!isTauri() ? <span>{tr("settings.demoMode")}</span> : null}
              <span>{tr("settings.accountCount", { n: state.providers.length })}</span>
            </div>
          </div>
        </div>
      </div>

      <div className="card settings-list" style={{ marginTop: 16 }}>
        <SettingRow label={tr("settings.language")} desc={tr("settings.languageDesc")}>
          <div className="segmented" id="setLang">
            {available.map((o) => (
              <button
                key={o.code}
                data-lang={o.code}
                className={lang === o.code ? "is-active" : undefined}
                onClick={() => onLang(o.code)}
              >
                {o.label}
              </button>
            ))}
          </div>
        </SettingRow>

        <SettingRow label={tr("settings.theme")} desc={tr("settings.themeDesc")}>
          <div className="segmented" id="setTheme">
            <button
              data-theme="dark"
              className={st?.theme === "dark" ? "is-active" : undefined}
              onClick={() => onTheme("dark")}
            >
              {tr("settings.themeDark")}
            </button>
            <button
              data-theme="light"
              className={st?.theme === "light" ? "is-active" : undefined}
              onClick={() => onTheme("light")}
            >
              {tr("settings.themeLight")}
            </button>
          </div>
        </SettingRow>

        <SettingRow label={tr("settings.currency")} desc={tr("settings.currencyDesc")}>
          <select
            className="select"
            id="setCurrency"
            data-name="currency"
            style={{ width: 150 }}
            value={(st && st.currency) || "CNY"}
            onChange={(e) => onCurrency(e.target.value)}
          >
            {["CNY", "USD", "POINT"].map((c) => (
              <option key={c} value={c}>
                {c === "POINT" ? tr("settings.currencyPoint") : c}
              </option>
            ))}
          </select>
        </SettingRow>

        <SettingRow label={tr("settings.autoRefresh")} desc={tr("settings.autoRefreshDesc")}>
          <button className={"switch" + (st && st.autoRefresh ? " is-on" : "")} id="setAuto" onClick={onAuto} />
        </SettingRow>

        <SettingRow label={tr("settings.interval")} desc={tr("settings.intervalDesc")}>
          <select
            className="select"
            id="setInterval"
            data-name="refreshIntervalMin"
            style={{ width: 150 }}
            value={st ? st.refreshIntervalMin : 15}
            onChange={(e) => onInterval(parseInt(e.target.value, 10) || 15)}
          >
            {[5, 10, 15, 30, 60].map((m) => (
              <option key={m} value={m}>
                {tr("settings.minutes", { n: m })}
              </option>
            ))}
          </select>
        </SettingRow>
      </div>

      <div className="card settings-list" style={{ marginTop: 16 }}>
        <SettingRow
          label={tr("settings.dataDir")}
          desc={(state.appInfo && state.appInfo.dataDir) || "—"}
        >
          <button className="btn" id="setOpenDir" onClick={onOpenDir}>
            <Icon name="folder" />
            {tr("settings.openDir")}
          </button>
        </SettingRow>

        <SettingRow label={tr("settings.dataNote")} desc={tr("settings.dataNoteDesc")}>
          <span className="chip chip-teal">{tr("settings.localStore")}</span>
        </SettingRow>

        {!isTauri() ? (
          <SettingRow label={tr("settings.demoData")} desc={tr("settings.demoDataDesc")}>
            <button className="btn" id="setReset" onClick={onReset}>
              <Icon name="refresh" />
              {tr("settings.resetDemo")}
            </button>
          </SettingRow>
        ) : null}
      </div>

      <div className="card" id="syncCard" style={{ marginTop: 16 }}>
        {loadError ? (
          <div className="sync-form">
            <div className="sync-status is-error">{loadError}</div>
          </div>
        ) : !syncCfg ? (
          <div className="sync-form">
            <div className="sync-status">
              <span className="spinner" />
            </div>
          </div>
        ) : (
          <>
            <div className="settings-list">
              <SettingRow label={tr("sync.cardTitle")} desc={tr("sync.cardDesc")}>
                <span className="chip chip-accent">Git</span>
              </SettingRow>
            </div>
            <div className="sync-form">
              <div className="form-grid" id="syncForm">
                <SyncField label={tr("sync.repoUrl")} wide>
                  <input
                    className="input"
                    data-name="repoUrl"
                    value={form.repoUrl}
                    placeholder={tr("sync.repoUrlPh")}
                    autoComplete="off"
                    onChange={(e) => setForm({ ...form, repoUrl: e.target.value })}
                  />
                </SyncField>
                <SyncField label={tr("sync.username")}>
                  <input
                    className="input"
                    data-name="username"
                    value={form.username}
                    placeholder={tr("sync.usernamePh")}
                    autoComplete="off"
                    onChange={(e) => setForm({ ...form, username: e.target.value })}
                  />
                </SyncField>
                <SyncField label={tr("sync.password")}>
                  <input
                    className="input"
                    data-name="password"
                    type="password"
                    value={form.password}
                    placeholder={tr("sync.passwordPh")}
                    autoComplete="off"
                    onChange={(e) => setForm({ ...form, password: e.target.value })}
                  />
                </SyncField>
                <SyncField label={tr("sync.branch")}>
                  <input
                    className="input"
                    data-name="branch"
                    value={form.branch}
                    placeholder={tr("sync.branchPh")}
                    autoComplete="off"
                    onChange={(e) => setForm({ ...form, branch: e.target.value })}
                  />
                </SyncField>
              </div>
              <div className="pick-note">
                <Icon name="info" />
                <span>
                  {tr("sync.warning")}
                  {!isTauri() ? " " + tr("sync.demoNote") : ""}
                </span>
              </div>
            </div>
            <div className="sync-bar">
              <div className={"sync-status" + (status.level ? " is-" + status.level : "")}>{status.text}</div>
              <div className="actions">
                <button className="btn btn-primary" id="syncRun" disabled={busy} onClick={runSync}>
                  {busy ? (
                    <>
                      <span className="spinner" />
                      <span>{tr("sync.running")}</span>
                    </>
                  ) : (
                    <>
                      <Icon name="swap" />
                      <span>{tr("sync.run")}</span>
                    </>
                  )}
                </button>
              </div>
            </div>
          </>
        )}
      </div>
    </>
  );
}