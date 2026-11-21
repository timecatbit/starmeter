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

/// The engine as the command line on a scratch computer, its JSON parsed loosely: just the numbers and names the
/// tests look for.
fn run(home: &std::path::Path, state: &std::path::Path, arguments: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_starmeter-engine"))
        .arg("cli")
        .args(arguments)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("OCX_STATE_DIR", state)
        .env("OCX_CACHE_DIR", home.join("cache"))
        .env("OCX_MAC_CACHE_DIR", home.join("cache"))
        .env("OCX_PRICING_UPDATES", "0")
        .env("OCX_SELF_UPDATES", "0")
        .env_remove("MAGPIE_USAGE_DIR")
        .stdin(Stdio::null())
        .output()
        .expect("the engine");
    assert!(output.status.success(), "{arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).expect("UTF-8")
}

/// The keys listed under a JSON field, as `"field": ["a", "b"]` writes them.
fn listed(json: &str, field: &str) -> Vec<String> {
    let start = json.find(&format!("\"{field}\": [")).expect(field) + field.len() + 5;
    let end = start + json[start..].find(']').expect("the list's end");
    json[start..end]
        .split(',')
        .map(|key| key.trim().trim_matches('"').to_string())
        .filter(|key| !key.is_empty())
        .collect()
}

#[test]
fn detect_adds_a_tool_found_after_the_first_run_and_keeps_the_other_choices() {
    // CLI-4: a CLI-only computer chose its modules on the first run; Magpie came later and no command re-detected it.
    let folder = std::env::temp_dir().join(format!("starmeter-detect-{}", std::process::id()));
    let (home, state) = (folder.join("home"), folder.join("state"));
    std::fs::create_dir_all(&home).expect("home");
    std::fs::create_dir_all(&state).expect("state");
    std::fs::write(state.join("preferences.json"), r#"{"schema": 1, "visible": ["summary", "claude-api"]}"#)
        .expect("the first run's choice");
    let logs = home.join(".config").join("magpie");
    std::fs::create_dir_all(&logs).expect("Magpie's folder");
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).expect("clock").as_secs_f64();
    std::fs::write(
        logs.join("usage.jsonl"),
        format!("{{\"t\": {}, \"agent\": \"codex\", \"provider\": \"test\", \"model\": \"gpt-5.4\", \"in\": 99, \"out\": 1}}\n", now - 60.0),
    )
    .expect("a record");
    let before = run(&home, &state, &["usage", "all", "--json", "--no-refresh"]);
    assert!(!before.contains("\"id\": \"magpie\""), "{before}");
    // Asked without --yes and with no terminal to answer on: it lists, and changes nothing.
    let looked = run(&home, &state, &["detect", "--json"]);
    assert!(listed(&looked, "new").contains(&"magpie".to_string()), "{looked}");
    assert!(listed(&looked, "added").is_empty(), "{looked}");
    assert!(!listed(&looked, "homepage").contains(&"magpie".to_string()), "{looked}");
    let added = run(&home, &state, &["detect", "--yes", "--json"]);
    let homepage = listed(&added, "homepage");
    for key in ["summary", "claude-api", "magpie"] {
        assert!(homepage.contains(&key.to_string()), "{key} missing: {added}");
    }
    let after = run(&home, &state, &["usage", "all", "--json", "--no-refresh"]);
    let magpie = after.find("\"id\": \"magpie\"").expect("Magpie's card");
    assert!(after[magpie..].contains("\"tokens\": 100,"), "{after}");
    let text = run(&home, &state, &["detect", "--lang", "en"]);
    assert!(text.contains("Everything found on this computer is on the homepage already."), "{text}");
    let _ = std::fs::remove_dir_all(folder);
}

#[test]
fn today_follows_the_system_zone_when_none_is_set() {
    // RANGE-4: no counting zone set, the system's changes (here a TZ only the system reads, ":Zone"): the cached 今天
    // of the zone before must not serve the new one.
    let folder = std::env::temp_dir().join(format!("starmeter-zone-{}", std::process::id()));
    let (home, state) = (folder.join("home"), folder.join("state"));
    let logs = home.join(".config").join("magpie");
    for path in [&state, &logs] {
        std::fs::create_dir_all(path).expect("folder");
    }
    std::fs::write(state.join("preferences.json"), r#"{"schema": 1, "visible": ["summary", "magpie"]}"#)
        .expect("preferences");
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).expect("clock").as_secs_f64();
    // Taipei's day begins 8 hours before UTC's: a moment between the two beginnings is today in one zone and not in
    // the other, whatever the hour.
    let utc_day = (now / 86400.0).floor() * 86400.0;
    let taipei_day = ((now + 28800.0) / 86400.0).floor() * 86400.0 - 28800.0;
    let between = (utc_day + taipei_day) / 2.0;
    let record = |at: f64, tokens: i64| {
        format!(
            "{{\"t\": {at}, \"agent\": \"codex\", \"provider\": \"test\", \"model\": \"gpt-5.4\", \"in\": {tokens}, \"out\": 0}}\n"
        )
    };
    std::fs::write(logs.join("usage.jsonl"), record(between, 100) + &record(now - 30.0, 40)).expect("records");
    let today = |cache: &str, zone: &str| {
        let output = Command::new(env!("CARGO_BIN_EXE_starmeter-engine"))
            .args(["cli", "usage", "today", "--json", "--no-refresh"])
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("OCX_STATE_DIR", &state)
            .env("OCX_CACHE_DIR", folder.join(cache))
            .env("OCX_MAC_CACHE_DIR", folder.join(cache))
            .env("OCX_PRICING_UPDATES", "0")
            .env("OCX_SELF_UPDATES", "0")
            .env("TZ", zone)
            .env_remove("OCX_TIMEZONE")
            .env_remove("MAGPIE_USAGE_DIR")
            .stdin(Stdio::null())
            .output()
            .expect("the engine");
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        let json = String::from_utf8(output.stdout).expect("UTF-8");
        let at = json.find("\"id\": \"magpie\"").expect("Magpie's card");
        let tokens = &json[at..][json[at..].find("\"tokens\": ").expect("tokens") + 10..];
        tokens[..tokens.find(|c: char| !c.is_ascii_digit()).unwrap_or(tokens.len())].parse::<i64>().expect("a number")
    };
    let taipei = today("cache", ":Asia/Taipei");
    let utc = today("cache", ":UTC");
    let fresh = today("fresh", ":UTC");
    assert_eq!(utc, fresh, "the system's new zone reads today afresh (Taipei read {taipei})");
    assert_ne!(taipei, utc, "the two zones' todays differ by the record between their beginnings");
    let _ = std::fs::remove_dir_all(folder);
}
