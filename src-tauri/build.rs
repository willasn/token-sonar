//! 声明应用命令清单，让每个命令都能在 capabilities 里被精确授权。
//!
//! 声明后 Tauri 会对所有自定义命令做 ACL 校验：主窗口在 `capabilities/default.json`
//! 里拿到对应权限，远程登录窗口在 `capabilities/login.json` 里只拿到回传 Token 的那一个。

const COMMANDS: &[&str] = &[
    "list_providers",
    "save_provider",
    "remove_provider",
    "reorder_providers",
    "test_provider",
    "refresh_provider",
    "refresh_all",
    "get_snapshots",
    "list_ledger",
    "add_ledger_entry",
    "remove_ledger_entry",
    "get_settings",
    "save_settings",
    "get_app_info",
    "open_data_dir",
    "open_external_url",
    "get_sync_config",
    "save_sync_config",
    "test_sync_connection",
    "pull_remote_backup",
    "sync_to_repo",
    "start_deepseek_login",
    "deepseek_login_token",
];

fn main() {
    let attributes = tauri_build::Attributes::new()
        .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS));
    tauri_build::try_build(attributes).expect("failed to run tauri-build");
}
