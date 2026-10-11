/* TokenSonar · Dashboard 总览视图（React 版）：把旧 ui/assets/js/views/dashboard.js 1:1 平移为组件 */
import { Fragment, type CSSProperties, type ReactNode } from "react";
import { Icon } from "../lib/icons";
import { useT } from "../lib/i18n";
import { fmt } from "../lib/format";
import { AreaChart } from "../components/charts";
import { useStore, type Metric } from "../state/store";
import { useShell } from "../App";
import { kindInfo, providerInfo, usageValue } from "../lib/kinds";

const METRICS: { key: Metric; label: string }[] = [
  { key: "cost", label: "metric.cost" },
  { key: "tokens", label: "metric.tokens" },
  { key: "requests", label: "metric.requests" },
];

const FALLBACK_ACCENT = "#4d6bfe";

export function Dashboard() {
  const { state, aggregate: agg, setMetric, navigate } = useStore();
  const { openProviderForm } = useShell();
  const t = useT();

  if (!state.providers.length) {
    return (
      <div className="card card-pad">
        <div className="empty">
          <div className="empty-illu">
            <Icon name="inbox" />
          </div>
          <h3>{t("dash.emptyTitle")}</h3>
          <p>{t("dash.emptyBody")}</p>
          <button className="btn btn-primary" onClick={() => openProviderForm(null)}>
            <Icon name="plus" />
            {t("dash.emptyCta")}
          </button>
        </div>
      </div>
    );
  }

  const currency = agg.currency;
  const money = (v?: number | null) => fmt.money(v, currency);

  /** 按厂商名返回主题色（用于模型归属着色），缺省查 KIND_INFO */
  const accentOf = (name: string): string => {
    const p = state.providers.find((x) => x.name === name);
    if (!p) return FALLBACK_ACCENT;
    return p.accent || kindInfo(p.kind).accent;
  };

  /** 环比涨跌标签 */
  const delta = (current: number, prev: number): ReactNode => {
    if (!Number.isFinite(current) || !Number.isFinite(prev) || !prev) return <span className="down">—</span>;
    const d = ((current - prev) / prev) * 100;
    const cls = d >= 0 ? "up" : "down";
    const sign = d >= 0 ? "+" : "";
    return <span className={cls}>{t("dash.wow", { p: sign + d.toFixed(1) + "%" })}</span>;
  };

  const statCard = (opts: { icon: string; label: string; value: ReactNode; foot: ReactNode }) => (
    <div className="card stat">
      <div className="stat-top">
        <Icon name={opts.icon} />
        <span>{opts.label}</span>
      </div>
      <div className="stat-value">{opts.value}</div>
      <div className="stat-foot">{opts.foot}</div>
    </div>
  );

  const maxBal = Math.max(...agg.perProvider.map((p) => p.balance || 0), 1);
  const topModels = agg.models.slice(0, 8);
  const maxModel = Math.max(...topModels.map((m) => Number.isFinite(m.cost) ? m.cost : 0), 1);

  return (
    <>
      <div className="dash-stats">
        {statCard({
          icon: "wallet",
          label: t("dash.totalBalance"),
          value: money(agg.balance),
          foot: (
            <>
              {t("dash.accountCount", { n: agg.perProvider.length })}
              {agg.mixedCurrency ? " · " + t("dash.mixedCurrency") : ""}
              {agg.errorCount ? (
                <>
                  {" "}
                  <span className="up">{t("dash.errorSuffix", { n: agg.errorCount })}</span>
                </>
              ) : null}
            </>
          ),
        })}
        {statCard({
          icon: "zap",
          label: t("dash.todayCost"),
          value: money(agg.todayCost),
          foot: (
            <>
              {t("dash.last7", { v: money(agg.last7Cost) })}
              {" · "}
              {delta(agg.last7Cost, agg.prev7Cost)}
            </>
          ),
        })}
        {statCard({
          icon: "coins",
          label: t("dash.todayTokens"),
          value: fmt.compact(agg.todayTokens),
          foot: t("dash.totalTokens", { v: fmt.compact(agg.tokensTotal) }),
        })}
        {statCard({
          icon: "activity",
          label: t("dash.todayRequests"),
          value: fmt.int(agg.todayRequests),
          foot: t("dash.totalRequests", { v: fmt.compact(agg.requestsTotal) }),
        })}
      </div>

      <div className="dash-main">
        <div className="card">
          <div className="card-head">
            <h3>{t("dash.trend")}</h3>
            <span className="sub">{t("dash.trendSub")}</span>
            <div className="right">
              <div className="segmented">
                {METRICS.map((m) => (
                  <button
                    key={m.key}
                    className={state.metric === m.key ? "is-active" : undefined}
                    onClick={() => setMetric(m.key)}
                  >
                    {t(m.label)}
                  </button>
                ))}
              </div>
            </div>
          </div>
          <div className="chart-box">
            <AreaChart
              data={agg.days.filter((d) => Number.isFinite(d[agg.metricKey])).map((d) => ({ label: d.date, value: d[agg.metricKey] }))}
              height={220}
              format={agg.metricKey === "cost" ? (v) => fmt.money(v, currency) : (v) => fmt.compact(v)}
              yFormat={(v) => fmt.compact(v)}
            />
          </div>
        </div>

        <div className="card">
          <div className="card-head">
            <h3>{t("dash.distribution")}</h3>
            <span className="sub">{t("dash.distributionSub")}</span>
          </div>
          <div className="dist-list">
            {!agg.perProvider.length ? (
              <div className="empty">
                <p>{t("dash.noData")}</p>
              </div>
            ) : (
              agg.perProvider.map((p) => {
                const accent = p.accent || kindInfo(p.kind).accent;
                const w = ((p.balance || 0) / maxBal) * 100;
                return (
                  <div className="dist-row" key={p.id}>
                    <div className="dist-row-top">
                      <span className="name">{p.name}</span>
                      <span className="val">{fmt.money(p.balance, p.currency)}</span>
                    </div>
                    <div
                      className="bar"
                      style={{ "--row-accent": `linear-gradient(90deg,${accent},transparent)` } as CSSProperties}
                    >
                      <i
                        style={{
                          width: w.toFixed(1) + "%",
                          background: `linear-gradient(90deg,${accent},transparent)`,
                        }}
                      />
                    </div>
                  </div>
                );
              })
            )}
          </div>
        </div>
      </div>

      <div className="dash-bottom">
        <div className="card">
          <div className="card-head">
            <h3>{t("dash.rank")}</h3>
            <span className="sub">{t("dash.rankSub")}</span>
          </div>
          <div className="rank-list">
            {agg.perProvider.length ? (
              agg.perProvider.slice(0, 8).map((p, i) => (
                <div className="rank-row" key={p.id} onClick={() => navigate("detail", p.id)}>
                  <span className="rank-idx">{i + 1}</span>
                  <span className="rank-dot" style={{ background: p.accent || kindInfo(p.kind).accent }} />
                  <span className="rank-name">
                    {p.name}
                    {p.error ? (
                      <>
                        {" "}
                        <span className="chip chip-danger">{t("dash.abnormal")}</span>
                      </>
                    ) : null}
                  </span>
                  <span className="rank-meta">
                    {providerInfo(p).quotaOnly ? <span title={t(providerInfo(p).planHint || "")}>
                      {state.snapshots[p.id]?.quotas?.length
                        ? state.snapshots[p.id]!.quotas.map((q) => `${q.label} · ${q.percent.toFixed(2)}%`).join(" / ")
                        : `${t(providerInfo(p).planHint || "")} · —`}
                    </span> : providerInfo(p).billingUsage ? (
                      <span title={t("billing.scope")}>
                        {t("billing.monthCost")} <b>{state.snapshots[p.id]?.month != null ? fmt.money(state.snapshots[p.id]?.month?.cost, p.currency) : "—"}</b>
                      </span>
                    ) : <>
                    <span>
                      {t("metric.tokens")} <b>{fmt.compact(usageValue(state.snapshots[p.id] ?? null, p.today.tokens))}</b>
                    </span>
                    <span>
                      {t("metric.requests")} <b>{fmt.int(usageValue(state.snapshots[p.id] ?? null, p.today.requests))}</b>
                    </span>
                    <span>
                      <b>{fmt.money(usageValue(state.snapshots[p.id] ?? null, p.today.cost), p.currency)}</b>
                    </span>
                    </>}
                  </span>
                </div>
              ))
            ) : (
              <div className="empty">
                <p>{t("dash.noData")}</p>
              </div>
            )}
          </div>
        </div>

        <div className="card">
          <div className="card-head">
            <h3>{t("dash.modelsTop")}</h3>
            <span className="sub">{t("dash.modelsSub")}</span>
          </div>
          <div className="rank-list">
            {topModels.length ? (
              topModels.map((m, i) => (
                <Fragment key={m.model}>
                  <div className="rank-row">
                    <span className="rank-idx">{i + 1}</span>
                    <span className="rank-dot" style={{ background: accentOf(m.providerNames[0]) }} />
                    <span className="rank-name">{m.model}</span>
                    <span className="rank-meta">
                      <span>
                        {t("metric.tokens")} <b>{fmt.compact(m.tokens)}</b>
                      </span>
                      <span>
                        <b>{fmt.money(m.cost, currency)}</b>
                      </span>
                    </span>
                  </div>
                  {Number.isFinite(m.cost) ? <div style={{ padding: "0 20px 6px" }}>
                    <div className="bar">
                      <i style={{ width: ((m.cost / maxModel) * 100).toFixed(1) + "%" }} />
                    </div>
                  </div> : null}
                </Fragment>
              ))
            ) : (
              <div className="empty">
                <p>{t("dash.noModels")}</p>
              </div>
            )}
          </div>
        </div>
      </div>
    </>
  );
}