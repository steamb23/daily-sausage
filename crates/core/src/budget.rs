//! Turns weekly usage readings into a daily budget.
//!
//! Rolling mode (default): at each day boundary the remaining weekly allowance
//! is spread over the time left until the weekly reset, in proportion to hours:
//!
//! ```text
//! today's budget = (100 - W0) * (segment hours / hours until reset)
//!                = base share (100 * segment / 168h) + carry-over
//! ```
//!
//! where `W0` is the weekly usage when today's segment began. Fixed mode keeps
//! the base share every day and reports the carry-over as a balance instead.

use crate::model::{ExtraUsage, Sample, Source, Window};
use chrono::{DateTime, Duration, LocalResult, NaiveDateTime, NaiveTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};

pub const WEEK_HOURS: f64 = 168.0;
/// Readings whose `resets_at` differ by more than this belong to different weeks.
const SAME_WEEK_TOLERANCE_SECS: i64 = 3600;
/// A reading this close before the day boundary counts as the exact start value.
const EXACT_GAP_SECS: i64 = 15 * 60;
/// Data older than this is shown as stale.
pub const STALE_SECS: i64 = 3600;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Re-split what is left over the time left, at every day boundary.
    #[default]
    Rolling,
    /// Same base share every day; surplus or overdraft shown as a balance.
    Fixed,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct WindowView {
    pub pct: f64,
    pub resets_at: Option<DateTime<Utc>>,
    /// The reset time has passed since the reading, so the window is empty now.
    pub expired: bool,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct WeekView {
    pub used_pct: f64,
    pub resets_at: DateTime<Utc>,
    pub week_start: DateTime<Utc>,
    /// Where usage would be if spent evenly over time.
    pub ideal_pct: f64,
    /// `ideal - used`: positive means under pace.
    pub slack_pct: f64,
    /// The slack expressed as hours of even spending.
    pub slack_hours: f64,
    pub remaining_pct: f64,
    /// Remaining allowance in units of one seventh of the week.
    pub remaining_day_units: f64,
    pub days_left: f64,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct TodayView {
    pub mode: Mode,
    /// Today's day boundary (local), which names the day.
    pub day_start: DateTime<Utc>,
    /// Segment = today clipped to the current weekly window.
    pub seg_start: DateTime<Utc>,
    pub seg_end: DateTime<Utc>,
    pub seg_ends_at_reset: bool,
    /// Weekly usage when the segment began (W0).
    pub start_pct: f64,
    /// W0 had to be guessed because no reading was taken near the boundary.
    pub estimated: bool,
    pub budget_pct: f64,
    pub base_pct: f64,
    /// Rolling mode: `budget - base`. Zero in fixed mode.
    pub carry_pct: f64,
    /// Even-pace position minus W0 at the segment start (fixed mode's balance).
    pub balance_pct: f64,
    pub used_pct: f64,
    /// `used / budget`; `None` when the budget is zero.
    pub ratio: Option<f64>,
    /// Negative once over budget.
    pub left_pct: f64,
    /// Rolling mode: tomorrow's budget if no more is used today.
    pub tomorrow_budget_pct: Option<f64>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct BudgetView {
    pub now: DateTime<Utc>,
    pub boundary_hour: u32,
    pub last_sample_at: Option<DateTime<Utc>>,
    pub last_source: Option<Source>,
    pub stale: bool,
    /// The weekly reset passed with no newer reading yet.
    pub waiting_new_week: bool,
    pub week: Option<WeekView>,
    pub today: Option<TodayView>,
    pub five_hour: Option<WindowView>,
    pub seven_day_opus: Option<WindowView>,
    pub seven_day_sonnet: Option<WindowView>,
    pub extra_usage: Option<ExtraUsage>,
}

fn hours(d: Duration) -> f64 {
    d.num_seconds() as f64 / 3600.0
}

fn resolve_local<Tz: TimeZone>(tz: &Tz, ndt: NaiveDateTime) -> DateTime<Utc> {
    match tz.from_local_datetime(&ndt) {
        LocalResult::Single(d) => d.with_timezone(&Utc),
        LocalResult::Ambiguous(a, _) => a.with_timezone(&Utc),
        // Inside a DST gap: the boundary moves to the first valid instant.
        LocalResult::None => tz
            .from_local_datetime(&(ndt + Duration::hours(1)))
            .earliest()
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|| Utc.from_utc_datetime(&ndt)),
    }
}

/// The local day containing `t`, as `[start, end)` boundaries at `boundary_hour`.
pub fn day_bounds<Tz: TimeZone>(
    t: DateTime<Utc>,
    tz: &Tz,
    boundary_hour: u32,
) -> (DateTime<Utc>, DateTime<Utc>) {
    let local = t.with_timezone(tz).naive_local();
    let bt = NaiveTime::from_hms_opt(boundary_hour.min(23), 0, 0).unwrap();
    let mut date = local.date();
    if local.time() < bt {
        date = date.pred_opt().unwrap_or(date);
    }
    let start = resolve_local(tz, date.and_time(bt));
    let next = date.succ_opt().unwrap_or(date);
    let end = resolve_local(tz, next.and_time(bt));
    (start, end)
}

fn window_view(w: &Window, now: DateTime<Utc>) -> WindowView {
    let expired = w.resets_at.is_some_and(|r| r <= now);
    WindowView {
        pct: if expired { 0.0 } else { w.pct },
        resets_at: w.resets_at,
        expired,
    }
}

fn latest_window<'a>(
    sorted: &[&'a Sample],
    pick: impl Fn(&'a Sample) -> Option<&'a Window>,
) -> Option<&'a Window> {
    sorted.iter().rev().find_map(|s| pick(s))
}

pub fn compute<Tz: TimeZone>(
    samples: &[Sample],
    now: DateTime<Utc>,
    tz: &Tz,
    boundary_hour: u32,
    mode: Mode,
) -> BudgetView {
    let boundary_hour = boundary_hour.min(23);
    let mut sorted: Vec<&Sample> = samples.iter().filter(|s| s.ts <= now).collect();
    sorted.sort_by_key(|s| s.ts);
    let latest = sorted.last().copied();

    let mut view = BudgetView {
        now,
        boundary_hour,
        last_sample_at: latest.map(|s| s.ts),
        last_source: latest.map(|s| s.source),
        stale: latest.is_none_or(|s| (now - s.ts).num_seconds() > STALE_SECS),
        waiting_new_week: false,
        week: None,
        today: None,
        five_hour: latest_window(&sorted, |s| s.five_hour.as_ref()).map(|w| window_view(w, now)),
        seven_day_opus: latest_window(&sorted, |s| s.seven_day_opus.as_ref())
            .map(|w| window_view(w, now)),
        seven_day_sonnet: latest_window(&sorted, |s| s.seven_day_sonnet.as_ref())
            .map(|w| window_view(w, now)),
        extra_usage: sorted.iter().rev().find_map(|s| s.extra_usage.clone()),
    };

    let Some((mut reset, mut used)) = sorted.iter().rev().find_map(|s| {
        let w = s.seven_day?;
        Some((w.resets_at?, w.pct))
    }) else {
        return view;
    };

    // Readings that belong to the same weekly window, oldest first.
    let mut week_samples: Vec<(DateTime<Utc>, f64)> = sorted
        .iter()
        .filter_map(|s| {
            let w = s.seven_day?;
            let r = w.resets_at?;
            ((r - reset).num_seconds().abs() <= SAME_WEEK_TOLERANCE_SECS).then_some((s.ts, w.pct))
        })
        .collect();

    if now >= reset {
        // The window reset and no reading of the new week has arrived yet.
        while reset <= now {
            reset += Duration::days(7);
        }
        used = 0.0;
        week_samples.clear();
        view.waiting_new_week = true;
    }
    let week_start = reset - Duration::days(7);

    // --- week ---
    let elapsed_h = hours(now - week_start).clamp(0.0, WEEK_HOURS);
    let ideal = 100.0 * elapsed_h / WEEK_HOURS;
    let slack = ideal - used;
    let remaining = (100.0 - used).max(0.0);
    view.week = Some(WeekView {
        used_pct: used,
        resets_at: reset,
        week_start,
        ideal_pct: ideal,
        slack_pct: slack,
        slack_hours: slack / 100.0 * WEEK_HOURS,
        remaining_pct: remaining,
        remaining_day_units: remaining / (100.0 / 7.0),
        days_left: hours(reset - now) / 24.0,
    });

    // --- today ---
    let (day_start, day_end) = day_bounds(now, tz, boundary_hour);
    let seg_start = day_start.max(week_start);
    let seg_end = day_end.min(reset);

    let (start_pct, estimated) = if seg_start <= week_start {
        (0.0, false)
    } else {
        let prev = week_samples.iter().rev().find(|(t, _)| *t <= seg_start);
        let next = week_samples.iter().find(|(t, _)| *t > seg_start);
        match (prev, next) {
            (Some(&(pt, pp)), next) => {
                let close = (seg_start - pt).num_seconds() <= EXACT_GAP_SECS;
                let unchanged = next.is_none_or(|&(_, np)| (np - pp).abs() < 0.05);
                (pp, !(close || unchanged))
            }
            (None, Some(&(nt, np))) => {
                // No reading before the boundary this week: interpolate from 0% at
                // the week start to the first reading.
                let total = (nt - week_start).num_seconds() as f64;
                let part = (seg_start - week_start).num_seconds() as f64;
                (if total > 0.0 { np * part / total } else { 0.0 }, true)
            }
            (None, None) => (0.0, !view.waiting_new_week),
        }
    };
    // Usage can only go down if limits were reset early; rebase on the lower value.
    let start_pct = start_pct.min(used);

    let seg_h = hours(seg_end - seg_start);
    let rem_h = hours(reset - seg_start);
    let base = 100.0 * seg_h / WEEK_HOURS;
    let rolling = if rem_h > 0.0 {
        (100.0 - start_pct).max(0.0) * seg_h / rem_h
    } else {
        0.0
    };
    let balance = 100.0 * hours(seg_start - week_start) / WEEK_HOURS - start_pct;
    let (budget, carry) = match mode {
        Mode::Rolling => (rolling, rolling - base),
        Mode::Fixed => (base, 0.0),
    };
    let used_today = (used - start_pct).max(0.0);

    let tomorrow = match mode {
        Mode::Rolling if seg_end < reset => {
            let (_, next_end) = day_bounds(seg_end, tz, boundary_hour);
            let next_seg = next_end.min(reset);
            let left_h = hours(reset - seg_end);
            (left_h > 0.0).then(|| (100.0 - used).max(0.0) * hours(next_seg - seg_end) / left_h)
        }
        _ => None,
    };

    view.today = Some(TodayView {
        mode,
        day_start,
        seg_start,
        seg_end,
        seg_ends_at_reset: seg_end == reset,
        start_pct,
        estimated,
        budget_pct: budget,
        base_pct: base,
        carry_pct: carry,
        balance_pct: balance,
        used_pct: used_today,
        ratio: (budget > 1e-9).then(|| used_today / budget),
        left_pct: budget - used_today,
        tomorrow_budget_pct: tomorrow,
    });
    view
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
    fn s(ts: DateTime<Utc>, pct: f64, reset: DateTime<Utc>) -> Sample {
        let mut x = Sample::new(ts, Source::Oauth);
        x.seven_day = Some(Window {
            pct,
            resets_at: Some(reset),
        });
        x
    }
    fn close(a: f64, b: f64) {
        assert!((a - b).abs() < 0.01, "{a} != {b}");
    }

    /// The worked example from the plan: reset Thu 10:00, now Mon 15:00,
    /// 49% at Monday midnight, 58% now.
    fn example() -> (Vec<Sample>, DateTime<Utc>) {
        let reset = at(5, 10, 0);
        (
            vec![s(at(1, 23, 50), 49.0, reset), s(at(2, 14, 55), 58.0, reset)],
            at(2, 15, 0),
        )
    }

    #[test]
    fn worked_example_rolling() {
        let (samples, now) = example();
        let v = compute(&samples, now, &kst(), 0, Mode::Rolling);
        let t = v.today.unwrap();
        assert!(!t.estimated);
        close(t.start_pct, 49.0);
        close(t.budget_pct, 14.93);
        close(t.base_pct, 14.29);
        close(t.carry_pct, 0.64);
        close(t.used_pct, 9.0);
        close(t.ratio.unwrap(), 0.6028);
        close(t.left_pct, 5.93);
        close(t.tomorrow_budget_pct.unwrap(), 42.0 * 24.0 / 58.0);
        assert_eq!(t.day_start, at(2, 0, 0));
        assert_eq!(t.seg_end, at(3, 0, 0));
        assert!(!t.seg_ends_at_reset);

        let w = v.week.unwrap();
        close(w.ideal_pct, 60.12);
        close(w.slack_pct, 2.12);
        assert!((w.slack_hours - 3.56).abs() < 0.01);
        close(w.remaining_day_units, 2.94);
        close(w.days_left, 2.79);
    }

    #[test]
    fn overshoot_shrinks_tomorrow() {
        let reset = at(5, 10, 0);
        let samples = vec![s(at(1, 23, 55), 49.0, reset), s(at(2, 23, 0), 69.0, reset)];
        let v = compute(&samples, at(2, 23, 30), &kst(), 0, Mode::Rolling);
        let t = v.today.unwrap();
        assert!(t.ratio.unwrap() > 1.0);
        assert!(t.left_pct < 0.0);
        close(t.tomorrow_budget_pct.unwrap(), 12.83);
    }

    #[test]
    fn fixed_mode_balance() {
        let (samples, now) = example();
        let t = compute(&samples, now, &kst(), 0, Mode::Fixed)
            .today
            .unwrap();
        close(t.budget_pct, 14.29);
        close(t.carry_pct, 0.0);
        close(t.balance_pct, 100.0 * 86.0 / 168.0 - 49.0); // +2.19
        close(t.ratio.unwrap(), 9.0 / (100.0 * 24.0 / 168.0));
        assert!(t.tomorrow_budget_pct.is_none());
    }

    #[test]
    fn last_day_segment_ends_at_reset() {
        let reset = at(5, 10, 0);
        let samples = vec![s(at(4, 23, 59), 90.0, reset), s(at(5, 7, 0), 95.0, reset)];
        let t = compute(&samples, at(5, 8, 0), &kst(), 0, Mode::Rolling)
            .today
            .unwrap();
        assert_eq!(t.seg_end, reset);
        assert!(t.seg_ends_at_reset);
        close(t.budget_pct, 10.0); // all that is left, 10h of 10h
        close(t.used_pct, 5.0);
        assert!(t.tomorrow_budget_pct.is_none());
    }

    #[test]
    fn first_segment_after_reset_starts_at_zero() {
        let old = at(5, 10, 0);
        let new = at(12, 10, 0);
        let samples = vec![s(at(5, 9, 0), 97.0, old), s(at(5, 11, 30), 3.0, new)];
        let v = compute(&samples, at(5, 12, 0), &kst(), 0, Mode::Rolling);
        let t = v.today.unwrap();
        assert_eq!(t.seg_start, old);
        assert!(!t.estimated);
        close(t.start_pct, 0.0);
        close(t.budget_pct, 100.0 * 14.0 / 168.0);
        close(t.used_pct, 3.0);
        assert!(!v.waiting_new_week);
    }

    #[test]
    fn waiting_for_new_week() {
        let reset = at(5, 10, 0);
        let samples = vec![s(at(5, 9, 0), 97.0, reset)];
        let v = compute(&samples, at(6, 12, 0), &kst(), 0, Mode::Rolling);
        assert!(v.waiting_new_week);
        let w = v.week.unwrap();
        assert_eq!(w.resets_at, at(12, 10, 0));
        close(w.used_pct, 0.0);
        let t = v.today.unwrap();
        close(t.used_pct, 0.0);
        close(t.start_pct, 0.0);
        assert!(!t.estimated);
    }

    #[test]
    fn gap_over_boundary_is_estimated() {
        let reset = at(5, 10, 0);
        let samples = vec![s(at(1, 20, 0), 47.0, reset), s(at(2, 10, 0), 49.0, reset)];
        let t = compute(&samples, at(2, 11, 0), &kst(), 0, Mode::Rolling)
            .today
            .unwrap();
        assert!(t.estimated);
        close(t.start_pct, 47.0);
        close(t.used_pct, 2.0);
    }

    #[test]
    fn gap_without_change_is_exact() {
        let reset = at(5, 10, 0);
        let samples = vec![s(at(1, 20, 0), 47.0, reset), s(at(2, 10, 0), 47.0, reset)];
        let t = compute(&samples, at(2, 11, 0), &kst(), 0, Mode::Rolling)
            .today
            .unwrap();
        assert!(!t.estimated);
    }

    #[test]
    fn no_reading_before_boundary_interpolates() {
        // Week started Thu 10:00; first reading Sat 10:00 at 40%. Day = Sat.
        let reset = at(12, 10, 0);
        let samples = vec![s(at(7, 10, 0), 40.0, reset)];
        let t = compute(&samples, at(7, 11, 0), &kst(), 0, Mode::Rolling)
            .today
            .unwrap();
        assert!(t.estimated);
        // Sat 00:00 is 38h into the 48h before the reading.
        close(t.start_pct, 40.0 * 38.0 / 48.0);
    }

    #[test]
    fn readings_from_old_week_are_ignored() {
        let old = at(5, 10, 0);
        let new = at(12, 10, 0);
        // Day Fri 6th: only an old-week reading before midnight.
        let samples = vec![s(at(5, 23, 0), 99.0, old), s(at(6, 9, 0), 12.0, new)];
        let t = compute(&samples, at(6, 10, 0), &kst(), 0, Mode::Rolling)
            .today
            .unwrap();
        assert!(t.estimated);
        assert!(t.start_pct < 12.0);
    }

    #[test]
    fn early_reset_rebases() {
        let reset = at(5, 10, 0);
        let samples = vec![s(at(1, 23, 55), 60.0, reset), s(at(2, 12, 0), 5.0, reset)];
        let t = compute(&samples, at(2, 13, 0), &kst(), 0, Mode::Rolling)
            .today
            .unwrap();
        close(t.start_pct, 5.0);
        close(t.used_pct, 0.0);
    }

    #[test]
    fn boundary_hour_four() {
        let (samples, _) = example();
        // 02:00 Tuesday still belongs to Monday when the day starts at 04:00.
        let v = compute(&samples, at(3, 2, 0), &kst(), 4, Mode::Rolling);
        let t = v.today.unwrap();
        assert_eq!(t.day_start, at(2, 4, 0));
        assert_eq!(t.seg_end, at(3, 4, 0));
    }

    #[test]
    fn five_hour_expiry_and_stale() {
        let reset = at(5, 10, 0);
        let mut x = s(at(2, 9, 0), 30.0, reset);
        x.five_hour = Some(Window {
            pct: 80.0,
            resets_at: Some(at(2, 11, 0)),
        });
        let v = compute(&[x], at(2, 12, 0), &kst(), 0, Mode::Rolling);
        let f = v.five_hour.unwrap();
        assert!(f.expired);
        close(f.pct, 0.0);
        assert!(v.stale);
    }

    #[test]
    fn empty_and_no_reset() {
        let v = compute(&[], at(2, 12, 0), &kst(), 0, Mode::Rolling);
        assert!(v.week.is_none() && v.today.is_none() && v.stale);
        let mut x = Sample::new(at(2, 11, 0), Source::Statusline);
        x.seven_day = Some(Window {
            pct: 10.0,
            resets_at: None,
        });
        let v = compute(&[x], at(2, 12, 0), &kst(), 0, Mode::Rolling);
        assert!(v.today.is_none());
    }
}
