//! Korean display strings and the popup model the UI renders as-is.

use crate::budget::{BudgetView, Mode, TodayView, WeekView, WindowView};
use crate::config::Config;
use crate::model::Source;
use chrono::{DateTime, Datelike, Duration, TimeZone, Timelike, Utc};
use serde::{Deserialize, Serialize};

const WEEKDAYS: [&str; 7] = ["월", "화", "수", "목", "금", "토", "일"];

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Ok,
    Warn,
    Over,
    Idle,
}

impl Level {
    pub fn for_ratio(ratio: f64) -> Level {
        if ratio >= 1.0 {
            Level::Over
        } else if ratio >= 0.7 {
            Level::Warn
        } else {
            Level::Ok
        }
    }

    fn for_window_pct(pct: f64) -> Level {
        if pct >= 90.0 {
            Level::Over
        } else if pct >= 70.0 {
            Level::Warn
        } else {
            Level::Ok
        }
    }
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Gauge {
    /// Filled share, 0..=1 (clamped for drawing; the text keeps the real value).
    pub ratio: f64,
    /// Optional tick, such as the even-pace position.
    pub marker: Option<f64>,
    pub level: Level,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Section {
    pub title: String,
    pub right: String,
    pub big: String,
    pub big_note: Option<String>,
    pub gauge: Gauge,
    pub lines: Vec<String>,
    pub note: Option<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Row {
    pub label: String,
    pub big: String,
    pub gauge: Gauge,
    pub right: String,
}

/// Everything the popup, tray icon and tooltip show.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct PopupModel {
    pub today: Option<Section>,
    pub week: Option<Section>,
    pub rows: Vec<Row>,
    pub footer: String,
    pub stale: bool,
    pub empty: Option<String>,
    /// Tray-wide state: today's level, or `Idle` without data / when stale.
    pub level: Level,
    /// Today's ratio for the tray icon (unclamped).
    pub today_ratio: Option<f64>,
    /// Text next to the macOS menu bar icon.
    pub tray_title: String,
    pub tooltip: String,
}

fn p1(x: f64) -> String {
    format!("{:.1}", (x * 10.0).round() / 10.0 + 0.0)
}

fn pct0(x: f64) -> String {
    format!("{}%", x.round() as i64)
}

/// "9시간", "2시간 5분", "45분"
pub fn duration_ko(d: Duration) -> String {
    let mins = d.num_minutes().max(0);
    let (h, m) = (mins / 60, mins % 60);
    if h == 0 {
        format!("{m}분")
    } else if h >= 10 || m == 0 {
        format!("{h}시간")
    } else {
        format!("{h}시간 {m}분")
    }
}

/// "방금", "3분 전", "2시간 전", "1일 전"
pub fn ago_ko(d: Duration) -> String {
    let mins = d.num_minutes();
    if mins < 1 {
        "방금".into()
    } else if mins < 60 {
        format!("{mins}분 전")
    } else if mins < 24 * 60 {
        format!("{}시간 전", mins / 60)
    } else {
        format!("{}일 전", mins / (24 * 60))
    }
}

/// "목 10:00"
pub fn weekday_time<Tz: TimeZone>(t: DateTime<Utc>, tz: &Tz) -> String {
    let l = t.with_timezone(tz);
    format!(
        "{} {:02}:{:02}",
        WEEKDAYS[l.weekday().num_days_from_monday() as usize],
        l.hour(),
        l.minute()
    )
}

fn hm<Tz: TimeZone>(t: DateTime<Utc>, tz: &Tz) -> String {
    let l = t.with_timezone(tz);
    format!("{:02}:{:02}", l.hour(), l.minute())
}

/// "약 3.6시간 분량" / "약 1.2일 분량"
fn amount_of_time(h: f64) -> String {
    let h = h.abs();
    if h >= 24.0 {
        format!("약 {}일 분량", p1(h / 24.0))
    } else {
        format!("약 {}시간 분량", p1(h))
    }
}

fn today_section<Tz: TimeZone>(t: &TodayView, v: &BudgetView, tz: &Tz) -> Section {
    let day = t.day_start.with_timezone(tz);
    let title = format!(
        "오늘 ({}/{} {})",
        day.month(),
        day.day(),
        WEEKDAYS[day.weekday().num_days_from_monday() as usize]
    );
    let until = duration_ko(t.seg_end - v.now);
    let right = if t.seg_ends_at_reset {
        format!("초기화까지 {until}")
    } else if v.boundary_hour == 0 {
        format!("자정까지 {until}")
    } else {
        format!("{:02}:00까지 {until}", v.boundary_hour)
    };
    let ratio = t
        .ratio
        .unwrap_or(if t.used_pct > 0.0 { f64::INFINITY } else { 0.0 });
    let big = match t.ratio {
        Some(r) => pct0(r * 100.0),
        None => "—".into(),
    };
    let left = if t.left_pct >= 0.0 {
        format!("{}%p 남음", p1(t.left_pct))
    } else {
        format!("{}%p 초과", p1(-t.left_pct))
    };
    let mut lines = vec![format!(
        "{} / {}%p 사용 · {left}",
        p1(t.used_pct),
        p1(t.budget_pct)
    )];
    match t.mode {
        Mode::Rolling => lines.push(if t.carry_pct >= 0.05 {
            format!(
                "↳ 기본 {} + 이월 {} (지난 날 아낀 몫)",
                p1(t.base_pct),
                p1(t.carry_pct)
            )
        } else if t.carry_pct <= -0.05 {
            format!(
                "↳ 기본 {} − {} (앞서 당겨 쓴 몫)",
                p1(t.base_pct),
                p1(-t.carry_pct)
            )
        } else {
            format!("↳ 기본 {} (이월 없음)", p1(t.base_pct))
        }),
        Mode::Fixed => lines.push(if t.balance_pct >= 0.0 {
            format!("↳ 이월 잔고 +{}%p (오늘 시작 기준)", p1(t.balance_pct))
        } else {
            format!("↳ 이월 잔고 −{}%p (오늘 시작 기준)", p1(-t.balance_pct))
        }),
    }
    let mut notes = Vec::new();
    if ratio >= 1.0 {
        if let Some(tb) = t.tomorrow_budget_pct {
            notes.push(format!(
                "초과분만큼 남은 날 예산이 줄어요 → 내일 {}%p",
                p1(tb)
            ));
        }
    }
    if t.estimated {
        notes.push("하루 시작 시점의 값은 추정치예요".into());
    }
    Section {
        title,
        right,
        big,
        big_note: None,
        gauge: Gauge {
            ratio: ratio.clamp(0.0, 1.0),
            marker: None,
            level: Level::for_ratio(ratio),
        },
        lines,
        note: (!notes.is_empty()).then(|| notes.join(" · ")),
    }
}

fn week_section<Tz: TimeZone>(w: &WeekView, waiting: bool, tz: &Tz) -> Section {
    let pace = if w.slack_pct >= 0.0 {
        format!(
            "페이스보다 {}%p 여유 · {}",
            p1(w.slack_pct),
            amount_of_time(w.slack_hours)
        )
    } else {
        format!(
            "페이스보다 {}%p 빠름 · {} 당겨 씀",
            p1(-w.slack_pct),
            amount_of_time(w.slack_hours)
        )
    };
    Section {
        title: "이번 주".into(),
        right: format!("{} 초기화", weekday_time(w.resets_at, tz)),
        big: pct0(w.used_pct),
        big_note: Some(format!("(이상선 {})", pct0(w.ideal_pct))),
        gauge: Gauge {
            ratio: (w.used_pct / 100.0).clamp(0.0, 1.0),
            marker: Some((w.ideal_pct / 100.0).clamp(0.0, 1.0)),
            level: if w.slack_pct >= 0.0 {
                Level::Ok
            } else if w.slack_pct > -10.0 {
                Level::Warn
            } else {
                Level::Over
            },
        },
        lines: vec![
            pace,
            format!(
                "남은 양 {}일치 / 남은 기간 {}일",
                p1(w.remaining_day_units),
                p1(w.days_left)
            ),
        ],
        note: waiting.then(|| "새 주가 시작됐어요 · 새 데이터를 기다리는 중".to_string()),
    }
}

fn window_row<Tz: TimeZone>(label: &str, w: &WindowView, tz: &Tz, weekly: bool) -> Row {
    let right = match (w.expired, w.resets_at) {
        (true, _) => "초기화됨".to_string(),
        (false, Some(r)) if weekly => format!("{} 초기화", weekday_time(r, tz)),
        (false, Some(r)) => format!("{} 초기화", hm(r, tz)),
        (false, None) => String::new(),
    };
    Row {
        label: label.into(),
        big: pct0(w.pct),
        gauge: Gauge {
            ratio: (w.pct / 100.0).clamp(0.0, 1.0),
            marker: None,
            level: Level::for_window_pct(w.pct),
        },
        right,
    }
}

fn source_label(s: Source) -> &'static str {
    match s {
        Source::Statusline => "Claude Code",
        Source::Oauth => "사용량 API(비공식)",
    }
}

pub fn popup<Tz: TimeZone>(v: &BudgetView, tz: &Tz) -> PopupModel {
    let today = v.today.as_ref().map(|t| today_section(t, v, tz));
    let week = v
        .week
        .as_ref()
        .map(|w| week_section(w, v.waiting_new_week, tz));

    let mut rows = Vec::new();
    if let Some(f) = &v.five_hour {
        rows.push(window_row("5시간 창", f, tz, false));
    }
    if let Some(o) = &v.seven_day_opus {
        rows.push(window_row("Opus 주간", o, tz, true));
    }
    if let Some(s) = &v.seven_day_sonnet {
        rows.push(window_row("Sonnet 주간", s, tz, true));
    }
    if let Some(e) = v.extra_usage.as_ref().filter(|e| e.is_enabled) {
        let pct = e.utilization.unwrap_or(0.0);
        rows.push(Row {
            label: "추가 사용량".into(),
            big: e.utilization.map(pct0).unwrap_or_else(|| "켜짐".into()),
            gauge: Gauge {
                ratio: (pct / 100.0).clamp(0.0, 1.0),
                marker: None,
                level: Level::for_window_pct(pct),
            },
            right: "월 한도 기준".into(),
        });
    }

    let footer = match (v.last_sample_at, v.last_source) {
        (Some(t), Some(src)) => format!("{} · {} 갱신", source_label(src), ago_ko(v.now - t)),
        _ => "아직 받은 데이터가 없어요".into(),
    };
    let empty = today.is_none().then(|| {
        if v.last_sample_at.is_none() {
            "아직 사용량 데이터가 없어요. 설정에서 Claude Code 연동을 설치하거나 사용량 API 조회를 켜 주세요."
        } else {
            "주간 한도 정보가 아직 없어요. Claude Code에서 메시지를 한 번 보내면 채워져요."
        }
        .to_string()
    });

    let today_ratio = v.today.as_ref().and_then(|t| t.ratio);
    let level = match (&today, v.stale) {
        (Some(s), false) => s.gauge.level,
        _ => Level::Idle,
    };
    let tray_title = match (&today, today_ratio) {
        (Some(_), Some(r)) => pct0(r * 100.0),
        _ => "–".into(),
    };
    let tooltip = tooltip(v);

    PopupModel {
        today,
        week,
        rows,
        footer,
        stale: v.stale,
        empty,
        level,
        today_ratio,
        tray_title,
        tooltip,
    }
}

fn tooltip(v: &BudgetView) -> String {
    let mut first = match &v.today {
        Some(t) => {
            let r = t
                .ratio
                .map(|r| pct0(r * 100.0))
                .unwrap_or_else(|| "—".into());
            if t.left_pct >= 0.0 {
                format!("오늘 {r} · {}%p 남음", p1(t.left_pct))
            } else {
                format!("오늘 {r} · {}%p 초과", p1(-t.left_pct))
            }
        }
        None => "Daily Sausage · 데이터 없음".into(),
    };
    if v.stale && v.last_sample_at.is_some() {
        first.push_str(" (오래된 값)");
    }
    let mut second = Vec::new();
    if let Some(w) = &v.week {
        second.push(format!("주 {}", pct0(w.used_pct)));
    }
    if let Some(f) = &v.five_hour {
        second.push(format!("5h {}", pct0(f.pct)));
    }
    if second.is_empty() {
        first
    } else {
        format!("{first}\n{}", second.join(" · "))
    }
}

fn bar(ratio: f64, width: usize) -> String {
    let filled = ((ratio.clamp(0.0, 1.0) * width as f64).round() as usize).min(width);
    format!("{}{}", "▓".repeat(filled), "░".repeat(width - filled))
}

/// One line for Claude Code's status line:
/// `오늘 ▓▓▓▓▓▓░░░░ 60% (5.9%p 남음) │ 주 58% (+2.1 여유) │ 5h 24%`
pub fn one_line(v: &BudgetView) -> String {
    let mut parts = Vec::new();
    match &v.today {
        Some(t) => {
            let r = t.ratio.unwrap_or(0.0);
            let left = if t.left_pct >= 0.0 {
                format!("{}%p 남음", p1(t.left_pct))
            } else {
                format!("{}%p 초과", p1(-t.left_pct))
            };
            parts.push(format!("오늘 {} {} ({left})", bar(r, 10), pct0(r * 100.0)));
        }
        None => parts.push("오늘 —".into()),
    }
    if let Some(w) = &v.week {
        let pace = if w.slack_pct >= 0.0 {
            format!("+{} 여유", p1(w.slack_pct))
        } else {
            format!("{} 당겨씀", p1(-w.slack_pct))
        };
        parts.push(format!("주 {} ({pace})", pct0(w.used_pct)));
    }
    if let Some(f) = &v.five_hour {
        parts.push(format!("5h {}", pct0(f.pct)));
    }
    parts.join(" │ ")
}

/// Which notifications were already sent today, persisted between runs.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct NotifyState {
    pub day_key: String,
    pub sent: Vec<String>,
    pub five_hour_key: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Notification {
    pub title: String,
    pub body: String,
}

/// Adds `id` to `sent`; true when it was not there yet.
fn mark(sent: &mut Vec<String>, id: &str) -> bool {
    if sent.iter().any(|s| s == id) {
        false
    } else {
        sent.push(id.to_string());
        true
    }
}

/// Notifications that became due, each at most once per day (per window for
/// the 5-hour one). Updates `state`.
pub fn due_notifications<Tz: TimeZone>(
    v: &BudgetView,
    cfg: &Config,
    state: &mut NotifyState,
    tz: &Tz,
) -> Vec<Notification> {
    let mut out = Vec::new();
    if v.stale {
        return out;
    }
    if let Some(t) = &v.today {
        let key = t.day_start.to_rfc3339();
        if state.day_key != key {
            state.day_key = key;
            state.sent.clear();
        }
        let ratio = t.ratio.unwrap_or(0.0);
        if ratio >= 1.0 && cfg.notify_today_100 {
            // Jumping straight past 100% should not also send the 80% notice later.
            mark(&mut state.sent, "80");
            if mark(&mut state.sent, "100") {
                let body = match (t.mode, t.tomorrow_budget_pct) {
                    (Mode::Rolling, Some(tb)) => format!(
                        "지금 멈추면 내일 예산은 {}%p예요. 더 쓰면 그만큼 남은 날 예산이 줄어요.",
                        p1(tb)
                    ),
                    (Mode::Fixed, _) => "더 쓰는 만큼 이월 잔고에서 빠져요.".to_string(),
                    _ => "이번 주 마지막 구간이에요. 남은 양을 확인해 주세요.".to_string(),
                };
                out.push(Notification {
                    title: "오늘 예산을 다 썼어요".into(),
                    body,
                });
            }
        } else if ratio >= 0.8 && cfg.notify_today_80 && mark(&mut state.sent, "80") {
            out.push(Notification {
                title: "오늘 예산 80% 도달".into(),
                body: format!(
                    "오늘 몫 {}%p 중 {}%p를 썼어요. 남은 몫은 {}%p예요.",
                    p1(t.budget_pct),
                    p1(t.used_pct),
                    p1(t.left_pct.max(0.0))
                ),
            });
        }
    }
    if let Some(f) = v.five_hour.as_ref().filter(|f| !f.expired) {
        if f.pct >= 90.0 && cfg.notify_five_hour_90 {
            let key = f.resets_at.map(|r| r.to_rfc3339()).unwrap_or_default();
            if state.five_hour_key != key {
                state.five_hour_key = key;
                let when = f
                    .resets_at
                    .map(|r| format!("{}에 초기화돼요.", hm(r, tz)))
                    .unwrap_or_default();
                out.push(Notification {
                    title: format!("5시간 창 {} 사용", pct0(f.pct)),
                    body: when,
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget::compute;
    use crate::model::{Sample, Window};
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
    fn view(now: DateTime<Utc>, w0: f64, w: f64, mode: Mode) -> BudgetView {
        let reset = at(5, 10, 0);
        let mut a = Sample::new(at(1, 23, 50), Source::Oauth);
        a.seven_day = Some(Window {
            pct: w0,
            resets_at: Some(reset),
        });
        let mut b = Sample::new(now - Duration::minutes(2), Source::Oauth);
        b.seven_day = Some(Window {
            pct: w,
            resets_at: Some(reset),
        });
        b.five_hour = Some(Window {
            pct: 24.0,
            resets_at: Some(at(2, 17, 40)),
        });
        compute(&[a, b], now, &kst(), 0, mode)
    }

    #[test]
    fn popup_matches_mockup() {
        let v = view(at(2, 15, 0), 49.0, 58.0, Mode::Rolling);
        let m = popup(&v, &kst());
        let t = m.today.unwrap();
        assert_eq!(t.title, "오늘 (11/2 월)");
        assert_eq!(t.right, "자정까지 9시간");
        assert_eq!(t.big, "60%");
        assert_eq!(t.lines[0], "9.0 / 14.9%p 사용 · 5.9%p 남음");
        assert_eq!(t.lines[1], "↳ 기본 14.3 + 이월 0.6 (지난 날 아낀 몫)");
        assert_eq!(t.gauge.level, Level::Ok);
        let w = m.week.unwrap();
        assert_eq!(w.right, "목 10:00 초기화");
        assert_eq!(w.big, "58%");
        assert_eq!(w.big_note.as_deref(), Some("(이상선 60%)"));
        assert_eq!(w.lines[0], "페이스보다 2.1%p 여유 · 약 3.6시간 분량");
        assert_eq!(w.lines[1], "남은 양 2.9일치 / 남은 기간 2.8일");
        assert_eq!(m.rows[0].label, "5시간 창");
        assert_eq!(m.rows[0].right, "17:40 초기화");
        assert_eq!(m.footer, "사용량 API(비공식) · 2분 전 갱신");
        assert_eq!(m.tray_title, "60%");
        assert_eq!(m.tooltip, "오늘 60% · 5.9%p 남음\n주 58% · 5h 24%");
        assert_eq!(
            one_line(&v),
            "오늘 ▓▓▓▓▓▓░░░░ 60% (5.9%p 남음) │ 주 58% (+2.1 여유) │ 5h 24%"
        );
    }

    #[test]
    fn over_budget_texts() {
        let v = view(at(2, 23, 30), 49.0, 69.0, Mode::Rolling);
        let m = popup(&v, &kst());
        let t = m.today.unwrap();
        assert_eq!(t.gauge.level, Level::Over);
        assert!(t.lines[0].ends_with("초과"), "{}", t.lines[0]);
        assert!(t.note.unwrap().contains("내일 12.8%p"));
        assert!(m.week.unwrap().lines[0].contains("빠름"));
    }

    #[test]
    fn fixed_mode_texts() {
        let v = view(at(2, 15, 0), 49.0, 58.0, Mode::Fixed);
        let t = popup(&v, &kst()).today.unwrap();
        assert_eq!(t.big, "63%");
        assert_eq!(t.lines[1], "↳ 이월 잔고 +2.2%p (오늘 시작 기준)");
    }

    #[test]
    fn empty_model() {
        let v = compute(&[], at(2, 15, 0), &kst(), 0, Mode::Rolling);
        let m = popup(&v, &kst());
        assert!(m.empty.is_some());
        assert_eq!(m.level, Level::Idle);
        assert_eq!(m.tray_title, "–");
    }

    #[test]
    fn notifications_fire_once() {
        let cfg = Config::default();
        let mut st = NotifyState::default();
        let v = view(at(2, 20, 0), 49.0, 61.0, Mode::Rolling); // 12/14.9 = 80%
        let n = due_notifications(&v, &cfg, &mut st, &kst());
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].title, "오늘 예산 80% 도달");
        assert!(due_notifications(&v, &cfg, &mut st, &kst()).is_empty());
        let v = view(at(2, 21, 0), 49.0, 65.0, Mode::Rolling);
        let n = due_notifications(&v, &cfg, &mut st, &kst());
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].title, "오늘 예산을 다 썼어요");
        // Next day resets.
        let v = view(at(3, 20, 0), 65.0, 66.0, Mode::Rolling);
        assert!(due_notifications(&v, &cfg, &mut st, &kst()).is_empty());
        assert_eq!(st.sent.len(), 0);
    }

    #[test]
    fn jump_straight_past_100_sends_only_100() {
        let cfg = Config::default();
        let mut st = NotifyState::default();
        let v = view(at(2, 23, 0), 49.0, 69.0, Mode::Rolling);
        let n = due_notifications(&v, &cfg, &mut st, &kst());
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].title, "오늘 예산을 다 썼어요");
    }

    #[test]
    fn durations() {
        assert_eq!(duration_ko(Duration::minutes(45)), "45분");
        assert_eq!(duration_ko(Duration::minutes(125)), "2시간 5분");
        assert_eq!(duration_ko(Duration::minutes(9 * 60)), "9시간");
        assert_eq!(duration_ko(Duration::minutes(11 * 60 + 30)), "11시간");
        assert_eq!(ago_ko(Duration::seconds(20)), "방금");
        assert_eq!(ago_ko(Duration::minutes(130)), "2시간 전");
    }
}
