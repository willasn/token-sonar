/* TokenSonar · 前端状态层：单一数据源 + 聚合计算 + 事件订阅
 *
 * 对应旧的 store.js + TL.emit("change") 全局重绘：这里用 React state 驱动，
 * 所有后端写操作统一走 api.ts（唯一调用层）。
 */
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { api } from "../lib/api";
import { kindInfo, providerInfo, usageValue } from "../lib/kinds";
import type {
  AppInfo,
  AppSettings,
  DailyUsage,
  ProviderConfig,
  RemoteBackup,
  SyncConfig,
  UsageSnapshot,
} from "../types";

export type ViewName = "dashboard" | "providers" | "pricing" | "detail" | "settings";
export type Metric = "cost" | "tokens" | "requests";

interface StoreState {
  providers: ProviderConfig[];
  snapshots: Record<string, UsageSnapshot>;
  settings: AppSettings | null;
  appInfo: AppInfo | null;
  sync: SyncConfig | null;

  view: ViewName;
  detailId: string | null;
  search: string;
  filter: string;
  metric: Metric;
  loading: boolean;
  lastError: string | null;
  ready: boolean;
}

const initialState: StoreState = {
  providers: [],
  snapshots: {},
  settings: null,
  appInfo: null,
  sync: null,

  view: "dashboard",
  detailId: null,
  search: "",
  filter: "all",
  metric: "cost",
  loading: false,
  lastError: null,
  ready: false,
};

export interface AggregateProviderRow {
  id: string;
  name: string;
  accent?: string | null;
  icon?: string | null;
  kind: string;
  planType?: ProviderConfig["planType"];
  currency?: string | null;
  balance?: number | null;
  used?: number | null;
  today: { requests: number; tokens: number; cost: number };
  last7: number;
  days: DailyUsage[];
  error?: string | null;
  fetchedAt: string;
}

export interface AggregateModel {
  model: string;
  cost: number;
  tokens: number;
  requests: number;
  providerNames: string[];
}

export interface Aggregate {
  rows: { provider: ProviderConfig; snap: UsageSnapshot | null }[];
  currencies: string[];
  currency: string;
  mixedCurrency: boolean;
  balance: number;
  used: number;
  todayCost: number;
  todayTokens: number;
  todayRequests: number;
  tokensTotal: number;
  requestsTotal: number;
  errorCount: number;
  days: DailyUsage[];
  models: AggregateModel[];
  perProvider: AggregateProviderRow[];
  last7Cost: number;
  prev7Cost: number;
  metricKey: Metric;
}

