//! Installs and removes the collector as Claude Code's `statusLine` command.
//!
//! The previous `statusLine` is kept in our config so the collector can chain it
//! and uninstall can restore it. Every write makes a timestamped backup first.

use crate::config::Config;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::io;
use std::path::{Path, PathBuf};

const KEY: &str = "statusLine";

pub fn settings_path() -> PathBuf {
    crate::paths::claude_config_dir().join("settings.json")
}

/// `"<exe>" collect`, with forward slashes so it works in bash, cmd and PowerShell.
pub fn collector_command(exe: &Path) -> String {
    let p = exe.to_string_lossy().replace('\\', "/");
    format!("\"{p}\" collect")
}

pub fn is_collector_command(cmd: &str) -> bool {
    let c = cmd.trim().to_ascii_lowercase();
    c.contains("daily-sausage") && c.ends_with(" collect")
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum StatuslineState {
    NotConfigured,
    Installed { command: String },
    Other { command: String },
    Unreadable { error: String },
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct InstallPreview {
    pub settings_path: String,
    pub before: String,
    pub after: String,
    /// The previous command that will keep running through the collector.
    pub chains: Option<String>,
}

pub fn read_settings(path: &Path) -> io::Result<Value> {
    match std::fs::read_to_string(path) {
        Ok(s) if s.trim().is_empty() => Ok(Value::Object(Map::new())),
        Ok(s) => {
            let v: Value = serde_json::from_str(&s)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            if v.is_object() {
                Ok(v)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "settings.json is not an object",
                ))
            }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Value::Object(Map::new())),
        Err(e) => Err(e),
    }
}

fn command_of(statusline: Option<&Value>) -> Option<String> {
    let s = statusline?;
    if s.get("type").and_then(Value::as_str) != Some("command") {
        return None;
    }
    s.get("command").and_then(Value::as_str).map(str::to_owned)
}

pub fn state(path: &Path) -> StatuslineState {
    match read_settings(path) {
        Err(e) => StatuslineState::Unreadable {
            error: e.to_string(),
        },
        Ok(v) => match command_of(v.get(KEY)) {
            Some(c) if is_collector_command(&c) => StatuslineState::Installed { command: c },
            Some(c) => StatuslineState::Other { command: c },
            None if v.get(KEY).is_some() => StatuslineState::Other {
                command: v[KEY].to_string(),
            },
            None => StatuslineState::NotConfigured,
        },
    }
}

fn new_statusline(current: Option<&Value>, exe: &Path) -> Value {
    let mut s = json!({ "type": "command", "command": collector_command(exe) });
    if let Some(cur) = current {
        for k in ["padding", "refreshInterval"] {
            if let Some(v) = cur.get(k) {
                s[k] = v.clone();
            }
        }
    }
    s
}

fn pretty(v: Option<&Value>) -> String {
    v.map(|v| serde_json::to_string_pretty(v).unwrap_or_default())
        .unwrap_or_else(|| "(없음)".into())
}

pub fn preview_install(path: &Path, exe: &Path) -> io::Result<InstallPreview> {
    let settings = read_settings(path)?;
    let current = settings.get(KEY);
    let ours = command_of(current).is_some_and(|c| is_collector_command(&c));
    Ok(InstallPreview {
        settings_path: path.display().to_string(),
        before: pretty(current),
        after: pretty(Some(&new_statusline(current, exe))),
        chains: if ours { None } else { command_of(current) },
    })
}

