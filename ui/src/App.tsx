/* TokenSonar · 应用外壳：侧边栏、顶栏、路由、主题与自动刷新
 *
 * 由旧 app.js 平移：视图切换、顶栏文案、账号迷你列表、主题、自动刷新、语言引导。
 */
import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import { Icon, BrandMark } from "./lib/icons";
import { fmt } from "./lib/format";
import { t, subscribe as subscribeLocale } from "./lib/i18n";
import { useStore, type ViewName } from "./state/store";
import { useUi } from "./components/ui";
import { Dashboard } from "./views/Dashboard";
import { Providers } from "./views/Providers";
import { Pricing } from "./views/Pricing";
import { Detail } from "./views/Detail";
import { Settings } from "./views/Settings";
import { Onboarding } from "./components/Onboarding";
import { ProviderFormModal } from "./components/ProviderForm";
import type { ProviderConfig } from "./types";

const VIEW_META: Record<ViewName, { title: string; sub: string; icon: string }> = {
  dashboard: { title: "view.dashboard.title", sub: "view.dashboard.sub", icon: "dashboard" },
  providers: { title: "view.providers.title", sub: "view.providers.sub", icon: "grid" },
  pricing: { title: "view.pricing.title", sub: "view.pricing.sub", icon: "coins" },
  detail: { title: "view.detail.title", sub: "view.detail.sub", icon: "grid" },
  settings: { title: "view.settings.title", sub: "view.settings.sub", icon: "settings" },
};

interface ShellContextValue {
  /** 打开「添加 / 编辑账号」弹窗 */
  openProviderForm: (provider?: ProviderConfig | null) => void;
}

const ShellContext = createContext<ShellContextValue | null>(null);

export function useShell(): ShellContextValue {
  const ctx = useContext(ShellContext);
  if (!ctx) throw new Error("useShell 必须在 App 内使用");
  return ctx;
}

function applyTheme(theme: string) {
  document.documentElement.setAttribute("data-theme", theme === "light" ? "light" : "dark");
}

