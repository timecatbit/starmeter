//! Claude Code and Codex sessions running or waiting for input now. Only the end of each transcript changed in the
//! last half hour is read. Of a session nothing is kept but its state, the title its client gave it, its project
//! folder's name, model, reasoning effort, how full its context is, and when it last changed; never prompts, replies
//! or paths.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use jiff::tz::TimeZone;

use crate::claude_usage::{self, state_of};
use crate::json::{Json, Object, Selection};
use crate::py::{self, FileError, ReadError};
use crate::runtime::{Loaded, Runtime, now};
use crate::tally::add;
use crate::time::IsoTime;
use crate::{claude_app, object, pricing};

pub const ACTIVE: f64 = 1800.0;
const STALLED: f64 = 600.0;
const TAIL: usize = 512 * 1024;
const DETAIL: usize = 2 * 1024 * 1024;
const TURNS: usize = 8;
const CLAUDE_WINDOWS: [f64; 2] = [200_000.0, 1_000_000.0];

/// A request's estimated cost for session detail: the model and the usage fields; None when unpriced.
pub type Price = Box<dyn Fn(Option<&Json>, &Object) -> Option<f64> + Send + Sync>;

/// Trimmed text, or None when it is empty or not text.
pub fn text(value: Option<&Json>) -> Option<String> {
    value.and_then(Json::as_str).map(str::trim).filter(|text| !text.is_empty()).map(String::from)
}

/// The value when it is a non-negative number (not a bool), else the integer 0.
fn number(value: Option<&Json>) -> Json {
    match value {
        Some(value) if value.as_bool().is_none() && value.number().is_some_and(|number| number >= 0.0) => value.clone(),
        _ => Json::Int(0),
    }
}

/// The JSON objects among the lines in the last `size` bytes of a file; the first is dropped when it was cut.
fn rows_at_end(path: &str, size: usize) -> Result<Vec<Object>, FileError> {
    let unreadable = || FileError::of_kind("unreadable", "OSError");
    let mut file = File::open(path).map_err(|_| unreadable())?;
    let end = file.seek(SeekFrom::End(0)).map_err(|_| unreadable())?;
    file.seek(SeekFrom::Start(end.saturating_sub(size as u64))).map_err(|_| unreadable())?;
    let mut data = Vec::new();
    file.read_to_end(&mut data).map_err(|_| unreadable())?;
    let mut lines: Vec<&[u8]> = data.split(|&byte| byte == b'\n').collect();
    if end > size as u64 && !lines.is_empty() {
        lines.remove(0);
    }
    Ok(lines
        .into_iter()
        .filter_map(|line| Json::parse_selected(line, &Selection::All, true).ok())
        .filter_map(|value| value.as_object().cloned())
        .collect())
}

fn first_row(path: &str) -> Result<Json, FileError> {
    let mut file = File::open(path).map_err(|_| FileError::of_kind("unreadable", "OSError"))?;
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    while line.len() < TAIL && file.read(&mut byte).unwrap_or(0) == 1 {
        line.push(byte[0]);
        if byte[0] == b'\n' {
            break;
        }
    }
    Ok(Json::parse(&line).unwrap_or(Json::Object(Object::new())))
}

fn kind_is(row: &Object, kind: &str) -> bool {
    row.get("type") == Some(&Json::from(kind))
}

