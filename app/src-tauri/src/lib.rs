mod commands;
mod poller;
mod state;
mod tray;

use tauri::{Manager, RunEvent};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_notification::{NotificationExt, PermissionState};

/// Label of the popup window declared in `tauri.conf.json`.
pub const POPUP: &str = "main";

pub fn run() {
    tauri::Builder::default()
        // Must be first: a second launch just opens the running app's popup.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_popup(app);
        }))
        .plugin(tauri_plugin_positioner::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let first_run = !state::config_exists();
            app.manage(state::AppState::load());
            tray::create(app.handle())?;
            tray::wire_popup(app.handle());

            let handle = app.handle().clone();
            let cfg = handle.state::<state::AppState>().config();
            if cfg.autostart && !handle.autolaunch().is_enabled().unwrap_or(false) {
                let _ = handle.autolaunch().enable();
            }
            if first_run {
                // Save defaults so the collector sees the same settings.
                let _ = cfg.save(&handle.state::<state::AppState>().data_dir);
            }
            if matches!(
                handle.notification().permission_state(),
                Ok(PermissionState::Prompt | PermissionState::PromptWithRationale)
            ) {
                let _ = handle.notification().request_permission();
            }

            poller::start(handle.clone());
            if first_run {
                tray::show_popup(&handle);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_snapshot,
            commands::get_config,
            commands::set_config,
            commands::statusline_preview,
            commands::statusline_install,
            commands::statusline_uninstall,
            commands::refresh,
            commands::hide_popup,
            commands::quit,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build Daily Sausage")
        .run(|_app, event| {
            // Closing the popup must not quit the tray app; only "종료" does.
            if let RunEvent::ExitRequested { api, code, .. } = event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}
