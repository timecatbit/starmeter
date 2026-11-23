//! Starmeter's data engine. Everything the panel, the SwiftBar plugin and the command line show comes from here;
//! the hosts only draw it and pass the user's actions back.

pub mod accounts;
pub mod actions;
pub mod alerts;
pub mod antigravity;
pub mod api_rows;
pub mod app_update;
pub mod background;
pub mod catalog;
pub mod claude_app;
pub mod claude_bridge;
pub mod claude_subscription;
pub mod claude_usage;
pub mod client_accounts;
pub mod cursor;
pub mod dashboard;
pub mod database;
pub mod desktop;
pub mod devices;
pub mod discovery;
pub mod files;
pub mod hant;
pub mod harness;
pub mod host;
pub mod http;
pub mod json;
pub mod keys;
pub mod lines;
pub mod menu_icons;
pub mod module_rows;
pub mod more_rows;
pub mod opencodex;
pub mod panel_snapshot;
pub mod platform;
pub mod preferences;
pub mod pricing;
pub mod projects;
pub mod py;
pub mod range_usage;
pub mod render;
pub mod rows;
pub mod runtime;
pub mod self_update;
pub mod sessions;
pub mod shell;
pub mod tally;
pub mod time;
pub mod tool_sessions;
pub mod updates;
pub mod wsl;
pub mod zstd;

/// The version every host reports and the update check compares.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// A failure the engine reports as text: SQLite, files, or a log it could not read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<database::SqlError> for Error {
    fn from(error: database::SqlError) -> Self {
        Error(error.to_string())
    }
}

impl From<files::Failure> for Error {
    fn from(error: files::Failure) -> Self {
        Error(error.0)
    }
}

pub type Fallible<T> = Result<T, Error>;