/// A Claude Code transcript: waiting once the last reply ended its turn, running otherwise. Its title is the last
/// one the transcript records: the user's or the App's, else the one Claude Code made up.
fn claude(path: &str) -> Result<Option<Object>, FileError> {
    let (mut last, mut reply, mut folder) = (None::<Object>, None::<Object>, None::<Json>);
    let (mut named, mut made) = (None::<String>, None::<String>);
    for row in rows_at_end(path, TAIL)? {
        if kind_is(&row, "custom-title") {
            named = text(row.get("customTitle")).or(named);
        } else if kind_is(&row, "ai-title") {
            made = text(row.get("aiTitle")).or(made);
        }
        if row.get("isSidechain").is_some_and(Json::is_truthy) {
            continue;
        }
        if let Some(cwd) = row.get("cwd").filter(|cwd| cwd.is_truthy()) {
            folder = Some(cwd.clone());
        }
        if kind_is(&row, "assistant") || kind_is(&row, "user") {
            last = Some(row.clone());
        }
        let message = row.get("message").and_then(Json::as_object).cloned().unwrap_or_default();
        if kind_is(&row, "assistant") && message.get("usage").and_then(Json::as_object).is_some() {
            reply = Some(message);
        }
    }
    let Some(last) = last else { return Ok(None) };
    let message = last.get("message").and_then(Json::as_object).cloned().unwrap_or_default();
    let stop = message.get("stop_reason").and_then(Json::as_str);
    let waiting = kind_is(&last, "assistant")
        && stop.is_some_and(|stop| ["end_turn", "stop_sequence", "max_tokens", "refusal"].contains(&stop));
    let (mut context, mut model) = (Json::Null, Json::Null);
    if let Some(reply) = reply
        && let Some(usage) = reply.get("usage").and_then(Json::as_object)
    {
        model = reply.get("model").cloned().unwrap_or(Json::Null);
        let used = ["input_tokens", "cache_read_input_tokens", "cache_creation_input_tokens", "output_tokens"]
            .iter()
            .fold(Json::Int(0), |sum, key| add(&sum, &number(usage.get(key))));
        let total = used.number().unwrap_or(0.0);
        let window = CLAUDE_WINDOWS.iter().copied().find(|window| total <= *window).unwrap_or(CLAUDE_WINDOWS[1]);
        context = if total != 0.0 { Json::Double((total / window).min(1.0)) } else { Json::Null };
    }
    Ok(Some(object! {
        "tool" => "Claude", "state" => if waiting { "waiting" } else { "running" }, "title" => named.or(made),
        "project" => claude_project(path, folder.as_ref()), "model" => model, "effort" => Json::Null, "context" => context,
    }))
}

/// A Codex rollout: running between task_started and task_complete (or an aborted turn). A subagent's thread
/// (Guardian reviews among them) works for another session and is left out.
fn codex(path: &str) -> Result<Option<Object>, ReadError> {
    let head = first_row(path)?;
    let head = head.as_object().ok_or(ReadError::Line)?;
    let payload = head.get("payload").and_then(Json::as_object).cloned().unwrap_or_default();
    if payload.get("source").and_then(Json::as_object).is_some_and(|source| source.contains_key("subagent")) {
        return Ok(None);
    }
    let mut folder = if kind_is(head, "session_meta") { payload.get("cwd").cloned() } else { None };
    let (mut state, mut model, mut context, mut effort) = (None::<&str>, Json::Null, Json::Null, None::<String>);
    for row in rows_at_end(path, TAIL)? {
        let data = row.get("payload").and_then(Json::as_object).cloned().unwrap_or_default();
        let kind = data.get("type").and_then(Json::as_str);
        if kind_is(&row, "turn_context") {
            model = py::or(data.get("model"), model);
            effort = text(data.get("effort")).or(effort);
        }
        if kind_is(&row, "session_meta")
            && let Some(cwd) = data.get("cwd").filter(|cwd| cwd.is_truthy())
        {
            folder = Some(cwd.clone());
        }
        match kind {
            Some("task_started") => state = Some("running"),
            Some("task_complete" | "turn_aborted") => state = Some("waiting"),
            Some("token_count") => {
                if let Some(info) = data.get("info").and_then(Json::as_object) {
                    let last = py::or(info.get("last_token_usage"), Json::Object(Object::new()))
                        .as_object()
                        .cloned()
                        .unwrap_or_default();
                    let window = number(info.get("model_context_window")).number().unwrap_or(0.0);
                    let used = add(&number(last.get("input_tokens")), &number(last.get("output_tokens")))
                        .number()
                        .unwrap_or(0.0);
                    if window != 0.0 && used != 0.0 {
                        context = Json::Double((used / window).min(1.0));
                    }
                }
            }
            _ => {}
        }
    }
    let Some(state) = state else { return Ok(None) };
    Ok(Some(object! {
        "tool" => "ChatGPT", "state" => state, "title" => Json::Null, "project" => project_name(folder.as_ref()),
        "model" => model, "effort" => effort, "context" => context,
    }))
}

