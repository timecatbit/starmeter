//! The panel's language (通用 → 语言), told to the shell so that its own words follow: the star's right-click menu.
//! The panel keeps the choice in its web view's storage; the shell keeps a copy in the settings folder the app's panel
//! and SwiftBar's share, beside `guide-shown`, so the menu is already right at launch, before the panel first loads.

use std::collections::HashMap;
use std::path::PathBuf;

use starmeter_core::desktop::{self, Shell};
use tauri::AppHandle;

fn file() -> PathBuf {
    let environment: HashMap<String, String> = std::env::vars().collect();
    PathBuf::from(desktop::state_dir(Shell::current(), &environment)).join("language")
}

/// Whether the panel was last set to English on this computer; Chinese until a panel says otherwise.
pub fn english() -> bool {
    std::fs::read_to_string(file()).is_ok_and(|language| language.trim() == "en")
}

/// The panel's language, as it loads (when one was ever chosen there) and at each choice: "en" or "zh".
#[tauri::command]
pub fn set_language(app: AppHandle, language: String) {
    let english = language == "en";
    if english == self::english() {
        return;
    }
    let path = file();
    if let Some(folder) = path.parent() {
        let _ = std::fs::create_dir_all(folder);
    }
    let _ = std::fs::write(&path, if english { "en" } else { "zh" });
    // Linux sets the star's menu once: its tray opens the menu on any click and tells the app nothing first. macOS and
    // Windows build it again at each right-click.
    if cfg!(target_os = "linux") {
        crate::refresh_star_menu(&app);
    }
}
