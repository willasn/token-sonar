/* TokenSonar · 单账号详情视图（React 版）
 *
 * 由旧 ui/assets/js/views/detail.js 平移：账号头部（名称 / 类型 / 状态 / 余额 / 账号 / 更新时间）、
 * 单账号刷新、编辑 / 删除、错误横幅、4 张指标卡、近 30 天用量趋势（费用 / Token / 请求数 / 套餐限额）、
 * 账户明细、模型用量表、最近请求表、Keys 管理（脱敏展示 / 复制 / 手动录入）。
 *
 * 厂商类型标签统一查 KIND_INFO（detailLabel / chip / accent），禁止 kind === "x" ? A : B 的二元判断；
 * 数据全部经 store / api 层获取，组件内不做任何直接 DOM 操作。
 */
import { useRef, useState, type ReactNode } from "react";
import { Icon } from "../lib/icons";
import { useT } from "../lib/i18n";
import { fmt } from "../lib/format";
import { AreaChart } from "../components/charts";
import { useStore, type Metric } from "../state/store";
import { useUi } from "../components/ui";
import { useShell } from "../App";
import { providerInfo, billingTokens, usageValue, MODE_INFO, type AccessModeKey } from "../lib/kinds";
import type { ApiKeyInfo, ProviderConfig } from "../types";

const SEGMENTS: { key: Metric; label: string }[] = [
  { key: "cost", label: "metric.cost" },
  { key: "tokens", label: "metric.tokens" },
  { key: "requests", label: "metric.requestsCount" },
];
/* 套餐用量限额作为「用量趋势」的一个标签页（排在费用左侧） */
const QUOTA_KEY = "quota";
const FALLBACK_ACCENT = "#4d6bfe";

