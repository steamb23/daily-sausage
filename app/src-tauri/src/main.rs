// Hide the console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `daily-sausage collect` is Claude Code's status line command: record the
    // usage and exit before any GUI starts.
    if std::env::args().nth(1).as_deref() == Some(daily_sausage_core::collect::SUBCOMMAND) {
        std::process::exit(daily_sausage_core::collect::main_entry());
    }
    daily_sausage_app::run();
}
