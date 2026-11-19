//! Starmeter's data engine. Everything the panel, the SwiftBar plugin and the command line show comes from here;
//! the hosts only draw it and pass the user's actions back.

pub mod catalog;
pub mod preferences;
pub mod time;

/// The version every host reports and the update check compares.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
