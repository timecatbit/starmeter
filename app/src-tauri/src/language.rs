//! The panel's language (通用 → 语言), told to the shell so that its own words follow: the star's right-click menu,
//! and the engine's notifications and SwiftBar's menu in 繁體中文. The panel keeps the choice in its web view's storage;
//! the shell keeps a copy in the settings folder the app's panel, SwiftBar's and the engine share, beside
//! `guide-shown`, so the menu is already right at launch, before the panel first loads.

use std::collections::HashMap;
use std::path::PathBuf;

use starmeter_core::desktop::{self, Shell};
use tauri::AppHandle;

/// The languages the panel speaks; Simplified Chinese is the source the others come from.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Language {
    Simplified,
    Traditional,
    English,
}

impl Language {
    fn parse(text: &str) -> Language {
        match text.trim() {
            "en" => Language::English,
            "zh-Hant" => Language::Traditional,
            _ => Language::Simplified,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Language::Simplified => "zh",
            Language::Traditional => "zh-Hant",
            Language::English => "en",
        }
    }
}

fn file() -> PathBuf {
    let environment: HashMap<String, String> = std::env::vars().collect();
    PathBuf::from(desktop::state_dir(Shell::current(), &environment)).join("language")
}

/// The language a panel last chose on this computer; Simplified Chinese until one says otherwise.
pub fn current() -> Language {
    std::fs::read_to_string(file()).map_or(Language::Simplified, |text| Language::parse(&text))
}

/// The panel's language, as it loads (when one was ever chosen there) and at each choice: "zh", "zh-Hant" or "en".
#[tauri::command]
pub fn set_language(app: AppHandle, language: String) {
    let chosen = Language::parse(&language);
    if chosen == current() {
        return;
    }
    let path = file();
    if let Some(folder) = path.parent() {
        let _ = std::fs::create_dir_all(folder);
    }
    let _ = std::fs::write(&path, chosen.key());
    // Linux and Windows keep the menu attached and show it on right-click without rebuilding there (16b94c0), so
    // rebuild it when the language changes (R10-10). macOS rebuilds on every right-click and must stay detached (R10-1).
    if !cfg!(target_os = "macos") {
        crate::refresh_star_menu(&app);
    }
}
