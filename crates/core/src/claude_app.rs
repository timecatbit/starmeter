//! Claude Desktop's own registry of the Claude Code sessions it runs, and where every Claude Code client keeps its
//! transcripts. Never reads credentials.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::Path;

use crate::claude_usage::state_of;
use crate::files::paths;
use crate::harness::{normalize, sorted_paths, walk};
use crate::json::{Json, Object};

pub fn is_uuid(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| {
            if [8, 13, 18, 23].contains(&index) { *byte == b'-' } else { byte.is_ascii_hexdigit() }
        })
}

/// A small JSON object file, or an empty object when it is missing, too large or not an object.
pub fn read_object(path: &str) -> Object {
    read_object_limited(path, 10 * 1024 * 1024)
}

pub fn read_object_limited(path: &str, limit: usize) -> Object {
    let Ok(file) = std::fs::File::open(path) else { return Object::new() };
    let mut data = Vec::new();
    if file.take(limit as u64 + 1).read_to_end(&mut data).is_err() || data.len() > limit {
        return Object::new();
    }
    Json::parse(&data).ok().and_then(|value| value.as_object().cloned()).unwrap_or_default()
}

pub fn default_root(home: &str, environment: &HashMap<String, String>) -> String {
    if cfg!(windows) {
        let roaming = environment.get("APPDATA").cloned().unwrap_or_else(|| format!("{home}\\AppData\\Roaming"));
        let local = environment.get("LOCALAPPDATA").cloned().unwrap_or_else(|| format!("{home}\\AppData\\Local"));
        let mut roots = vec![format!("{roaming}\\Claude")];
        let packages = format!("{local}\\Packages");
        let mut names: Vec<String> = std::fs::read_dir(&packages)
            .map(|entries| {
                entries.filter_map(Result::ok).map(|e| e.file_name().to_string_lossy().into_owned()).collect()
            })
            .unwrap_or_default();
        names.sort();
        for name in names {
            if name.starts_with("Claude_") || name.starts_with("AnthropicPBC.Claude_") {
                roots.push(format!("{packages}\\{name}\\LocalCache\\Roaming\\Claude"));
            }
        }
        let mut best: Option<(std::time::SystemTime, String)> = None;
        for root in &roots {
            let newest = ["config.json", "claude-code-sessions", "plan-usage-history.json"]
                .iter()
                .filter_map(|name| std::fs::metadata(format!("{root}\\{name}")).ok()?.modified().ok())
                .max();
            if let Some(newest) = newest
                && best.as_ref().is_none_or(|(time, _)| newest > *time)
            {
                best = Some((newest, root.clone()));
            }
        }
        best.map_or_else(|| roots[0].clone(), |(_, root)| root)
    } else {
        format!("{home}/Library/Application Support/Claude")
    }
}

fn children(path: &str) -> Vec<String> {
    std::fs::read_dir(path)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| format!("{path}/{}", entry.file_name().to_string_lossy()))
                .collect()
        })
        .unwrap_or_default()
}

/// Only the app's registry, never recursive scans of Cowork workspaces.
pub fn registry_files(root: &str) -> Vec<String> {
    let base = format!("{root}/claude-code-sessions");
    let account = read_object(&format!("{root}/config.json"))
        .get("lastKnownAccountUuid")
        .and_then(Json::as_str)
        .map(String::from);
    let folders: Vec<String> = match account.filter(|account| is_uuid(account)) {
        Some(account) => children(&format!("{base}/{account}")),
        None => children(&base).iter().flat_map(|path| children(path)).collect(),
    };
    sorted_paths(
        folders
            .iter()
            .flat_map(|folder| children(folder))
            .filter(|path| path.ends_with(".json") && !Path::new(path).is_dir())
            .collect(),
    )
}

pub fn registered_ids(root: &str) -> HashSet<String> {
    let mut ids = HashSet::new();
    for path in registry_files(root) {
        let row = read_object(&path);
        if !row.get("sessionId").is_some_and(Json::is_truthy) {
            continue;
        }
        let mut candidates: Vec<&Json> =
            ["sessionId", "cliSessionId", "unarchivedCliSessionId", "historyOnlyCliSessionId"]
                .iter()
                .filter_map(|key| row.get(key))
                .collect();
        if let Some(previous) = row.get("previousCliSessionIds").and_then(Json::as_array) {
            candidates.extend(previous);
        }
        ids.extend(candidates.into_iter().filter_map(Json::as_str).filter(|id| is_uuid(id)).map(String::from));
    }
    ids
}

