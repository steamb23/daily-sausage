//! `daily-sausage collect`: registered as Claude Code's `statusLine.command`.
//!
//! Reads the status line JSON from stdin, records `rate_limits` as a sample, then
//! prints either the user's previous status line (chained with the same input)
//! or the one-line daily budget summary. It never fails loudly: whatever goes
//! wrong, Claude Code's status line must keep working.

use crate::budget::compute;
use crate::claude_settings::is_collector_command;
use crate::config::Config;
use crate::store::Store;
use crate::{format, parse, paths};
use chrono::{DateTime, TimeZone, Utc};
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const SUBCOMMAND: &str = "collect";
const MAX_INPUT: u64 = 4 * 1024 * 1024;
const CHAIN_TIMEOUT: Duration = Duration::from_secs(5);

/// Records the input and returns the text to print.
pub fn handle<Tz: TimeZone>(
    input: &str,
    store: &Store,
    cfg: &Config,
    now: DateTime<Utc>,
    tz: &Tz,
) -> String {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(input) {
        if let Some(sample) = parse::from_statusline(&v, now) {
            let _ = store.append(&sample);
        }
    }
    if let Some(cmd) = cfg
        .chain_command
        .as_deref()
        .filter(|c| !c.trim().is_empty() && !is_collector_command(c))
    {
        return run_chain(cmd, input).unwrap_or_default();
    }
    if cfg.statusline_summary {
        let samples = store.load_recent(now, 8).unwrap_or_default();
        let view = compute(&samples, now, tz, cfg.boundary_hour, cfg.mode);
        return format::one_line(&view);
    }
    String::new()
}

#[cfg(windows)]
fn find_git_bash() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    if let Some(p) = std::env::var_os("CLAUDE_CODE_GIT_BASH_PATH") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    for base in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
        if let Some(b) = std::env::var_os(base) {
            for rel in ["Git\\bin\\bash.exe", "Programs\\Git\\bin\\bash.exe"] {
                let p = PathBuf::from(&b).join(rel);
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }
    None
}

/// Runs `cmd` through a shell the way Claude Code runs status line commands:
/// `sh -c` on macOS/Linux, Git Bash (or `cmd /C` without it) on Windows.
fn shell(cmd: &str) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut c = match find_git_bash() {
            Some(bash) => {
                let mut c = Command::new(bash);
                c.arg("-c").arg(cmd);
                c
            }
            None => {
                let mut c = Command::new("cmd");
                c.arg("/C").raw_arg(cmd);
                c
            }
        };
        c.creation_flags(CREATE_NO_WINDOW);
        c
    }
    #[cfg(not(windows))]
    {
        let mut c = Command::new("sh");
        c.arg("-c").arg(cmd);
        c
    }
}

/// Runs the previous status line command with the same stdin and returns its stdout.
pub fn run_chain(cmd: &str, input: &str) -> Option<String> {
    let mut child = shell(cmd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    if let Some(mut stdin) = child.stdin.take() {
        let data = input.to_owned();
        std::thread::spawn(move || {
            let _ = stdin.write_all(data.as_bytes());
        });
    }
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut out = String::new();
        let _ = stdout.read_to_string(&mut out);
        let _ = tx.send(out);
    });
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() < CHAIN_TIMEOUT => {
                std::thread::sleep(Duration::from_millis(10))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
        }
    }
    // A grandchild may still hold the pipe open; do not wait for it past the budget.
    let left = CHAIN_TIMEOUT
        .saturating_sub(start.elapsed())
        .max(Duration::from_millis(200));
    Some(rx.recv_timeout(left).unwrap_or_default())
}

/// Entry point for the `collect` subcommand. Always returns exit code 0.
pub fn main_entry() -> i32 {
    let _ = std::panic::catch_unwind(|| {
        let mut input = String::new();
        let _ = std::io::stdin().take(MAX_INPUT).read_to_string(&mut input);
        let dir = paths::data_dir();
        let cfg = Config::load(&dir);
        let store = Store::new(&dir);
        let out = handle(&input, &store, &cfg, Utc::now(), &chrono::Local);
        let mut so = std::io::stdout().lock();
        let _ = so.write_all(out.as_bytes());
        let _ = so.flush();
    });
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    const INPUT: &str = include_str!("../tests/fixtures/statusline.json");

    fn now() -> DateTime<Utc> {
        // Before the fixture's resets_at (2025-02-06), so the week is current.
        Utc.with_ymd_and_hms(2025, 2, 1, 12, 0, 0).unwrap()
    }

    #[test]
    fn records_and_prints_summary() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        let cfg = Config::default();
        let out = handle(
            INPUT,
            &store,
            &cfg,
            now(),
            &FixedOffset::east_opt(9 * 3600).unwrap(),
        );
        assert!(out.starts_with("오늘 "), "{out}");
        assert!(out.contains("주 41%"), "{out}");
        assert!(out.contains("5h 24%") || out.contains("5h 0%"), "{out}");
        assert_eq!(store.load_recent(now(), 8).unwrap().len(), 1);
    }

    #[test]
    fn ignores_input_without_rate_limits() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        let cfg = Config {
            statusline_summary: false,
            ..Config::default()
        };
        let out = handle(r#"{"model":{}}"#, &store, &cfg, now(), &Utc);
        assert_eq!(out, "");
        assert!(store.last().unwrap().is_none());
        let out = handle("garbage", &store, &cfg, now(), &Utc);
        assert_eq!(out, "");
    }

    #[cfg(unix)]
    #[test]
    fn chains_previous_command_with_same_stdin() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        let cfg = Config {
            chain_command: Some("printf 'prev:'; head -c 20".into()),
            ..Config::default()
        };
        let out = handle(INPUT, &store, &cfg, now(), &Utc);
        assert!(out.starts_with("prev:{"), "{out}");
        assert!(store.last().unwrap().is_some(), "still records");
    }

    #[cfg(unix)]
    #[test]
    fn chain_timeout_is_bounded() {
        let started = Instant::now();
        let out = run_chain("sleep 30", "{}");
        assert_eq!(out.as_deref(), Some(""));
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn never_chains_itself() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        let cfg = Config {
            chain_command: Some(
                "\"/Applications/Daily Sausage.app/Contents/MacOS/daily-sausage\" collect".into(),
            ),
            ..Config::default()
        };
        let out = handle(INPUT, &store, &cfg, now(), &Utc);
        assert!(out.starts_with("오늘 "), "{out}");
    }
}
