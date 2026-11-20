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
    if held_elsewhere(wanted) {
        return false;
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

/// Whether another process holds the combination already. Windows (and X11) refuse the registration itself in that
/// case, so the plugin's answer is enough there; only macOS has to be asked first.
#[cfg(not(target_os = "macos"))]
fn held_elsewhere(_wanted: Shortcut) -> bool {
    false
}

#[cfg(target_os = "macos")]
use carbon::held_elsewhere;

/// macOS accepts the plugin's registration (shared, the way global-hotkey asks for it) even while another process holds
/// the same combination exclusively: it reports success, but the presses keep going to that process until it lets go,
/// so the panel would drop its working shortcut for a dead one. An exclusive registration, on the other hand, is
/// refused while another process holds the combination exclusively. So the combination is asked for exclusively first
/// and given back at once: a refusal means it is taken, and nothing is kept from the other app; a grant is followed by
/// the plugin's own, shared registration as before. A process sharing the combination is not in the way: the system
/// hands the press to every shared registrant, so the panel's shortcut works beside it, as it did.
#[cfg(target_os = "macos")]
mod carbon {
    use std::ffi::c_void;
    use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut};

    /// kEventHotKeyExclusive.
    const EXCLUSIVE: u32 = 1;
    /// eventHotKeyExistsErr: an exclusive registration while another hot key exists for the combination.
    const EXISTS: i32 = -9878;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct EventHotKeyID {
        signature: u32,
        id: u32,
    }

    #[link(name = "Carbon", kind = "framework")]
    unsafe extern "C" {
        fn GetApplicationEventTarget() -> *mut c_void;
        fn RegisterEventHotKey(
            code: u32,
            modifiers: u32,
            id: EventHotKeyID,
            target: *mut c_void,
            options: u32,
            out: *mut *mut c_void,
        ) -> i32;
        fn UnregisterEventHotKey(hot_key: *mut c_void) -> i32;
    }

    /// The key as Carbon knows it, (virtual key code, modifier flags), in the values global-hotkey 0.8 registers under,
    /// so the question is about the very key the plugin then takes. None for a key this version never offers.
    fn carbon_key(shortcut: Shortcut) -> Option<(u32, u32)> {
        let code = match shortcut.key {
            Code::KeyS => 0x01,
            Code::Space => 0x31,
            Code::F18 => 0x4f,
            _ => return None,
        };
        let flags = [
            (Modifiers::SHIFT, 512),
            (Modifiers::SUPER | Modifiers::META, 256),
            (Modifiers::ALT, 2048),
            (Modifiers::CONTROL, 4096),
        ];
        let modifiers =
            flags.into_iter().filter(|(ours, _)| shortcut.mods.intersects(*ours)).map(|(_, carbon)| carbon).sum();
        Some((code, modifiers))
    }

    /// A hot key registered here, given back when dropped.
    struct Registration(*mut c_void);

    impl Drop for Registration {
        fn drop(&mut self) {
            unsafe {
                UnregisterEventHotKey(self.0);
            }
        }
    }

    /// Asks the system for the key, exclusively or shared as the plugin does; Err carries the status it refused with.
    fn register((code, modifiers): (u32, u32), exclusive: bool) -> Result<Registration, i32> {
        let id = EventHotKeyID { signature: u32::from_be_bytes(*b"stmr"), id: 1 };
        let options = if exclusive { EXCLUSIVE } else { 0 };
        let mut hot_key: *mut c_void = std::ptr::null_mut();
        let status =
            unsafe { RegisterEventHotKey(code, modifiers, id, GetApplicationEventTarget(), options, &mut hot_key) };
        if status == 0 { Ok(Registration(hot_key)) } else { Err(status) }
    }

    /// Whether another process holds the combination exclusively: the exclusive registration is refused then. Granted,
    /// it is given back at once, so the plugin's own registration follows and nothing is kept from other apps.
    pub fn held_elsewhere(shortcut: Shortcut) -> bool {
        carbon_key(shortcut).is_some_and(|key| matches!(register(key, true), Err(EXISTS)))
    }

    #[cfg(test)]
    mod tests {
        //! The other process is this test binary, started again with HOLDS set: it registers the test combination
        //! ⌃⌥⇧F18 (a key no keyboard here sends; KEY-2's probe used the same), reports the status it got, and keeps the
        //! key until its stdin closes. The tests take turns on the one combination.
        use super::super::shortcut;
        use super::{carbon_key, held_elsewhere, register};
        use std::io::{BufRead, BufReader};
        use std::process::{Child, ChildStdout, Command, Stdio};
        use std::sync::Mutex;
        use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut};

        const HOLDS: &str = "STARMETER_TEST_HOLDS_HOTKEY";
        static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

        fn turn() -> std::sync::MutexGuard<'static, ()> {
            ONE_AT_A_TIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
        }

        fn test_key() -> Shortcut {
            Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT | Modifiers::SHIFT), Code::F18)
        }

        /// The holder's side; nothing in the test run itself.
        #[test]
        fn holds_the_test_combination_for_another_test() {
            let Ok(mode) = std::env::var(HOLDS) else { return };
            let registration = register(carbon_key(test_key()).unwrap(), mode == "exclusive");
            let status = match &registration {
                Ok(_) => 0,
                Err(status) => *status,
            };
            println!("holding {status}");
            let _ = std::io::stdin().read_line(&mut String::new());
            drop(registration);
        }

        /// Another process holding the test combination, exclusively or shared, until dropped; `status` is the system's
        /// answer to its registration.
        struct Holder {
            child: Child,
            _stdout: BufReader<ChildStdout>,
            status: i32,
        }

        impl Holder {
            fn new(mode: &str) -> Self {
                let mut child = Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "shortcut::carbon::tests::holds_the_test_combination_for_another_test",
                        "--nocapture",
                    ])
                    .env(HOLDS, mode)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn()
                    .unwrap();
                let mut stdout = BufReader::new(child.stdout.take().unwrap());
                let mut line = String::new();
                let status = loop {
                    line.clear();
                    assert!(stdout.read_line(&mut line).unwrap() > 0, "the holder reports its registration");
                    if let Some(status) = line.trim().strip_prefix("holding ") {
                        break status.parse().unwrap();
                    }
                };
                Holder { child, _stdout: stdout, status }
            }
        }

        impl Drop for Holder {
            fn drop(&mut self) {
                drop(self.child.stdin.take());
                let _ = self.child.wait();
            }
        }

        #[test]
        fn the_choices_are_keys_carbon_knows_by_the_plugins_values() {
            assert_eq!(carbon_key(shortcut("ctrl-alt-s").unwrap()), Some((0x01, 4096 + 2048)));
            assert_eq!(carbon_key(shortcut("ctrl-alt-shift-s").unwrap()), Some((0x01, 4096 + 2048 + 512)));
            assert_eq!(carbon_key(shortcut("ctrl-alt-space").unwrap()), Some((0x31, 4096 + 2048)));
            assert_eq!(carbon_key(Shortcut::new(Some(Modifiers::META), Code::KeyA)), None);
        }

        /// KEY-2: the plain registration is accepted while another process holds the key exclusively, so it alone
        /// cannot tell; the exclusive question can, and the key is free again once the holder lets go.
        #[test]
        fn a_combination_another_process_holds_exclusively_is_taken_until_it_lets_go() {
            let _turn = turn();
            let holder = Holder::new("exclusive");
            assert_eq!(holder.status, 0, "the holder got the key");
            let shared = register(carbon_key(test_key()).unwrap(), false);
            assert!(shared.is_ok(), "the plugin's kind of registration is accepted");
            drop(shared);
            assert!(held_elsewhere(test_key()));
            drop(holder);
            assert!(!held_elsewhere(test_key()));
        }

        /// A shared registration elsewhere is not reported: the system refuses the exclusive question only for an
        /// exclusive holder, and a press reaches every shared registrant, so the panel's shortcut works beside it.
        #[test]
        fn a_combination_another_process_shares_is_free() {
            let _turn = turn();
            let holder = Holder::new("shared");
            assert_eq!(holder.status, 0, "the holder got the key");
            assert!(!held_elsewhere(test_key()));
            let shared = register(carbon_key(test_key()).unwrap(), false);
            assert!(shared.is_ok(), "the plugin's registration beside it");
            drop(shared);
            drop(holder);
        }

        /// Nothing stays behind: another process may still take the key exclusively, and the shared registration the
        /// plugin makes next goes through.
        #[test]
        fn the_question_leaves_the_combination_free() {
            let _turn = turn();
            assert!(!held_elsewhere(test_key()));
            let holder = Holder::new("exclusive");
            assert_eq!(holder.status, 0, "exclusive registration elsewhere after the question");
            drop(holder);
            let ours = register(carbon_key(test_key()).unwrap(), false);
            assert!(ours.is_ok(), "shared registration after the question");
            drop(ours);
        }
    }
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
