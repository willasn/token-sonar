//! 本地持久化：所有数据以 JSON 存放在应用数据目录。
//!
//! - `providers.json`  账号配置
//! - `snapshots.json`  最近一次抓取到的用量快照
//! - `ledger.json`     本地台账（无用量接口厂商的手工记录）
//! - `settings.json`   应用设置
//! - `sync.json`       Git 仓库同步配置
//! - `sync-repo/`      远端同步仓库在本机的暂存副本

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use tauri::{Manager, Runtime};

use crate::models::{AppSettings, LedgerEntry, ProviderConfig, SyncConfig, UsageSnapshot};

pub struct AppStore {
    dir: PathBuf,
}

impl AppStore {
    pub fn new<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<Self, String> {
        let dir = app
            .path()
            .app_data_dir()
            .map_err(|e| format!("无法定位应用数据目录: {e}"))?;
        fs::create_dir_all(&dir).map_err(|e| format!("创建数据目录失败: {e}"))?;
        Ok(AppStore { dir })
    }

    fn file(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    fn read_json<T: serde::de::DeserializeOwned + Default>(&self, name: &str) -> T {
        let path = self.file(name);
        match fs::read_to_string(&path) {
            Ok(content) => serde_json::from_str::<T>(&content).unwrap_or_default(),
            Err(_) => T::default(),
        }
    }

    /// 原子写入：先写临时文件再重命名，避免写入中断导致配置损坏。
    fn write_json<T: serde::Serialize>(&self, name: &str, value: &T) -> Result<(), String> {
        let path = self.file(name);
        let tmp = self.file(&format!("{name}.tmp"));
        let content =
            serde_json::to_string_pretty(value).map_err(|e| format!("序列化失败: {e}"))?;
        fs::write(&tmp, content).map_err(|e| format!("写入 {name} 失败: {e}"))?;
        fs::rename(&tmp, &path).map_err(|e| format!("保存 {name} 失败: {e}"))?;
        Ok(())
    }

    pub fn data_dir(&self) -> String {
        self.dir.to_string_lossy().to_string()
    }

    // -- 账号配置 ----------------------------------------------------------

    pub fn load_providers(&self) -> Vec<ProviderConfig> {
        let mut list = self.read_json::<Vec<ProviderConfig>>("providers.json");
        for p in &mut list {
            p.normalize();
        }
        list
    }

    pub fn save_providers(&self, providers: &[ProviderConfig]) -> Result<(), String> {
        self.write_json("providers.json", &providers)
    }

    pub fn get_provider(&self, id: &str) -> Option<ProviderConfig> {
        self.load_providers().into_iter().find(|p| p.id == id)
    }

    pub fn upsert_provider(&self, provider: ProviderConfig) -> Result<(), String> {
        let mut all = self.load_providers();
        match all.iter_mut().find(|p| p.id == provider.id) {
            Some(existing) => *existing = provider,
            None => all.push(provider),
        }
        self.save_providers(&all)
    }

    /// 按给定的 id 顺序重排账号并落盘，返回重排后的完整列表。
    ///
    /// `ids` 里没有出现的账号（例如筛选后看不到的）保持原有相对顺序追加到末尾，
    /// 避免因为只传了部分 id 就丢账号。
    pub fn reorder_providers(&self, ids: &[String]) -> Result<Vec<ProviderConfig>, String> {
        let all = self.load_providers();
        let mut ordered: Vec<ProviderConfig> = Vec::with_capacity(all.len());
        for id in ids {
            if let Some(p) = all.iter().find(|p| &p.id == id) {
                ordered.push(p.clone());
            }
        }
        for p in all {
            if !ids.iter().any(|id| id == &p.id) {
                ordered.push(p);
            }
        }
        self.save_providers(&ordered)?;
        Ok(ordered)
    }

    pub fn delete_provider(&self, id: &str) -> Result<(), String> {
        let all: Vec<ProviderConfig> = self
            .load_providers()
            .into_iter()
            .filter(|p| p.id != id)
            .collect();
        self.save_providers(&all)?;

        let mut snapshots = self.load_snapshots();
        snapshots.remove(id);
        self.save_snapshots(&snapshots)?;

        let ledger: Vec<LedgerEntry> = self
            .load_ledger()
            .into_iter()
            .filter(|e| e.provider_id != id)
            .collect();
        self.save_ledger(&ledger)
    }

    // -- 快照 --------------------------------------------------------------

    pub fn load_snapshots(&self) -> HashMap<String, UsageSnapshot> {
        self.read_json::<HashMap<String, UsageSnapshot>>("snapshots.json")
    }

    pub fn save_snapshots(&self, snapshots: &HashMap<String, UsageSnapshot>) -> Result<(), String> {
        self.write_json("snapshots.json", snapshots)
    }

    pub fn save_snapshot(&self, snapshot: UsageSnapshot) -> Result<(), String> {
        let mut all = self.load_snapshots();
        all.insert(snapshot.provider_id.clone(), snapshot);
        self.save_snapshots(&all)
    }

    // -- 本地台账 ----------------------------------------------------------

    pub fn load_ledger(&self) -> Vec<LedgerEntry> {
        self.read_json::<Vec<LedgerEntry>>("ledger.json")
    }

    pub fn save_ledger(&self, ledger: &[LedgerEntry]) -> Result<(), String> {
        self.write_json("ledger.json", &ledger)
    }

    // -- 设置 --------------------------------------------------------------

    pub fn load_settings(&self) -> AppSettings {
        self.read_json::<AppSettings>("settings.json")
    }

    pub fn save_settings(&self, settings: &AppSettings) -> Result<(), String> {
        self.write_json("settings.json", settings)
    }

    // -- Git 同步 ----------------------------------------------------------

    pub fn load_sync_config(&self) -> SyncConfig {
        self.read_json::<SyncConfig>("sync.json")
    }

    pub fn save_sync_config(&self, config: &SyncConfig) -> Result<(), String> {
        self.write_json("sync.json", config)
    }

    /// 远端仓库在本机的暂存副本目录
    pub fn sync_dir(&self) -> PathBuf {
        self.dir.join("sync-repo")
    }
}
