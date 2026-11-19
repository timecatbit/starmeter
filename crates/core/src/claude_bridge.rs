//! Claude Code's status-line callback, connected only when the user turns on 本机用量回传: it keeps the subscription
//! percentages Claude Code hands its status line, then runs the status line the user had before. No network,
//! cookies or keychain. The bridge is a copy of this engine.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::claude_subscription::{BACKUP_FILE, SNAPSHOT_FILE, local_enabled};
use crate::files::{self, paths};
use crate::host::{HostFailure, terminal_jobs};
use crate::json::{Json, Object};
use crate::rows::g;
use crate::runtime::{self, now};
use crate::time::utc_string;
use crate::{object, platform, py, shell};

const BRIDGE: &str = "integrations/claude-statusline";
const LIMIT: usize = 2 * 1024 * 1024;

fn read(path: &str) -> Object {
    let Ok(file) = std::fs::File::open(path) else { return Object::new() };
    let mut raw = Vec::new();
    if file.take(LIMIT as u64 + 1).read_to_end(&mut raw).is_err() || raw.len() > LIMIT {
        return Object::new();
    }
    Json::parse(&raw).ok().and_then(|value| value.as_object().cloned()).unwrap_or_default()
}

fn write(path: &str, value: &Object) -> Result<(), HostFailure> {
    let target = Path::new(path);
    if let Some(folder) = target.parent() {
        files::make_directory(folder, 0o700).map_err(|error| HostFailure(error.to_string()))?;
    }
    let prefix = format!(".{}", target.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default());
    files::replace_atomically(target, Json::Object(value.clone()).serialized().as_bytes(), &prefix, || Ok(()))
        .map_err(|error| HostFailure(error.to_string()))
}

pub fn settings_path(environment: &HashMap<String, String>) -> String {
    let raw = environment.get("CLAUDE_CONFIG_DIR").and_then(|value| value.split(',').next()).unwrap_or("").trim();
    let mut root = if raw.is_empty() {
        format!("{}/.claude", paths::home().to_string_lossy())
    } else {
        paths::expand_user(raw).to_string_lossy().into_owned()
    };
    if Path::new(&root).file_name().is_some_and(|name| name == "projects") {
        root = Path::new(&root).parent().map(|parent| parent.to_string_lossy().into_owned()).unwrap_or(root);
    }
    format!("{root}/settings.json")
}

fn number(value: Option<&Json>) -> Option<f64> {
    match value {
        Some(Json::Int(number)) => Some(*number as f64),
        Some(Json::Double(number)) if number.is_finite() => Some(*number),
        _ => None,
    }
}

fn normalized(data: &Object, now: f64) -> Object {
    let mut windows = Vec::new();
    if let Some(rates) = data.get("rate_limits").and_then(Json::as_object) {
        for (key, label) in [("five_hour", "5 小时"), ("seven_day", "本周")] {
            let Some(row) = rates.get(key).and_then(Json::as_object) else { continue };
            let (Some(percent), Some(reset)) = (number(row.get("used_percentage")), number(row.get("resets_at")))
            else {
                continue;
            };
            if !(0.0..=100.0).contains(&percent) || !(now < reset && reset < now + 366.0 * 86400.0) {
                continue;
            }
            windows.push(Json::from(object! {
                "label" => label, "used" => row.get("used_percentage").cloned().unwrap_or(Json::Null), "resetsAt" => utc_string(reset),
            }));
        }
    }
    let empty = windows.is_empty();
    object! {
        "schema" => 1, "source" => "claude-code", "windows" => Json::Array(windows), "updatedAt" => now,
        "state" => if empty { "等待客户端返回订阅用量" } else { "本机用量" }, "stale" => empty,
    }
}

pub fn publish(state: &str, data: &Object, now: f64, refresh: bool) -> Result<Object, HostFailure> {
    let path = format!("{state}/{SNAPSHOT_FILE}");
    let previous = read(&path);
    let result = normalized(data, now);
    let age = number(previous.get("updatedAt")).map_or(f64::INFINITY, |at| now - at);
    // The status line can run several times a second; never start a polling loop.
    let same =
        previous.get("windows").cloned().unwrap_or(Json::Null) == result.get("windows").cloned().unwrap_or(Json::Null);
    if (0.0..if same { 60.0 } else { 10.0 }).contains(&age)
        && (same || result.get("windows").is_some_and(Json::is_truthy))
    {
        return Ok(previous);
    }
    write(&path, &result)?;
    if refresh {
        let _ = shell::run(&["/usr/bin/open", "-g", "swiftbar://refreshplugin?name=starmeter"], 3.0, None, None);
    }
    Ok(result)
}

pub fn enabled(state: &str) -> bool {
    local_enabled(state)
}

fn resolved(path: &str) -> String {
    paths::resolve(Path::new(path))
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| runtime::absolute(path))
}

