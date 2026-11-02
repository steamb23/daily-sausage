// Mirrors the serde output of daily-sausage-core (format.rs, config.rs, ...).

export type Level = "ok" | "warn" | "over" | "idle";

export interface Gauge {
  ratio: number;
  marker: number | null;
  level: Level;
}

export interface Section {
  title: string;
  right: string;
  big: string;
  big_note: string | null;
  gauge: Gauge;
  lines: string[];
  note: string | null;
}

export interface Row {
  label: string;
  big: string;
  gauge: Gauge;
  right: string;
}

export interface PopupModel {
  today: Section | null;
  week: Section | null;
  rows: Row[];
  footer: string;
  stale: boolean;
  empty: string | null;
  level: Level;
  today_ratio: number | null;
  tray_title: string;
  tooltip: string;
}

export interface PollSchedule {
  next_due: string | null;
  backoff_secs: number;
  last_attempt: string | null;
  last_ok: string | null;
  message: string | null;
  stopped: boolean;
}

export type StatuslineState =
  | { state: "not_configured" }
  | { state: "installed"; command: string }
  | { state: "other"; command: string }
  | { state: "unreadable"; error: string };

export interface Snapshot {
  model: PopupModel;
  oauth: PollSchedule;
  oauth_enabled: boolean;
  statusline: StatuslineState;
}

export type Mode = "rolling" | "fixed";

export interface Config {
  boundary_hour: number;
  mode: Mode;
  oauth_enabled: boolean;
  oauth_acknowledged: boolean;
  poll_minutes: number;
  notify_today_80: boolean;
  notify_today_100: boolean;
  notify_five_hour_90: boolean;
  statusline_summary: boolean;
  chain_command: string | null;
  original_statusline: unknown;
  autostart: boolean;
}

export interface InstallPreview {
  settings_path: string;
  before: string;
  after: string;
  chains: string | null;
}
