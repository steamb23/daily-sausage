use crate::state::{lock, AppState};
use crate::POPUP;
use daily_sausage_core::format::{Level, PopupModel};
use daily_sausage_core::icon::render_tray_rect;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewWindow, WindowEvent};
use tauri_plugin_positioner::{Position, WindowExt};

const TRAY_ID: &str = "daily-sausage";

/// macOS menu bar items can be wide; the Windows tray wants a square icon.
fn icon_size() -> (u32, u32) {
    if cfg!(target_os = "macos") {
        (64, 32)
    } else {
        (32, 32)
    }
}

fn icon(ratio: Option<f64>, level: Level) -> Image<'static> {
    let (w, h) = icon_size();
    Image::new_owned(render_tray_rect(w, h, ratio, level), w, h)
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "열기", true, None::<&str>)?;
    let refresh = MenuItem::with_id(app, "refresh", "지금 새로고침", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Daily Sausage 종료", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &refresh, &sep, &quit])?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon(None, Level::Idle))
        .tooltip("Daily Sausage")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_popup(app),
            "refresh" => request_refresh(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            tauri_plugin_positioner::on_tray_event(tray.app_handle(), &event);
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_popup(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

pub fn update(app: &AppHandle, model: &PopupModel) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let _ = tray.set_icon(Some(icon(model.today_ratio, model.level)));
    let _ = tray.set_tooltip(Some(&model.tooltip));
    #[cfg(target_os = "macos")]
    let _ = tray.set_title(Some(&model.tray_title));
}

pub fn request_refresh(app: &AppHandle) {
    let state = app.state::<AppState>();
    state.force_poll.store(true, Ordering::SeqCst);
    state.wake.notify_one();
}

pub fn toggle_popup(app: &AppHandle) {
    let Some(w) = app.get_webview_window(POPUP) else {
        return;
    };
    if w.is_visible().unwrap_or(false) {
        let _ = w.hide();
        return;
    }
    // The tray click that took focus away already hid the popup; do not reopen it.
    let just_hidden = lock(&app.state::<AppState>().last_blur_hide)
        .is_some_and(|t| t.elapsed() < Duration::from_millis(300));
    if !just_hidden {
        show_popup(app);
    }
}

/// The popup's logical size, as declared in `tauri.conf.json`.
const POPUP_SIZE: (f64, f64) = (360.0, 540.0);

fn known_size(w: &WebviewWindow) -> Option<PhysicalSize<u32>> {
    w.outer_size().ok().filter(|s| s.width > 0 && s.height > 0)
}

/// Bottom-right of the work area (top-right on macOS), used until the tray
/// icon has been clicked once and its position is known.
fn place_in_corner(w: &WebviewWindow) {
    let monitor = w
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| w.primary_monitor().ok().flatten());
    let Some(mon) = monitor else {
        return;
    };
    let scale = mon.scale_factor();
    let size = known_size(w).unwrap_or_else(|| {
        PhysicalSize::new((POPUP_SIZE.0 * scale) as u32, (POPUP_SIZE.1 * scale) as u32)
    });
    let area = mon.work_area();
    let margin = (8.0 * scale) as i32;
    let x = area.position.x + area.size.width as i32 - size.width as i32 - margin;
    let y = if cfg!(target_os = "macos") {
        area.position.y + margin
    } else {
        area.position.y + area.size.height as i32 - size.height as i32 - margin
    };
    let _ = w.set_position(PhysicalPosition::new(x, y));
}

fn position_popup(w: &WebviewWindow) {
    // The positioner needs the real window size; a window that was never shown
    // may still report zero.
    if known_size(w).is_some() && w.move_window_constrained(Position::TrayCenter).is_ok() {
        return;
    }
    place_in_corner(w);
}

pub fn show_popup(app: &AppHandle) {
    let Some(w) = app.get_webview_window(POPUP) else {
        return;
    };
    position_popup(&w);
    let _ = w.show();
    // A window that was never shown may report a zero size, which puts it off
    // screen; place it again now that it has its real size.
    position_popup(&w);
    let _ = w.set_focus();
    request_refresh(app);
}

pub fn hide_popup(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(POPUP) {
        let _ = w.hide();
    }
}

/// Hide on focus loss and on close, so the window is reused instead of destroyed.
pub fn wire_popup(app: &AppHandle) {
    let Some(w) = app.get_webview_window(POPUP) else {
        return;
    };
    let app = app.clone();
    // Debug builds only: keep the popup open (screenshots, window managers
    // that report focus oddly).
    let keep_open = cfg!(debug_assertions) && std::env::var_os("DAILY_SAUSAGE_KEEP_OPEN").is_some();
    w.on_window_event(move |event| match event {
        WindowEvent::Focused(false) if !keep_open => {
            *lock(&app.state::<AppState>().last_blur_hide) = Some(Instant::now());
            hide_popup(&app);
        }
        WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            hide_popup(&app);
        }
        _ => {}
    });
}
