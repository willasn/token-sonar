/* TokenSonar · 添加 / 编辑账号弹窗（React 版）
 *
 * 由旧 ui/assets/js/views/providers.js 的「账号表单」部分平移：
 * 类型选择 / 接入方式 / 各字段 / 登录取凭证 / 测试连接 / 保存。
 *
 * 所有厂商类型分支一律查表（KIND_INFO / MODES_BY_KIND / MODE_INFO），
 * 不再写 `kind === "deepseek" ? A : B` 的二元判断。
 * 弹层复用 ui.tsx 的 useUi().openModal（旧 modal.js），不另造遮罩。
 */
import { useEffect, useRef, useState, useSyncExternalStore, type ReactNode } from "react";
import { useUi } from "./ui";
import { useStore } from "../state/store";
import { api, listen } from "../lib/api";
import { t } from "../lib/i18n";
import { Icon } from "../lib/icons";
import {
  KIND_INFO,
  KIND_ORDER,
  MODES,
  MODES_BY_KIND,
  MODE_INFO,
  kindInfo,
  providerInfo,
  type AccessModeKey,
  type KindInfo,
} from "../lib/kinds";
import type { ProviderConfig, ProviderKind } from "../types";

/* ------------------------------ 查表辅助 ------------------------------ */

/** 某渠道支持的接入方式（含图标），查 MODES_BY_KIND */
function modesOf(kind: string) {
  const keys = MODES_BY_KIND[kind as ProviderKind] || MODES_BY_KIND.deepseek;
  return MODES.filter((m) => keys.indexOf(m.key) >= 0);
}

/** 接入方式文案（label / desc / gets），查 MODE_INFO */
function modeInfo(kind: string, mode: AccessModeKey) {
  const table = MODE_INFO[kind] || MODE_INFO.deepseek;
  return table[mode] || table.api || { label: "", desc: "", gets: "" };
}

function modeLabel(kind: string, mode: AccessModeKey): string {
  return t(modeInfo(kind, mode).label);
}

/** 渠道默认站点地址：官方厂商用内置地址，其余为空 */
function defaultBaseUrl(info: KindInfo): string {
  return info.official ? info.url.placeholder || "" : "";
}

/* ------------------------------ 表单状态 ------------------------------ */

interface FormSnap {
  draft: ProviderConfig;
  note: string;
  noteLevel: "info" | "ok" | "danger";
}

/**
 * 极简外部状态源：让 useUi().openModal 里存放的 body / footNote 元素
 * 能在字段变化时重渲染（React 元素本身是静态描述，需借助订阅触发更新）。
 */
class FormModel {
  private snap: FormSnap;
  private listeners = new Set<() => void>();

  constructor(draft: ProviderConfig) {
    this.snap = { draft, note: "", noteLevel: "info" };
  }

  subscribe = (fn: () => void): (() => void) => {
    this.listeners.add(fn);
    return () => {
      this.listeners.delete(fn);
    };
  };

  getSnapshot = (): FormSnap => this.snap;

  private emit(): void {
    this.listeners.forEach((fn) => fn());
  }

  get draft(): ProviderConfig {
    return this.snap.draft;
  }

  setDraft(draft: ProviderConfig): void {
    this.snap = { ...this.snap, draft };
    this.emit();
  }

  setNote(note: string, level: FormSnap["noteLevel"] = "info"): void {
    this.snap = { ...this.snap, note, noteLevel: level };
    this.emit();
  }
}

