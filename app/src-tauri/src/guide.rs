//! The first-run tour, shown once on this computer: the app's panel and SwiftBar's share the settings folder, so a
//! tour shown in one is not shown again in the other. 通用 → 新手指引 shows it again whenever asked.

use std::collections::HashMap;
use std::path::PathBuf;

use starmeter_core::desktop::{self, Shell};

fn marker() -> PathBuf {
    let environment: HashMap<String, String> = std::env::vars().collect();
    PathBuf::from(desktop::state_dir(Shell::current(), &environment)).join("guide-shown")
}

/// Whether the tour has been shown on this computer, in either panel.
#[tauri::command]
pub fn guide_shown() -> bool {
    marker().exists()
}

/// Notes that the tour has been shown: as it starts, so a panel closed halfway doesn't bring it back.
#[tauri::command]
pub fn mark_guide_shown() {
    let path = marker();
    if let Some(folder) = path.parent() {
        let _ = std::fs::create_dir_all(folder);
    }
    let _ = std::fs::write(&path, b"");
}
