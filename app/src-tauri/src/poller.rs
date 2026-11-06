//! Background loop: every 30 seconds (or when woken) poll the optional usage
//! endpoint if due (asking Claude Code to refresh an expired login first),
//! recompute the budget, update the tray and the popup, and send notifications.

use crate::state::{lock, AppState, Snapshot};
use crate::tray;
use chrono::{Duration, Local, Utc};
use daily_sausage_core::credentials::{self, OauthToken};
use daily_sausage_core::oauth::{self, FetchError};
use daily_sausage_core::schedule::PollOutcome;
use daily_sausage_core::{claude_cli, claude_settings, compute, format, Sample};
use std::path::PathBuf;
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

/// What `poll_once` needs to hand an expired login to Claude Code.
struct CliRefresh {
    claude_path: Option<String>,
    cwd: PathBuf,
}

/// Runs `claude -p /usage` so Claude Code refreshes its own login, then
/// re-reads the login to see whether that worked.
fn refresh_with_claude(cli: &CliRefresh) -> Result<OauthToken, String> {
    claude_cli::refresh_login(cli.claude_path.as_deref(), &cli.cwd).map_err(|e| {
        format!("로그인이 만료돼서 Claude Code로 갱신하려고 했지만 실패했어요: {e}")
    })?;
    let token = credentials::load().map_err(|e| e.to_string())?;
    if token.is_expired(Utc::now()) {
        return Err(
            "Claude Code를 실행했지만 로그인이 갱신되지 않았어요. Claude Code를 한 번 실행해 주세요"
                .into(),
        );
    }
    Ok(token)
}

/// Polls the usage endpoint. Returns whether Claude Code was asked to refresh
/// the login, which `cli` (when given) allows for an expired login.
fn poll_once(
    now: chrono::DateTime<Utc>,
    cli: Option<CliRefresh>,
) -> (PollOutcome, Option<Sample>, bool) {
    let mut token = match credentials::load() {
        Ok(t) => t,
        Err(e) => return (PollOutcome::AuthProblem(e.to_string()), None, false),
    };
    let mut attempted = false;
    if token.is_expired(now) {
        let Some(cli) = cli else {
            return (
                PollOutcome::AuthProblem(FetchError::Unauthorized.to_string()),
                None,
                false,
            );
        };
        attempted = true;
        match refresh_with_claude(&cli) {
            Ok(t) => token = t,
            Err(msg) => return (PollOutcome::AuthProblem(msg), None, true),
        }
    }
    let (outcome, sample) = match oauth::fetch(&token, Utc::now()) {
        Ok(s) => (PollOutcome::Ok, Some(s)),
        Err(e @ FetchError::Unauthorized) => (PollOutcome::AuthProblem(e.to_string()), None),
        Err(e @ FetchError::Forbidden) => (PollOutcome::Forbidden(e.to_string()), None),
        Err(FetchError::RateLimited { retry_after_secs }) => {
            (PollOutcome::RateLimited { retry_after_secs }, None)
        }
        Err(e) => (PollOutcome::Transient(e.to_string()), None),
    };
    (outcome, sample, attempted)
}

async fn tick(app: &AppHandle) {
    let state = app.state::<AppState>();
    let now = Utc::now();
    let cfg = state.config();
    let force = state.force_poll.swap(false, Ordering::SeqCst);

    if cfg.oauth_enabled {
        let due = lock(&state.oauth).due(now, force, &Local, cfg.boundary_hour);
        if due {
            let cli = (cfg.cli_refresh && lock(&state.cli_refresh).allows(now, force)).then(|| {
                CliRefresh {
                    claude_path: cfg.claude_path.clone(),
                    cwd: state.data_dir.clone(),
                }
            });
            let (outcome, sample, attempted) =
                tauri::async_runtime::spawn_blocking(move || poll_once(now, cli))
                    .await
                    .unwrap_or_else(|e| (PollOutcome::Transient(e.to_string()), None, false));
            if attempted {
                lock(&state.cli_refresh).record(now);
            }
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

    let claude_cli = (cfg.oauth_enabled && cfg.cli_refresh)
        .then(|| claude_cli::find_claude(cfg.claude_path.as_deref()))
        .flatten()
        .map(|p| p.display().to_string());
    let snapshot = Snapshot {
        model,
        oauth: lock(&state.oauth).clone(),
        oauth_enabled: cfg.oauth_enabled,
        statusline: claude_settings::state(&claude_settings::settings_path()),
        claude_cli,
    };
    *lock(&state.snapshot) = Some(snapshot.clone());
    let _ = app.emit("snapshot", snapshot);

    let mut last_prune = lock(&state.last_prune);
    if last_prune.is_none_or(|t| now - t > Duration::hours(24)) {
        let _ = state.store.prune_before(now - Duration::days(KEEP_DAYS));
        *last_prune = Some(now);
    }
}