pub fn install(state: &str, given: Option<&str>, environment: &HashMap<String, String>) -> Result<(), HostFailure> {
    let state = resolved(state);
    let target = given.map(String::from).unwrap_or_else(|| settings_path(environment));
    let settings = resolved(&target);
    let mut current = read(&settings);
    if Path::new(&settings).exists() && current.is_empty() {
        // {} is a valid empty settings object; a malformed file is never replaced.
        let empty =
            std::fs::read(&settings).ok().and_then(|data| Json::parse(&data).ok()) == Some(Json::Object(Object::new()));
        if !empty {
            return Err(HostFailure("Claude 设置格式异常".into()));
        }
    }
    // The engine itself under SwiftBar; an app ships the engine program in bin/ among its resources.
    let name = if cfg!(windows) { "starmeter-engine.exe" } else { "starmeter-engine" };
    let shipped = environment
        .get("OCX_PLUGIN_PATH")
        .and_then(|plugin| Path::new(plugin).parent().map(|resources| resources.join("bin").join(name)))
        .filter(|path| path.is_file());
    let executable = match shipped {
        Some(path) => path,
        None => std::env::current_exe().map_err(|_| HostFailure("找不到状态栏程序".into()))?,
    };
    let bridge = format!("{state}/{BRIDGE}");
    let command = terminal_jobs::join(&[&bridge, "--claude-statusline", "--state-dir", &state]);
    let copy_bridge = || -> Result<(), HostFailure> {
        if let Some(folder) = Path::new(&bridge).parent() {
            files::make_directory(folder, 0o700).map_err(|error| HostFailure(error.to_string()))?;
        }
        let temporary = format!("{bridge}.new-{}", runtime::random_hex(12));
        std::fs::copy(&executable, &temporary).map_err(|error| HostFailure(error.to_string()))?;
        platform::set_mode(Path::new(&temporary), 0o700);
        if !platform::replace(Path::new(&temporary), Path::new(&bridge)) {
            platform::remove(Path::new(&temporary));
            return Err(HostFailure("无法写入状态栏程序".into()));
        }
        Ok(())
    };
    let backup = format!("{state}/{BACKUP_FILE}");
    if enabled(&state) {
        copy_bridge()?;
        // An earlier bridge is pointed at this one; what ran before it is kept.
        let mut meta = read(&backup);
        if meta.get("managedCommand") != Some(&Json::from(command.as_str()))
            && let Some(mut status) = current.get("statusLine").and_then(Json::as_object).cloned()
        {
            status.insert("command", command.as_str());
            current.insert("statusLine", status);
            meta.insert("managedCommand", command.as_str());
            write(&backup, &meta)?;
            write(&settings, &current)?;
        }
        return Ok(());
    }
    let original = current.get("statusLine").cloned();
    if let Some(original) = &original
        && (original.get("type") != Some(&Json::from("command"))
            || original.get("command").and_then(Json::as_str).is_none())
    {
        return Err(HostFailure("已有状态栏格式尚未支持，保留原设置".into()));
    }
    write(
        &backup,
        &object! {
            "settings" => settings.as_str(), "original" => original.clone().unwrap_or(Json::Null), "managedCommand" => command.as_str(),
        },
    )?;
    copy_bridge()?;
    let mut status = original.and_then(|original| original.as_object().cloned()).unwrap_or_default();
    status.insert("type", "command");
    status.insert("command", command.as_str());
    current.insert("statusLine", status);
    write(&settings, &current)
}

pub fn uninstall(state: &str) {
    let meta = read(&format!("{state}/{BACKUP_FILE}"));
    if enabled(state)
        && let Some(settings) = meta.get("settings").and_then(Json::as_str)
    {
        let mut current = read(settings);
        match meta.get("original").filter(|original| !original.is_null()) {
            Some(original) => current.insert("statusLine", original.clone()),
            None => {
                current.remove("statusLine");
            }
        }
        let _ = write(settings, &current);
    }
    platform::remove(Path::new(&format!("{state}/{SNAPSHOT_FILE}")));
    // The backup stays if the user replaced the integration themselves.
}

/// The status-line callback: keep the percentages, then hand the input to the status line that was there before.
pub fn run(state: &str) -> i32 {
    let mut raw = Vec::new();
    let _ = std::io::stdin().lock().take(LIMIT as u64 + 1).read_to_end(&mut raw);
    let mut result = Object::new();
    if raw.len() <= LIMIT
        && let Some(data) = Json::parse(&raw).ok().and_then(|value| value.as_object().cloned())
    {
        result = publish(state, &data, now(), true).unwrap_or_default();
    }
    let meta = read(&format!("{state}/{BACKUP_FILE}"));
    let command = meta
        .get("original")
        .and_then(|original| original.get("command"))
        .and_then(Json::as_str)
        .filter(|command| !command.is_empty())
        .map(String::from);
    if let Some(command) = command {
        if let Ok(mut child) = Command::new("/bin/sh").args(["-c", &command]).stdin(Stdio::piped()).spawn() {
            if let Some(mut input) = child.stdin.take() {
                let _ = input.write_all(&raw);
            }
            let deadline = Instant::now() + Duration::from_secs(4);
            while matches!(child.try_wait(), Ok(None)) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
            if matches!(child.try_wait(), Ok(None)) {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
    } else {
        let values: Vec<String> = result
            .get("windows")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .filter_map(Json::as_object)
            .map(|window| {
                format!(
                    "{} {}%",
                    py::str(window.get("label")),
                    g(window.get("used").and_then(Json::number).unwrap_or(0.0))
                )
            })
            .collect();
        let suffix = if values.is_empty() { String::new() } else { format!(" · {}", values.join(" · ")) };
        println!("Claude{suffix}");
    }
    0
}
