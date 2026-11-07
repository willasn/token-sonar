mod auto_update;
mod commands;
mod http;
mod login;
mod models;
mod providers;
mod storage;
mod sync;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use storage::AppStore;

/// 主窗口标签；关闭按钮只隐藏该窗口，其余窗口（登录窗口等）保持正常关闭。
const MAIN_WINDOW: &str = "main";

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// 托盘图标：左键恢复主窗口，菜单提供「显示主窗口 / 退出」。
fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出 TokenSonar", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    let mut tray = TrayIconBuilder::with_id("token-sonar-tray")
        .tooltip("TokenSonar")
        .menu(&menu)
        // 左键点击直接恢复窗口；退出走菜单
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            let store = AppStore::new(&app.handle().clone())?;
            app.manage(store);
            build_tray(app)?;
            Ok(())
        })
        // 关闭主窗口只收进托盘，不退出应用
        .on_window_event(|window, event| {
            if window.label() == MAIN_WINDOW {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
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
            auto_update::auto_update_check,
            auto_update::auto_update_install,
            login::start_deepseek_login,
            login::deepseek_login_token,
            login::start_mimo_login,
        ])
        .build(tauri::generate_context!())
        .expect("error while running TokenSonar")
        .run(|app, event| {
            // macOS 点击 Dock 图标时恢复被隐藏的主窗口
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                show_main_window(app);
            }
            #[cfg(not(target_os = "macos"))]
            let _ = (app, event);
        });
}
