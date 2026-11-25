//! The panel's look (外观 → 风格: 标准 or 手绘), told to the shell so that a phone (在手机上看) follows it: the phone's
//! snapshot carries it, and the phone redraws in it at its next reading. The panel keeps the choice in its web view's
//! storage; the shell keeps a copy in the settings folder, beside `language`, so a phone has it before the panel loads.

use std::collections::HashMap;
use std::path::PathBuf;

use starmeter_core::desktop::{self, Shell};

fn file() -> PathBuf {
    let environment: HashMap<String, String> = std::env::vars().collect();
    PathBuf::from(desktop::state_dir(Shell::current(), &environment)).join("panel-style")
}

/// What a stored or given name stands for: 手绘, or 标准 for anything else.
fn parse(text: &str) -> &'static str {
    if text.trim() == "sketch" { "sketch" } else { "standard" }
}

/// The look a panel last chose on this computer; 标准 until one says otherwise.
pub fn current() -> &'static str {
    std::fs::read_to_string(file()).map_or("standard", |text| parse(&text))
}

/// The panel's look, as it loads and at each choice: "standard" or "sketch".
#[tauri::command]
pub fn set_style(style: String) {
    let chosen = parse(&style);
    if chosen == current() {
        return;
    }
    let path = file();
    if let Some(folder) = path.parent() {
        let _ = std::fs::create_dir_all(folder);
    }
    let _ = std::fs::write(&path, chosen);
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_look_is_sketch_or_standard() {
        assert_eq!(super::parse("sketch"), "sketch");
        assert_eq!(super::parse(" sketch\n"), "sketch");
        assert_eq!(super::parse("standard"), "standard");
        assert_eq!(super::parse("neon"), "standard");
        assert_eq!(super::parse(""), "standard");
    }
}
