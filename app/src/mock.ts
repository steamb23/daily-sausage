import type { Config, InstallPreview, Snapshot } from "./types";

// The worked example from the plan (reset Thu 10:00, now Mon 15:00, 58%).
// `?state=over|empty|fixed` switches the preview.
export function mockSnapshot(): Snapshot {
  const state = new URLSearchParams(location.search).get("state");
  const base: Snapshot = {
    model: {
      today: {
        title: "오늘 (11/2 월)",
        right: "자정까지 9시간",
        big: "60%",
        big_note: null,
        gauge: { ratio: 0.603, marker: null, level: "ok" },
        lines: ["9.0 / 14.9%p 사용 · 5.9%p 남음", "↳ 기본 14.3 + 이월 0.6 (지난 날 아낀 몫)"],
        note: null,
      },
      week: {
        title: "이번 주",
        right: "목 10:00 초기화",
        big: "58%",
        big_note: "(이상선 60%)",
        gauge: { ratio: 0.58, marker: 0.601, level: "ok" },
        lines: ["페이스보다 2.1%p 여유 · 약 3.6시간 분량", "남은 양 2.9일치 / 남은 기간 2.8일"],
        note: null,
      },
      rows: [
        { label: "5시간 창", big: "24%", gauge: { ratio: 0.24, marker: null, level: "ok" }, right: "17:40 초기화" },
        { label: "Sonnet 주간", big: "12%", gauge: { ratio: 0.12, marker: null, level: "ok" }, right: "목 10:00 초기화" },
      ],
      footer: "사용량 API(비공식) · 2분 전 갱신",
      stale: false,
      empty: null,
      level: "ok",
      today_ratio: 0.603,
      tray_title: "60%",
      tooltip: "오늘 60% · 5.9%p 남음\n주 58% · 5h 24%",
    },
    oauth: { next_due: null, backoff_secs: 0, last_attempt: null, last_ok: null, message: null, stopped: false },
    oauth_enabled: true,
    statusline: { state: "installed", command: '"/Applications/Daily Sausage.app/Contents/MacOS/daily-sausage" collect' },
    claude_cli: "/Users/me/.local/bin/claude",
  };
  if (state === "over") {
    const t = base.model.today!;
    t.big = "134%";
    t.gauge = { ratio: 1, marker: null, level: "over" };
    t.right = "자정까지 30분";
    t.lines = ["20.0 / 14.9%p 사용 · 5.1%p 초과", "↳ 기본 14.3 + 이월 0.6 (지난 날 아낀 몫)"];
    t.note = "초과분만큼 남은 날 예산이 줄어요 → 내일 12.8%p";
    const w = base.model.week!;
    w.big = "69%";
    w.big_note = "(이상선 66%)";
    w.gauge = { ratio: 0.69, marker: 0.655, level: "warn" };
    w.lines = ["페이스보다 3.5%p 빠름 · 약 5.9시간 분량 당겨 씀", "남은 양 2.2일치 / 남은 기간 2.4일"];
    base.model.rows[0] = { label: "5시간 창", big: "93%", gauge: { ratio: 0.93, marker: null, level: "over" }, right: "23:40 초기화" };
    base.model.level = "over";
  } else if (state === "empty") {
    base.model = {
      ...base.model,
      today: null,
      week: null,
      rows: [],
      footer: "아직 받은 데이터가 없어요",
      stale: true,
      empty: "아직 사용량 데이터가 없어요. 설정에서 Claude Code 연동을 설치하거나 사용량 API 조회를 켜 주세요.",
      level: "idle",
    };
    base.statusline = { state: "not_configured" };
    base.oauth_enabled = false;
  } else if (state === "fixed") {
    const t = base.model.today!;
    t.big = "63%";
    t.gauge = { ratio: 0.63, marker: null, level: "ok" };
    t.lines = ["9.0 / 14.3%p 사용 · 5.3%p 남음", "↳ 이월 잔고 +2.2%p (오늘 시작 기준)"];
  }
  return base;
}

export function mockConfig(): Config {
  return {
    boundary_hour: 0,
    mode: "rolling",
    oauth_enabled: false,
    oauth_acknowledged: false,
    poll_minutes: 5,
    cli_refresh: true,
    claude_path: null,
    notify_today_80: true,
    notify_today_100: true,
    notify_five_hour_90: true,
    statusline_summary: true,
    chain_command: null,
    original_statusline: null,
    autostart: true,
  };
}

export function mockPreview(): InstallPreview {
  return {
    settings_path: "~/.claude/settings.json",
    before: '{\n  "type": "command",\n  "command": "bash ~/.claude/statusline.sh"\n}',
    after: '{\n  "type": "command",\n  "command": "\\"/Applications/Daily Sausage.app/Contents/MacOS/daily-sausage\\" collect"\n}',
    chains: "bash ~/.claude/statusline.sh",
  };
}
