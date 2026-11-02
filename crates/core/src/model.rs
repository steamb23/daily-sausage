use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// One rate-limit window reading: how much is used (0-100, may exceed 100)
/// and when the window resets.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Window {
    pub pct: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<DateTime<Utc>>,
}

/// Where a sample came from.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// Claude Code status line input (`rate_limits`), the documented path.
    Statusline,
    /// Unofficial `/api/oauth/usage` endpoint (personal-use option).
    Oauth,
}

/// Pay-as-you-go extra usage state reported by the usage endpoint.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ExtraUsage {
    pub is_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub monthly_limit: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub used_credits: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub utilization: Option<f64>,
}

/// One reading of every window at a point in time. Stored one per line in
/// `samples.jsonl`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Sample {
    pub ts: DateTime<Utc>,
    pub source: Source,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub five_hour: Option<Window>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seven_day: Option<Window>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seven_day_opus: Option<Window>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seven_day_sonnet: Option<Window>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_usage: Option<ExtraUsage>,
}

impl Sample {
    pub fn new(ts: DateTime<Utc>, source: Source) -> Self {
        Sample {
            ts,
            source,
            five_hour: None,
            seven_day: None,
            seven_day_opus: None,
            seven_day_sonnet: None,
            extra_usage: None,
        }
    }

    pub fn has_any_window(&self) -> bool {
        self.five_hour.is_some()
            || self.seven_day.is_some()
            || self.seven_day_opus.is_some()
            || self.seven_day_sonnet.is_some()
    }

    /// True when every reading (ignoring `ts` and `source`) matches `other`.
    pub fn same_readings(&self, other: &Sample) -> bool {
        self.five_hour == other.five_hour
            && self.seven_day == other.seven_day
            && self.seven_day_opus == other.seven_day_opus
            && self.seven_day_sonnet == other.seven_day_sonnet
            && self.extra_usage == other.extra_usage
    }
}