/// The ccusage-compatible folders where every Claude Code client keeps its transcripts.
pub fn project_roots(home: &str, environment: &HashMap<String, String>) -> Vec<String> {
    let configured = environment.get("CLAUDE_CONFIG_DIR").map(|value| value.trim()).unwrap_or("");
    let roots: Vec<String> = if !configured.is_empty() {
        configured
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(|part| normalize(&paths::expand_user(part).to_string_lossy()))
            .collect()
    } else {
        let xdg = environment
            .get("XDG_CONFIG_HOME")
            .filter(|value| !value.is_empty())
            .cloned()
            .unwrap_or_else(|| format!("{home}/.config"));
        vec![format!("{home}/.claude"), format!("{}/claude", normalize(&xdg))]
    };
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    // On Windows, also Claude Code's folders inside the WSL distributions that are running.
    let roots = roots.into_iter().chain(crate::wsl::claude_projects());
    for root in roots {
        let last = root.rsplit('/').next().unwrap_or("");
        let projects = if last == "projects" { root.clone() } else { format!("{root}/projects") };
        if seen.insert(projects.clone()) {
            result.push(projects);
        }
    }
    result
}

/// Every transcript that counts: all project folders, plus the app's own folder for sessions it registered.
pub fn transcript_files(
    root: &str,
    home: &str,
    environment: &HashMap<String, String>,
) -> (Vec<String>, HashSet<String>) {
    let ids = registered_ids(root);
    let mut found: HashSet<String> = HashSet::new();
    for base in project_roots(home, environment) {
        if Path::new(&base).is_dir() {
            found.extend(walk(&base).into_iter().filter(|path| path.ends_with(".jsonl")));
        }
    }
    let app = format!("{root}/claude-code-sessions");
    if !ids.is_empty() && Path::new(&app).is_dir() {
        for path in walk(&app) {
            if !path.ends_with(".jsonl") {
                continue;
            }
            let relative: Vec<&str> = path[app.len() + 1..].split('/').filter(|part| !part.is_empty()).collect();
            let last = relative.last().copied().unwrap_or("");
            let stem = Path::new(last).file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
            if ids.contains(&stem)
                || relative[..relative.len().saturating_sub(1)].iter().any(|part| ids.contains(*part))
            {
                found.insert(path);
            }
        }
    }
    (sorted_paths(found.into_iter().collect()), ids)
}

/// The app's title and reasoning effort for each session it touched since `since`, by transcript session ID.
/// Cached per registry file print (`inode:size:mtime`) so 2-second live ticks avoid re-parsing unchanged files.
pub fn session_settings(root: &str, since: f64) -> HashMap<String, (Option<String>, Option<String>)> {
    type Entry = (String, Option<(String, Option<String>, Option<String>)>);
    static KNOWN: std::sync::Mutex<Option<HashMap<String, Entry>>> = std::sync::Mutex::new(None);
    let mut found = HashMap::new();
    for path in registry_files(root) {
        let Some(state) = state_of(Path::new(&path)) else { continue };
        if (state.mtime as f64) / 1e9 < since {
            continue;
        }
        let print = format!("{}:{}:{}", state.inode, state.size, state.mtime);
        let cached = KNOWN.lock().ok().and_then(|known| {
            known.as_ref()?.get(&path).filter(|(seen, _)| *seen == print).map(|(_, val)| val.clone())
        });
        let parsed = match cached {
            Some(entry) => entry,
            None => {
                let row = read_object(&path);
                let entry = row.get("cliSessionId").and_then(Json::as_str).filter(|id| is_uuid(id)).map(|session| {
                    let title = row
                        .get("title")
                        .and_then(Json::as_str)
                        .map(str::trim)
                        .filter(|t| !t.is_empty())
                        .map(String::from);
                    let effort = row.get("effort").and_then(Json::as_str).filter(|e| !e.is_empty()).map(String::from);
                    (session.to_string(), title, effort)
                });
                if let Ok(mut known) = KNOWN.lock() {
                    known.get_or_insert_with(HashMap::new).insert(path, (print, entry.clone()));
                }
                entry
            }
        };
        if let Some((session, title, effort)) = parsed {
            found.insert(session, (title, effort));
        }
    }
    found
}