/// The project's name (see projects); none for the folders the ChatGPT app makes for a chat or a project, named by
/// date and ID, or the Claude app's conversations without a folder (the session already shows its title).
pub fn project_name(folder: Option<&Json>) -> Json {
    if let Some(path) = folder.and_then(Json::as_str) {
        let trimmed = path.trim().trim_end_matches(['/', '\\']).replace('\\', "/");
        if app_folder(&trimmed) {
            return Json::Null;
        }
    }
    chat_free(py::project_name(folder))
}

/// A Claude conversation's project: the folder Claude files it under, else the folder it last worked in.
fn claude_project(path: &str, folder: Option<&Json>) -> Json {
    match crate::projects::transcript_project(path) {
        Some(name) => chat_free(Json::from(name)),
        None => project_name(folder),
    }
}

fn chat_free(name: Json) -> Json {
    if name.as_str() == Some(crate::projects::CLAUDE_CHATS) { Json::Null } else { name }
}

/// …/Codex/YYYY-MM-DD/<name> or …/.chatgpt-projects/g-p-<hex>: folders the ChatGPT app makes.
fn app_folder(path: &str) -> bool {
    let Some((head, last)) = path.rsplit_once('/') else { return false };
    if let Some(hex) = last.strip_prefix("g-p-")
        && !hex.is_empty()
        && hex.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        && head.ends_with("/.chatgpt-projects")
    {
        return true;
    }
    let Some((head, date)) = head.rsplit_once('/') else { return false };
    let day = date.as_bytes();
    !last.is_empty()
        && day.len() == 10
        && day
            .iter()
            .enumerate()
            .all(|(index, byte)| if index == 4 || index == 7 { *byte == b'-' } else { byte.is_ascii_digit() })
        && head.ends_with("/Codex")
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

fn stamp(value: Option<&Json>) -> Json {
    value
        .map(|value| py::str(Some(value)))
        .and_then(|text| IsoTime::parse(&text).ok())
        .map_or(Json::Null, |time| Json::Double(time.timestamp(&TimeZone::system())))
}

fn turn(at: Json) -> Object {
    object! { "at" => at, "requests" => 0, "tokens" => 0, "cost" => 0.0, "unpriced" => 0, "tools" => Object::new() }
}

fn bump(turn: &mut Object, key: &str, amount: Json) {
    let current = turn.get(key).cloned().unwrap_or(Json::Int(0));
    turn.insert(key, add(&current, &amount));
}

fn count_tool(turn: &mut Object, name: &str) {
    let mut tools = turn.get("tools").and_then(Json::as_object).cloned().unwrap_or_default();
    let current = tools.get(name).cloned().unwrap_or(Json::Int(0));
    tools.insert(name, add(&current, &Json::Int(1)));
    turn.insert("tools", tools);
}

fn last_turns(turns: Vec<Object>) -> Vec<Json> {
    let kept: Vec<Object> = turns
        .into_iter()
        .filter(|turn| !turn.get("at").is_none_or(Json::is_null) || turn.get("requests").is_some_and(Json::is_truthy))
        .collect();
    let start = kept.len().saturating_sub(TURNS);
    kept[start..].iter().rev().cloned().map(Json::Object).collect()
}

/// The last turns of a Claude Code session, newest first: from one prompt to the next. A reply streamed over several
/// lines counts once; a subagent's work belongs to the turn that started it.
fn claude_turns(path: &str, price: &Price) -> Result<Vec<Json>, FileError> {
    let mut turns = vec![turn(Json::Null)];
    let mut seen: HashSet<String> = HashSet::new();
    for row in rows_at_end(path, DETAIL)? {
        let message = row.get("message").and_then(Json::as_object).cloned().unwrap_or_default();
        let content = message.get("content");
        if kind_is(&row, "user")
            && !row.get("isSidechain").is_some_and(Json::is_truthy)
            && !row.get("isMeta").is_some_and(Json::is_truthy)
        {
            let kinds: HashSet<String> = content
                .and_then(Json::as_array)
                .into_iter()
                .flatten()
                .filter_map(|item| item.get("type").map(py::dumps))
                .collect();
            let spoken = content.and_then(Json::as_str).is_some_and(|text| !text.trim().is_empty());
            if spoken || (!kinds.is_empty() && !kinds.contains(&py::dumps(&Json::from("tool_result")))) {
                turns.push(turn(stamp(row.get("timestamp"))));
            }
        } else if kind_is(&row, "assistant") {
            for item in content.and_then(Json::as_array).into_iter().flatten() {
                if let Some(fields) = item.as_object()
                    && fields.get("type") == Some(&Json::from("tool_use"))
                    && let Some(name) = fields.get("name").filter(|name| name.is_truthy())
                {
                    count_tool(turns.last_mut().expect("a turn"), &py::str(Some(name)));
                }
            }
            let key = py::dumps(&py::or(message.get("id"), row.get("uuid").cloned().unwrap_or(Json::Null)));
            if let Some(usage) = message.get("usage").and_then(Json::as_object)
                && !seen.contains(&key)
            {
                seen.insert(key);
                let current = turns.last_mut().expect("a turn");
                bump(current, "requests", Json::Int(1));
                let tokens =
                    ["input_tokens", "cache_read_input_tokens", "cache_creation_input_tokens", "output_tokens"]
                        .iter()
                        .fold(Json::Int(0), |sum, key| add(&sum, &number(usage.get(key))));
                bump(current, "tokens", tokens);
                match price(message.get("model"), usage) {
                    Some(cost) => bump(current, "cost", Json::Double(cost)),
                    None => bump(current, "unpriced", Json::Int(1)),
                }
            }
        }
    }
    Ok(last_turns(turns))
}

/// The last turns of a Codex session, newest first: from task_started to task_complete; each change of the running
/// token total is one request.
fn codex_turns(path: &str, price: &Price) -> Result<Vec<Json>, FileError> {
    let mut turns = vec![turn(Json::Null)];
    let mut model = Json::Null;
    let mut total: Option<Option<Json>> = None;
    for row in rows_at_end(path, DETAIL)? {
        let data = row.get("payload").and_then(Json::as_object).cloned().unwrap_or_default();
        let kind = data.get("type").and_then(Json::as_str);
        if kind_is(&row, "turn_context") {
            model = py::or(data.get("model"), model);
        }
        if kind == Some("task_started") {
            turns.push(turn(stamp(row.get("timestamp"))));
        } else if kind_is(&row, "response_item")
            && let Some(kind) = kind.filter(|kind| {
                ["function_call", "custom_tool_call", "local_shell_call", "web_search_call"].contains(kind)
            })
        {
            let fallback = match kind {
                "local_shell_call" => Some("shell".to_string()),
                "web_search_call" => Some("web_search".to_string()),
                _ => None,
            };
            let name = data.get("name").filter(|name| name.is_truthy()).map(|name| py::str(Some(name))).or(fallback);
            if let Some(name) = name {
                count_tool(turns.last_mut().expect("a turn"), &name);
            }
        } else if kind == Some("token_count")
            && let Some(info) = data.get("info").and_then(Json::as_object)
        {
            let running = info.get("total_token_usage").cloned();
            let last = py::or(info.get("last_token_usage"), Json::Object(Object::new()))
                .as_object()
                .cloned()
                .unwrap_or_default();
            if last.is_empty() || total.as_ref() == Some(&running) {
                continue;
            }
            total = Some(running);
            let current = turns.last_mut().expect("a turn");
            bump(current, "requests", Json::Int(1));
            bump(current, "tokens", add(&number(last.get("input_tokens")), &number(last.get("output_tokens"))));
            match price(Some(&model), &last) {
                Some(cost) => bump(current, "cost", Json::Double(cost)),
                None => bump(current, "unpriced", Json::Int(1)),
            }
        }
    }
    Ok(last_turns(turns))
}

/// How many of a Claude session's last reply IDs are kept between readings, so a reply streamed across the point where
/// one reading stopped still counts once.
const SEEN_KEPT: usize = 256;

/// What a whole session has cost: every request in its transcript, counted as the turns count them (a Claude reply
/// streamed over several lines once; each change of Codex's running token total once). With `kept`, the tally of the
/// last reading (by inode and the offset it stopped at), only what was written since is read: a long running session
/// isn't read whole every half minute. Gives the cost and the tally to keep.
fn spent(path: &str, tool: &str, price: &Price, kept: Option<&Object>) -> Result<(Json, Object), FileError> {
    let unreadable = || FileError::of_kind("unreadable", "OSError");
    let state = state_of(Path::new(path)).ok_or_else(unreadable)?;
    let resumed = kept.filter(|kept| {
        kept.get("inode").and_then(Json::integer) == Some(state.inode)
            && kept.get("offset").and_then(Json::integer).is_some_and(|offset| offset <= state.size)
    });
    let field = |name: &str| resumed.and_then(|kept| kept.get(name)).cloned();
    let mut requests = field("requests").and_then(|value| value.integer()).unwrap_or(0);
    let mut cost = field("cost").and_then(|value| value.python_float()).unwrap_or(0.0);
    let mut unpriced = field("unpriced").and_then(|value| value.integer()).unwrap_or(0);
    let mut seen: Vec<String> = field("seen")
        .and_then(|value| value.as_array().cloned())
        .map(|keys| keys.iter().filter_map(|key| key.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let mut model = field("model").unwrap_or(Json::Null);
    let mut total: Option<Option<Json>> = field("total").map(|total| total.get("value").cloned());
    let start = field("offset").and_then(|value| value.integer()).unwrap_or(0) as u64;

    let mut file = File::open(path).map_err(|_| unreadable())?;
    file.seek(SeekFrom::Start(start)).map_err(|_| unreadable())?;
    let mut data = Vec::new();
    file.read_to_end(&mut data).map_err(|_| unreadable())?;
    // Complete lines only: one being written is read next time.
    let complete = data.iter().rposition(|&byte| byte == b'\n').map_or(0, |at| at + 1);
    let mut known: HashSet<String> = seen.iter().cloned().collect();
    let mut add_request = |model: Option<&Json>, usage: &Object| {
        requests += 1;
        match price(model, usage) {
            Some(spent) => cost += spent,
            None => unpriced += 1,
        }
    };
    for line in data[..complete].split(|&byte| byte == b'\n') {
        let Some(row) =
            Json::parse_selected(line, &Selection::All, true).ok().and_then(|value| value.as_object().cloned())
        else {
            continue;
        };
        if tool == "Claude" {
            if !kind_is(&row, "assistant") {
                continue;
            }
            let message = row.get("message").and_then(Json::as_object).cloned().unwrap_or_default();
            let key = py::dumps(&py::or(message.get("id"), row.get("uuid").cloned().unwrap_or(Json::Null)));
            if let Some(usage) = message.get("usage").and_then(Json::as_object)
                && !known.contains(&key)
            {
                known.insert(key.clone());
                seen.push(key);
                add_request(message.get("model"), usage);
            }
        } else {
            let data = row.get("payload").and_then(Json::as_object).cloned().unwrap_or_default();
            if kind_is(&row, "turn_context") {
                model = py::or(data.get("model"), model);
            }
            if data.get("type").and_then(Json::as_str) == Some("token_count")
                && let Some(info) = data.get("info").and_then(Json::as_object)
            {
                let running = info.get("total_token_usage").cloned();
                let last = py::or(info.get("last_token_usage"), Json::Object(Object::new()))
                    .as_object()
                    .cloned()
                    .unwrap_or_default();
                if last.is_empty() || total.as_ref() == Some(&running) {
                    continue;
                }
                total = Some(running);
                add_request(Some(&model), &last);
            }
        }
    }
    let cut = seen.len().saturating_sub(SEEN_KEPT);
    seen.drain(..cut);
    let tally = object! {
        "inode" => state.inode, "offset" => (start + complete as u64) as i64, "requests" => requests,
        "cost" => cost, "unpriced" => unpriced, "seen" => Json::Array(seen.into_iter().map(Json::from).collect()),
        "model" => model, "total" => total.map_or(Json::Null, |value| object! { "value" => value.unwrap_or(Json::Null) }.into()),
    };
    Ok((object! { "requests" => requests, "cost" => cost, "unpriced" => unpriced }.into(), tally))
}

fn recent(root: &str, depth: usize, since: f64, matching: &dyn Fn(&str) -> bool) -> Vec<String> {
    fn visit(
        folder: &str,
        level: usize,
        depth: usize,
        since: f64,
        matching: &dyn Fn(&str) -> bool,
        found: &mut Vec<String>,
    ) {
        let mut names: Vec<String> = std::fs::read_dir(folder)
            .map(|entries| {
                entries.filter_map(Result::ok).map(|entry| entry.file_name().to_string_lossy().into_owned()).collect()
            })
            .unwrap_or_default();
        names.sort();
        for name in names {
            let path = format!("{folder}/{name}");
            if level < depth {
                if Path::new(&path).is_dir() {
                    visit(&path, level + 1, depth, since, matching, found);
                }
            } else if matching(&name)
                && !Path::new(&path).is_dir()
                && state_of(Path::new(&path)).is_some_and(|state| state.mtime as f64 / 1e9 >= since)
            {
                found.push(path);
            }
        }
    }
    let mut found = Vec::new();
    visit(root, 0, depth, since, matching, &mut found);
    found
}

/// Sessions changed in the last half hour, the running ones first, then the most recent; a running one silent for ten
/// minutes is dropped. With prices, each carries its last turns and what the whole session has cost; `tallies`, kept
/// between readings by session, lets the cost be read from where the last reading stopped.
pub fn active(
    claude_roots: &[String],
    codex_root: Option<&str>,
    now: f64,
    prices: Option<&HashMap<&str, Price>>,
    claude_app: &HashMap<String, (Option<String>, Option<String>)>,
    tallies: Option<&mut Object>,
) -> Vec<Json> {
    let previous = tallies.as_deref().cloned().unwrap_or_default();
    let mut kept = Object::new();
    let mut candidates: Vec<(&str, String)> = Vec::new();
    for root in claude_roots {
        for path in recent(root, 1, now - ACTIVE, &|name: &str| name.ends_with(".jsonl")) {
            if !path.split('/').any(|part| part == "subagents") {
                candidates.push(("Claude", path));
            }
        }
    }
    if let Some(codex_root) = codex_root {
        // A rollout is filed under the day its thread started, and a thread can still be going days later.
        let matching = |name: &str| name.starts_with("rollout-") && name.ends_with(".jsonl");
        for path in recent(&format!("{codex_root}/sessions"), 3, now - ACTIVE, &matching) {
            candidates.push(("ChatGPT", path));
        }
    }
    let mut titles: Option<HashMap<String, String>> = None;
    let mut found: Vec<(f64, Object)> = Vec::new();
    for (tool, path) in candidates {
        let Some(state) = state_of(Path::new(&path)) else { continue };
        let changed = state.mtime as f64 / 1e9;
        let read = if tool == "Claude" { claude(&path).ok().flatten() } else { codex(&path).ok().flatten() };
        let Some(mut session) = read else { continue };
        if session.get("state") == Some(&Json::from("running")) && now - changed > STALLED {
            continue;
        }
        let stem = Path::new(&path).file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
        if tool == "Claude" {
            let app = claude_app.get(&stem);
            if session.get("title").is_none_or(Json::is_null) {
                session.insert("title", app.and_then(|(title, _)| title.clone()));
            }
            session.insert("effort", app.and_then(|(_, effort)| effort.clone()));
        } else {
            let names = titles.get_or_insert_with(|| codex_titles(Path::new(codex_root.unwrap_or(""))));
            let tail: String = stem.chars().rev().take(36).collect::<Vec<_>>().into_iter().rev().collect();
            session.insert("title", names.get(&tail).cloned());
        }
        let id = py::sha256_hex(path.as_bytes())[..16].to_string();
        session.insert("id", id.as_str());
        session.insert("at", changed);
        // The turns and the whole session's cost are read here, so the path itself never leaves this function.
        if let Some(price) = prices.and_then(|prices| prices.get(tool)) {
            let turns = if tool == "Claude" { claude_turns(&path, price) } else { codex_turns(&path, price) };
            session.insert("turns", Json::Array(turns.unwrap_or_default()));
            match spent(&path, tool, price, previous.get(&id).and_then(Json::as_object)) {
                Ok((cost, tally)) => {
                    session.insert("spent", cost);
                    kept.insert(id.as_str(), tally);
                }
                Err(_) => session.insert("spent", Json::Null),
            }
        }
        found.push((changed, session));
    }
    if let Some(tallies) = tallies {
        *tallies = kept;
    }
    // The running ones first, then the most recent.
    let running = |session: &Object| session.get("state") == Some(&Json::from("running"));
    let mut indexed: Vec<(usize, (f64, Object))> = found.into_iter().enumerate().collect();
    indexed.sort_by(|(a_index, (a, a_session)), (b_index, (b, b_session))| {
        running(b_session).cmp(&running(a_session)).then(b.total_cmp(a)).then(a_index.cmp(b_index))
    });
    indexed.into_iter().map(|(_, (_, session))| Json::Object(session)).collect()
}

/// Sessions that were running at the last read and have just finished their turn.
pub fn newly_waiting(previous: &[Json], current: &[Json], now: f64, recent: f64) -> Vec<Object> {
    let mut before: HashMap<String, Json> = HashMap::new();
    for session in previous {
        if let Some(fields) = session.as_object()
            && let Some(id) = fields.get("id")
        {
            before.insert(py::dumps(id), fields.get("state").cloned().unwrap_or(Json::Null));
        }
    }
    current
        .iter()
        .filter_map(Json::as_object)
        .filter(|session| {
            session.get("state") == Some(&Json::from("waiting"))
                && before.get(&py::dumps(session.get("id").unwrap_or(&Json::Null))) == Some(&Json::from("running"))
                && now - session.get("at").and_then(Json::number).unwrap_or(0.0) <= recent
        })
        .cloned()
        .collect()
}

impl Runtime {
    /// A request's estimated cost for session detail, by tool, as the cards price it; None when unpriced.
    pub fn session_prices(&self) -> HashMap<&'static str, Price> {
        let claude: Price = Box::new(|model, usage| {
            let metrics = claude_usage::normalize(Some(&Json::Object(usage.clone()))).ok()?;
            let metrics = metrics.map(|count| count as f64);
            let speed = usage.get("speed").and_then(Json::as_str).unwrap_or("standard");
            pricing::claude::cost(&py::str(Some(&py::or(model, Json::from("")))), &metrics, speed, "global", "standard")
        });
        let chatgpt: Price = Box::new(|model, usage| {
            let inputs = py::number(usage.get("input_tokens"));
            let cached = py::number(usage.get("cached_input_tokens"));
            let cached =
                if cached.number().unwrap_or(0.0) < inputs.number().unwrap_or(0.0) { cached } else { inputs.clone() };
            let fresh = match (&inputs, &cached) {
                (Json::Int(x), Json::Int(y)) => Json::Int(x - y),
                _ => Json::Double(inputs.number().unwrap_or(0.0) - cached.number().unwrap_or(0.0)),
            };
            let event = object! {
                "model" => py::str(Some(&py::or(model, Json::from("未知模型")))),
                "input" => py::number(Some(&fresh)), "output" => py::number(usage.get("output_tokens")),
                "cached" => py::number(Some(&cached)), "writes" => py::number(Some(&Json::Int(0))), "cost" => Json::Null,
            };
            pricing::codex_cost(&event)
        });
        HashMap::from([("Claude", claude), ("ChatGPT", chatgpt)])
    }

    /// Claude Code and Codex sessions running or waiting now; with the alert on, one that has just finished its turn
    /// is told of.
    pub fn sessions_cached(&self, ttl: Option<f64>) -> Loaded {
        let previous = self
            .read_cache("sessions")
            .get("payload")
            .and_then(|payload| payload.get("sessions"))
            .and_then(Json::as_array)
            .cloned()
            .unwrap_or_default();
        let ttl = ttl.unwrap_or_else(|| self.refresh_ttl(30.0));
        self.cached_fetch("sessions", ttl, 60.0, || {
            let now = now();
            let roots = claude_app::project_roots(&self.home, &self.environment);
            let app = claude_app::session_settings(&self.source_path("claude"), now - ACTIVE);
            let prices = self.session_prices();
            let codex_root = self.codex_home_absolute();
            // Each session's cost read so far, by session, kept beside the sessions to go on from.
            let mut tallies =
                self.read_cache("session-spent").get("sessions").and_then(Json::as_object).cloned().unwrap_or_default();
            let found = active(&roots, Some(&codex_root), now, Some(&prices), &app, Some(&mut tallies));
            self.write_json_cache(
                &self.cache_path("session-spent"),
                &object! { "schema" => 1, "sessions" => tallies }.into(),
            );
            if self.settings().get("sessionAlerts").is_some_and(Json::is_truthy) {
                for session in newly_waiting(&previous, &found, now, 180.0).into_iter().take(3) {
                    let name = session
                        .get("title")
                        .filter(|title| title.is_truthy())
                        .or(session.get("project").filter(|project| project.is_truthy()))
                        .map_or_else(|| "未知项目".to_string(), |value| py::str(Some(value)));
                    let tool = py::str(Some(session.get("tool").unwrap_or(&Json::Null)));
                    self.post_notice("会话完成", &format!("{tool} · {name} 这一轮已完成"));
                }
            }
            Ok((Some(object! { "sessions" => Json::Array(found) }), None))
        })
    }

    /// Codex's home as the sessions and ChatGPT account read it: OCX_CHATGPT_HOME, CODEX_HOME or ~/.codex, absolute.
    pub fn codex_home_absolute(&self) -> String {
        let given = self
            .env("OCX_CHATGPT_HOME")
            .or(self.env("CODEX_HOME"))
            .map(String::from)
            .unwrap_or_else(|| format!("{}/.codex", self.home));
        crate::runtime::absolute(&crate::files::paths::expand_user(&given).to_string_lossy())
    }
}

#[cfg(test)]
mod tests {
    use super::{Price, spent};
    use crate::json::Json;

    fn line(id: &str, output: i64) -> String {
        format!(
            "{{\"type\":\"assistant\",\"message\":{{\"id\":\"{id}\",\"model\":\"m\",\"usage\":{{\"input_tokens\":10,\"output_tokens\":{output}}}}}}}\n"
        )
    }

    #[test]
    fn a_session_read_on_from_where_it_stopped_costs_what_a_whole_reading_does() {
        let folder = std::env::temp_dir().join(format!("starmeter-spent-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("session.jsonl");
        let price: Price = Box::new(|_, usage| Some(usage.get("output_tokens").and_then(Json::integer)? as f64 * 0.001));
        // A reply streamed over two lines, a second reply, and a line still being written.
        std::fs::write(&path, format!("{}{}{}{{\"type\":\"assist", line("a", 5), line("a", 5), line("b", 7))).unwrap();
        let path = path.to_string_lossy().into_owned();
        let (first, tally) = spent(&path, "Claude", &price, None).unwrap();
        assert_eq!(first.get("requests").and_then(Json::integer), Some(2));
        // The half-written line finished, the streamed reply "b" continued across the stop, and a new reply.
        let mut whole = std::fs::read_to_string(&path).unwrap();
        whole.truncate(whole.rfind('\n').unwrap() + 1);
        whole += &line("b", 7);
        whole += &line("c", 11);
        std::fs::write(&path, &whole).unwrap();
        let (resumed, _) = spent(&path, "Claude", &price, Some(&tally)).unwrap();
        let (fresh, _) = spent(&path, "Claude", &price, None).unwrap();
        assert_eq!(resumed.serialized(), fresh.serialized());
        assert_eq!(fresh.get("requests").and_then(Json::integer), Some(3));
        let _ = std::fs::remove_dir_all(&folder);
    }
}
