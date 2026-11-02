//! Reads Claude Code's stored claude.ai login, only for the optional unofficial
//! usage endpoint. The token stays in memory: it is never written, logged or
//! sent anywhere but api.anthropic.com. It is never refreshed here either, since
//! refreshing would rotate the refresh token under Claude Code.

use chrono::{DateTime, Duration, TimeZone, Utc};
use serde_json::Value;
use std::fmt;
use std::path::PathBuf;

#[derive(Clone, PartialEq)]
pub struct OauthToken {
    pub access_token: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub subscription_type: Option<String>,
}

impl fmt::Debug for OauthToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OauthToken")
            .field("access_token", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .field("subscription_type", &self.subscription_type)
            .finish()
    }
}

impl OauthToken {
    /// Expired, or expiring within a minute.
    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        self.expires_at
            .is_some_and(|e| e <= now + Duration::seconds(60))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CredError {
    /// No stored login was found.
    NotFound,
    /// A login exists but is not a claude.ai subscription login.
    NoOauth,
    Unreadable(String),
}

impl fmt::Display for CredError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CredError::NotFound => write!(f, "Claude Code 로그인 정보를 찾지 못했어요"),
            CredError::NoOauth => write!(f, "claude.ai 구독 로그인이 아니에요"),
            CredError::Unreadable(e) => write!(f, "로그인 정보를 읽지 못했어요: {e}"),
        }
    }
}

/// Parses `{"claudeAiOauth": {"accessToken", "expiresAt" (ms), "subscriptionType"}}`.
pub fn parse(json: &str) -> Result<OauthToken, CredError> {
    let v: Value =
        serde_json::from_str(json.trim()).map_err(|e| CredError::Unreadable(e.to_string()))?;
    let o = v.get("claudeAiOauth").ok_or(CredError::NoOauth)?;
    let access_token = o
        .get("accessToken")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or(CredError::NoOauth)?
        .to_owned();
    let expires_at = o
        .get("expiresAt")
        .and_then(Value::as_i64)
        .and_then(|ms| Utc.timestamp_millis_opt(ms).single());
    let subscription_type = o
        .get("subscriptionType")
        .and_then(Value::as_str)
        .map(str::to_owned);
    Ok(OauthToken {
        access_token,
        expires_at,
        subscription_type,
    })
}

pub fn credentials_file() -> PathBuf {
    crate::paths::claude_config_dir().join(".credentials.json")
}

#[cfg(target_os = "macos")]
fn from_keychain() -> Option<OauthToken> {
    let out = std::process::Command::new("/usr/bin/security")
        .args([
            "find-generic-password",
            "-s",
            "Claude Code-credentials",
            "-w",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse(&String::from_utf8_lossy(&out.stdout)).ok()
}

/// macOS: Keychain first (Claude Code's default), then the file fallback.
/// Windows/Linux: `~/.claude/.credentials.json` (or under `CLAUDE_CONFIG_DIR`).
pub fn load() -> Result<OauthToken, CredError> {
    #[cfg(target_os = "macos")]
    if let Some(t) = from_keychain() {
        return Ok(t);
    }
    match std::fs::read_to_string(credentials_file()) {
        Ok(s) => parse(&s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(CredError::NotFound),
        Err(e) => Err(CredError::Unreadable(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_redacts() {
        let t = parse(
            r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-secret","refreshToken":"r","expiresAt":1793000000000,"scopes":["user:inference","user:profile"],"subscriptionType":"max"}}"#,
        )
        .unwrap();
        assert_eq!(t.access_token, "sk-ant-oat01-secret");
        assert_eq!(t.subscription_type.as_deref(), Some("max"));
        assert_eq!(t.expires_at.unwrap().timestamp(), 1_793_000_000);
        let dbg = format!("{t:?}");
        assert!(!dbg.contains("secret"), "{dbg}");
    }

    #[test]
    fn expiry() {
        let t = OauthToken {
            access_token: "x".into(),
            expires_at: Some(Utc.with_ymd_and_hms(2026, 11, 2, 0, 0, 0).unwrap()),
            subscription_type: None,
        };
        assert!(t.is_expired(Utc.with_ymd_and_hms(2026, 11, 2, 0, 0, 0).unwrap()));
        assert!(!t.is_expired(Utc.with_ymd_and_hms(2026, 11, 1, 23, 0, 0).unwrap()));
    }

    #[test]
    fn rejects_non_oauth() {
        assert_eq!(parse(r#"{"apiKey":"x"}"#), Err(CredError::NoOauth));
        assert!(matches!(parse("nope"), Err(CredError::Unreadable(_))));
    }
}
