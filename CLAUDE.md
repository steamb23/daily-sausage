# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Daily Sausage is a Tauri v2 tray app (Windows tray / macOS menu bar) that turns Claude subscription **weekly usage percentages** into a **daily budget**. It only ever sees percentages, never absolute token counts. It is an unofficial third-party tool; keep the "not made or endorsed by Anthropic" disclaimer wherever the product is described (README, `tauri.conf.json` bundle descriptions, the release body in `.github/workflows/build.yml`).

## Commands

Requires Rust stable ≥ 1.89 (`store.rs` uses `std::fs::File::lock`), Node.js 22, and the Tauri prerequisites.

```bash
# Core logic tests (always pass --all-features: the `oauth` feature gates the oauth and claude_cli modules)
cargo test -p daily-sausage-core --all-features

# Single test (substring match on the test path)
cargo test -p daily-sausage-core --all-features store::tests::append_dedupes_and_loads_sorted

# What CI runs before bundling
cargo fmt --all -- --check
cargo clippy -p daily-sausage-core --all-features --all-targets -- -D warnings
cd app && npm ci && npm run build      # tsc --noEmit + vite build

# Run the full app in dev mode / build installers
cd app && npm run tauri dev
cd app && npm run tauri build

# Popup UI only, in a plain browser with mock data (http://localhost:1420)
# Query params: ?state=over | ?state=empty | ?state=fixed | ?view=settings
cd app && npm run dev

# Render tray icon PNG previews
cargo run -p daily-sausage-core --example render_icons -- <out_dir>
```

Exercise the collector directly (after a debug build):

```bash
echo "{\"rate_limits\":{\"seven_day\":{\"used_percentage\":58,\"resets_at\":$(( $(date +%s) + 3*86400 ))}}}" | target/debug/daily-sausage collect
```

Set `DAILY_SAUSAGE_DATA_DIR` to a scratch directory when trying things manually so the real `samples.jsonl`/`config.json` are not touched, and `CLAUDE_CONFIG_DIR` to a scratch directory before testing the status line installer so the real `~/.claude/settings.json` is not rewritten.

Some `collect.rs` tests (chaining, chain timeout) are `#[cfg(unix)]` and do not run on Windows; CI runs tests on Ubuntu.

## Architecture

Cargo workspace with two members: `crates/core` (`daily-sausage-core`, pure logic, no UI) and `app/src-tauri` (`daily-sausage`, the Tauri shell). The TypeScript popup lives in `app/src`.

### One binary, two processes, one data directory

`app/src-tauri/src/main.rs` checks for the `collect` argument before Tauri starts. So the same executable is both:

1. **The collector** (`daily-sausage collect`), registered as Claude Code's `statusLine.command`. It reads the status line JSON from stdin, appends a `Sample` from `rate_limits`, then prints either the user's previous status line command (run with the same stdin, 5 s timeout) or a one-line summary. It must never break Claude Code's status line: `main_entry` wraps everything in `catch_unwind` and always exits 0.
2. **The tray app**, which also reads/writes the same files.

Both share the data dir from `paths::data_dir()` (`samples.jsonl`, `config.json`, `notify_state.json`). `samples.jsonl` is append-only, guarded by an OS file lock on `samples.lock`, dedupes identical readings within 10 minutes, and is pruned to 14 days by the app. `config.json` is loaded with `#[serde(default)]` + `sanitized()` so unknown/missing fields are tolerated across versions.

### Data flow

```
statusline stdin ─┐                         (parse::from_statusline)
oauth usage API ──┴─> Sample ─> Store (samples.jsonl)
                                     │
            budget::compute(samples, now, tz, boundary_hour, mode) -> BudgetView
                                     │
            format::popup(view, tz) -> PopupModel   (all Korean display strings)
                                     │
        tray::update (icon + title/tooltip)   +   emit("snapshot") -> webview
```

- `budget.rs` holds the math (rolling vs. fixed mode, day segments clipped to the weekly window, DST-safe day boundaries via `resolve_local`). Functions are generic over `Tz: TimeZone`; tests use `FixedOffset`/`Utc`, production passes `chrono::Local`.
- `format.rs` builds every user-visible string and the `PopupModel`. The frontend renders `PopupModel` as-is and does no calculation. Change wording or numbers in Rust, not in TS.
- `icon.rs` renders the sausage tray icon in plain Rust (signed distance field); there are no tray image assets.

### Rust ↔ TypeScript contract

`app/src/types.ts` hand-mirrors the serde output of core structs (`PopupModel`, `Config`, `PollSchedule`, `StatuslineState`, `InstallPreview`) and the app's `Snapshot` (`app/src-tauri/src/state.rs`). When a serialized struct changes, update `types.ts` and the mock data in `app/src/mock.ts` too. `app/src/backend.ts` switches between real `invoke`/`listen` calls and mocks depending on `__TAURI_INTERNALS__`. Tauri commands are registered in `app/src-tauri/src/lib.rs` and implemented in `commands.rs`.

### Background poller (`app/src-tauri/src/poller.rs`)

A 30 s loop (woken early by `AppState.wake`) that optionally polls the unofficial `https://api.anthropic.com/api/oauth/usage` endpoint, then calls `refresh_view` to recompute, update the tray, send notifications, and push a snapshot. Scheduling, backoff, and the stop-after-403 rule are in `core/src/schedule.rs` (`PollSchedule`, `CliRefreshGate`).

### Credential handling constraints (deliberate design, keep them)

- `credentials.rs` reads Claude Code's stored login (macOS Keychain `Claude Code-credentials` first, else `.credentials.json`). The token stays in memory: never write, log, or send it anywhere other than `api.anthropic.com`.
- The app **never refreshes the OAuth token itself** (that would rotate the refresh token under Claude Code and counts as credential brokering). When the token is expired, `claude_cli.rs` runs `claude auth status --json` windowless so Claude Code refreshes and stores it, then the app re-reads the login. `CliRefreshGate` rate-limits failed retries.
- The oauth path is opt-in and requires `oauth_acknowledged`; `Config::sanitized()` forces `oauth_enabled = false` otherwise.

### Status line integration (`core/src/claude_settings.rs`)

Install rewrites `statusLine` in Claude Code's `settings.json` to `"<exe>" collect`, saves the previous `statusLine` into our config (`original_statusline`, `chain_command`), and writes a timestamped `settings.json.daily-sausage-backup-<time>` before every change. `is_collector_command` keeps the collector from chaining itself. `commands::set_config` preserves `chain_command`, `original_statusline`, and `claude_path` because the settings form does not own them (`claude_path` is only set by editing `config.json`).

### App lifecycle notes

- Single-instance plugin: a second launch just opens the running popup.
- The popup window label is `main` (`POPUP` in `lib.rs`); it is hidden, not closed, and `RunEvent::ExitRequested` is suppressed so the tray app only quits from its menu.
- On macOS the app runs as an Accessory (no Dock icon) and refuses to install the status line while running from `AppTranslocation`.

## Conventions

- User-facing text (popup, notifications, error strings returned from commands) is Korean. Identifiers, code comments, and commit messages are English; commit subjects are short imperative sentences.
- Keep `crates/core` free of UI and network code except behind the `oauth` feature, so it stays unit-testable.

## CI / release

`.github/workflows/build.yml`: on push/PR it runs fmt, clippy (core only), core tests, and the frontend build on Ubuntu, then bundles Windows NSIS and a macOS universal dmg as artifacts. Pushing a `v*` tag also creates a draft GitHub release.