/** 归一化草稿：补默认值 + 收敛接入方式 + 按渠道取图标 */
function normalizeDraft(provider: ProviderConfig | null): ProviderConfig {
  const base: ProviderConfig = {
    id: "",
    name: "",
    kind: "deepseek",
    enabled: true,
    baseUrl: "",
    authType: "token",
    accessMode: "api",
    token: "",
    credentialName: "",
    accessKeyId: "",
    accessKeySecret: "",
    username: "",
    password: "",
    sessionToken: "",
    apiKeys: [],
    icon: "",
    accent: "",
    currency: "CNY",
    quotaPerUnit: 500000,
    displayInCurrency: true,
    notes: "",
    cardView: null,
    createdAt: "",
    updatedAt: "",
  };
  const current: ProviderConfig = { ...base, ...(provider || {}) };
  // 旧配置没有 accessMode：有会话凭证的就按登录方式打开
  let accessMode: AccessModeKey =
    current.accessMode || (current.sessionToken || current.platformToken ? "login" : "api");
  current.sessionToken = current.sessionToken || current.platformToken || "";
  // 当前渠道不支持的接入方式一律回落到默认方式
  const keys = MODES_BY_KIND[current.kind] || MODES_BY_KIND.deepseek;
  if (keys.indexOf(accessMode) < 0) accessMode = modesOf(current.kind)[0].key;
  current.accessMode = accessMode;
  current.icon = kindInfo(current.kind).icon;
  return current;
}

/* ------------------------------ 通用字段件 ------------------------------ */

function Field({
  label,
  wide,
  hint,
  children,
}: {
  label: string;
  wide?: boolean;
  hint?: string;
  children: ReactNode;
}) {
  return (
    <div className={"field" + (wide ? " span-2" : "")}>
      <label>{label}</label>
      {children}
      {hint ? <span className="hint">{hint}</span> : null}
    </div>
  );
}

/** 窗口底部（footer）的状态 / 错误提示 */
function FormNote({ model }: { model: FormModel }) {
  const snap = useSyncExternalStore(model.subscribe, model.getSnapshot);
  const color =
    snap.noteLevel === "danger" ? "var(--danger)" : snap.noteLevel === "ok" ? "var(--ok)" : "var(--text-mute)";
  return (
    <div className="modal-foot-note" style={{ color }}>
      {snap.note}
    </div>
  );
}

/* ------------------------------ 表单主体 ------------------------------ */

