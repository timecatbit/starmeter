//! The Linux tray's way to run a CLI's update in a terminal (LINUX-1), in a process of its own: the test sets PATH.
#![cfg(target_os = "linux")]

use std::path::Path;
use std::time::{Duration, Instant};

use starmeter_core::host::terminal_jobs::launch_linux;

/// A terminal found on the PATH runs the command, from a script of its own, the way that terminal is told to: here a
/// stand-in xterm that notes its arguments and runs what follows -e. With no terminal, the reason says what to install.
#[test]
fn the_command_runs_in_the_first_terminal_found() {
    let root = std::env::temp_dir().join(format!("starmeter-terminal-{}", std::process::id()));
    let (bin, state) = (root.join("bin"), root.join("state"));
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&state).unwrap();
    let noted = root.join("arguments");
    let xterm = bin.join("xterm");
    std::fs::write(&xterm, format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nshift\nexec \"$@\"\n", noted.display()))
        .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&xterm, std::fs::Permissions::from_mode(0o755)).unwrap();
    let ran = root.join("ran");
    // SAFETY: this test is the only one in its process.
    unsafe { std::env::set_var("PATH", &bin) };

    let state_text = state.to_str().unwrap();
    assert_eq!(launch_linux(&format!("echo updated > '{}'", ran.display()), state_text).unwrap(), "file");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ran.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(std::fs::read_to_string(&ran).unwrap().trim(), "updated");
    let arguments = std::fs::read_to_string(&noted).unwrap();
    let arguments: Vec<&str> = arguments.lines().collect();
    assert_eq!(&arguments[..2], &["-e", "/bin/bash"]);
    let script = Path::new(arguments[2]);
    assert!(
        script.starts_with(state.join("terminal-jobs")) && script.extension().is_some_and(|e| e == "sh"),
        "{script:?}"
    );
    assert_eq!(std::fs::metadata(script).unwrap().permissions().mode() & 0o777, 0o700);

    // No terminal at all: the update is not started, and the reason names some to install.
    unsafe { std::env::set_var("PATH", root.join("nothing")) };
    let failure = launch_linux("true", state_text).unwrap_err();
    assert!(failure.0.contains("xterm"), "{}", failure.0);
    let _ = std::fs::remove_dir_all(&root);
}
