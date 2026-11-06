//! Lets Claude Code refresh its own claude.ai login when the stored token has
//! expired, so this app never touches the refresh token.
//!
//! `claude -p /usage` runs Claude Code's own usage command headless. Claude Code
//! refreshes an expired (or nearly expired) login while it starts, and the
//! command reads the same usage endpoint this app polls, so it needs a valid
//! login either way. The new tokens are saved with Claude Code's own locking
//! and storage (file or macOS Keychain). `/usage` is a local command: it sends
//! no message and uses no subscription usage. This is how Claude Code behaves,
//! not a documented contract, so a future version may stop doing it; the
//! caller re-reads the login to find out.

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Well past Claude Code's own exit wait (about 8.5 s) and its refresh lock
/// retries (about 10 s), so a refresh in progress is not cut off.
pub const REFRESH_TIMEOUT: Duration = Duration::from_secs(30);

/// `claude -p /usage`, plus flags that keep the run from leaving a trace:
/// no saved session (it would show up in `claude --resume`) and none of the
/// user's MCP servers started. Claude Code exits on a flag it does not know,
/// before refreshing anything, so only long-standing flags belong here.
const REFRESH_ARGS: &[&str] = &[
    "-p",
    "/usage",
    "--no-session-persistence",
    "--strict-mcp-config",
];

/// Safe mode also skips the user's hooks and plugins, so a SessionStart hook
/// does not fire every time the login expires. Set as a variable rather than
/// `--safe-mode` because a Claude Code too old to know it ignores the variable
/// but would reject the flag. Auth, proxies and other `env` settings still apply.
const SAFE_MODE_VAR: (&str, &str) = ("CLAUDE_CODE_SAFE_MODE", "1");

/// Variables that make Claude Code use another credential than the stored
/// login; with any of them set it would not refresh that login.
const AUTH_OVERRIDE_VARS: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "CLAUDE_CODE_OAUTH_TOKEN",
    "CLAUDE_CODE_OAUTH_TOKEN_FILE_DESCRIPTOR",
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_USE_FOUNDRY",
];

#[cfg(windows)]
const EXE_NAMES: &[&str] = &["claude.exe", "claude.cmd"];
#[cfg(not(windows))]
const EXE_NAMES: &[&str] = &["claude"];

#[derive(Debug, Clone, PartialEq)]
pub enum CliError {
    NotFound,
    Spawn(String),
    Timeout,
    /// The exit code, or `None` when a signal ended it.
    Exit(Option<i32>),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::NotFound => write!(f, "Claude Code 실행 파일을 찾지 못했어요"),
            CliError::Spawn(e) => write!(f, "Claude Code를 실행하지 못했어요 ({e})"),
            CliError::Timeout => write!(
                f,
                "Claude Code가 {}초 안에 끝나지 않았어요",
                REFRESH_TIMEOUT.as_secs()
            ),
            CliError::Exit(Some(c)) => write!(f, "Claude Code가 종료 코드 {c}로 끝났어요"),
            CliError::Exit(None) => write!(f, "Claude Code가 비정상적으로 끝났어요"),
        }
    }
}

fn is_executable(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

/// The first `dir/name` that is an executable file, trying every name in a
/// directory before moving to the next one. Relative directories are skipped
/// so nothing is picked up from whatever the working directory happens to be.
pub fn find_in(dirs: &[PathBuf], names: &[&str]) -> Option<PathBuf> {
    dirs.iter()
        .filter(|d| d.is_absolute())
        .flat_map(|d| names.iter().map(move |n| d.join(n)))
        .find(|p| is_executable(p))
}

/// Install locations that a GUI app's `PATH` often lacks (macOS apps do not
/// inherit the shell's `PATH`).
fn known_dirs() -> Vec<PathBuf> {
    let home = dirs::home_dir();
    let mut v = Vec::new();
    #[cfg(windows)]
    {
        if let Some(h) = &home {
            v.push(h.join(".local").join("bin"));
        }
        if let Some(appdata) = dirs::config_dir() {
            v.push(appdata.join("npm"));
        }
    }
    #[cfg(not(windows))]
    {
        if let Some(h) = &home {
            v.push(h.join(".local/bin"));
            v.push(h.join(".claude/local"));
        }
        v.push(PathBuf::from("/opt/homebrew/bin"));
        v.push(PathBuf::from("/usr/local/bin"));
    }
    v
}

/// Finds the `claude` executable: `override_path` if given (and nothing else,
/// so a wrong setting shows up instead of being papered over), otherwise `PATH`
/// and then the usual install locations.
pub fn find_claude(override_path: Option<&str>) -> Option<PathBuf> {
    if let Some(p) = override_path.map(str::trim).filter(|p| !p.is_empty()) {
        let p = PathBuf::from(p);
        return is_executable(&p).then_some(p);
    }
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    dirs.extend(known_dirs());
    find_in(&dirs, EXE_NAMES)
}

/// Runs `exe -p /usage` (see `REFRESH_ARGS`) without a window and waits up to
/// `timeout`. Output is discarded.
pub fn run_usage(exe: &Path, cwd: &Path, timeout: Duration) -> Result<(), CliError> {
    let mut cmd = Command::new(exe);
    cmd.args(REFRESH_ARGS)
        .env(SAFE_MODE_VAR.0, SAFE_MODE_VAR.1)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for var in AUTH_OVERRIDE_VARS {
        cmd.env_remove(var);
    }
    if cwd.is_dir() {
        cmd.current_dir(cwd);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd.spawn().map_err(|e| CliError::Spawn(e.to_string()))?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => return Err(CliError::Exit(status.code())),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(100)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(CliError::Timeout);
            }
            Err(e) => return Err(CliError::Spawn(e.to_string())),
        }
    }
}