function ProviderFormBody({ model }: { model: FormModel }) {
  const snap = useSyncExternalStore(model.subscribe, model.getSnapshot);
  const { toast } = useUi();
  const d = snap.draft;

  const info = providerInfo(d);
  const modes = modesOf(d.kind);
  const modeKeys = modes.map((m) => m.key);
  const draftMode = d.accessMode || "api";
  const mode: AccessModeKey = modeKeys.indexOf(draftMode) >= 0 ? draftMode : modeKeys[0];
  const current = modeInfo(d.kind, mode);
  // 凭证字段文案按当前渠道 + 接入方式查表
  const cred = (mode === "login" ? info.login : info.api) || info.api;

  const [loginBusy, setLoginBusy] = useState(false);

  const setField = (patch: Partial<ProviderConfig>) => {
    model.setDraft({ ...model.draft, ...patch });
  };

  /** 切换渠道类型：名称 / 主题色 / 地址仍是上一渠道默认值时跟随切换 */
  function switchKind(next: ProviderKind) {
    const cur = model.draft;
    const prevInfo = providerInfo(cur);
    const nextInfo = providerInfo({ kind: next, planType: cur.planType });
    const prevDefault = t(prevInfo.name);
    const name = !cur.name || cur.name === prevDefault ? t(nextInfo.name) : cur.name;
    const accent = !cur.accent || cur.accent === prevInfo.accent ? nextInfo.accent : cur.accent;
    const prevDefaultUrl = defaultBaseUrl(prevInfo);
    const urlUntouched = !String(cur.baseUrl || "").trim() || cur.baseUrl === prevDefaultUrl;
    let baseUrl = cur.baseUrl || "";
    if (cur.kind !== next && (urlUntouched || nextInfo.official)) {
      baseUrl = nextInfo.official ? nextInfo.url.placeholder || "" : "";
    } else if (nextInfo.official) {
      baseUrl = baseUrl || nextInfo.url.placeholder || "";
    }
    // 切换渠道后，当前接入方式若不被支持（如 New API 的登录授权）则回落到默认方式
    const keys = MODES_BY_KIND[next] || MODES_BY_KIND.deepseek;
    let accessMode = cur.accessMode || "api";
    if (keys.indexOf(accessMode) < 0) accessMode = keys[0];
    model.setDraft({ ...cur, kind: next, name, accent, baseUrl, accessMode, icon: nextInfo.icon });
    model.setNote("");
  }

  /** 只切换接入方式，其它字段（含另一种方式的凭证）原样保留 */
  function switchMode(next: AccessModeKey) {
    model.setDraft({ ...model.draft, accessMode: next });
    model.setNote("");
  }

  /** 打开外置登录窗口取回凭证（后端把「捕获 / 验证」进度实时推过来） */
  async function runLogin() {
    const credName = t("form.platformToken");
    setLoginBusy(true);
    model.setNote(t("form.loginOpened"));
    const unlisten = await listen<string>("login-progress", (payload) => {
      const msg = payload == null ? "" : String(payload);
      if (msg) model.setNote(msg);
    });
    try {
      const token = await api.startDeepseekLogin();
      model.setDraft({ ...model.draft, sessionToken: token });
      model.setNote("✓ " + t("form.loginOk", { v: credName }), "ok");
      toast(t("form.loginSuccess", { v: credName }), "ok");
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      model.setNote("✗ " + msg, "danger");
      toast(msg, "error");
    } finally {
      if (unlisten) unlisten();
      setLoginBusy(false);
    }
  }

  return (
    <>
      {/* 一级分类：渠道类型 */}
      <div className="pick-group">
        <div className="pick-head">
          <span className="pick-tag is-primary">{t("form.level1")}</span>
          {t("form.type")}
        </div>
        <div className="kind-pick is-primary" id="kindPick">
          {KIND_ORDER.map((k) => {
            const it = KIND_INFO[k];
            return (
              <button
                key={k}
                type="button"
                className={"kind-card" + (d.kind === k ? " is-active" : "")}
                onClick={() => switchKind(k)}
              >
                <span className="kind-icon" style={{ background: it.accent }}>
                  {it.icon}
                </span>
                <span className="kind-text">
                  <b>{t(it.name)}</b>
                  <i>{t(it.desc)}</i>
                </span>
              </button>
            );
          })}
        </div>
      </div>

      {/* 二级分类：该渠道支持的接入方式 */}
      <div className="pick-group">
        <div className="pick-head">
          <span className="pick-tag">{t("form.level2")}</span>
          {t("form.mode")}
        </div>
        <div className="kind-pick is-secondary" id="modePick">
          {modes.map((m) => {
            const it = modeInfo(d.kind, m.key);
            return (
              <button
                key={m.key}
                type="button"
                className={"kind-card" + (mode === m.key ? " is-active" : "")}
                onClick={() => switchMode(m.key)}
              >
                <span
                  className="kind-icon"
                  style={{ background: m.key === "login" ? "var(--ok)" : info.accent }}
                >
                  <Icon name={m.icon} />
                </span>
                <span className="kind-text">
                  <b>{t(info.cliUsage ? "plan.cliMode" : it.label)}</b>
                  <i>{t(info.planHint || it.desc)}</i>
                </span>
              </button>
            );
          })}
        </div>
        <div className="pick-note">
          <Icon name="info" />
          <span>{info.planHint ? t(info.planHint) : t("form.reads", { v: t(current.gets) })}</span>
        </div>
      </div>

      <div className="form-grid">
        {info.plans ? <Field label={t("plan.type")} wide hint={t(info.planHint || "")}>
          <select className="select" value={d.planType || "bss"} onChange={(e) => {
            const planType = e.target.value as ProviderConfig["planType"];
            const nextInfo = providerInfo({ ...d, planType });
            setField({ planType, baseUrl: nextInfo.url.placeholder || "", cardView: nextInfo.quotaOnly ? "quota" : null });
            model.setNote("");
          }}>
            {info.plans.map((plan) => <option key={plan.key} value={plan.key}>{t(plan.label)}</option>)}
          </select>
        </Field> : null}
        <Field label={t("form.displayName")} wide>
          <input
            className="input"
            value={d.name}
            placeholder={t(info.name)}
            onChange={(e) => setField({ name: e.target.value })}
          />
        </Field>

        {info.cliUsage ? <Field label={t("plan.cliProfile")} wide hint={t("plan.cliHint")}>
          <input className="input" value={d.cliProfile || ""} placeholder="plan-demo" onChange={(e) => setField({ cliProfile: e.target.value })} />
        </Field> : <Field label={t(info.url.label)} wide hint={t(info.url.hint || "")}>
          <input
            className="input"
            value={d.baseUrl || (info.official ? info.url.placeholder : "")}
            placeholder={info.url.placeholder}
            onChange={(e) => setField({ baseUrl: e.target.value })}
          />
        </Field>}

        {/* 凭证字段：两种接入方式互斥，各自的凭证分开存，不互相覆盖 */}
        {info.cliUsage ? null : mode === "login" ? (
          <Field label={t(cred.label)} wide hint={t(cred.hint || "")}>
            <div className="field-row">
              <input
                className="input"
                type="password"
                value={d.sessionToken || ""}
                placeholder={t(cred.placeholder || "")}
                onChange={(e) => setField({ sessionToken: e.target.value })}
              />
              <button type="button" className="btn" disabled={loginBusy} onClick={runLogin}>
                {loginBusy ? (
                  <>
                    <span className="spinner" />
                    {t("form.waitingLogin")}
                  </>
                ) : (
                  <>
                    <Icon name="logout" />
                    {t("form.signIn")}
                  </>
                )}
              </button>
            </div>
          </Field>
        ) : info.akSk ? (
          // 云厂商：Access Key ID + Secret Key 双字段，签名鉴权（不是单个 API Key）
          <>
            <Field label={t(info.accessKeyIdLabel || "form.accessKeyId")}>
              <input
                className="input"
                value={d.accessKeyId || ""}
                placeholder={info.api.placeholder || "AKLTxxxxxxxxxxxxxxxxxxxxxxxx"}
                onChange={(e) => setField({ accessKeyId: e.target.value })}
              />
            </Field>
            <Field label={t(info.accessKeySecretLabel || "form.accessKeySecret")} hint={t(info.api.hint || "")}>
              <input
                className="input"
                type="password"
                value={d.accessKeySecret || ""}
                placeholder={t("form.accessKeySecretPlaceholder")}
                onChange={(e) => setField({ accessKeySecret: e.target.value })}
              />
            </Field>
          </>
        ) : (
          <Field label={t(cred.label)} wide hint={t(cred.hint || "")}>
            <input
              className="input"
              type="password"
              value={d.token || ""}
              placeholder={t(cred.placeholder || "")}
              onChange={(e) => setField({ token: e.target.value })}
            />
          </Field>
        )}

        {mode === "api" ? (
          <Field label={t("form.credentialName")} wide>
            <input
              className="input"
              value={d.credentialName || ""}
              placeholder={t("detail.defaultName")}
              onChange={(e) => setField({ credentialName: e.target.value })}
            />
          </Field>
        ) : null}

        {info.currencyFields ? (
          <>
            <Field label={t("form.currency")}>
              <select
                className="select"
                value={d.currency || "CNY"}
                onChange={(e) => setField({ currency: e.target.value })}
              >
                <option value="CNY">{t("form.currencyCny")}</option>
                <option value="USD">{t("form.currencyUsd")}</option>
                <option value="POINT">{t("form.currencyPoint")}</option>
              </select>
            </Field>
            <Field label={t("form.quotaPerUnit")} hint={t("form.quotaPerUnitHint")}>
              <input
                className="input"
                value={String(d.quotaPerUnit ?? "")}
                placeholder="500000"
                onChange={(e) => setField({ quotaPerUnit: parseFloat(e.target.value) || 500000 })}
              />
            </Field>
            <Field label={t("form.displayInCurrency")} hint={t("form.displayInCurrencyHint")}>
              <button
                type="button"
                className={"switch" + (d.displayInCurrency ? " is-on" : "")}
                onClick={() => setField({ displayInCurrency: !d.displayInCurrency })}
              />
            </Field>
          </>
        ) : null}

        <Field label={t("form.accent")}>
          <input
            type="color"
            value={d.accent || info.accent}
            style={{
              width: "100%",
              height: 38,
              padding: 2,
              borderRadius: 10,
              border: "1px solid var(--border)",
              background: "var(--panel-2)",
            }}
            onChange={(e) => setField({ accent: e.target.value })}
          />
        </Field>
        <Field label={t("form.notes")} wide>
          <input
            className="input"
            value={d.notes || ""}
            placeholder={t("form.notesPlaceholder")}
            onChange={(e) => setField({ notes: e.target.value })}
          />
        </Field>
      </div>
    </>
  );
}

