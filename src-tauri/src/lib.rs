mod commands;
mod http;
mod login;
mod models;
mod providers;
mod storage;
mod sync;

use tauri::Manager;

use storage::AppStore;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            let store = AppStore::new(&app.handle().clone())?;
            app.manage(store);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_providers,
            commands::save_provider,
            commands::remove_provider,
            commands::reorder_providers,
            commands::test_provider,
            commands::refresh_provider,
            commands::refresh_all,
            commands::get_snapshots,
            commands::list_ledger,
            commands::add_ledger_entry,
            commands::remove_ledger_entry,
            commands::get_settings,
            commands::save_settings,
            commands::get_app_info,
            commands::open_data_dir,
            commands::open_external_url,
            commands::get_sync_config,
            commands::save_sync_config,
            commands::test_sync_connection,
            commands::pull_remote_backup,
            commands::sync_to_repo,
            login::start_deepseek_login,
            login::deepseek_login_token,
            login::start_mimo_login,
        ])
        .run(tauri::generate_context!())
        .expect("error while running TokenSonar");
}
