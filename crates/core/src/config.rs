//! Settings shared by the app and the collector (`config.json` in the data dir).

use crate::budget::Mode;
use serde::{Deserialize, Serialize};
use std::io;
use std::path::Path;

const FILE: &str = "config.json";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Config {
    /// Local hour (0-23) at which "today" rolls over.
    pub boundary_hour: u32,
    pub mode: Mode,
    /// Poll the unofficial usage endpoint with Claude Code's stored login.
    pub oauth_enabled: bool,
    /// The user read and accepted the notice about the unofficial endpoint.
    pub oauth_acknowledged: bool,
    /// Minutes between unofficial endpoint polls (2-15).
    pub poll_minutes: u32,
    /// When the stored login has expired, run `claude auth status` so Claude
    /// Code refreshes it itself (see `claude_cli`).
    pub cli_refresh: bool,
    /// Path to the `claude` executable when it is not found automatically.
    /// Set by editing `config.json`; the settings form does not change it.
    pub claude_path: Option<String>,
    pub notify_today_80: bool,
    pub notify_today_100: bool,
    pub notify_five_hour_90: bool,
    /// Print the one-line summary in Claude Code's status line when no earlier
    /// status line command is chained.
    pub statusline_summary: bool,
    /// The user's previous status line command, run by the collector with the
    /// same input so their status line keeps working.
    pub chain_command: Option<String>,
    /// The previous `statusLine` object, restored on uninstall.
    pub original_statusline: Option<serde_json::Value>,
    pub autostart: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            boundary_hour: 0,
            mode: Mode::Rolling,
            oauth_enabled: false,
            oauth_acknowledged: false,
            poll_minutes: 5,
            cli_refresh: true,
            claude_path: None,
            notify_today_80: true,
            notify_today_100: true,
            notify_five_hour_90: true,
            statusline_summary: true,
            chain_command: None,
            original_statusline: None,
            autostart: true,
        }
    }
}

impl Config {
    /// Clamps values into their valid ranges.
    pub fn sanitized(mut self) -> Self {
        self.boundary_hour = self.boundary_hour.min(23);
        self.poll_minutes = self.poll_minutes.clamp(2, 15);
        if !self.oauth_acknowledged {
            self.oauth_enabled = false;
        }
        self.claude_path = self
            .claude_path
            .map(|p| p.trim().to_owned())
            .filter(|p| !p.is_empty());
        self
    }

    /// Loads the config, falling back to defaults when it is missing or broken.
    pub fn load(dir: &Path) -> Config {
        std::fs::read_to_string(dir.join(FILE))
            .ok()
            .and_then(|s| serde_json::from_str::<Config>(&s).ok())
            .unwrap_or_default()
            .sanitized()
    }

    pub fn save(&self, dir: &Path) -> io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let json = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        let tmp = dir.join("config.json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, dir.join(FILE))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Config::load(dir.path()), Config::default());
        let c = Config {
            boundary_hour: 4,
            mode: Mode::Fixed,
            chain_command: Some("bash ~/.claude/statusline.sh".into()),
            ..Config::default()
        };
        c.save(dir.path()).unwrap();
        assert_eq!(Config::load(dir.path()), c);
    }

    #[test]
    fn sanitizes() {
        let c = Config {
            boundary_hour: 99,
            poll_minutes: 1,
            oauth_enabled: true,
            oauth_acknowledged: false,
            ..Config::default()
        }
        .sanitized();
        assert_eq!(c.boundary_hour, 23);
        assert_eq!(c.poll_minutes, 2);
        assert!(!c.oauth_enabled, "oauth needs acknowledgement");
        let c = Config {
            claude_path: Some("  ".into()),
            ..Config::default()
        }
        .sanitized();
        assert_eq!(c.claude_path, None);
    }

    #[test]
    fn tolerates_unknown_and_missing_fields() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(FILE),
            r#"{"boundary_hour": 4, "future_field": 1}"#,
        )
        .unwrap();
        let c = Config::load(dir.path());
        assert_eq!(c.boundary_hour, 4);
        assert_eq!(c.poll_minutes, 5);
    }
}
