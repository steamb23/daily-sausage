use crate::poller::refresh_view;
use crate::state::{lock, AppState, Snapshot};
use crate::tray;
use chrono::Utc;
use daily_sausage_core::claude_settings::{self, InstallPreview};
use daily_sausage_core::schedule::{CliRefreshGate, PollSchedule};
use daily_sausage_core::Config;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt;

#[tauri::command]
pub fn get_snapshot(state: State<'_, AppState>) -> Option<Snapshot> {
    lock(&state.snapshot).clone()
}

#[tauri::command]
pub fn get_config(state: State<'_, AppState>) -> Config {
    state.config()
}

#[tauri::command]
pub fn set_config(
    app: AppHandle,
    state: State<'_, AppState>,
    config: Config,
) -> Result<Config, String> {
    let old = state.config();
    let mut new = config.sanitized();
    // Owned by the status line installer, not by the settings form.
    new.chain_command = old.chain_command.clone();
    new.original_statusline = old.original_statusline.clone();
    // Only set by editing config.json.
    new.claude_path = old.claude_path.clone();

    if new.oauth_enabled != old.oauth_enabled {
        *lock(&state.oauth) = PollSchedule::default();
        *lock(&state.cli_refresh) = CliRefreshGate::default();
    }
    if new.autostart != old.autostart {
        let al = app.autolaunch();
        let res = if new.autostart {
            al.enable()
        } else {
            al.disable()
        };
        res.map_err(|e| format!("자동 시작 설정을 바꾸지 못했어요: {e}"))?;
    }
    new.save(&state.data_dir)
        .map_err(|e| format!("설정을 저장하지 못했어요: {e}"))?;
    *lock(&state.config) = new.clone();
    if new.oauth_enabled && !old.oauth_enabled {
        state.force_poll.store(true, Ordering::SeqCst);
    }
    // Show the new settings' status right away; a poll that refreshes the
    // login through Claude Code can take several seconds.
    refresh_view(&app);
    state.wake.notify_one();
    Ok(new)
}

fn collector_exe() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let s = exe.to_string_lossy();
    if s.contains("/AppTranslocation/") {
        return Err(
            "앱이 임시 위치에서 실행되고 있어요. Daily Sausage.app을 '응용 프로그램' 폴더로 옮긴 뒤 다시 실행해 주세요."
                .into(),
        );
    }
    Ok(exe)
}

#[tauri::command]
pub fn statusline_preview() -> Result<InstallPreview, String> {
    let exe = collector_exe()?;
    claude_settings::preview_install(&claude_settings::settings_path(), &exe)
        .map_err(|e| format!("Claude Code 설정 파일을 읽지 못했어요: {e}"))
}

#[tauri::command]
pub fn statusline_install(app: AppHandle, state: State<'_, AppState>) -> Result<String, String> {
    let exe = collector_exe()?;
    let path = claude_settings::settings_path();
    let backup = {
        let mut cfg = lock(&state.config);
        let backup = claude_settings::install(&path, &exe, &mut cfg, Utc::now())
            .map_err(|e| format!("Claude Code 설정을 바꾸지 못했어요: {e}"))?;
        cfg.save(&state.data_dir).map_err(|e| e.to_string())?;
        backup
    };
    refresh_view(&app);
    Ok(match backup {
        Some(b) => format!(
            "연동을 설치했어요. 원래 설정은 {}에 백업했어요.",
            b.display()
        ),
        None => "연동을 설치했어요.".into(),
    })
}

#[tauri::command]
pub fn statusline_uninstall(app: AppHandle, state: State<'_, AppState>) -> Result<String, String> {
    let path = claude_settings::settings_path();
    let backup = {
        let mut cfg = lock(&state.config);
        let backup = claude_settings::uninstall(&path, &mut cfg, Utc::now())
            .map_err(|e| format!("Claude Code 설정을 되돌리지 못했어요: {e}"))?;
        cfg.save(&state.data_dir).map_err(|e| e.to_string())?;
        backup
    };
    refresh_view(&app);
    Ok(match backup {
        Some(b) => format!(
            "연동을 제거하고 원래 상태줄을 되돌렸어요. 백업: {}",
            b.display()
        ),
        None => "연동을 제거했어요.".into(),
    })
}

#[tauri::command]
pub fn refresh(app: AppHandle) {
    tray::request_refresh(&app);
}

#[tauri::command]
pub fn hide_popup(app: AppHandle) {
    tray::hide_popup(&app);
}

#[tauri::command]
pub fn quit(app: AppHandle) {
    app.exit(0);
}
