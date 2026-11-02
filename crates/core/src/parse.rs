//! Turn the two known JSON shapes into a [`Sample`].

use crate::model::{ExtraUsage, Sample, Source, Window};
use chrono::{DateTime, TimeZone, Utc};
use serde_json::Value;

fn num(v: Option<&Value>) -> Option<f64> {
    v.and_then(Value::as_f64).filter(|x| x.is_finite())
}

fn epoch_secs(v: Option<&Value>) -> Option<DateTime<Utc>> {
    let secs = num(v)?;
    // Defensive: accept milliseconds too.
    let secs = if secs > 1.0e11 { secs / 1000.0 } else { secs };
    Utc.timestamp_opt(secs.floor() as i64, 0).single()
}

fn iso(v: Option<&Value>) -> Option<DateTime<Utc>> {
    let s = v?.as_str()?;
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

/// Status line input: `rate_limits.{five_hour,seven_day}.{used_percentage,resets_at}`
/// with `resets_at` in Unix epoch seconds. Returns `None` when `rate_limits`
/// carries no subscription window (API-key sessions, before the first response).
pub fn from_statusline(input: &Value, now: DateTime<Utc>) -> Option<Sample> {
    let rl = input.get("rate_limits")?;
    let window = |key: &str| -> Option<Window> {
        let w = rl.get(key)?;
        Some(Window {
            pct: num(w.get("used_percentage"))?,
            resets_at: epoch_secs(w.get("resets_at")),
        })
    };
    let mut s = Sample::new(now, Source::Statusline);
    s.five_hour = window("five_hour");
    s.seven_day = window("seven_day");
    s.has_any_window().then_some(s)
}

/// Unofficial usage endpoint: top-level windows with `utilization` (0-100) and
/// `resets_at` as an ISO 8601 string; any of them may be `null` or absent.
pub fn from_oauth(body: &Value, now: DateTime<Utc>) -> Option<Sample> {
    let window = |key: &str| -> Option<Window> {
        let w = body.get(key)?;
        Some(Window {
            pct: num(w.get("utilization"))?,
            resets_at: iso(w.get("resets_at")),
        })
    };
    let mut s = Sample::new(now, Source::Oauth);
    s.five_hour = window("five_hour");
    s.seven_day = window("seven_day");
    s.seven_day_opus = window("seven_day_opus");
    s.seven_day_sonnet = window("seven_day_sonnet");
    s.extra_usage = body
        .get("extra_usage")
        .filter(|v| v.is_object())
        .map(|e| ExtraUsage {
            is_enabled: e
                .get("is_enabled")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            monthly_limit: num(e.get("monthly_limit")),
            used_credits: num(e.get("used_credits")),
            utilization: num(e.get("utilization")),
        });
    s.has_any_window().then_some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 2, 6, 0, 0).unwrap()
    }

    #[test]
    fn statusline_full() {
        let v: Value =
            serde_json::from_str(include_str!("../tests/fixtures/statusline.json")).unwrap();
        let s = from_statusline(&v, now()).unwrap();
        assert_eq!(s.source, Source::Statusline);
        assert_eq!(s.five_hour.unwrap().pct, 23.5);
        assert_eq!(
            s.five_hour.unwrap().resets_at.unwrap(),
            Utc.timestamp_opt(1738425600, 0).unwrap()
        );
        assert_eq!(s.seven_day.unwrap().pct, 41.2);
    }

    #[test]
    fn statusline_without_rate_limits() {
        let v: Value = serde_json::json!({"model": {"display_name": "Opus"}});
        assert!(from_statusline(&v, now()).is_none());
        let v: Value = serde_json::json!({"rate_limits": {"spend_limit": {"used_percentage": 3}}});
        assert!(from_statusline(&v, now()).is_none());
    }

    #[test]
    fn statusline_partial_window() {
        let v = serde_json::json!({"rate_limits": {"seven_day": {"used_percentage": 7}}});
        let s = from_statusline(&v, now()).unwrap();
        assert!(s.five_hour.is_none());
        assert_eq!(s.seven_day.unwrap().pct, 7.0);
        assert!(s.seven_day.unwrap().resets_at.is_none());
    }

    #[test]
    fn oauth_full() {
        let v: Value =
            serde_json::from_str(include_str!("../tests/fixtures/oauth_usage.json")).unwrap();
        let s = from_oauth(&v, now()).unwrap();
        assert_eq!(s.source, Source::Oauth);
        assert_eq!(s.five_hour.unwrap().pct, 24.0);
        let sd = s.seven_day.unwrap();
        assert_eq!(sd.pct, 58.0);
        assert_eq!(
            sd.resets_at.unwrap(),
            Utc.with_ymd_and_hms(2026, 11, 5, 1, 0, 0).unwrap()
        );
        assert_eq!(s.seven_day_sonnet.unwrap().pct, 12.0);
        assert!(s.seven_day_opus.is_none());
        assert!(!s.extra_usage.unwrap().is_enabled);
    }

    #[test]
    fn oauth_all_null() {
        let v = serde_json::json!({"five_hour": null, "seven_day": null});
        assert!(from_oauth(&v, now()).is_none());
    }
}
