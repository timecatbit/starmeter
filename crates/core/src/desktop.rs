//! The environment a desktop shell runs the core in, as 5.x's hosts set it up: the macOS app sources the user's
//! config.local.sh like the SwiftBar entry does, but keeps its own cache folder; the Windows and Linux shells keep
//! their connection overrides in a JSON file beside their settings. Preferences and named keys are shared.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use crate::files::{self, paths};
use crate::host::{HostFailure, connections};
use crate::json::{Json, Object};

/// Which shell is asking.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shell {
    /// The macOS menu bar app (5.x's standalone app).
    Mac,
    /// The Windows tray.
    Windows,
    /// The Linux tray.
    Linux,
}

impl Shell {
    pub fn current() -> Shell {
        if cfg!(target_os = "macos") {
            Shell::Mac
        } else if cfg!(windows) {
            Shell::Windows
        } else {
            Shell::Linux
        }
    }
}

/// Connection settings a shell keeps in config.local.json: paths, addresses and non-secret options. A secret may
/// only be cleared there; keys the user types go to the key store.
static OVERRIDE_NAMES: LazyLock<HashSet<String>> = LazyLock::new(|| {
    let mut names: HashSet<String> = [
        "OCX_HOME",
        "OCX_BASE_URL",
        "OCX_CHATGPT_HOME",
        "KOMARI_BASE_URL",
        "XAI_TEAM_ID",
        "MINIMAX_REGION",
        "MOONSHOT_REGION",
        "QODER_CN_ORG_ID",
        "OCX_TOKEN_FILE",
    ]
    .iter()
    .map(|name| name.to_string())
    .collect();
    names.extend(connections::KEYS.iter().filter(|key| key.ends_with("_FILE")).cloned());
    names
});

fn allowed(key: &str, value: &str) -> bool {
    !value.contains('\0')
        && !value.contains('\n')
        && (OVERRIDE_NAMES.contains(key) || (connections::KEYS.contains(key) && value.is_empty()))
}

fn home() -> String {
    paths::home().to_string_lossy().into_owned()
}

/// The settings folder a shell uses when OCX_STATE_DIR doesn't name one.
pub fn state_dir(shell: Shell, environment: &HashMap<String, String>) -> String {
    if let Some(explicit) = environment.get("OCX_STATE_DIR").filter(|value| !value.is_empty()) {
        let path = paths::expand_user(explicit);
        return paths::resolve(&path).unwrap_or(path).to_string_lossy().into_owned();
    }
    let given = |name: &str| environment.get(name).filter(|value| !value.is_empty()).cloned();
    match shell {
        Shell::Mac => format!("{}/Library/Application Support/SwiftBarToken", home()),
        Shell::Windows => {
            let local = given("LOCALAPPDATA").unwrap_or_else(|| format!("{}\\AppData\\Local", home()));
            format!("{local}\\Token Monitor")
        }
        Shell::Linux => {
            let data = given("XDG_DATA_HOME").unwrap_or_else(|| format!("{}/.local/share", home()));
            format!("{data}/starmeter")
        }
    }
}

/// The JSON overrides file of the Windows and Linux shells, created empty when missing.
pub fn config_file(state: &str) -> String {
    let path = format!("{state}/config.local.json");
    if !Path::new(&path).exists() {
        let _ = files::make_directory(Path::new(state), 0o700);
        let _ = files::replace_atomically(Path::new(&path), b"{}", ".config-", || Ok(()));
    }
    path
}

/// The overrides in config.local.json that a shell applies; anything else in the file is ignored.
pub fn json_overrides(state: &str) -> Vec<(String, String)> {
    let Some(value) = std::fs::read(format!("{state}/config.local.json"))
        .ok()
        .and_then(|data| Json::parse(&data).ok())
        .and_then(|value| value.as_object().cloned())
    else {
        return Vec::new();
    };
    value
        .iter()
        .filter_map(|(key, value)| value.as_str().map(|text| (key.clone(), text.to_string())))
        .filter(|(key, value)| allowed(key, value))
        .collect()
}

/// Saves connection overrides in config.local.json, keeping the others; refused unless every change is allowed.
pub fn save_json_overrides(state: &str, values: &[(String, String)]) -> Result<(), HostFailure> {
    if values.is_empty() || !values.iter().all(|(key, value)| allowed(key, value)) {
        return Err(HostFailure("invalid config".into()));
    }
    let path = config_file(state);
    let mut current: Object = json_overrides(state).into_iter().map(|(key, value)| (key, Json::from(value))).collect();
    for (key, value) in values {
        current.insert(key.clone(), value.as_str());
    }
    let document = Json::Object(current);
    files::replace_atomically(Path::new(&path), document.serialized().as_bytes(), ".config-", || Ok(()))
        .map_err(|error| HostFailure(error.to_string()))?;
    let saved = std::fs::read(&path).ok().and_then(|data| Json::parse(&data).ok());
    if saved.as_ref() != Some(&document) {
        return Err(HostFailure("settings could not be saved".into()));
    }
    Ok(())
}

/// The variables config.local.sh leaves set, sourced by bash as the SwiftBar entry does; None when it couldn't run.
fn sourced(path: &str, environment: &HashMap<String, String>) -> Option<HashMap<String, String>> {
    let mut child = Command::new("/bin/bash")
        .args(["-p", "-c", "source \"$1\" >/dev/null 2>&1; exec /usr/bin/env -0", "starmeter", path])
        .env_clear()
        .envs(environment)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut data = Vec::new();
        let _ = stdout.read_to_end(&mut data);
        data
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    let data = reader.join().ok()?;
    if !status.success() {
        return None;
    }
    Some(
        data.split(|byte| *byte == 0)
            .filter_map(|entry| {
                let text = String::from_utf8_lossy(entry);
                let (key, value) = text.split_once('=')?;
                (!key.is_empty()).then(|| (key.to_string(), value.to_string()))
            })
            .collect(),
    )
}

