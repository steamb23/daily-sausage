//! Optional client for the unofficial `/api/oauth/usage` endpoint, the one
//! Claude Code's `/usage` uses. Undocumented: it may change or disappear.

use crate::credentials::OauthToken;
use crate::model::Sample;
use crate::parse;
use chrono::{DateTime, Utc};
use std::time::Duration;

pub const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";

#[derive(Debug, Clone, PartialEq)]
pub enum FetchError {
    /// 401: the stored login is expired or revoked.
    Unauthorized,
    /// 403: this account (for example a Team seat) cannot read usage this way.
    Forbidden,
    RateLimited {
        retry_after_secs: Option<u64>,
    },
    Http(u16),
    Network(String),
    Parse,
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::Unauthorized => write!(
                f,
                "로그인이 만료됐어요. Claude Code를 한 번 실행하면 다시 갱신돼요"
            ),
            FetchError::Forbidden => write!(f, "이 계정에서는 사용량 API를 쓸 수 없어요"),
            FetchError::RateLimited { .. } => {
                write!(f, "요청이 너무 잦아요. 잠시 뒤 다시 시도할게요")
            }
            FetchError::Http(c) => write!(f, "서버 응답 오류 ({c})"),
            FetchError::Network(e) => write!(f, "네트워크 오류: {e}"),
            FetchError::Parse => write!(f, "응답 형식이 예상과 달라요"),
        }
    }
}

pub fn fetch(token: &OauthToken, now: DateTime<Utc>) -> Result<Sample, FetchError> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(5)))
        .http_status_as_error(false)
        .build()
        .into();
    let mut resp = agent
        .get(USAGE_URL)
        .header("Authorization", &format!("Bearer {}", token.access_token))
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("Accept", "application/json")
        .header(
            "User-Agent",
            concat!("daily-sausage/", env!("CARGO_PKG_VERSION")),
        )
        .call()
        .map_err(|e| FetchError::Network(e.to_string()))?;
    match resp.status().as_u16() {
        200 => {
            let body: serde_json::Value =
                resp.body_mut().read_json().map_err(|_| FetchError::Parse)?;
            parse::from_oauth(&body, now).ok_or(FetchError::Parse)
        }
        401 => Err(FetchError::Unauthorized),
        403 => Err(FetchError::Forbidden),
        429 => Err(FetchError::RateLimited {
            retry_after_secs: resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.trim().parse().ok()),
        }),
        code => Err(FetchError::Http(code)),
    }
}
