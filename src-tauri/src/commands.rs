//! 暴露给前端的 Tauri 命令。

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Manager, State};

use crate::http::Http;
use crate::models::{
    AppSettings, LedgerEntry, ProviderConfig, RemoteBackup, SyncConfig, SyncResult, UsageSnapshot,
};
use crate::providers;
use crate::storage::AppStore;
use crate::sync;

static SEQ: AtomicU64 = AtomicU64::new(0);

fn new_id(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::SeqCst);
    format!("{prefix}_{nanos:x}{seq:x}")
}

fn now_str() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

// ---------------------------------------------------------------------------
// 账号管理
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_providers(store: State<AppStore>) -> Vec<ProviderConfig> {
    store.load_providers()
}

#[tauri::command]
pub fn save_provider(store: State<AppStore>, provider: ProviderConfig) -> Result<ProviderConfig, String> {
    let mut p = provider;
    if p.id.trim().is_empty() {
        p.id = new_id("pv");
    }
    let now = now_str();
    if p.created_at.trim().is_empty() {
        p.created_at = now.clone();
    }
    p.updated_at = now;
    if p.name.trim().is_empty() {
        p.name = p.kind.default_name().to_string();
    }
    store.upsert_provider(p.clone())?;
    Ok(p)
}

#[tauri::command]
pub fn remove_provider(store: State<AppStore>, id: String) -> Result<(), String> {
    store.delete_provider(&id)
}

#[tauri::command]
pub fn reorder_providers(
    store: State<AppStore>,
    ids: Vec<String>,
) -> Result<Vec<ProviderConfig>, String> {
    store.reorder_providers(&ids)
}

#[tauri::command]
pub async fn test_provider(provider: ProviderConfig) -> Result<String, String> {
    providers::probe(&provider).await
}

// ---------------------------------------------------------------------------
// 数据抓取
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn refresh_provider(app: AppHandle, id: String) -> Result<UsageSnapshot, String> {
    let cfg = {
        let store = app.state::<AppStore>();
        store.get_provider(&id).ok_or_else(|| "未找到该账号".to_string())?
    };

    let http = Http::new()?;
    let snap = if cfg.enabled {
        providers::fetch_snapshot(&cfg, &http).await
    } else {
        let mut s = UsageSnapshot::empty(&cfg);
        s.error = Some("账号已停用".to_string());
        s
    };

    let store = app.state::<AppStore>();
    store.save_snapshot(snap.clone())?;
    Ok(snap)
}

#[tauri::command]
pub async fn refresh_all(app: AppHandle) -> Result<Vec<UsageSnapshot>, String> {
    let configs = {
        let store = app.state::<AppStore>();
        store.load_providers()
    };

    let http = std::sync::Arc::new(Http::new()?);
    let mut handles = Vec::with_capacity(configs.len());

    for cfg in configs.iter().filter(|c| c.enabled) {
        let cfg = cfg.clone();
        let http = http.clone();
        handles.push(tauri::async_runtime::spawn(async move {
            providers::fetch_snapshot(&cfg, &http).await
        }));
    }

    let mut map = {
        let store = app.state::<AppStore>();
        store.load_snapshots()
    };

    for h in handles {
        if let Ok(snap) = h.await {
            map.insert(snap.provider_id.clone(), snap);
        }
    }

    {
        let store = app.state::<AppStore>();
        store.save_snapshots(&map)?;
    }

    Ok(configs
        .iter()
        .filter_map(|c| map.get(&c.id).cloned())
        .collect())
}

#[tauri::command]
pub fn get_snapshots(store: State<AppStore>) -> Vec<UsageSnapshot> {
    let map = store.load_snapshots();
    store
        .load_providers()
        .iter()
        .filter_map(|c| map.get(&c.id).cloned())
        .collect()
}

// ---------------------------------------------------------------------------
// 本地台账
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_ledger(store: State<AppStore>, provider_id: Option<String>) -> Vec<LedgerEntry> {
    store
        .load_ledger()
        .into_iter()
        .filter(|e| match &provider_id {
            Some(id) => &e.provider_id == id,
            None => true,
        })
        .collect()
}

#[tauri::command]
pub fn add_ledger_entry(store: State<AppStore>, entry: LedgerEntry) -> Result<LedgerEntry, String> {
    let mut e = entry;
    if e.id.trim().is_empty() {
        e.id = new_id("lg");
    }
    if e.date.trim().is_empty() {
        e.date = chrono::Local::now().format("%Y-%m-%d").to_string();
    }
    if e.created_at.trim().is_empty() {
        e.created_at = now_str();
    }
    let mut all = store.load_ledger();
    all.push(e.clone());
    store.save_ledger(&all)?;
    Ok(e)
}

#[tauri::command]
pub fn remove_ledger_entry(store: State<AppStore>, id: String) -> Result<(), String> {
    let all: Vec<LedgerEntry> = store
        .load_ledger()
        .into_iter()
        .filter(|e| e.id != id)
        .collect();
    store.save_ledger(&all)
}

