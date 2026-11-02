//! Background loop: every 30 seconds (or when woken) poll the optional usage
//! endpoint if due, recompute the budget, update the tray and the popup, and
//! send notifications.

use crate::state::{lock, AppState, Snapshot};
use crate::tray;
use chrono::{Duration, Local, Utc};
use daily_sausage_core::credentials;
use daily_sausage_core::oauth::{self, FetchError};
use daily_sausage_core::schedule::PollOutcome;
use daily_sausage_core::{claude_settings, compute, format, Sample};
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

const TICK: std::time::Duration = std::time::Duration::from_secs(30);
const KEEP_DAYS: i64 = 14;

pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tick(&app).await;
            let state = app.state::<AppState>();
            tokio::select! {
                _ = tokio::time::sleep(TICK) => {}
                _ = state.wake.notified() => {}
            }
        }
    });
}

fn poll_once(now: chrono::DateTime<Utc>) -> (PollOutcome, Option<Sample>) {
    let token = match credentials::load() {
        Ok(t) => t,
        Err(e) => return (PollOutcome::AuthProblem(e.to_string()), None),
    };
    if token.is_expired(now) {
        return (
            PollOutcome::AuthProblem(FetchError::Unauthorized.to_string()),
            None,
        );
    }
    match oauth::fetch(&token, now) {
        Ok(s) => (PollOutcome::Ok, Some(s)),
        Err(e @ FetchError::Unauthorized) => (PollOutcome::AuthProblem(e.to_string()), None),
        Err(e @ FetchError::Forbidden) => (PollOutcome::Forbidden(e.to_string()), None),
        Err(FetchError::RateLimited { retry_after_secs }) => {
            (PollOutcome::RateLimited { retry_after_secs }, None)
        }
        Err(e) => (PollOutcome::Transient(e.to_string()), None),
    }
}

async fn tick(app: &AppHandle) {
    let state = app.state::<AppState>();
    let now = Utc::now();
    let cfg = state.config();
    let force = state.force_poll.swap(false, Ordering::SeqCst);

    if cfg.oauth_enabled {
        let due = lock(&state.oauth).due(now, force, &Local, cfg.boundary_hour);
        if due {
            let (outcome, sample) = tauri::async_runtime::spawn_blocking(move || poll_once(now))
                .await
                .unwrap_or_else(|e| (PollOutcome::Transient(e.to_string()), None));
            if let Some(s) = sample {
                let _ = state.store.append(&s);
            }
            lock(&state.oauth).record(now, cfg.poll_minutes, outcome);
        }
    }
    refresh_view(app);
}

/// Recomputes everything from the stored samples and pushes it out.
pub fn refresh_view(app: &AppHandle) {
    let state = app.state::<AppState>();
    let now = Utc::now();
    let cfg = state.config();

    let samples = state.store.load_recent(now, 8).unwrap_or_default();
    let view = compute(&samples, now, &Local, cfg.boundary_hour, cfg.mode);
    let model = format::popup(&view, &Local);
    tray::update(app, &model);

    let due = {
        let mut ns = lock(&state.notify);
        let before = ns.clone();
        let due = format::due_notifications(&view, &cfg, &mut ns, &Local);
        if *ns != before {
            state.save_notify(&ns);
        }
        due
    };
    for n in due {
        let _ = app
            .notification()
            .builder()
            .title(n.title)
            .body(n.body)
            .show();
    }

    let snapshot = Snapshot {
        model,
        oauth: lock(&state.oauth).clone(),
        oauth_enabled: cfg.oauth_enabled,
        statusline: claude_settings::state(&claude_settings::settings_path()),
    };
    *lock(&state.snapshot) = Some(snapshot.clone());
    let _ = app.emit("snapshot", snapshot);

    let mut last_prune = lock(&state.last_prune);
    if last_prune.is_none_or(|t| now - t > Duration::hours(24)) {
        let _ = state.store.prune_before(now - Duration::days(KEEP_DAYS));
        *last_prune = Some(now);
    }
}
