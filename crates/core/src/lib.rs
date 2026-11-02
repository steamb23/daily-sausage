//! Daily Sausage core: turns Claude subscription weekly usage percentages into a
//! daily budget. No UI and no network here (except the optional `oauth` feature),
//! so everything is unit-testable.

pub mod budget;
pub mod claude_settings;
pub mod collect;
pub mod config;
pub mod credentials;
pub mod format;
pub mod icon;
pub mod model;
#[cfg(feature = "oauth")]
pub mod oauth;
pub mod parse;
pub mod paths;
pub mod schedule;
pub mod store;

pub use budget::{compute, BudgetView, Mode};
pub use config::Config;
pub use model::{Sample, Source, Window};
pub use store::Store;
