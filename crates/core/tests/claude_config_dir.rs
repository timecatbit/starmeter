//! R9-1: the folder Claude Code's settings are in is the runtime's CLAUDE_CONFIG_DIR (config.local.sh's), not the
//! process's: the app opened from Finder or at login has none in its own environment. A test binary of its own, so no
//! other runtime made at the same time tells the projects reader another folder.
use std::collections::HashMap;

#[test]
fn a_projects_name_comes_from_the_settings_in_the_runtimes_claude_folder() {
    let root = std::env::temp_dir().join(format!("starmeter-claude-config-{}", std::process::id()));
    let folder = root.join("claude-work");
    std::fs::create_dir_all(&folder).unwrap();
    // Claude Code wrote the project's folder under its own name: "my app.v2", which the folder name alone can't tell.
    std::fs::write(folder.join(".claude.json"), r#"{"projects":{"/Users/me/Documents/my app.v2":{}}}"#).unwrap();
    assert!(std::env::var("CLAUDE_CONFIG_DIR").is_err(), "the test's own process names no folder");
    let environment = HashMap::from([
        ("CLAUDE_CONFIG_DIR".to_string(), folder.to_string_lossy().into_owned()),
        ("OCX_STATE_DIR".to_string(), root.join("state").to_string_lossy().into_owned()),
    ]);
    let _runtime = starmeter_core::runtime::Runtime::new(environment, "starmeter");
    assert_eq!(starmeter_core::projects::folder_project("-Users-me-Documents-my-app-v2").as_deref(), Some("my app.v2"));
    let _ = std::fs::remove_dir_all(&root);
}
