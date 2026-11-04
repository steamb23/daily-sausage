use daily_sausage_core::claude_settings::StatuslineState;
use daily_sausage_core::format::{NotifyState, PopupModel};
use daily_sausage_core::schedule::{CliRefreshGate, PollSchedule};
use daily_sausage_core::{paths, Config, Store};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Mutex, MutexGuard};
use std::time::Instant;

const NOTIFY_FILE: &str = "notify_state.json";

/// Everything the popup needs, pushed as the `snapshot` event.
#[derive(Serialize, Clone)]
pub struct Snapshot {
    pub model: PopupModel,
    pub oauth: PollSchedule,
    pub oauth_enabled: bool,
    pub statusline: StatuslineState,
    /// The `claude` executable used to refresh an expired login, when that is
    /// enabled and one was found.
    pub claude_cli: Option<String>,
}

pub struct AppState {
    pub data_dir: PathBuf,
    pub store: Store,
    pub config: Mutex<Config>,
    pub notify: Mutex<NotifyState>,
    pub oauth: Mutex<PollSchedule>,
    /// Limits how often Claude Code is asked to refresh an expired login.
    pub cli_refresh: Mutex<CliRefreshGate>,
    pub snapshot: Mutex<Option<Snapshot>>,
    /// Wakes the poller before its next 30-second tick.
    pub wake: tokio::sync::Notify,
    /// The next tick should poll the usage endpoint now (rate limited to 1/min).
    pub force_poll: AtomicBool,
    /// When the popup was last hidden by losing focus.
    pub last_blur_hide: Mutex<Option<Instant>>,
    pub last_prune: Mutex<Option<chrono::DateTime<chrono::Utc>>>,
}

/// Locks a mutex, recovering the data if a panic poisoned it.
pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn config_exists() -> bool {
    paths::data_dir().join("config.json").exists()
}

impl AppState {
    pub fn load() -> Self {
        let data_dir = paths::data_dir();
        let notify = std::fs::read_to_string(data_dir.join(NOTIFY_FILE))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        AppState {
            store: Store::new(&data_dir),
            config: Mutex::new(Config::load(&data_dir)),
            notify: Mutex::new(notify),
            oauth: Mutex::new(PollSchedule::default()),
            cli_refresh: Mutex::new(CliRefreshGate::default()),
            snapshot: Mutex::new(None),
            wake: tokio::sync::Notify::new(),
            force_poll: AtomicBool::new(false),
            last_blur_hide: Mutex::new(None),
            last_prune: Mutex::new(None),
            data_dir,
        }
    }

    pub fn config(&self) -> Config {
        lock(&self.config).clone()
    }

    pub fn save_notify(&self, st: &NotifyState) {
        if let Ok(json) = serde_json::to_string(st) {
            let _ = std::fs::create_dir_all(&self.data_dir);
            let _ = std::fs::write(self.data_dir.join(NOTIFY_FILE), json);
        }
    }
}
