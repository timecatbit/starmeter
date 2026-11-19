//! The global shortcut that opens and closes the panel from anywhere, as a click on the star does. Off until chosen in
//! 外观与设置 → 显示 → 全局快捷键; the panel keeps the choice (in its own storage, like the language) and hands it over
//! each time it loads, so the shell registers what the panel says and nothing else.

use tauri::plugin::TauriPlugin;
use tauri::{AppHandle, Wry};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

/// The shortcuts the panel offers, by the name it keeps. Control and Option (Alt) together: combinations apps rarely
/// take for themselves, on macOS and on Windows alike.
fn shortcut(choice: &str) -> Option<Shortcut> {
    let both = Modifiers::CONTROL | Modifiers::ALT;
    match choice {
        "ctrl-alt-s" => Some(Shortcut::new(Some(both), Code::KeyS)),
        "ctrl-alt-shift-s" => Some(Shortcut::new(Some(both | Modifiers::SHIFT), Code::KeyS)),
        "ctrl-alt-space" => Some(Shortcut::new(Some(both), Code::Space)),
        _ => None,
    }
}

/// The plugin, with what a press does: the same as the star.
pub fn plugin(pressed: fn(&AppHandle)) -> TauriPlugin<Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(move |app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                pressed(app);
            }
        })
        .build()
}

/// Registers the panel's choice in place of the last one; "none" (or a name this version doesn't know) clears it.
/// False when the system or another app already holds the combination: the panel says so and keeps the old one.
#[tauri::command]
pub fn set_shortcut(app: AppHandle, choice: String) -> bool {
    let shortcuts = app.global_shortcut();
    let Some(wanted) = shortcut(&choice) else {
        let _ = shortcuts.unregister_all();
        return true;
    };
    if shortcuts.is_registered(wanted) {
        return true;
    }
    let previous: Vec<Shortcut> = ["ctrl-alt-s", "ctrl-alt-shift-s", "ctrl-alt-space"]
        .into_iter()
        .filter_map(shortcut)
        .filter(|known| shortcuts.is_registered(*known))
        .collect();
    if shortcuts.register(wanted).is_err() {
        return false;
    }
    for old in previous {
        let _ = shortcuts.unregister(old);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::shortcut;

    #[test]
    fn the_choices_and_nothing_else() {
        for name in ["ctrl-alt-s", "ctrl-alt-shift-s", "ctrl-alt-space"] {
            assert!(shortcut(name).is_some(), "{name}");
        }
        assert!(shortcut("none").is_none());
        assert!(shortcut("cmd-q").is_none());
        assert_ne!(shortcut("ctrl-alt-s"), shortcut("ctrl-alt-shift-s"));
    }
}