/* ------------------------------ 弹窗入口 ------------------------------ */

export function ProviderFormModal(props: {
  open: boolean;
  provider: ProviderConfig | null;
  onClose: () => void;
}) {
  const { openModal, closeModal, toast } = useUi();
  const { saveProvider, refreshOne } = useStore();
  // 防止 StrictMode 下 effect 二次执行时重复打开（第二次会触发 onClose 关掉弹窗）
  const openedRef = useRef(false);

  useEffect(() => {
    if (!props.open) {
      openedRef.current = false;
      return;
    }
    if (openedRef.current) return;
    openedRef.current = true;

    const model = new FormModel(normalizeDraft(props.provider));
    const isEdit = !!(props.provider && props.provider.id);
    const errMsg = (e: unknown) => (e instanceof Error ? e.message : String(e));

    openModal({
      title: isEdit ? t("form.editTitle") : t("form.addTitle"),
      subtitle: isEdit ? props.provider?.name || "" : t("form.addSub"),
      body: <ProviderFormBody model={model} />,
      wide: true,
      footNote: <FormNote model={model} />,
      buttons: [
        {
          label: t("form.test"),
          onClick: async () => {
            model.setNote(t("form.testing"));
            try {
              const msg = await api.testProvider(model.draft);
              model.setNote("✓ " + msg, "ok");
            } catch (e) {
              model.setNote("✗ " + errMsg(e), "danger");
            }
          },
        },
        { label: t("common.cancel"), onClick: () => closeModal() },
        {
          label: isEdit ? t("common.save") : t("common.add"),
          kind: "btn-primary",
          onClick: async () => {
            const d = model.draft;
            if (!d.name.trim()) {
              toast(t("form.needName"), "error");
              return;
            }
            if (!kindInfo(d.kind).official && !String(d.baseUrl || "").trim()) {
              toast(t("form.needSiteUrl"), "error");
              return;
            }
            // 两种接入方式互斥，只校验当前选中的那一种的凭证
            if (d.accessMode === "login") {
              if (!String(d.sessionToken || "").trim()) {
                toast(
                  t("form.needLogin", {
                    v: kindInfo(d.kind).official ? t("form.deepseekPlatform") : t("form.site"),
                  }),
                  "error"
                );
                return;
              }
            } else if (providerInfo(d).cliUsage) {
              if (!/^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/.test((d.cliProfile || "").trim())) {
                toast(t("plan.needProfile"), "error");
                return;
              }
            } else if (providerInfo(d).akSk) {
              if (!String(d.accessKeyId || "").trim() || !String(d.accessKeySecret || "").trim()) {
                toast(t("form.needAccessKey"), "error");
                return;
              }
            } else if (!String(d.token || "").trim()) {
              toast(t("form.needApiCred", { v: modeLabel(d.kind, "api") }), "error");
              return;
            }
            try {
              const saved = await saveProvider({
                ...d,
                credentialName: d.accessMode === "api" ? d.credentialName?.trim() || t("detail.defaultName") : d.credentialName,
              });
              closeModal();
              toast(isEdit ? t("common.saved") : t("form.added"), "ok");
              await refreshOne(saved.id);
            } catch (e) {
              toast(errMsg(e), "error");
            }
          },
        },
      ],
      onClose: props.onClose,
    });
  }, [props.open, props.provider, props.onClose, openModal, closeModal, toast, saveProvider, refreshOne]);

  return null;
}