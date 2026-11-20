//! The starmeter command as a program: its JSON read only in part by the next program in a pipe.

use std::io::Read;
use std::process::{Command, Stdio};

#[test]
fn a_pipe_closed_early_ends_the_command_quietly() {
    // CLI-3: 600 models give JSON far larger than a pipe holds; the reader takes one line and closes.
    let folder = std::env::temp_dir().join(format!("starmeter-pipe-{}", std::process::id()));
    let (logs, state, home) = (folder.join("magpie"), folder.join("state"), folder.join("home"));
    for path in [&logs, &state, &home] {
        std::fs::create_dir_all(path).expect("folder");
    }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).expect("clock").as_secs_f64();
    let records: String = (0..600)
        .map(|index| {
            format!(
                "{{\"t\": {}, \"agent\": \"codex\", \"provider\": \"test\", \"model\": \"model-{index:03}\", \"in\": 100, \"out\": 1}}\n",
                now - 60.0
            )
        })
        .collect();
    std::fs::write(logs.join("usage.jsonl"), records).expect("records");
    let magpie = logs.to_string_lossy().replace('\\', "\\\\");
    std::fs::write(
        state.join("preferences.json"),
        format!("{{\"schema\": 1, \"visible\": [\"summary\", \"magpie\"], \"paths\": {{\"magpie\": \"{magpie}\"}}}}"),
    )
    .expect("preferences");
    let mut child = Command::new(env!("CARGO_BIN_EXE_starmeter-engine"))
        .args(["cli", "usage", "all", "-g", "model", "--json", "--no-refresh"])
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("OCX_STATE_DIR", &state)
        .env("OCX_CACHE_DIR", folder.join("cache"))
        .env("OCX_MAC_CACHE_DIR", folder.join("cache"))
        .env("OCX_PRICING_UPDATES", "0")
        .env("OCX_SELF_UPDATES", "0")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the engine");
    let mut first = [0u8; 2];
    child.stdout.take().expect("stdout").read_exact(&mut first).expect("the start of the JSON");
    assert_eq!(&first, b"{\n");
    let output = child.wait_with_output().expect("the end");
    let errors = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{:?}: {errors}", output.status);
    assert!(!errors.contains("panicked") && !errors.contains("Broken pipe"), "{errors}");
    let _ = std::fs::remove_dir_all(folder);
}
