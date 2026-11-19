//! Active coding sessions from Claude Code transcripts and Codex rollouts: which are running, which wait for the
//! user, how full their context is. Only status, title, folder name, model and counts are kept.

use std::collections::HashMap;
use std::path::Path;

use crate::json::Json;

/// Trimmed text, or None when it is empty or not text.
pub fn text(value: Option<&Json>) -> Option<String> {
    value.and_then(Json::as_str).map(str::trim).filter(|text| !text.is_empty()).map(String::from)
}

/// Codex's thread names by thread ID, from its session index; the last name given wins.
pub fn codex_titles(root: &Path) -> HashMap<String, String> {
    let Ok(data) = std::fs::read(root.join("session_index.jsonl")) else { return HashMap::new() };
    let mut names = HashMap::new();
    for line in data.split(|&byte| byte == b'\n').filter(|line| !line.is_empty()) {
        let Ok(row) = Json::parse(line) else { continue };
        let (Some(id), Some(name)) = (row.get("id").and_then(Json::as_str), text(row.get("thread_name"))) else {
            continue;
        };
        names.insert(id.to_string(), name);
    }
    names
}
