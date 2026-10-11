/* TokenSonar · 厂商列表视图（React 版）
 *
 * 由旧 ui/assets/js/views/providers.js 的「列表视图」部分 1:1 平移：
 * 卡片 / 列表两种形态、筛选、搜索、启用开关、刷新、删除、跳详情、控制台入口；
 * 「添加 / 编辑账号」表单拆到 components/ProviderForm.tsx。
 *
 * 厂商类型一律查表（KIND_INFO / MODE_INFO），禁止 `kind === "x" ? A : B`。
 * 拖拽排序改用 React 的 HTML5 drag 事件（onDragStart / onDragOver / onDrop），不操作 DOM。
 */
import { useRef, useState, type CSSProperties, type DragEvent } from "react";
import { useStore } from "../state/store";
import { useShell } from "../App";
import { useUi } from "../components/ui";
import { Sparkline } from "../components/charts";
import { Icon } from "../lib/icons";
import { fmt } from "../lib/format";
import { t } from "../lib/i18n";
import { openExternal } from "../lib/api";
import { FILTERS, KIND_FILTERS, MODE_INFO, kindInfo, providerInfo, billingTokens, usageValue, type AccessModeKey } from "../lib/kinds";
import type { ProviderConfig, ProviderKind, QuotaWindow, UsageSnapshot } from "../types";

/* ------------------------------ 查表辅助 ------------------------------ */

/** 接入方式文案（查 MODE_INFO） */
function modeLabel(kind: string, mode: AccessModeKey): string {
  const table = MODE_INFO[kind] || MODE_INFO.deepseek;
  const it = table[mode] || table.api;
  return t(it ? it.label : "");
}

/** 渠道标签：颜色类名 + 列表用短名 + 图标 + 主题色 */
function kindChip(kind: string) {
  const info = kindInfo(kind);
  return { cls: info.chip, label: t(info.kindLabel), icon: info.icon, accent: info.accent };
}

/** 卡片左上角「控制台」入口地址：官方厂商用内置地址，中转站用站点地址 */
function consoleUrl(p: ProviderConfig): string {
  return providerInfo(p).console || String(p.baseUrl || "").trim();
}