function errMsg(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/** 日期字段：支持时间戳（秒 / 毫秒）与字符串，统一为 yyyy-mm-dd */
function keyDate(s?: string | null): string {
  if (!s) return "—";
  const str = String(s);
  if (/^\d+$/.test(str)) {
    const ts = str.length > 10 ? Number(str) : Number(str) * 1000;
    const d = new Date(ts);
    if (Number.isNaN(d.getTime())) return "—";
    return d.getFullYear() + "-" + fmt.pad(d.getMonth() + 1) + "-" + fmt.pad(d.getDate());
  }
  return str.slice(0, 10);
}

export function Detail() {
  const {
    state,
    provider,
    snapshot,
    refreshOne,
    saveProvider,
    removeProvider,
    navigate,
  } = useStore();
  const { toast, confirm, openModal, closeModal } = useUi();
  const { openProviderForm } = useShell();
  const t = useT();

  const [seg, setSeg] = useState<string>("cost");
  const [busy, setBusy] = useState(false);

  /* API Key 手动录入表单字段：模态框内为非受控输入，通过 ref 读取 */
  const keyNameRef = useRef<HTMLInputElement>(null);
  const keyAccessIdRef = useRef<HTMLInputElement>(null);
  const keyTypeRef = useRef<HTMLSelectElement>(null);
  const keySecretRef = useRef<HTMLInputElement>(null);
  const keyCreatedRef = useRef<HTMLInputElement>(null);


  const found = provider(state.detailId);
  if (!found) {
    return (
      <div className="card card-pad">
        <div className="empty">
          <div className="empty-illu">
            <Icon name="alert" />
          </div>
          <h3>{t("detail.notFound")}</h3>
          <button className="btn" onClick={() => navigate("providers")}>
            {t("detail.backList")}
          </button>
        </div>
      </div>
    );
  }

  const p: ProviderConfig = found;
  const snap = snapshot(p.id);
  const cur = (snap && snap.currency) || p.currency || "CNY";
  const money = (v?: number | null) => fmt.money(v, cur);

  const info = providerInfo(p);
  const balanceOnly = !!info.balanceOnly;
  const billingUsage = !!info.billingUsage;
  const billedTokens = billingTokens(snap);
  const balancePart = (pattern: RegExp) => {
    const value = snap?.metrics.find((m) => pattern.test(m.label))?.value?.trim();
    return value && Number.isFinite(Number(value)) ? fmt.money(Number(value), snap?.currency) : "—";
  };
  const tReq = usageValue(snap, snap?.today?.requests ?? (info.independentUsageSources ? null : 0));
  const tTok = usageValue(snap, snap?.today?.tokens ?? (info.independentUsageSources ? null : 0));
  const tCost = usageValue(snap, snap?.today?.cost ?? (info.independentUsageSources ? null : 0));
  const mReq = usageValue(snap, snap?.month?.requests ?? (info.independentUsageSources ? null : 0));
  const mTok = usageValue(snap, snap?.month?.tokens ?? (info.independentUsageSources ? null : 0));

  const hasQuota = !!(snap && snap.quotas && snap.quotas.length);
  const segments = info.quotaOnly ? [{ key: QUOTA_KEY, label: "detail.codePlan" }] : hasQuota ? [{ key: QUOTA_KEY, label: "detail.codePlan" }, ...SEGMENTS] : SEGMENTS;
  const activeSeg = info.quotaOnly ? QUOTA_KEY : segments.some((m) => m.key === seg) ? seg : "cost";
  const isQuota = activeSeg === QUOTA_KEY;
  const chartKey = activeSeg as Metric;

  const days = snap?.days || [];
  const noUsage = balanceOnly;

  /** 接入方式文案：查 MODE_INFO 表，缺省回退 api */
  function modeLabelOf(kind: string, mode: AccessModeKey): string {
    const table = MODE_INFO[kind] || MODE_INFO.deepseek;
    const mi = table[mode] || table.api;
    return mi ? t(mi.label) : "";
  }

  async function onRefresh() {
    setBusy(true);
    try {
      await refreshOne(p.id);
      toast(t("common.refreshed"), "ok");
    } catch (e) {
      toast(errMsg(e), "error");
    } finally {
      setBusy(false);
    }
  }

  async function onDelete() {
    const ok = await confirm({
      title: t("form.deleteTitle"),
      message: t("form.deleteMsg", { name: p.name }),
      okLabel: t("form.deleteConfirm"),
      danger: true,
    });
    if (!ok) return;
    try {
      await removeProvider(p.id);
      toast(t("common.deleted"), "ok");
      navigate("providers");
    } catch (e) {
      toast(errMsg(e), "error");
    }
  }

  const statCard = (icon: string, label: string, value: ReactNode, foot: ReactNode) => (
    <div className="card stat">
      <div className="stat-top">
        <Icon name={icon} />
        <span>{label}</span>
      </div>
      <div className="stat-value">{value}</div>
      <div className="stat-foot">{foot}</div>
    </div>
  );

  function metricRows(): ReactNode {
    const list = (snap && snap.metrics) || [];
    if (!list.length) {
      return (
        <div className="metric-row">
          <span className="k">{t("detail.noMetrics")}</span>
          <span className="v">—</span>
        </div>
      );
    }
    return list.map((m, i) => (
      <div className="metric-row" key={i} data-tip={m.hint || undefined}>
        <span className="k">{m.label}</span>
        {m.hint ? <span className="h">{m.hint}</span> : null}
        <span className="v">{m.value}</span>
      </div>
    ));
  }

  function quotaRows(): ReactNode {
    const list = (snap && snap.quotas) || [];
    if (!list.length) return <div className="chart-box">—</div>;
    return list.map((q, i) => {
      const pct = Math.max(0, Math.min(100, Number(q.percent) || 0));
      return (
        <div className="quota-item" key={i}>
          <div className="quota-top">
            <span className="q-label">{q.label}</span>
            {q.resetIn || q.resetAt ? (
              <span className="q-reset">
                <Icon name="clock" />
                {q.resetIn || q.resetAt}
              </span>
            ) : null}
          </div>
          <div className="quota-pct">
            {pct.toFixed(2)} <span className="unit">%</span>
          </div>
          {info.quotaOnly ? <div className="sub">{t(info.cliUsage ? "plan.quotaUsage" : "plan.creditsUsage", { used: q.used.toLocaleString(undefined, { maximumFractionDigits: 8 }), total: q.quota.toLocaleString(undefined, { maximumFractionDigits: 8 }), remaining: (q.quota - q.used).toLocaleString(undefined, { maximumFractionDigits: 8 }) })}</div> : null}
          <div className="bar">
            <i style={{ width: pct.toFixed(2) + "%" }} />
          </div>
          <div className="quota-scale">
            <span>0%</span>
            <span>50%</span>
            <span>100%</span>
          </div>
        </div>
      );
    });
  }

  function modelTable(): ReactNode {
    const models = (snap && snap.models) || [];
    if (!models.length) return null;
    const max = Math.max(...models.map((m) => m.cost), 1);
    return (
      <div className="card" style={{ marginTop: 16 }}>
        <div className="card-head">
          <h3>{t(billingUsage ? "billing.models" : "detail.models")}</h3>
          <span className="sub">{t(billingUsage ? "billing.modelsSub" : info.independentUsageSources ? "usage.monthModelsSub" : "detail.modelsSub", { n: models.length })}</span>
        </div>
        <div className="table-wrap" style={{ border: "none", borderRadius: 0 }}>
          <table className="data">
            <thead>
              <tr>
                <th>{t("detail.th.model")}</th>
                {!billingUsage ? <th className="num">{t("detail.th.requests")}</th> : null}
                <th className="num">{billingUsage ? t("billing.monthTokens") : "Token"}</th>
                <th className="num">{t(billingUsage ? "billing.billedCost" : "detail.th.cost")}</th>
                <th style={{ width: 180 }}>{t("detail.th.share")}</th>
              </tr>
            </thead>
            <tbody>
              {models.map((m, i) => (
                <tr key={i}>
                  <td className="model-cell">{m.model}</td>
                  {!billingUsage ? <td className="num">{fmt.int(m.requests)}</td> : null}
                  <td className="num">{billingUsage && (billedTokens == null || m.tokens <= 0) ? "—" : fmt.compact(m.tokens)}</td>
                  <td className="num">{money(usageValue(snap, m.cost))}</td>
                  <td>
                    {usageValue(snap, m.cost) == null ? "—" : <div className="bar">
                      <i style={{ width: ((m.cost / max) * 100).toFixed(1) + "%" }} />
                    </div>}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
    );
  }

  function logTable(): ReactNode {
    const logs = (snap && snap.logs) || [];
    return (
      <div className="card" style={{ marginTop: 16 }}>
        <div className="card-head">
          <h3>{t("detail.logs")}</h3>
          <span className="sub">{t("detail.logsSub")}</span>
        </div>
        <div className="table-wrap" style={{ border: "none", borderRadius: 0 }}>
          <table className="data">
            <thead>
              <tr>
                <th>{t("detail.th.time")}</th>
                <th>{t("detail.th.model")}</th>
                <th className="num">Token</th>
                <th className="num">{t("detail.th.cost")}</th>
                <th className="num">{t("detail.th.duration")}</th>
                <th>{t("detail.th.status")}</th>
              </tr>
            </thead>
            <tbody>
              {logs.map((l) => (
                <tr key={l.id}>
                  <td>{fmt.shortTime(l.time)}</td>
                  <td className="model-cell">{l.model}</td>
                  <td className="num">{fmt.compact(l.tokens)}</td>
                  <td className="num">{fmt.money(l.cost, cur)}</td>
                  <td className="num">{fmt.duration(l.durationMs)}</td>
                  <td>{l.status || t("detail.statusSuccess")}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
    );
  }

  /* ----------------------------- API Keys ------------------------------ */

  async function saveKeys(current: ProviderConfig, keys: ApiKeyInfo[]) {
    try {
      await saveProvider({ ...current, apiKeys: keys });
      toast(t("common.saved"), "ok");
      await refreshOne(current.id);
    } catch (e) {
      toast(errMsg(e), "error");
    }
  }

  function openKeyForm() {
    openModal({
      title: t("detail.keyAddTitle"),
      subtitle: t("detail.keyAddSub"),
      body: (
        <div className="form-grid">
          <div className="field span-2">
            <label>{t("detail.keyName")}</label>
            <input ref={keyNameRef} className="input" placeholder={t("detail.keyNamePlaceholder")} />
          </div>
          <div className="field span-2">
            <label>{t("detail.keyType")}</label>
            <select ref={keyTypeRef} className="select" defaultValue="api">
              <option value="api">{t("detail.typeApi")}</option>
              <option value="aksk">{t("detail.typeAksk")}</option>
              <option value="system">{t("detail.typeSystem")}</option>
            </select>
          </div>
          <div className="field span-2">
            <label>{t("form.accessKeyId")} ({t("detail.typeAksk")})</label>
            <input ref={keyAccessIdRef} className="input" placeholder={t("form.accessKeyId")} />
          </div>
          <div className="field span-2">
            <label>{t("detail.keySecret")}</label>
            <input ref={keySecretRef} className="input" type="password" placeholder="sk-xxxxxxxx" />
            <span className="hint">{t("detail.keySecretHint")}</span>
          </div>
          <div className="field">
            <label>{t("detail.keyCreated")}</label>
            <input ref={keyCreatedRef} className="input" type="date" />
          </div>
        </div>
      ),
      buttons: [
        { label: t("common.cancel"), onClick: () => closeModal() },
        {
          label: t("common.save"),
          kind: "btn-primary",
          onClick: async () => {
            const name = (keyNameRef.current?.value || "").trim() || t("detail.defaultName");
            const keyType = keyTypeRef.current?.value as ApiKeyInfo["keyType"] || "api";
            const current = provider(p.id) || p;
            const keys = [...(current.apiKeys || [])];
            keys.push({
              id: "key_" + Date.now().toString(16),
              name,
              keyType,
              masked: null,
              secret: keyType === "aksk"
                ? [keyAccessIdRef.current?.value.trim(), keySecretRef.current?.value.trim()].filter(Boolean).join(" / ") || null
                : (keySecretRef.current?.value || "").trim() || null,
              createdAt: keyCreatedRef.current?.value || null,
              source: "manual",
            });
            await saveKeys(current, keys);
            closeModal();
          },
        },
      ],
    });
  }

  async function copyKey(k: ApiKeyInfo) {
    const text = k.secret || k.masked || "";
    if (!text) {
      toast(t("common.nothingToCopy"), "error");
      return;
    }
    try {
      if (!navigator.clipboard || !navigator.clipboard.writeText) throw new Error("clipboard unavailable");
      await navigator.clipboard.writeText(text);
      toast(t("common.copyOk"), "ok");
    } catch {
      toast(t("common.copyFail"), "error");
    }
  }

  async function deleteKey(k: ApiKeyInfo) {
    const current = provider(p.id) || p;
    const ok = await confirm({
      title: t("detail.keyDeleteTitle"),
      message: t("detail.keyDeleteMsg", { name: k.name || t("detail.defaultName") }),
      okLabel: t("common.delete"),
      danger: true,
    });
    if (!ok) return;
    const keys = (current.apiKeys || []).filter((item) => item.id !== k.id);
    await saveKeys(current, keys);
  }

  function apiKeysSection(): ReactNode {
    // 手动录入的完整密钥只存在账号配置里，快照里只有脱敏值
    const manual: ApiKeyInfo[] = (p.apiKeys || []).map((k) => ({ ...k, source: "manual" }));
    const remote = ((snap && snap.apiKeys) || []).filter((k) => k.source !== "manual");
    const name = p.credentialName?.trim() || t("detail.defaultName");
    const isAksk = !!info.akSk;
    const secret = isAksk ? p.accessKeySecret : p.token;
    const credential: ApiKeyInfo[] = (p.accessMode || (p.sessionToken || p.platformToken ? "login" : "api")) === "api" && (isAksk ? p.accessKeyId : secret)
      ? [{
          id: "credential_" + p.id,
          name,
          keyType: isAksk ? "aksk" : info.credentialType || "api",
          masked: (isAksk ? p.accessKeyId : secret)
            ? (isAksk ? p.accessKeyId! : secret!).slice(0, 3) + "****" + (isAksk ? p.accessKeyId! : secret!).slice(-4)
            : null,
          secret: isAksk ? p.accessKeyId && secret ? p.accessKeyId + " / " + secret : p.accessKeyId : secret,
          source: "manual",
        }]
      : [];
    const keys: ApiKeyInfo[] = credential.concat(manual, remote.filter((k) =>
      !credential.some((c) => c.keyType === "api" && c.masked === k.masked)
    ));
    const sub = keys.length
      ? t("detail.apiKeysCount", { n: keys.length, m: manual.length })
      : t("detail.apiKeysNone");

    return (
      <div className="card" style={{ marginTop: 16 }}>
        <div className="card-head">
          <h3>{t("detail.apiKeys")}</h3>
          <span className="sub">{sub}</span>
          <div className="right">
            <button className="btn" onClick={openKeyForm}>
              <Icon name="plus" />
              {t("detail.keyAdd")}
            </button>
          </div>
        </div>
        {keys.length ? (
          <div className="table-wrap" style={{ border: "none", borderRadius: 0 }}>
            <table className="data">
              <thead>
                <tr>
                  <th>{t("detail.th.name")}</th>
                  <th>{t("detail.keyType")}</th>
                  <th>{t("detail.th.key")}</th>
                  <th>{t("detail.th.createdAt")}</th>
                  <th style={{ width: 110 }}></th>
                </tr>
              </thead>
              <tbody>
                {keys.map((k, i) => {
                  const isManual = k.source === "manual" && !k.id.startsWith("credential_");
                  return (
                    <tr key={k.id || i}>
                      <td className="model-cell">
                        {k.name || t("detail.defaultName")}
                        {isManual ? (
                          <>
                            {" "}
                            <span className="chip chip-warn">{t("detail.manual")}</span>
                          </>
                        ) : null}
                      </td>
                      <td>{t(k.keyType === "aksk" ? "detail.typeAksk" : k.keyType === "system" ? "detail.typeSystem" : "detail.typeApi")}</td>
                      <td>
                        {k.masked || k.secret ? (
                          <span className="key-cell">{k.masked || k.secret!.slice(0, 3) + "****" + k.secret!.slice(-4)}</span>
                        ) : (
                          <span style={{ color: "var(--text-mute)" }}>—</span>
                        )}
                      </td>
                      <td>{keyDate(k.createdAt)}</td>
                      <td>
                        <button
                          className="btn btn-icon btn-ghost"
                          title={t("common.copy")}
                          onClick={() => void copyKey(k)}
                        >
                          <Icon name="key" />
                        </button>
                        {isManual ? (
                          <button
                            className="btn btn-icon btn-ghost"
                            title={t("common.delete")}
                            onClick={() => void deleteKey(k)}
                          >
                            <Icon name="trash" />
                          </button>
                        ) : null}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        ) : (
          <div className="empty" style={{ padding: 24 }}>
            <p>{t("detail.noKeys")}</p>
            <button className="btn" onClick={openKeyForm}>
              <Icon name="plus" />
              {t("detail.keyAdd")}
            </button>
          </div>
        )}
      </div>
    );
  }

  return (
    <>
      <div className="card">
        <div className="detail-head">
          <button className="detail-back" onClick={() => navigate("providers")}>
            <Icon name="arrowLeft" />
            {t("common.back")}
          </button>
          <div className="detail-icon" style={{ background: p.accent || info.accent || FALLBACK_ACCENT }}>
            {p.icon || info.icon}
          </div>
          <div className="detail-title">
            <h2>
              {p.name}
              <span className={"chip " + info.chip}>{t(info.detailLabel)}</span>
              <span className="chip">{modeLabelOf(p.kind, p.accessMode === "login" ? "login" : "api")}</span>
              {p.enabled === false ? <span className="chip">{t("detail.disabled")}</span> : null}
            </h2>
            <div className="meta">
              <span>{p.baseUrl || "—"}</span>
              {snap && snap.account ? <span>{t("common.account", { v: snap.account })}</span> : null}
              <span>{t("common.updatedAt", { t: fmt.ago(snap ? snap.fetchedAt : undefined) })}</span>
              {p.notes ? <span>{p.notes}</span> : null}
            </div>
          </div>
          <div className="detail-actions">
            <button className="btn" onClick={() => void onRefresh()} disabled={busy}>
              {busy ? (
                <span className="spinner" />
              ) : (
                <>
                  <Icon name="refresh" />
                  {t("common.refresh")}
                </>
              )}
            </button>
            <button className="btn" onClick={() => openProviderForm(p)}>
              <Icon name="edit" />
              {t("common.edit")}
            </button>
            <button className="btn btn-danger" onClick={() => void onDelete()}>
              <Icon name="trash" />
              {t("common.delete")}
            </button>
          </div>
        </div>
        {snap && snap.error ? (
          <div style={{ padding: "0 20px 18px" }}>
            <div className="error-banner">
              <Icon name="alert" />
              <span>{snap.error}</span>
            </div>
          </div>
        ) : null}
      </div>

      {info.quotaOnly ? <div className="sub" style={{ margin: "16px 0" }}>{t(info.planHint || "")}</div> : <div className="detail-stats" style={{ marginTop: 16 }}>
        {statCard(
          "wallet",
          t(billingUsage ? "billing.accountBalance" : balanceOnly ? "kimi.availableBalance" : "detail.remaining"),
          snap && snap.balance != null ? money(snap.balance) : "—",
          billingUsage ? t("billing.accountScope") : snap && snap.balanceRaw != null
            ? t("detail.rawPoints", { v: fmt.int(snap.balanceRaw) })
            : snap && snap.account
              ? t("common.account", { v: snap.account })
              : "—"
        )}
        {billingUsage ? (
          <>
            {statCard("coins", t("billing.monthCost"), snap?.month != null ? money(snap.month.cost) : "—", t("billing.scope"))}
            {statCard("activity", t("billing.monthTokens"), billedTokens != null ? fmt.compact(billedTokens) : "—", t("billing.tokenScope"))}
          </>
        ) : balanceOnly ? (
          <>
            {statCard("coins", t("kimi.voucherBalance"), balancePart(/代金券|赠送|voucher/i), "")}
            {statCard("wallet", t("kimi.cashBalance"), balancePart(/现金|cash/i), "")}
          </>
        ) : (
          <>
            {statCard("zap", t("detail.todayCost"), money(tCost), t("detail.todayReqs", { n: fmt.int(tReq) }))}
            {statCard("coins", t("detail.monthTokens"), fmt.compact(mTok), t("detail.today", { v: fmt.compact(tTok) }))}
            {statCard(
              "activity",
              t(info.independentUsageSources ? "billing.monthCost" : "detail.totalCost"),
              info.independentUsageSources ? money(usageValue(snap, snap?.month?.cost)) : snap && snap.used != null ? money(snap.used) : "—",
              t("detail.monthReqs", { n: fmt.int(mReq) })
            )}
          </>
        )}
      </div>}

      {balanceOnly ? <div className="sub" style={{ margin: "16px 0" }}>{t("kimi.noUsageApi")}</div> : null}
      {billingUsage ? <div className="sub" style={{ margin: "16px 0" }}>{t("billing.scope")}</div> : null}
      {info.usageScope ? <div className="sub" style={{ margin: "16px 0" }}>{t(info.usageScope)}</div> : null}
      <div className="detail-grid" style={billingUsage && !days.length ? { gridTemplateColumns: "1fr" } : undefined}>
        {(!billingUsage || days.length > 0) ? <div className="card">
          <div className="card-head">
            <h3>{t(info.quotaOnly ? "detail.codePlan" : "detail.trend")}</h3>
            {info.quotaOnly ? null : <span className="sub">{t("detail.last30")}</span>}
            <div className="right">
              <div className="segmented">
                {segments.map((m) => (
                  <button
                    key={m.key}
                    className={activeSeg === m.key ? "is-active" : undefined}
                    onClick={() => setSeg(m.key)}
                  >
                    {t(m.label)}
                  </button>
                ))}
              </div>
            </div>
          </div>
          {isQuota ? (
            <div className="quota-list">{quotaRows()}</div>
          ) : noUsage ? (
            <div className="chart-box">{t("kimi.noUsageApi")}</div>
          ) : info.independentUsageSources && !days.some((d) => usageValue(snap, d[chartKey]) != null) ? (
            <div className="chart-box">{t("usage.unknown")}</div>
          ) : (
            <div className="chart-box">
              <AreaChart
                data={days.slice(-30).filter((d) => usageValue(snap, d[chartKey]) != null).map((d) => ({ label: d.date, value: d[chartKey] || 0 }))}
                height={210}
                color={p.accent || FALLBACK_ACCENT}
                format={activeSeg === "cost" ? (v) => fmt.money(v, cur) : (v) => fmt.compact(v)}
              />
            </div>
          )}
        </div> : null}

        <div className="card">
          <div className="card-head">
            <h3>{t("detail.metrics")}</h3>
            <span className="sub">{t("detail.metricsSub")}</span>
          </div>
          <div className="metric-list">{metricRows()}</div>
        </div>
      </div>

      {modelTable()}
      {snap && snap.logs && snap.logs.length ? logTable() : null}
      {apiKeysSection()}
    </>
  );
}