export function computeAggregate(state: StoreState): Aggregate {
  const rows = state.providers
    .filter((p) => p.enabled !== false && state.snapshots[p.id])
    .map((p) => ({ provider: p, snap: state.snapshots[p.id] }));

  const currencies: Record<string, true> = {};
  let balance = 0;
  let used = 0;
  let todayCost = 0;
  let todayTokens = 0;
  let todayRequests = 0;
  let tokensTotal = 0;
  let requestsTotal = 0;
  let errorCount = 0;

  const dayMap: Record<string, DailyUsage> = {};
  const modelMap: Record<string, { model: string; cost: number; tokens: number; requests: number; providers: Record<string, true> }> = {};
  const perProvider: AggregateProviderRow[] = [];

  rows.forEach(({ provider: p, snap: s }) => {
    if (!s) return;
    currencies[s.currency || p.currency || "CNY"] = true;

    if (s.error) errorCount += 1;
    if (s.balance != null) balance += s.balance;
    if (s.used != null) used += s.used;
    if (s.tokensTotal != null || kindInfo(s.kind).independentUsageSources || providerInfo(p).quotaOnly)
      tokensTotal += usageValue(s, s.tokensTotal) ?? Number.NaN;
    if (s.requestsTotal != null || kindInfo(s.kind).independentUsageSources || providerInfo(p).quotaOnly)
      requestsTotal += usageValue(s, s.requestsTotal) ?? Number.NaN;

    const unknown = kindInfo(s.kind).independentUsageSources || providerInfo(p).quotaOnly ? Number.NaN : 0;
    const t = s.today || { requests: unknown, tokens: unknown, cost: unknown, from: "", to: "" };
    todayCost += usageValue(s, t.cost) ?? Number.NaN;
    todayTokens += usageValue(s, t.tokens) ?? Number.NaN;
    todayRequests += usageValue(s, t.requests) ?? Number.NaN;

    (s.days || []).forEach((d) => {
      if (!dayMap[d.date]) dayMap[d.date] = { date: d.date, cost: 0, tokens: 0, requests: 0 };
      dayMap[d.date].cost += usageValue(s, d.cost) ?? Number.NaN;
      dayMap[d.date].tokens += usageValue(s, d.tokens) ?? Number.NaN;
      dayMap[d.date].requests += usageValue(s, d.requests) ?? Number.NaN;
    });

    (s.models || []).forEach((m) => {
      if (!modelMap[m.model])
        modelMap[m.model] = { model: m.model, cost: 0, tokens: 0, requests: 0, providers: {} };
      modelMap[m.model].cost += usageValue(s, m.cost) ?? Number.NaN;
      modelMap[m.model].tokens += usageValue(s, m.tokens) ?? Number.NaN;
      modelMap[m.model].requests += usageValue(s, m.requests) ?? Number.NaN;
      modelMap[m.model].providers[p.name] = true;
    });

    let last7 = 0;
    (s.days || []).slice(-7).forEach((d) => {
      last7 += usageValue(s, d.cost) ?? Number.NaN;
    });

    perProvider.push({
      id: p.id,
      name: p.name,
      accent: p.accent,
      icon: p.icon,
      kind: p.kind,
      planType: p.planType,
      currency: s.currency || p.currency,
      balance: s.balance,
      used: s.used,
      today: kindInfo(s.kind).independentUsageSources || providerInfo(p).quotaOnly
        ? { requests: s.today?.requests ?? -1, tokens: s.today?.tokens ?? -1, cost: s.today?.cost ?? -1 }
        : { requests: t.requests || 0, tokens: t.tokens || 0, cost: t.cost || 0 },
      last7: last7,
      days: s.days || [],
      error: s.error,
      fetchedAt: s.fetchedAt,
    });
  });

  const days = Object.keys(dayMap)
    .sort()
    .map((k) => dayMap[k]);

  const models = Object.keys(modelMap)
    .map((k) => {
      const m = modelMap[k];
      return {
        model: m.model,
        cost: m.cost,
        tokens: m.tokens,
        requests: m.requests,
        providerNames: Object.keys(m.providers),
      };
    })
    .sort((a, b) => (Number.isFinite(b.cost) ? b.cost : -1) - (Number.isFinite(a.cost) ? a.cost : -1));

  perProvider.sort((a, b) => (b.today.cost || 0) - (a.today.cost || 0));

  const currencyKeys = Object.keys(currencies);
  return {
    rows,
    currencies: currencyKeys,
    currency: currencyKeys.length === 1 ? currencyKeys[0] : "CNY",
    mixedCurrency: currencyKeys.length > 1,
    balance,
    used,
    todayCost,
    todayTokens,
    todayRequests,
    tokensTotal,
    requestsTotal,
    errorCount,
    days,
    models,
    perProvider,
    last7Cost: days.length || Number.isFinite(todayCost) ? days.slice(-7).reduce((a, d) => a + d.cost, 0) : Number.NaN,
    prev7Cost: days.length || Number.isFinite(todayCost) ? days.slice(-14, -7).reduce((a, d) => a + d.cost, 0) : Number.NaN,
    metricKey: state.metric,
  };
}

interface StoreContextValue {
  state: StoreState;
  aggregate: Aggregate;
  provider: (id: string | null) => ProviderConfig | null;
  snapshot: (id: string) => UsageSnapshot | null;