/// Asks Claude Code to refresh its stored login. The caller re-reads the
/// login afterwards to see whether it actually changed.
pub fn refresh_login(override_path: Option<&str>, cwd: &Path) -> Result<(), CliError> {
    let exe = find_claude(override_path).ok_or(CliError::NotFound)?;
    run_usage(&exe, cwd, REFRESH_TIMEOUT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let p = dir.join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    /// Other tests spawn processes concurrently, and a child forked while this
    /// test still had the script open makes exec fail with "Text file busy".
    #[cfg(unix)]
    fn run(exe: &Path, cwd: &Path, timeout: Duration) -> Result<(), CliError> {
        for _ in 0..20 {
            match run_usage(exe, cwd, timeout) {
                Err(CliError::Spawn(e)) if e.contains("os error 26") => {
                    std::thread::sleep(Duration::from_millis(50))
                }
                r => return r,
            }
        }
        run_usage(exe, cwd, timeout)
    }

    #[cfg(unix)]
    #[test]
    fn finds_first_executable_in_order() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        std::fs::write(a.path().join("claude"), "not executable").unwrap();
        let want = script(b.path(), "claude", "exit 0");
        let dirs = vec![
            PathBuf::from("relative/dir"),
            a.path().to_path_buf(),
            b.path().to_path_buf(),
        ];
        assert_eq!(find_in(&dirs, &["claude"]), Some(want));
        assert_eq!(find_in(&dirs, &["nope"]), None);
    }

    #[cfg(unix)]
    #[test]
    fn override_path_wins_and_is_not_second_guessed() {
        let d = tempfile::tempdir().unwrap();
        let exe = script(d.path(), "my-claude", "exit 0");
        assert_eq!(find_claude(exe.to_str()), Some(exe.clone()));
        let missing = d.path().join("missing");
        assert_eq!(find_claude(missing.to_str()), None);
        // Blank means "not set".
        assert_eq!(find_claude(Some("  ")), find_claude(None));
    }

    #[cfg(unix)]
    #[test]
    fn runs_usage_in_cwd_in_safe_mode() {
        let d = tempfile::tempdir().unwrap();
        let out = d.path().join("out.txt");
        let exe = script(
            d.path(),
            "claude",
            &format!(
                "echo \"$@\" > '{o}'; pwd >> '{o}'; echo \"safe=$CLAUDE_CODE_SAFE_MODE\" >> '{o}'",
                o = out.display()
            ),
        );
        let cwd = tempfile::tempdir().unwrap();
        assert_eq!(run(&exe, cwd.path(), Duration::from_secs(10)), Ok(()));
        let got = std::fs::read_to_string(&out).unwrap();
        let mut lines = got.lines();
        assert_eq!(
            lines.next(),
            Some("-p /usage --no-session-persistence --strict-mcp-config")
        );
        let pwd = PathBuf::from(lines.next().unwrap());
        assert_eq!(
            pwd.canonicalize().unwrap(),
            cwd.path().canonicalize().unwrap()
        );
        assert_eq!(lines.next(), Some("safe=1"));
    }

    #[cfg(unix)]
    #[test]
    fn reports_exit_code_and_timeout() {
        let d = tempfile::tempdir().unwrap();
        let fail = script(d.path(), "fail", "exit 1");
        assert_eq!(
            run(&fail, d.path(), Duration::from_secs(10)),
            Err(CliError::Exit(Some(1)))
        );
        let slow = script(d.path(), "slow", "exec sleep 5");
        let started = Instant::now();
        assert_eq!(
            run(&slow, d.path(), Duration::from_millis(200)),
            Err(CliError::Timeout)
        );
        assert!(started.elapsed() < Duration::from_secs(4));
    }

    #[test]
    fn missing_executable_is_a_spawn_error() {
        let d = tempfile::tempdir().unwrap();
        let r = run_usage(&d.path().join("missing"), d.path(), Duration::from_secs(1));
        assert!(matches!(r, Err(CliError::Spawn(_))), "{r:?}");
    }
}