function errMsg(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/* ------------------------------ 类型 ------------------------------ */

type ItemAct = "refresh" | "toggle" | "edit" | "delete" | "console" | "cardView";
interface ItemOpts {
  url?: string;
  view?: "metrics" | "quota";
}

interface ItemProps {
  p: ProviderConfig;
  snap: UsageSnapshot | null;
  refreshing: boolean;
  dragging: boolean;
  onOpen: () => void;
  onAction: (act: ItemAct, opts?: ItemOpts) => void;
  onDragStart: (e: DragEvent) => void;
  onDragOver: (e: DragEvent) => void;
  onDrop: (e: DragEvent) => void;
  onDragEnd: () => void;
}

/* ------------------------------ 小组件 ------------------------------ */

function StatusChip({ p, snap }: { p: ProviderConfig; snap: UsageSnapshot | null }) {
  const err = snap && snap.error;
  if (p.enabled === false) {
    return (
      <span className="pstatus is-off" title={t("providers.status.off")}>
        <Icon name="stop" />
      </span>
    );
  }
  if (err) {
    return (
      <span className="chip chip-danger">
        <span className="status-dot is-error" />
        {t("providers.status.error")}
      </span>
    );
  }
  if (!snap) return <span className="chip">{t("providers.status.stale")}</span>;
  return (
    <span className="pstatus is-run" title={t("providers.status.ok")}>
      <Icon name="play" />
    </span>
  );
}

function Metric({ k, v, cls }: { k: string; v: string; cls?: string }) {
  return (
    <div className="pmetric">
      <span className="k">{k}</span>
      <span className={"v " + (cls || "")}>{v}</span>
    </div>
  );
}

/** 用量指标行：余额 / 今日费用 / 今日 Token / 今日请求 */
function MetricsRow({
  snap,
  cur,
  today,
  balanceOnly,
  billingUsage,
}: {
  snap: UsageSnapshot | null;
  cur: string;
  today: { requests: number; tokens: number; cost: number };
  balanceOnly: boolean;
  billingUsage: boolean;
}) {
  if (billingUsage) {
    const tokens = billingTokens(snap);
    return (
      <div className="pcard-metrics" title={t("billing.scope")}>
        <Metric k={t("billing.accountBalance")} v={snap?.balance != null ? fmt.money(snap.balance, cur) : "—"} />
        <Metric k={t("billing.monthCost")} v={snap?.month != null ? fmt.money(snap.month.cost, cur) : "—"} />
        <Metric k={t("billing.monthTokens")} v={tokens != null ? fmt.compact(tokens) : "—"} />
      </div>
    );
  }
  if (balanceOnly) {
    const balancePart = (pattern: RegExp) => {
      const value = snap?.metrics.find((m) => pattern.test(m.label))?.value?.trim();
      return value && Number.isFinite(Number(value)) ? fmt.money(Number(value), snap?.currency) : "—";
    };
    return (
      <div className="pcard-metrics">
        <Metric k={t("kimi.availableBalance")} v={snap?.balance != null ? fmt.money(snap.balance, cur) : "—"} />
        <Metric k={t("kimi.voucherBalance")} v={balancePart(/代金券|赠送|voucher/i)} />
        <Metric k={t("kimi.cashBalance")} v={balancePart(/现金|cash/i)} />
      </div>
    );
  }
  return (
    <div className="pcard-metrics">
      <Metric
        k={t("providers.metric.balance")}
        v={snap && snap.balance != null ? fmt.money(snap.balance, cur) : "—"}
      />
      <Metric k={t("providers.metric.todayCost")} v={fmt.money(usageValue(snap, snap?.today?.cost ?? (snap && kindInfo(snap.kind).independentUsageSources ? null : today.cost)), cur)} />
      <Metric k={t("providers.metric.todayTokens")} v={fmt.compact(usageValue(snap, snap?.today?.tokens ?? (snap && kindInfo(snap.kind).independentUsageSources ? null : today.tokens)))} />
      <Metric k={t("providers.metric.todayRequests")} v={fmt.int(usageValue(snap, snap?.today?.requests ?? (snap && kindInfo(snap.kind).independentUsageSources ? null : today.requests)))} />
    </div>
  );
}

/** 套餐额度行：近 5 小时 / 近一周 / 近一月使用率 */
function QuotaRow({ quotas }: { quotas: QuotaWindow[] }) {
  return (
    <div className="pcard-metrics pcard-quotas">
      {!quotas.length ? <Metric k={t("providers.card.showQuota")} v="—" /> : null}
      {quotas.map((q, i) => {
        const pct = Math.max(0, Math.min(100, Number(q.percent) || 0));
        const cls = pct >= 90 ? " is-danger" : pct >= 70 ? " is-warn" : "";
        return (
          <div className="pmetric" key={i}>
            <span className="k">{q.label}</span>
            <span className={"v" + cls}>
              {pct.toFixed(1)}
              <span className="unit">%</span>
            </span>
            <span className="q-reset">{q.resetIn || q.resetAt || "—"}</span>
          </div>
        );
      })}
    </div>
  );
}

/** 卡片中间区域：用量指标行 / 套餐额度行互斥显示，可切换 */
function CardMid({
  p,
  snap,
  quotas,
  cur,
  today,
  onCardView,
}: {
  p: ProviderConfig;
  snap: UsageSnapshot | null;
  quotas: QuotaWindow[];
  cur: string;
  today: { requests: number; tokens: number; cost: number };
  onCardView: (next: "metrics" | "quota") => void;
}) {
  const hasQuota = quotas.length > 0;
  const balanceOnly = !!providerInfo(p).balanceOnly;
  let view: "metrics" | "quota" =
    p.cardView === "quota" || p.cardView === "metrics" ? p.cardView : hasQuota ? "quota" : "metrics";
  if (view === "quota" && !hasQuota) view = "metrics";
  const next: "metrics" | "quota" = view === "quota" ? "metrics" : "quota";
  const title = view === "quota" ? t("providers.card.showMetrics") : t("providers.card.showQuota");
  return (
    <div className="pcard-mid">
      {providerInfo(p).quotaOnly || view === "quota" ? <QuotaRow quotas={quotas} /> : <MetricsRow snap={snap} cur={cur} today={today} balanceOnly={balanceOnly} billingUsage={!!providerInfo(p).billingUsage} />}
      {hasQuota && !providerInfo(p).quotaOnly ? (
        <button
          type="button"
          className="btn btn-icon btn-ghost pcard-mid-toggle"
          title={title}
          onClick={(e) => {
            e.stopPropagation();
            onCardView(next);
          }}
        >
          <Icon name="swap" />
        </button>
      ) : null}
    </div>
  );
}

/** 卡片 / 列表行共用的四个操作按钮：刷新 / 启停 / 编辑 / 删除 */
function ActButtons({
  p,
  refreshing,
  onAction,
}: {
  p: ProviderConfig;
  refreshing: boolean;
  onAction: (act: ItemAct, opts?: ItemOpts) => void;
}) {
  return (
    <div className="pcard-actions">
      <button
        className="btn btn-icon btn-ghost"
        title={t("common.refresh")}
        disabled={refreshing}
        onClick={(e) => {
          e.stopPropagation();
          onAction("refresh");
        }}
      >
        {refreshing ? <span className="spinner" /> : <Icon name="refresh" />}
      </button>
      <button
        className="btn btn-icon btn-ghost"
        title={p.enabled === false ? t("providers.act.enable") : t("providers.act.disable")}
        onClick={(e) => {
          e.stopPropagation();
          onAction("toggle");
        }}
      >
        <Icon name="power" />
      </button>
      <button
        className="btn btn-icon btn-ghost"
        title={t("common.edit")}
        onClick={(e) => {
          e.stopPropagation();
          onAction("edit");
        }}
      >
        <Icon name="edit" />
      </button>
      <button
        className="btn btn-icon btn-ghost"
        title={t("common.delete")}
        onClick={(e) => {
          e.stopPropagation();
          onAction("delete");
        }}
      >
        <Icon name="trash" />
      </button>
    </div>
  );
}

/* ------------------------------ 卡片 / 列表行 ------------------------------ */

function Card(props: ItemProps) {
  const { p, snap, refreshing, dragging, onOpen, onAction, onDragStart, onDragOver, onDrop, onDragEnd } = props;
  const info = providerInfo(p);
  const chip = kindChip(p.kind);
  const accent = p.accent || info.accent;
  const iconText = p.icon || info.icon;
  const cur = (snap && snap.currency) || p.currency || "CNY";
  const today = snap?.today || { requests: 0, tokens: 0, cost: 0, from: "", to: "" };
  const err = snap && snap.error;
  const quotas = (snap && snap.quotas) || [];
  const url = consoleUrl(p);
  const accentStyle = { "--p-accent": accent } as CSSProperties;

  return (
    <div
      className={"pcard card card-hover" + (p.enabled === false ? " is-off" : "") + (dragging ? " is-dragging" : "")}
      data-id={p.id}
      style={accentStyle}
      draggable
      onClick={onOpen}
      onDragStart={onDragStart}
      onDragOver={onDragOver}
      onDrop={onDrop}
      onDragEnd={onDragEnd}
    >
      <div className="pcard-head">
        <span className="pcard-grip" title={t("providers.dragHint")}>
          <Icon name="grip" />
        </span>
        <div className="pcard-icon">{iconText}</div>
        <div className="pcard-title">
          <div className="name">
            {p.name}
            <span className={"chip " + chip.cls}>{chip.label}</span>
            {url ? (
              <button
                type="button"
                className="pcard-console"
                title={t("providers.consoleTitle")}
                onClick={(e) => {
                  e.stopPropagation();
                  onAction("console", { url });
                }}
              >
                <Icon name="external" />
                <span>{t("providers.console")}</span>
              </button>
            ) : null}
          </div>
          <div className="meta">
            {p.baseUrl || (info.official ? info.url.placeholder : t("providers.noBaseUrl"))}
            {snap && snap.account ? " · " + snap.account : ""}
            {" · " + modeLabel(p.kind, p.accessMode === "login" ? "login" : "api")}
          </div>
        </div>
        <div className="pcard-status">
          <StatusChip p={p} snap={snap} />
        </div>
      </div>

      <CardMid
        p={p}
        snap={snap}
        quotas={quotas}
        cur={cur}
        today={today}
        onCardView={(v) => onAction("cardView", { view: v })}
      />

      <div className="pcard-foot">
        <div className="spark" data-spark={p.id}>
          {!info.quotaOnly && ((!info.balanceOnly && !info.billingUsage) || (snap?.days.length ?? 0) > 0) ? (
            <Sparkline values={((snap && snap.days) || []).slice(-14).filter((d) => usageValue(snap, d.cost) != null).map((d) => d.cost || 0)} height={34} color={accent} />
          ) : null}
        </div>
        <div className="hint">
          {err ? (
            <span style={{ color: "var(--danger)" }}>{String(err).slice(0, 26)}</span>
          ) : (
            t("common.updatedAt", { t: fmt.ago(snap && snap.fetchedAt) })
          )}
        </div>
        <ActButtons p={p} refreshing={refreshing} onAction={onAction} />
      </div>
    </div>
  );
}

function Row(props: ItemProps) {
  const { p, snap, refreshing, dragging, onOpen, onAction, onDragStart, onDragOver, onDrop, onDragEnd } = props;
  const info = providerInfo(p);
  const chip = kindChip(p.kind);
  const accent = p.accent || info.accent;
  const iconText = p.icon || info.icon;
  const cur = (snap && snap.currency) || p.currency || "CNY";
  const today = snap?.today || { requests: 0, tokens: 0, cost: 0, from: "", to: "" };
  const err = snap && snap.error;
  const quotas = (snap && snap.quotas) || [];
  const url = consoleUrl(p);
  const accentStyle = { "--p-accent": accent } as CSSProperties;

  return (
    <div
      className={"prow card card-hover" + (p.enabled === false ? " is-off" : "") + (dragging ? " is-dragging" : "")}
      data-id={p.id}
      style={accentStyle}
      draggable
      onClick={onOpen}
      onDragStart={onDragStart}
      onDragOver={onDragOver}
      onDrop={onDrop}
      onDragEnd={onDragEnd}
    >
      <span className="prow-grip" title={t("providers.dragHint")}>
        <Icon name="grip" />
      </span>
      <div className="prow-icon">{iconText}</div>
      <div className="prow-main">
        <div className="prow-top">
          <span className="name">{p.name}</span>
          <span className={"chip " + chip.cls}>{chip.label}</span>
          {url ? (
            <button
              type="button"
              className="prow-console"
              title={t("providers.consoleTitle")}
              onClick={(e) => {
                e.stopPropagation();
                onAction("console", { url });
              }}
            >
              <Icon name="external" />
            </button>
          ) : null}
        </div>
        <CardMid
          p={p}
          snap={snap}
          quotas={quotas}
          cur={cur}
          today={today}
          onCardView={(v) => onAction("cardView", { view: v })}
        />
      </div>
      <div className="prow-right">
        <div className="hint">
          {err ? (
            <span style={{ color: "var(--danger)" }}>{String(err).slice(0, 26)}</span>
          ) : (
            t("common.updatedAt", { t: fmt.ago(snap && snap.fetchedAt) })
          )}
        </div>
        <ActButtons p={p} refreshing={refreshing} onAction={onAction} />
      </div>
    </div>
  );
}

/* ------------------------------ 视图主体 ------------------------------ */

export function Providers() {
  const {
    state,
    refreshOne,
    reorderProviders,
    removeProvider,
    toggleEnabled,
    saveProvider,
    saveSettings,
    navigate,
    setFilter,
  } = useStore();
  const { openProviderForm } = useShell();
  const { toast, confirm } = useUi();

  const [refreshingId, setRefreshingId] = useState<string | null>(null);
  const [dragId, setDragId] = useState<string | null>(null);
  const [dragIds, setDragIds] = useState<string[] | null>(null);
  // 拖拽结束的瞬间会紧跟一次 click，用它把这 300ms 内的点击忽略掉，避免误进详情
  const suppressRef = useRef(0);

  const q = (state.search || "").trim().toLowerCase();
  const view = state.settings && state.settings.providersView === "list" ? "list" : "card";

  let rows = state.providers.map((p) => ({ provider: p, snap: state.snapshots[p.id] || null }));

  // 类型 / 状态筛选
  if (KIND_FILTERS.indexOf(state.filter as ProviderKind) >= 0) {
    rows = rows.filter((r) => r.provider.kind === state.filter);
  } else if (state.filter === "error") {
    rows = rows.filter((r) => r.snap && r.snap.error);
  } else if (state.filter === "off") {
    rows = rows.filter((r) => r.provider.enabled === false);
  }

  // 搜索：按「渠道商名称」或「自定义显示名称」匹配
  if (q) {
    rows = rows.filter((r) => {
      const p = r.provider;
      return (
        (p.name || "").toLowerCase().indexOf(q) >= 0 ||
        t(kindInfo(p.kind).name).toLowerCase().indexOf(q) >= 0
      );
    });
  }

  function setView(view: "card" | "list") {
    if (!state.settings || state.settings.providersView === view) return;
    void saveSettings({ ...state.settings, providersView: view }).catch(() => {});
  }

  async function doRefresh(id: string) {
    setRefreshingId(id);
    try {
      await refreshOne(id);
      toast(t("common.refreshed"), "ok");
    } catch (e) {
      toast(errMsg(e), "error");
    } finally {
      setRefreshingId(null);
    }
  }

  async function doDelete(p: ProviderConfig) {
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
      if (state.view === "detail") navigate("providers");
    } catch (e) {
      toast(errMsg(e), "error");
    }
  }

  function handleAction(act: ItemAct, p: ProviderConfig, opts?: ItemOpts) {
    switch (act) {
      case "refresh":
        void doRefresh(p.id);
        break;
      case "toggle":
        void toggleEnabled(p).catch((e) => toast(errMsg(e), "error"));
        break;
      case "edit":
        openProviderForm(p);
        break;
      case "delete":
        void doDelete(p);
        break;
      case "console":
        if (opts?.url) void openExternal(opts.url).catch((e) => toast(errMsg(e), "error"));
        break;
      case "cardView":
        if (opts?.view) void saveProvider({ ...p, cardView: opts.view }).catch((e) => toast(errMsg(e), "error"));
        break;
    }
  }

  function openItem(p: ProviderConfig) {
    if (Date.now() < suppressRef.current) {
      suppressRef.current = 0;
      return;
    }
    navigate("detail", p.id);
  }

  /* -------------------------- 拖拽排序（HTML5 drag） -------------------------- */

  function handleDragStart(e: DragEvent, id: string) {
    e.dataTransfer.effectAllowed = "move";
    e.dataTransfer.setData("text/plain", id);
    setDragId(id);
    setDragIds(rows.map((r) => r.provider.id));
  }

  function handleDragOver(e: DragEvent, targetId: string) {
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";
    if (!dragId || !dragIds || dragId === targetId) return;
    const from = dragIds.indexOf(dragId);
    const to = dragIds.indexOf(targetId);
    if (from < 0 || to < 0 || from === to) return;
    const next = dragIds.slice();
    next.splice(from, 1);
    next.splice(to, 0, dragId);
    setDragIds(next);
  }

  function handleDrop(e: DragEvent) {
    e.preventDefault();
    if (!dragIds) {
      setDragId(null);
      return;
    }
    // 可见项的新顺序填回完整顺序里，被筛掉的账号保持原位
    const visibleSet: Record<string, boolean> = {};
    dragIds.forEach((id) => {
      visibleSet[id] = true;
    });
    let i = 0;
    const ids = state.providers.map((p) => (visibleSet[p.id] ? dragIds[i++] : p.id));
    suppressRef.current = Date.now() + 300;
    setDragId(null);
    setDragIds(null);
    void reorderProviders(ids).catch((err) => toast(errMsg(err), "error"));
  }

  function handleDragEnd() {
    setDragId(null);
    setDragIds(null);
  }

  const orderedRows = dragIds
    ? rows.slice().sort((a, b) => dragIds.indexOf(a.provider.id) - dragIds.indexOf(b.provider.id))
    : rows;

  /* ------------------------------ 渲染 ------------------------------ */

  if (!state.providers.length) {
    return (
      <div className="card card-pad">
        <div className="empty">
          <div className="empty-illu">
            <Icon name="server" />
          </div>
          <h3>{t("providers.emptyTitle")}</h3>
          <p>{t("providers.emptyBody")}</p>
          <button className="btn btn-primary" onClick={() => openProviderForm(null)}>
            <Icon name="plus" />
            {t("dash.emptyCta")}
          </button>
        </div>
      </div>
    );
  }

  const toolbar = (
    <div className="plist-toolbar">
      <div className="filters">
        {FILTERS.map((f) => (
          <button
            key={f.key}
            className={"filter-pill" + (state.filter === f.key ? " is-active" : "")}
            onClick={() => setFilter(f.key)}
          >
            {t(f.label)}
          </button>
        ))}
      </div>
      <div className="plist-tools">
        <span className="count">{t("providers.count", { a: rows.length, b: state.providers.length })}</span>
        <div className="segmented plist-viewtoggle">
          <button
            type="button"
            className={view === "card" ? "is-active" : ""}
            title={t("providers.view.card")}
            onClick={() => setView("card")}
          >
            <Icon name="grid" />
          </button>
          <button
            type="button"
            className={view === "list" ? "is-active" : ""}
            title={t("providers.view.list")}
            onClick={() => setView("list")}
          >
            <Icon name="list" />
          </button>
        </div>
      </div>
    </div>
  );

  if (!rows.length) {
    return (
      <>
        {toolbar}
        <div className="card card-pad">
          <div className="empty">
            <div className="empty-illu">
              <Icon name="search" />
            </div>
            <h3>{t("providers.noMatchTitle")}</h3>
            <p>{t("providers.noMatchBody")}</p>
          </div>
        </div>
      </>
    );
  }

  return (
    <>
      {toolbar}
      <div className={"plist" + (view === "list" ? " is-list" : "") + (dragIds ? " is-reordering" : "")}>
        {orderedRows.map((r) => {
          const p = r.provider;
          const common: ItemProps = {
            p,
            snap: r.snap,
            refreshing: refreshingId === p.id,
            dragging: dragId === p.id,
            onOpen: () => openItem(p),
            onAction: (act, opts) => handleAction(act, p, opts),
            onDragStart: (e) => handleDragStart(e, p.id),
            onDragOver: (e) => handleDragOver(e, p.id),
            onDrop: handleDrop,
            onDragEnd: handleDragEnd,
          };
          return view === "list" ? <Row key={p.id} {...common} /> : <Card key={p.id} {...common} />;
        })}
      </div>
    </>
  );
}