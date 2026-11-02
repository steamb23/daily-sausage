use std::path::PathBuf;

/// Directory holding `samples.jsonl`, `config.json` and `notify_state.json`.
///
/// `DAILY_SAUSAGE_DATA_DIR` overrides it (tests, portable setups). Otherwise:
/// Windows `%APPDATA%\daily-sausage`, macOS `~/Library/Application Support/daily-sausage`,
/// Linux `~/.local/share/daily-sausage`.
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("DAILY_SAUSAGE_DATA_DIR") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("daily-sausage")
}

/// Claude Code's configuration directory: `CLAUDE_CONFIG_DIR` or `~/.claude`.
pub fn claude_config_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude")
}