// ---------------------------------------------------------------------------
// 设置 / 杂项
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_settings(store: State<AppStore>) -> AppSettings {
    store.load_settings()
}

#[tauri::command]
pub fn save_settings(store: State<AppStore>, settings: AppSettings) -> Result<(), String> {
    store.save_settings(&settings)
}

#[tauri::command]
pub fn get_app_info(store: State<AppStore>) -> serde_json::Value {
    serde_json::json!({
        "name": "TokenSonar",
        "version": env!("CARGO_PKG_VERSION"),
        "dataDir": store.data_dir(),
    })
}

#[tauri::command]
pub fn open_data_dir(store: State<AppStore>) -> Result<(), String> {
    let dir = store.data_dir();
    #[cfg(target_os = "macos")]
    let cmd = ("open", dir);
    #[cfg(target_os = "windows")]
    let cmd = ("explorer", dir);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let cmd = ("xdg-open", dir);

    std::process::Command::new(cmd.0)
        .arg(cmd.1)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("打开数据目录失败: {e}"))
}

// ---------------------------------------------------------------------------
// Git 仓库同步
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_sync_config(store: State<AppStore>) -> SyncConfig {
    store.load_sync_config()
}

#[tauri::command]
pub fn save_sync_config(store: State<AppStore>, config: SyncConfig) -> Result<SyncConfig, String> {
    let mut c = config;
    if c.branch.trim().is_empty() {
        c.branch = "main".to_string();
    }
    store.save_sync_config(&c)?;
    Ok(c)
}

/// 只做远端探测，不写入任何文件：用于「测试连通性」按钮。
#[tauri::command]
pub async fn test_sync_connection(config: SyncConfig) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || sync::test_connection(&config))
        .await
        .map_err(|e| format!("连通性测试任务异常: {e}"))?
}

/// 一键同步：导出全部账号数据 → 提交 → 推送到远端仓库。
///
/// `providers` 是本机与远端合并、并经用户确认冲突后的账号列表；
/// 传入时先落盘到本机再推送，省略（None）则直接推送本机现有数据。
/// `include_credentials` 为 false 时导出前剔除账号凭证（密码 / Token / 云 AK·SK）。
/// 同步结果的时间与说明会回写进 `sync.json`，设置页据此展示「上次同步」。
#[tauri::command]
pub async fn sync_to_repo(
    app: AppHandle,
    providers: Option<Vec<ProviderConfig>>,
    include_credentials: Option<bool>,
) -> Result<SyncResult, String> {
    if let Some(list) = providers {
        let store = app.state::<AppStore>();
        store.save_providers(&list)?;
    }
    let with_credentials = include_credentials.unwrap_or(true);

    let (repo_dir, config, files) = {
        let store = app.state::<AppStore>();
        (
            store.sync_dir(),
            store.load_sync_config(),
            sync::export_files(&store, with_credentials)?,
        )
    };
    let config_for_task = config.clone();

    let result = tauri::async_runtime::spawn_blocking(move || {
        sync::push_backup(&repo_dir, &config_for_task, &files)
    })
    .await
    .map_err(|e| format!("同步任务异常: {e}"))??;

    let mut saved = config;
    saved.last_sync_at = result.synced_at.clone();
    saved.last_message = result.message.clone();
    let store = app.state::<AppStore>();
    store.save_sync_config(&saved)?;
    Ok(result)
}

/// 拉取远端仓库备份里的账号列表（只读，不写本机）：供同步前合并与冲突提示使用。
#[tauri::command]
pub async fn pull_remote_backup(app: AppHandle) -> Result<RemoteBackup, String> {
    let (repo_dir, config) = {
        let store = app.state::<AppStore>();
        (store.sync_dir(), store.load_sync_config())
    };

    tauri::async_runtime::spawn_blocking(move || sync::pull_backup(&repo_dir, &config))
        .await
        .map_err(|e| format!("拉取任务异常: {e}"))?
}

/// 用系统默认浏览器打开外部链接（厂商控制台 / 官网），只在应用内跳转无法访问的站点。
#[tauri::command]
pub fn open_external_url(url: String) -> Result<(), String> {
    let url = url.trim();
    // 只放行 http(s)，避免把任意协议交给系统打开器
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err("仅支持打开 http/https 链接".into());
    }
    #[cfg(target_os = "macos")]
    let mut cmd = std::process::Command::new("open");
    #[cfg(target_os = "windows")]
    let mut cmd = std::process::Command::new("cmd");
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut cmd = std::process::Command::new("xdg-open");

    // cmd.exe 的 start 需要显式的空窗口标题，否则会把 URL 当成标题
    #[cfg(target_os = "windows")]
    cmd.args(["/C", "start", ""]);

    cmd.arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("打开链接失败: {e}"))
}