fn backup_and_write(
    path: &Path,
    settings: &Value,
    now: DateTime<Utc>,
) -> io::Result<Option<PathBuf>> {
    let backup = if path.exists() {
        let name = format!(
            "{}.daily-sausage-backup-{}",
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("settings.json"),
            now.format("%Y%m%d-%H%M%S")
        );
        let b = path.with_file_name(name);
        std::fs::copy(path, &b)?;
        Some(b)
    } else {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        None
    };
    let mut text = serde_json::to_string_pretty(settings).map_err(io::Error::other)?;
    text.push('\n');
    let tmp = path.with_extension("json.daily-sausage-tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)?;
    Ok(backup)
}

/// Points `statusLine` at the collector. Remembers the previous one in `cfg`
/// (the caller saves `cfg`). Returns the backup path, if a file existed.
pub fn install(
    path: &Path,
    exe: &Path,
    cfg: &mut Config,
    now: DateTime<Utc>,
) -> io::Result<Option<PathBuf>> {
    let mut settings = read_settings(path)?;
    let current = settings.get(KEY).cloned();
    let ours = command_of(current.as_ref()).is_some_and(|c| is_collector_command(&c));
    if !ours {
        cfg.chain_command = command_of(current.as_ref());
        cfg.original_statusline = current.clone();
    }
    settings[KEY] = new_statusline(current.as_ref(), exe);
    backup_and_write(path, &settings, now)
}

/// Restores the previous `statusLine` (or removes ours). Leaves a status line
/// that is not ours untouched. Returns the backup path when the file changed.
pub fn uninstall(path: &Path, cfg: &mut Config, now: DateTime<Utc>) -> io::Result<Option<PathBuf>> {
    let mut settings = read_settings(path)?;
    let ours = command_of(settings.get(KEY)).is_some_and(|c| is_collector_command(&c));
    let result = if ours {
        let obj = settings.as_object_mut().expect("checked object");
        match cfg.original_statusline.take() {
            Some(orig) => {
                obj.insert(KEY.into(), orig);
            }
            None => {
                obj.remove(KEY);
            }
        }
        backup_and_write(path, &settings, now)?
    } else {
        None
    };
    cfg.chain_command = None;
    cfg.original_statusline = None;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 2, 6, 0, 0).unwrap()
    }
    fn exe() -> PathBuf {
        PathBuf::from("/Applications/Daily Sausage.app/Contents/MacOS/daily-sausage")
    }

    #[test]
    fn command_format() {
        assert_eq!(
            collector_command(Path::new(
                r"C:\Users\me\AppData\Local\Daily Sausage\daily-sausage.exe"
            )),
            "\"C:/Users/me/AppData/Local/Daily Sausage/daily-sausage.exe\" collect"
        );
        assert!(is_collector_command(&collector_command(&exe())));
        assert!(!is_collector_command("bash ~/.claude/statusline.sh"));
    }

    #[test]
    fn install_without_existing_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        assert_eq!(state(&path), StatuslineState::NotConfigured);
        let mut cfg = Config::default();
        let backup = install(&path, &exe(), &mut cfg, now()).unwrap();
        assert!(backup.is_none());
        assert!(matches!(state(&path), StatuslineState::Installed { .. }));
        assert!(cfg.chain_command.is_none());
        uninstall(&path, &mut cfg, now()).unwrap();
        assert_eq!(state(&path), StatuslineState::NotConfigured);
    }

    #[test]
    fn install_chains_and_uninstall_restores() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let original = r#"{
  "model": "opus",
  "statusLine": { "type": "command", "command": "bash ~/.claude/sl.sh", "padding": 1, "refreshInterval": 5 },
  "permissions": { "allow": ["Bash(ls:*)"] }
}"#;
        std::fs::write(&path, original).unwrap();
        let mut cfg = Config::default();

        let pv = preview_install(&path, &exe()).unwrap();
        assert!(pv.before.contains("sl.sh"));
        assert!(pv.after.contains("daily-sausage"));
        assert_eq!(pv.chains.as_deref(), Some("bash ~/.claude/sl.sh"));

        let backup = install(&path, &exe(), &mut cfg, now()).unwrap().unwrap();
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), original);
        assert_eq!(cfg.chain_command.as_deref(), Some("bash ~/.claude/sl.sh"));
        let v = read_settings(&path).unwrap();
        assert_eq!(v["statusLine"]["padding"], 1);
        assert_eq!(v["statusLine"]["refreshInterval"], 5);
        assert_eq!(v["permissions"]["allow"][0], "Bash(ls:*)");
        // Key order is preserved.
        let keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys, ["model", "statusLine", "permissions"]);

        // Re-installing (e.g. the app moved) keeps the remembered chain.
        install(&path, &exe(), &mut cfg, now()).unwrap();
        assert_eq!(cfg.chain_command.as_deref(), Some("bash ~/.claude/sl.sh"));

        uninstall(&path, &mut cfg, now()).unwrap();
        let v = read_settings(&path).unwrap();
        assert_eq!(v["statusLine"]["command"], "bash ~/.claude/sl.sh");
        assert!(cfg.chain_command.is_none() && cfg.original_statusline.is_none());
    }

    #[test]
    fn refuses_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "{ not json").unwrap();
        let mut cfg = Config::default();
        assert!(install(&path, &exe(), &mut cfg, now()).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
        assert!(matches!(state(&path), StatuslineState::Unreadable { .. }));
    }

    #[test]
    fn uninstall_leaves_foreign_statusline() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(
            &path,
            r#"{"statusLine":{"type":"command","command":"other"}}"#,
        )
        .unwrap();
        let mut cfg = Config::default();
        assert!(uninstall(&path, &mut cfg, now()).unwrap().is_none());
        assert_eq!(
            read_settings(&path).unwrap()["statusLine"]["command"],
            "other"
        );
    }
}
