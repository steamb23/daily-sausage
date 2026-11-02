//! When to poll the optional usage endpoint: every `poll_minutes`, once right
//! after the day boundary, on demand (at most once a minute), with exponential
//! backoff on rate limits and server errors, and never again after a 403.

use crate::budget::day_bounds;
use chrono::{DateTime, Duration, TimeZone, Utc};
use serde::Serialize;

const MAX_BACKOFF_SECS: i64 = 30 * 60;
const MIN_FORCED_GAP_SECS: i64 = 60;

#[derive(Debug, Clone, PartialEq)]
pub enum PollOutcome {
    Ok,
    RateLimited {
        retry_after_secs: Option<u64>,
    },
    /// Network or 5xx: retry with backoff.
    Transient(String),
    /// Missing or expired login: retry at the normal pace.
    AuthProblem(String),
    /// The account cannot use the endpoint: stop until the option is toggled.
    Forbidden(String),
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct PollSchedule {
    pub next_due: Option<DateTime<Utc>>,
    pub backoff_secs: i64,
    pub last_attempt: Option<DateTime<Utc>>,
    pub last_ok: Option<DateTime<Utc>>,
    /// Shown under the settings toggle; `None` when the last poll worked.
    pub message: Option<String>,
    pub stopped: bool,
}

impl PollSchedule {
    pub fn due<Tz: TimeZone>(
        &self,
        now: DateTime<Utc>,
        force: bool,
        tz: &Tz,
        boundary_hour: u32,
    ) -> bool {
        if self.stopped {
            return false;
        }
        let Some(last) = self.last_attempt else {
            return true;
        };
        if force {
            return (now - last).num_seconds() >= MIN_FORCED_GAP_SECS;
        }
        if self.backoff_secs == 0 {
            let (day_start, _) = day_bounds(now, tz, boundary_hour);
            if last < day_start {
                return true;
            }
        }
        self.next_due.is_none_or(|d| now >= d)
    }

    pub fn record(&mut self, now: DateTime<Utc>, poll_minutes: u32, outcome: PollOutcome) {
        let poll_secs = i64::from(poll_minutes.max(1)) * 60;
        self.last_attempt = Some(now);
        match outcome {
            PollOutcome::Ok => {
                self.last_ok = Some(now);
                self.backoff_secs = 0;
                self.message = None;
                self.next_due = Some(now + Duration::seconds(poll_secs));
            }
            PollOutcome::RateLimited { retry_after_secs } => {
                self.backoff(now, poll_secs, retry_after_secs);
                self.message = Some(format!(
                    "요청이 너무 잦다는 응답을 받았어요. {}분 뒤에 다시 시도할게요",
                    (self.backoff_secs + 59) / 60
                ));
            }
            PollOutcome::Transient(msg) => {
                self.backoff(now, poll_secs, None);
                self.message = Some(msg);
            }
            PollOutcome::AuthProblem(msg) => {
                self.backoff_secs = 0;
                self.next_due = Some(now + Duration::seconds(poll_secs));
                self.message = Some(msg);
            }
            PollOutcome::Forbidden(msg) => {
                self.stopped = true;
                self.next_due = None;
                self.message = Some(msg);
            }
        }
    }

    fn backoff(&mut self, now: DateTime<Utc>, poll_secs: i64, retry_after: Option<u64>) {
        let next = if self.backoff_secs == 0 {
            poll_secs
        } else {
            self.backoff_secs * 2
        };
        let floor = retry_after.map_or(0, |r| r.min(MAX_BACKOFF_SECS as u64) as i64);
        self.backoff_secs = next.max(floor).min(MAX_BACKOFF_SECS);
        self.next_due = Some(now + Duration::seconds(self.backoff_secs));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    fn kst() -> FixedOffset {
        FixedOffset::east_opt(9 * 3600).unwrap()
    }
    fn at(d: u32, h: u32, m: u32) -> DateTime<Utc> {
        kst()
            .with_ymd_and_hms(2026, 11, d, h, m, 0)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn regular_interval() {
        let mut s = PollSchedule::default();
        assert!(s.due(at(2, 10, 0), false, &kst(), 0));
        s.record(at(2, 10, 0), 5, PollOutcome::Ok);
        assert!(!s.due(at(2, 10, 4), false, &kst(), 0));
        assert!(s.due(at(2, 10, 5), false, &kst(), 0));
    }

    #[test]
    fn forced_at_most_once_a_minute() {
        let mut s = PollSchedule::default();
        s.record(at(2, 10, 0), 5, PollOutcome::Ok);
        assert!(!s.due(at(2, 10, 0) + Duration::seconds(30), true, &kst(), 0));
        assert!(s.due(at(2, 10, 1), true, &kst(), 0));
    }

    #[test]
    fn polls_right_after_day_boundary() {
        let mut s = PollSchedule::default();
        s.record(at(2, 23, 58), 15, PollOutcome::Ok);
        assert!(!s.due(at(2, 23, 59), false, &kst(), 0));
        assert!(s.due(at(3, 0, 0), false, &kst(), 0));
    }

    #[test]
    fn backoff_doubles_and_caps() {
        let mut s = PollSchedule::default();
        let t = at(2, 10, 0);
        s.record(t, 5, PollOutcome::Transient("x".into()));
        assert_eq!(s.backoff_secs, 300);
        s.record(
            t,
            5,
            PollOutcome::RateLimited {
                retry_after_secs: None,
            },
        );
        assert_eq!(s.backoff_secs, 600);
        for _ in 0..5 {
            s.record(t, 5, PollOutcome::Transient("x".into()));
        }
        assert_eq!(s.backoff_secs, 1800);
        // In backoff, the day boundary does not trigger an extra poll.
        assert!(!s.due(at(2, 10, 20), false, &kst(), 0));
        s.record(t, 5, PollOutcome::Ok);
        assert_eq!(s.backoff_secs, 0);
        assert!(s.message.is_none());
    }

    #[test]
    fn retry_after_is_respected() {
        let mut s = PollSchedule::default();
        s.record(
            at(2, 10, 0),
            2,
            PollOutcome::RateLimited {
                retry_after_secs: Some(900),
            },
        );
        assert_eq!(s.backoff_secs, 900);
        assert!(s.message.as_deref().unwrap().contains("15분"));
    }

    #[test]
    fn forbidden_stops() {
        let mut s = PollSchedule::default();
        s.record(at(2, 10, 0), 5, PollOutcome::Forbidden("no".into()));
        assert!(!s.due(at(3, 10, 0), true, &kst(), 0));
    }
}
