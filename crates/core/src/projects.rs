//! Which project a request belongs to, by the name the user gave its folder; only that name is kept, never the path.
//!
//! Claude (and Qoder CN, which writes Claude's format) files each conversation under projects/<folder>, the folder
//! written with everything but letters and digits as '-': the folder the conversation was started in, or moved to
//! in the Claude app. That folder is the project, whatever folders the conversation went on to work in (a subfolder,
//! a scratchpad). A worktree Claude makes for a conversation (<repository>/.claude/worktrees/<name>) belongs to its
//! repository, and the Claude app's conversations without a folder (its scratch workspaces) are Claude 对话.
//!
//! The written name cannot tell a '-' that was '/' from a space, a dot or a dash, and the folders themselves are never
//! looked at (reading Documents or Desktop makes macOS ask the user for access): the path comes from the folders Claude
//! Code lists in its own settings (only the keys of `projects` in ~/.claude.json) or from the working folders the
//! conversation's first lines record.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;

use crate::json::Json;

pub const CLAUDE_CHATS: &str = "Claude 对话";
const HEAD: usize = 256 * 1024;

type Listed = ((PathBuf, Option<std::time::SystemTime>), HashMap<String, String>);

thread_local! {
    static SETTINGS: RefCell<Option<Listed>> = const { RefCell::new(None) };
    static TRANSCRIPTS: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

/// Claude's name for a folder: every character but an ASCII letter or digit becomes '-'.
fn encoded(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

/// A path without the Claude worktree it is in (<repository>/.claude/worktrees/<name>/…).
fn repository(path: &str) -> &str {
    path.find("/.claude/worktrees/").map_or(path, |at| &path[..at])
}

/// CLAUDE_CONFIG_DIR as the runtime has it (config.local.sh's), told when one is made: opened from Finder or at login,
/// the process's own environment doesn't hold it (R9-1). The process's own until a runtime tells it.
static CONFIG_DIR: std::sync::RwLock<Option<Option<String>>> = std::sync::RwLock::new(None);

/// The runtime's environment, for the folder Claude Code's settings are in.
pub fn use_environment(environment: &HashMap<String, String>) {
    *CONFIG_DIR.write().unwrap_or_else(|poisoned| poisoned.into_inner()) =
        Some(environment.get("CLAUDE_CONFIG_DIR").cloned());
}

fn configured_dir() -> Option<String> {
    match &*CONFIG_DIR.read().unwrap_or_else(|poisoned| poisoned.into_inner()) {
        Some(dir) => dir.clone(),
        None => std::env::var("CLAUDE_CONFIG_DIR").ok(),
    }
}

/// Claude Code's settings file: in the first folder CLAUDE_CONFIG_DIR names (it may list several, comma separated, and
/// start with ~), else in the home folder (found on Windows too, where HOME is often not set).
fn settings_file(configured: Option<&str>) -> PathBuf {
    match configured.map(str::trim).filter(|dirs| !dirs.is_empty()) {
        Some(dirs) => {
            crate::files::paths::expand_user(dirs.split(',').next().unwrap_or("").trim()).join(".claude.json")
        }
        None => crate::files::paths::home().join(".claude.json"),
    }
}

/// The folders Claude Code has run in, by their written name: the keys of `projects` in its settings, each also under
/// its repository's name when it is a worktree. Read again when the file changes.
fn listed(written: &str) -> Option<String> {
    let file = settings_file(configured_dir().as_deref());
    let stamp = (file.clone(), std::fs::metadata(&file).ok().and_then(|meta| meta.modified().ok()));
    SETTINGS.with(|settings| {
        let mut settings = settings.borrow_mut();
        if settings.as_ref().is_none_or(|(known, _)| *known != stamp) {
            let mut folders = HashMap::new();
            let parsed = std::fs::read(&file).ok().and_then(|data| Json::parse(&data).ok());
            if let Some(projects) = parsed.as_ref().and_then(|value| value.get("projects")).and_then(Json::as_object) {
                for (path, _) in projects.iter() {
                    let path = path.replace('\\', "/");
                    let path = path.trim_end_matches('/');
                    let root = repository(path);
                    folders.entry(encoded(root)).or_insert_with(|| root.to_string());
                    folders.entry(encoded(path)).or_insert_with(|| path.to_string());
                }
            }
            *settings = Some((stamp, folders));
        }
        settings.as_ref().and_then(|(_, folders)| folders.get(written).cloned())
    })
}

/// The written name a conversation folder stands for, without the worktree part; None for the Claude app's
/// conversations without a folder.
fn target(folder: &str) -> Option<&str> {
    if folder.contains("-Library-Application-Support-Claude-scratch-workspaces-") {
        return None;
    }
    Some(folder.find("--claude-worktrees-").map_or(folder, |at| &folder[..at]))
}

/// The project a Claude conversation folder name stands for, when Claude Code's settings list it.
pub fn folder_project(folder: &str) -> Option<String> {
    match target(folder) {
        None => Some(CLAUDE_CHATS.to_string()),
        Some(written) => listed(written).and_then(|path| name_of(&path)),
    }
}

/// The folder of a working path that a conversation folder name stands for: the path itself or one of the folders it
/// is in. The project's name then, without reading anything.
pub fn within(cwd: &str, folder: &str) -> Option<String> {
    let written = target(folder)?;
    let path = cwd.trim().replace('\\', "/");
    let mut end = path.trim_end_matches('/').len();
    loop {
        let prefix = &path[..end];
        if !prefix.is_empty() && encoded(prefix) == written {
            return name_of(prefix);
        }
        end = prefix.rfind('/')?;
    }
}

/// The conversation folder a transcript is filed under (…/projects/<folder>/…).
pub fn transcript_folder(file: &str) -> Option<&str> {
    let parts: Vec<&str> = file.split(['/', '\\']).collect();
    let at = parts.iter().rposition(|part| *part == "projects")?;
    (at + 2 < parts.len()).then(|| parts[at + 1])
}

/// The working folders a transcript's first lines record, in order.
fn first_folders(file: &str) -> Vec<String> {
    let mut data = Vec::new();
    let Ok(handle) = std::fs::File::open(file) else { return vec![] };
    if handle.take(HEAD as u64).read_to_end(&mut data).is_err() {
        return vec![];
    }
    let complete = data.len() < HEAD;
    let mut lines: Vec<&[u8]> = data.split(|byte| *byte == b'\n').collect();
    if !complete {
        lines.pop();
    }
    lines
        .into_iter()
        .filter_map(|line| Json::parse(line).ok())
        .filter_map(|row| row.get("cwd").and_then(Json::as_str).map(String::from))
        .filter(|cwd| !cwd.trim().is_empty())
        .collect()
}

/// The project of a Claude-format transcript, by the folder it is filed under: as Claude Code's settings list it, else
/// as the working folders its first lines record show it; None when neither says.
pub fn transcript_project(file: &str) -> Option<String> {
    if let Some(known) = TRANSCRIPTS.with(|known| known.borrow().get(file).cloned()) {
        return Some(known);
    }
    let folder = transcript_folder(file)?;
    let found = folder_project(folder).or_else(|| first_folders(file).iter().find_map(|cwd| within(cwd, folder)))?;
    TRANSCRIPTS.with(|known| known.borrow_mut().insert(file.to_string(), found.clone()));
    Some(found)
}

/// The project of a folder a request ran in: the folder's last name, except a Claude worktree's repository, a Claude
/// scratchpad's conversation folder, and Claude 对话 for the Claude app's scratch workspaces.
pub fn name_of(path: &str) -> Option<String> {
    let path = path.trim().replace('\\', "/");
    let path = repository(path.trim_end_matches('/'));
    if path.contains("/Library/Application Support/Claude/scratch-workspaces/") {
        return Some(CLAUDE_CHATS.to_string());
    }
    if let Some(folder) = scratchpad(path) {
        return folder_project(folder);
    }
    let last = path.rsplit('/').next().unwrap_or("");
    (!last.is_empty()).then(|| last.to_string())
}

/// /tmp/claude-<uid>/<conversation folder>/…: where Claude keeps a conversation's scratch files.
fn scratchpad(path: &str) -> Option<&str> {
    let rest = path.strip_prefix("/private").unwrap_or(path).strip_prefix("/tmp/claude-")?;
    let (uid, rest) = rest.split_once('/')?;
    if uid.is_empty() || !uid.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    rest.split('/').next().filter(|folder| !folder.is_empty())
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_settings_file_is_in_the_first_configured_folder_with_its_tilde_expanded() {
        let home = crate::files::paths::home();
        assert_eq!(super::settings_file(None), home.join(".claude.json"));
        assert_eq!(super::settings_file(Some("  ")), home.join(".claude.json"));
        assert_eq!(super::settings_file(Some("~/.claude-work, /other")), home.join(".claude-work/.claude.json"));
        assert_eq!(super::settings_file(Some("/opt/claude")), std::path::PathBuf::from("/opt/claude/.claude.json"));
    }

    use super::*;

    #[test]
    fn folders_name_their_project_without_being_read() {
        let folder = "-Users-me-Documents-my-app-v2";
        // The working folders tell the written name apart: "my app.v2", not my/app/v2.
        assert_eq!(within("/Users/me/Documents/my app.v2/src/ui", folder).as_deref(), Some("my app.v2"));
        assert_eq!(within("/Users/me/Documents/other", folder), None);
        // A worktree, there or already removed, is its repository's.
        let worktree = "-Users-me-repo--claude-worktrees-fix-a1b2c3";
        assert_eq!(within("/Users/me/repo/.claude/worktrees/fix-a1b2c3/app", worktree).as_deref(), Some("repo"));
        assert_eq!(
            folder_project("-Users-me-Library-Application-Support-Claude-scratch-workspaces-a-b").as_deref(),
            Some(CLAUDE_CHATS)
        );
        assert_eq!(transcript_folder("/h/.claude/projects/-Users-me-x/abc.jsonl"), Some("-Users-me-x"));
        assert_eq!(transcript_folder("/h/.claude/projects/-Users-me-x/abc/subagents/a.jsonl"), Some("-Users-me-x"));
        assert_eq!(transcript_folder("/x/projects/abc.jsonl"), None);
        assert_eq!(name_of("/r/.claude/worktrees/x/app").as_deref(), Some("r"));
        assert_eq!(
            name_of("/Users/me/Library/Application Support/Claude/scratch-workspaces/a/b/scratch-1").as_deref(),
            Some(CLAUDE_CHATS)
        );
        assert_eq!(name_of("C:\\Users\\me\\proj\\").as_deref(), Some("proj"));
        assert_eq!(name_of(" "), None);
    }
}