  init: () => Promise<void>;
  reload: () => Promise<void>;
  refreshAll: (silent?: boolean) => Promise<void>;
  refreshOne: (id: string) => Promise<UsageSnapshot>;
  saveProvider: (p: ProviderConfig) => Promise<ProviderConfig>;
  removeProvider: (id: string) => Promise<void>;
  reorderProviders: (ids: string[]) => Promise<ProviderConfig[]>;
  toggleEnabled: (p: ProviderConfig) => Promise<void>;
  saveSettings: (next: AppSettings) => Promise<void>;
  loadSync: () => Promise<SyncConfig>;
  saveSyncConfig: (cfg: SyncConfig) => Promise<SyncConfig>;
  pullRemote: () => Promise<RemoteBackup>;
  pushMerged: (providers: ProviderConfig[], includeCredentials: boolean) => Promise<unknown>;

  navigate: (view: ViewName, id?: string) => void;
  setSearch: (v: string) => void;
  setFilter: (v: string) => void;
  setMetric: (v: Metric) => void;
}

const StoreContext = createContext<StoreContextValue | null>(null);

export function StoreProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<StoreState>(initialState);
  // 供异步链路读取最新 state（避免闭包拿到旧值）
  const ref = useRef(state);
  ref.current = state;

  const init = useCallback(async () => {
    const [providers, snapshots, settings, appInfo] = await Promise.all([
      api.listProviders(),
      api.getSnapshots(),
      api.getSettings(),
      api.getAppInfo(),
    ]);
    const snapMap: Record<string, UsageSnapshot> = {};
    (snapshots || []).forEach((s) => {
      snapMap[s.providerId] = s;
    });
    setState((prev) => ({
      ...prev,
      providers: providers || [],
      snapshots: snapMap,
      settings,
      appInfo,
      view: ((settings && settings.lastView) || "dashboard") as ViewName,
      ready: true,
    }));
  }, []);

  const reload = useCallback(async () => {
    const [providers, snapshots, settings] = await Promise.all([
      api.listProviders(),
      api.getSnapshots(),
      api.getSettings(),
    ]);
    const snapMap: Record<string, UsageSnapshot> = {};
    (snapshots || []).forEach((s) => {
      snapMap[s.providerId] = s;
    });
    setState((prev) => ({
      ...prev,
      providers: providers || [],
      snapshots: snapMap,
      settings: settings || prev.settings,
    }));
  }, []);

  const refreshAll = useCallback(async (silent?: boolean) => {
    if (ref.current.loading) return;
    setState((prev) => ({ ...prev, loading: true, lastError: null }));
    try {
      const snaps = await api.refreshAll();
      setState((prev) => {
        const next = { ...prev.snapshots };
        (snaps || []).forEach((s) => {
          next[s.providerId] = s;
        });
        return { ...prev, snapshots: next };
      });
    } catch (e) {
      setState((prev) => ({ ...prev, lastError: e instanceof Error ? e.message : String(e) }));
    } finally {
      setState((prev) => ({ ...prev, loading: false }));
    }
    void silent;
  }, []);

  const refreshOne = useCallback(async (id: string) => {
    const s = await api.refreshProvider(id);
    setState((prev) => ({ ...prev, snapshots: { ...prev.snapshots, [s.providerId]: s } }));
    return s;
  }, []);

  const saveProvider = useCallback(async (p: ProviderConfig) => {
    const saved = await api.saveProvider(p);
    setState((prev) => {
      const idx = prev.providers.findIndex((x) => x.id === saved.id);
      const providers = prev.providers.slice();
      if (idx >= 0) providers[idx] = saved;
      else providers.push(saved);
      return { ...prev, providers };
    });
    return saved;
  }, []);

  const removeProvider = useCallback(async (id: string) => {
    await api.removeProvider(id);
    setState((prev) => {
      const snapshots = { ...prev.snapshots };
      delete snapshots[id];
      return {
        ...prev,
        providers: prev.providers.filter((p) => p.id !== id),
        snapshots,
      };
    });
  }, []);

  const reorderProviders = useCallback(async (ids: string[]) => {
    const list = await api.reorderProviders(ids);
    if (Array.isArray(list)) setState((prev) => ({ ...prev, providers: list }));
    return Array.isArray(list) ? list : [];
  }, []);

  const toggleEnabled = useCallback(async (p: ProviderConfig) => {
    const next = Object.assign({}, p, { enabled: !p.enabled });
    await saveProvider(next);
    if (next.enabled) await refreshOne(next.id);
  }, [saveProvider, refreshOne]);

  const saveSettings = useCallback(async (next: AppSettings) => {
    setState((prev) => ({ ...prev, settings: next }));
    await api.saveSettings(next);
  }, []);

  const loadSync = useCallback(async () => {
    const sync = await api.getSyncConfig();
    setState((prev) => ({ ...prev, sync }));
    return sync;
  }, []);

  const saveSyncConfig = useCallback(async (cfg: SyncConfig) => {
    const sync = await api.saveSyncConfig(cfg);
    setState((prev) => ({ ...prev, sync }));
    return sync;
  }, []);

  const pullRemote = useCallback(async () => api.pullRemoteBackup(), []);

  const pushMerged = useCallback(
    async (providers: ProviderConfig[], includeCredentials: boolean) => {
      const res = await api.syncToRepo(providers, includeCredentials);
      await reload();
      return res;
    },
    [reload]
  );

  const navigate = useCallback((view: ViewName, id?: string) => {
    setState((prev) => {
      // 价格视图与其它视图的搜索语义不同，进出时清空，避免互相干扰
      const clearSearch = view !== prev.view && (view === "pricing" || prev.view === "pricing");
      if (view !== "settings" && prev.settings) {
        const last = view === "detail" ? "providers" : view;
        if (prev.settings.lastView !== last) {
          const settings = { ...prev.settings, lastView: last };
          api.saveSettings(settings).catch(() => {});
          return {
            ...prev,
            view,
            detailId: id || prev.detailId,
            search: clearSearch ? "" : prev.search,
            settings,
          };
        }
      }
      return {
        ...prev,
        view,
        detailId: id || prev.detailId,
        search: clearSearch ? "" : prev.search,
      };
    });
  }, []);

  const setSearch = useCallback((v: string) => setState((prev) => ({ ...prev, search: v })), []);
  const setFilter = useCallback((v: string) => setState((prev) => ({ ...prev, filter: v })), []);
  const setMetric = useCallback((v: Metric) => setState((prev) => ({ ...prev, metric: v })), []);

  const provider = useCallback(
    (id: string | null) => (id ? ref.current.providers.find((p) => p.id === id) || null : null),
    []
  );
  const snapshot = useCallback((id: string) => state.snapshots[id] || null, [state.snapshots]);

  const aggregate = useMemo(() => computeAggregate(state), [state]);

  const value: StoreContextValue = {
    state,
    aggregate,
    provider,
    snapshot,
    init,
    reload,
    refreshAll,
    refreshOne,
    saveProvider,
    removeProvider,
    reorderProviders,
    toggleEnabled,
    saveSettings,
    loadSync,
    saveSyncConfig,
    pullRemote,
    pushMerged,
    navigate,
    setSearch,
    setFilter,
    setMetric,
  };

  return <StoreContext.Provider value={value}>{children}</StoreContext.Provider>;
}

export function useStore(): StoreContextValue {
  const ctx = useContext(StoreContext);
  if (!ctx) throw new Error("useStore 必须在 StoreProvider 内使用");
  return ctx;
}

/** 组件挂载时执行一次的便捷 hook（避免额外依赖 eslint 规则） */
export function useMount(fn: () => void) {
  const done = useRef(false);
  useEffect(() => {
    if (done.current) return;
    done.current = true;
    fn();
  }, [fn]);
}