/// The environment a shell's core runs with. `resources` is the folder holding lib/ (the price tables) and assets/
/// (the logos) that the app ships with.
pub fn environment(shell: Shell, base: HashMap<String, String>, resources: &str) -> HashMap<String, String> {
    let mut environment = base;
    let state = state_dir(shell, &environment);
    let plugin = Path::new(resources).join("starmeter").to_string_lossy().into_owned();
    environment.insert("OCX_STATE_DIR".into(), state.clone());
    match shell {
        Shell::Mac => {
            // An app opened from Finder has a bare PATH; the CLIs it looks for live in these folders.
            let path = environment.get("PATH").cloned().unwrap_or_default();
            let home = home();
            let prefix = format!("/opt/homebrew/bin:/usr/local/bin:{home}/.local/bin:/usr/bin:/bin");
            environment.insert("PATH".into(), if path.is_empty() { prefix } else { format!("{prefix}:{path}") });
            // Its own cache folder keeps it out of the SwiftBar plugin's snapshots, locks and receipts.
            let cache = environment
                .get("OCX_MAC_CACHE_DIR")
                .filter(|value| !value.is_empty())
                .cloned()
                .unwrap_or_else(|| format!("{home}/Library/Caches/Token Monitor"));
            let fixed = [("OCX_MAC_APP", "1".to_string()), ("OCX_PLUGIN_PATH", plugin), ("OCX_CACHE_DIR", cache)];
            for (key, value) in &fixed {
                environment.insert(key.to_string(), value.clone());
            }
            let config = format!("{state}/config.local.sh");
            if Path::new(&config).is_file()
                && let Some(loaded) = sourced(&config, &environment)
            {
                environment = loaded;
            }
            // The user's file can't move the app onto the plugin's files.
            for (key, value) in fixed {
                environment.insert(key.to_string(), value);
            }
        }
        Shell::Windows | Shell::Linux => {
            environment
                .entry("OCX_CACHE_DIR".into())
                .or_insert_with(|| Path::new(&state).join("cache").to_string_lossy().into_owned());
            environment.insert("OCX_PLUGIN_PATH".into(), plugin);
            for (key, value) in json_overrides(&state) {
                environment.insert(key, value);
            }
        }
    }
    environment
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_overrides_keep_only_paths_and_cleared_secrets() {
        let folder = std::env::temp_dir().join(format!("starmeter-desktop-{}", crate::runtime::random_hex(8)));
        let state = folder.to_string_lossy().into_owned();
        save_json_overrides(
            &state,
            &[("OCX_HOME".into(), "/tmp/ocx".into()), ("DEEPSEEK_API_KEY".into(), String::new())],
        )
        .expect("saved");
        assert!(save_json_overrides(&state, &[("DEEPSEEK_API_KEY".into(), "sk-secret".into())]).is_err());
        assert!(save_json_overrides(&state, &[("PATH".into(), "/bin".into())]).is_err());
        std::fs::write(
            format!("{state}/config.local.json"),
            br#"{"OCX_HOME": "/x", "OPENAI_API_KEY": "sk", "KOMARI_BASE_URL": 3}"#,
        )
        .expect("written");
        assert_eq!(json_overrides(&state), vec![("OCX_HOME".to_string(), "/x".to_string())]);
        let environment = environment(Shell::Windows, HashMap::from([("OCX_STATE_DIR".into(), state.clone())]), "/app");
        assert_eq!(environment.get("OCX_HOME").map(String::as_str), Some("/x"));
        assert!(environment.get("OCX_CACHE_DIR").is_some_and(|cache| cache.ends_with("cache")));
        let _ = std::fs::remove_dir_all(folder);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_mac_app_sources_config_but_keeps_its_own_folders() {
        let folder = std::env::temp_dir().join(format!("starmeter-desktop-{}", crate::runtime::random_hex(8)));
        std::fs::create_dir_all(&folder).expect("folder");
        let state = folder.to_string_lossy().into_owned();
        std::fs::write(
            format!("{state}/config.local.sh"),
            "export KOMARI_BASE_URL=https://monitor.example.org\nexport OCX_CACHE_DIR=/elsewhere\nexport OCX_MAC_APP=0\n",
        )
        .expect("config");
        let base =
            HashMap::from([("OCX_STATE_DIR".to_string(), state.clone()), ("PATH".to_string(), "/usr/bin".to_string())]);
        let environment = environment(Shell::Mac, base, "/Applications/Starmeter.app/Contents/Resources");
        assert_eq!(environment.get("KOMARI_BASE_URL").map(String::as_str), Some("https://monitor.example.org"));
        assert_eq!(environment.get("OCX_MAC_APP").map(String::as_str), Some("1"));
        assert!(environment.get("OCX_CACHE_DIR").is_some_and(|cache| cache.ends_with("Library/Caches/Token Monitor")));
        assert_eq!(
            environment.get("OCX_PLUGIN_PATH").map(String::as_str),
            Some("/Applications/Starmeter.app/Contents/Resources/starmeter")
        );
        assert!(environment.get("PATH").is_some_and(|path| path.starts_with("/opt/homebrew/bin:")));
        let _ = std::fs::remove_dir_all(folder);
    }
}