export default function App() {
  const store = useStore();
  const { state, aggregate, navigate, setSearch } = store;
  const { toast } = useUi();

  const [tick, setTick] = useState(0);
  const [formOpen, setFormOpen] = useState(false);
  const [formProvider, setFormProvider] = useState<ProviderConfig | null>(null);
  const [onboardingNeeded, setOnboardingNeeded] = useState(false);
  const refreshTimer = useRef<number | null>(null);

  // 语言切换后整体重绘
  useEffect(() => subscribeLocale(() => setTick((n) => n + 1)), []);

  const openProviderForm = useCallback((provider?: ProviderConfig | null) => {
    setFormProvider(provider || null);
    setFormOpen(true);
  }, []);

  // 启动：拉数据 + 决定是否弹语言引导
  useEffect(() => {
    let cancelled = false;
    store
      .init()
      .then(() => {
        if (cancelled) return;
      })
      .catch((e) => {
        toast(t("shell.initFailed", { msg: e instanceof Error ? e.message : String(e) }), "error");
      });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 主题跟随设置
  useEffect(() => {
    applyTheme(state.settings?.theme || "dark");
  }, [state.settings?.theme]);

  // 首次拿到设置后判断是否需要语言引导；随后触发一次静默刷新
  useEffect(() => {
    if (!state.ready) return;
    const settings = state.settings || null;
    if (!settings || !settings.language) setOnboardingNeeded(true);
    if (state.providers.length) store.refreshAll(true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state.ready]);

  // 自动刷新
  useEffect(() => {
    if (refreshTimer.current) {
      window.clearInterval(refreshTimer.current);
      refreshTimer.current = null;
    }
    const st = state.settings || null;
    if (!st || !st.autoRefresh) return;
    const min = Number(st.refreshIntervalMin) || 15;
    refreshTimer.current = window.setInterval(
      () => {
        store.refreshAll(true);
      },
      Math.max(1, min) * 60 * 1000
    );
    return () => {
      if (refreshTimer.current) window.clearInterval(refreshTimer.current);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state.settings?.autoRefresh, state.settings?.refreshIntervalMin]);

  const view: ViewName = state.view;
  const meta = VIEW_META[view] || VIEW_META.dashboard;
  const detailProvider = state.detailId ? state.providers.find((p) => p.id === state.detailId) : null;
  const title = view === "detail" ? detailProvider?.name || t("view.detail.title") : t(meta.title);
  const rows = aggregate.rows;

  const toggleTheme = () => {
    const next = document.documentElement.getAttribute("data-theme") === "light" ? "dark" : "light";
    applyTheme(next);
    if (state.settings) store.saveSettings({ ...state.settings, theme: next }).catch(() => {});
  };

  const navItems: { key: ViewName; badge?: number }[] = useMemo(
    () => [
      { key: "dashboard" },
      { key: "providers", badge: rows.length },
      { key: "pricing" },
      { key: "settings" },
    ],
    [rows.length]
  );

  const errCount = rows.filter((r) => r.snap && r.snap.error).length;
  const statusText = state.loading
    ? t("shell.syncing")
    : errCount
      ? t("shell.errorCount", { n: errCount })
      : t("shell.synced");

  void tick; // 语言切换重绘

  return (
    <ShellContext.Provider value={{ openProviderForm }}>
      <div className="app">
        <aside className="sidebar">
          <div className="brand">
            <div className="brand-mark" id="brandMark">
              <BrandMark />
            </div>
            <div className="brand-text">
              <span className="brand-name">TokenSonar</span>
              <span className="brand-sub">{t("shell.brandSub")}</span>
            </div>
          </div>

          <nav className="nav">
            {navItems.map((item) => {
              const active = view === item.key || (view === "detail" && item.key === "providers");
              return (
                <button
                  key={item.key}
                  className={"nav-item" + (active ? " is-active" : "")}
                  onClick={() => navigate(item.key)}
                >
                  <Icon name={VIEW_META[item.key].icon} />
                  <span>{t(VIEW_META[item.key].title)}</span>
                  {item.badge != null && <span className="nav-badge">{item.badge}</span>}
                </button>
              );
            })}
          </nav>

          <div className="sidebar-foot">
            <div className="mini-title">{t("shell.accounts")}</div>
            <div className="mini-list" id="sidebarProviders">
              {rows.length === 0 ? (
                <div style={{ padding: "6px 12px", fontSize: 11.5, color: "var(--text-mute)" }}>
                  {t("shell.noAccounts")}
                </div>
              ) : (
                rows.map(({ provider: p, snap }) => {
                  const off = p.enabled === false;
                  const err = !off && snap && snap.error;
                  const cls = "mini-dot" + (off ? " is-off" : err ? " is-error" : "");
                  const accent = p.accent || "#4d6bfe";
                  const bal =
                    snap && snap.balance != null ? fmt.money(snap.balance, snap.currency || p.currency) : "—";
                  return (
                    <button
                      key={p.id}
                      className="mini-item"
                      title={p.name}
                      onClick={() => navigate("detail", p.id)}
                    >
                      <span
                        className={cls}
                        style={
                          off || err
                            ? undefined
                            : { background: accent, boxShadow: "0 0 0 3px " + accent + "22" }
                        }
                      />
                      <span className="mini-name">{p.name}</span>
                      <span className="mini-value">{bal}</span>
                    </button>
                  );
                })
              )}
            </div>
            <div className="app-meta">
              <span id="metaStatus">{statusText}</span>
              <span id="metaVersion">v{(state.appInfo && state.appInfo.version) || "0.1.0"}</span>
            </div>
          </div>
        </aside>

        <main className="main">
          <header className="topbar">
            <div className="topbar-left">
              <h1 id="viewTitle">{title}</h1>
              <span className="subtitle" id="viewSubtitle">
                {t(meta.sub)}
              </span>
            </div>

            <div className="topbar-right">
              <div className="search">
                <span id="searchIcon">
                  <Icon name="search" />
                </span>
                <input
                  id="searchInput"
                  spellCheck={false}
                  value={state.search}
                  placeholder={t(view === "pricing" ? "shell.searchPricing" : "shell.search")}
                  onChange={(e) => setSearch(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Escape") setSearch("");
                  }}
                />
              </div>
              <button
                className={"btn" + (state.loading ? " is-loading" : "")}
                id="btnRefresh"
                title={t("shell.refreshAll")}
                onClick={() => store.refreshAll()}
              >
                {state.loading ? (
                  <>
                    <span className="spinner" />
                    <span>{t("shell.syncingBtn")}</span>
                  </>
                ) : (
                  <>
                    <Icon name="refresh" />
                    <span>{t("shell.refreshAll")}</span>
                  </>
                )}
              </button>
              <button className="btn btn-primary" id="btnAdd" onClick={() => openProviderForm(null)}>
                <Icon name="plus" />
                <span>{t("shell.add")}</span>
              </button>
              <button
                className="btn btn-icon btn-ghost"
                id="btnTheme"
                title={t("shell.toggleTheme")}
                onClick={toggleTheme}
              >
                <Icon name={document.documentElement.getAttribute("data-theme") === "light" ? "sun" : "moon"} />
              </button>
            </div>
          </header>

          <div className="content" id="content">
            <section className={"view" + (view === "dashboard" ? " is-active" : "")} id="view-dashboard">
              {view === "dashboard" && <Dashboard />}
            </section>
            <section className={"view" + (view === "providers" ? " is-active" : "")} id="view-providers">
              {view === "providers" && <Providers />}
            </section>
            <section className={"view" + (view === "pricing" ? " is-active" : "")} id="view-pricing">
              {view === "pricing" && <Pricing />}
            </section>
            <section className={"view" + (view === "detail" ? " is-active" : "")} id="view-detail">
              {view === "detail" && <Detail />}
            </section>
            <section className={"view" + (view === "settings" ? " is-active" : "")} id="view-settings">
              {view === "settings" && <Settings />}
            </section>
          </div>
        </main>
      </div>

      <ProviderFormModal
        open={formOpen}
        provider={formProvider}
        onClose={() => setFormOpen(false)}
      />

      {onboardingNeeded && (
        <Onboarding
          onDone={(lang) => {
            setOnboardingNeeded(false);
            if (state.settings) store.saveSettings({ ...state.settings, language: lang }).catch(() => {});
          }}
        />
      )}
    </ShellContext.Provider>
  );
}
