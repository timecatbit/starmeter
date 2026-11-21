//! Claude Code and Codex sessions running or idle now. Only the end of each transcript changed in the last day is
//! read, and only when it has changed since the last reading. Of a session nothing is kept but its state, the title its client gave it, its project
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

/// A session stays listed a day after its last change.
pub const ACTIVE: f64 = 86400.0;
/// A running session silent this long has most likely stopped without saying so: it is idle.
const STALLED: f64 = 600.0;
const TAIL: usize = 512 * 1024;
/// How far back a session's state is looked for when the usual tail holds none (a long tool output can fill it).
const FAR: usize = 16 * 1024 * 1024;
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

/// The complete JSON lines at a file's end: the usual tail, else twice as far back each time (up to FAR) until `enough`
/// finds what it needs among them. A long tool output can push a turn's start, or every whole line, out of the tail.
fn rows_back(path: &str, first: usize, enough: impl Fn(&[Object]) -> bool) -> Result<Vec<Object>, FileError> {
    let length = std::fs::metadata(path).map_err(|_| FileError::of_kind("unreadable", "OSError"))?.len();
    let mut size = first;
    loop {
        let rows = rows_at_end(path, size)?;
        if enough(&rows) || size as u64 >= length || size >= FAR {
            return Ok(rows);
        }
        size *= 2;
    }
}

/// The complete JSON lines in the first `size` bytes of a file, in order.
fn rows_at_start(path: &str, size: usize) -> Result<Vec<Object>, FileError> {
    let file = File::open(path).map_err(|_| FileError::of_kind("unreadable", "OSError"))?;
    let mut data = Vec::new();
    file.take(size as u64).read_to_end(&mut data).map_err(|_| FileError::of_kind("unreadable", "OSError"))?;
    let mut lines: Vec<&[u8]> = data.split(|&byte| byte == b'\n').collect();
    if data.len() >= size && !lines.is_empty() {
        lines.pop();
    }
    Ok(lines
        .into_iter()
        .filter_map(|line| Json::parse_selected(line, &Selection::All, true).ok())
        .filter_map(|value| value.as_object().cloned())
        .collect())
}

/// A rollout's thread, from its name: rollout-<time>-<thread>.jsonl, or rollout-<time>-<thread>_<segment>.jsonl for a
/// long thread written in segments, each going on from the one before. None for a name of neither shape.
pub fn codex_thread(path: &str) -> Option<String> {
    let stem = Path::new(path).file_stem()?.to_str()?;
    let uuid = |text: &str| {
        text.len() == 36
            && text.bytes().enumerate().all(|(index, byte)| {
                if [8, 13, 18, 23].contains(&index) { byte == b'-' } else { byte.is_ascii_hexdigit() }
            })
    };
    let rest = stem.strip_prefix("rollout-")?;
    if let Some(thread) = rest.get(20..56).filter(|thread| uuid(thread) && rest.as_bytes().get(19) == Some(&b'-')) {
        return Some(thread.to_string());
    }
    stem.get(stem.len().checked_sub(36)?..).filter(|thread| uuid(thread)).map(String::from)
}

/// A file's first line (up to TAIL bytes), read through a buffer: a session's first line can be long.
fn first_row(path: &str) -> Result<Json, FileError> {
    use std::io::BufRead;
    let file = File::open(path).map_err(|_| FileError::of_kind("unreadable", "OSError"))?;
    let mut line = Vec::new();
    let _ = std::io::BufReader::new(file.take(TAIL as u64)).read_until(b'\n', &mut line);
    Ok(Json::parse(&line).unwrap_or(Json::Object(Object::new())))
}

fn kind_is(row: &Object, kind: &str) -> bool {
    row.get("type") == Some(&Json::from(kind))
}

/// A row a fork copied from the session it began from: Claude Code keeps that session's ID on the history it copies
/// into a fork's (or a hand-off's) transcript, so the row is that session's to count, not this one's. A session handed
/// off from a long one otherwise showed all of the long one's cost again.
fn copied(row: &Object, own: &str) -> bool {
    row.get("sessionId").and_then(Json::as_str).is_some_and(|id| !id.is_empty() && id != own)
}

/// A transcript's own session ID: its file's name.
fn own_id(path: &str) -> String {
    Path::new(path).file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default()
}

/// When a session began: the first time its first file records (a fork's is the session it began from).
fn started(path: &str) -> Json {
    rows_at_start(path, TAIL)
        .ok()
        .into_iter()
        .flatten()
        .map(|row| stamp(row.get("timestamp")))
        .find(|at| !at.is_null())
        .unwrap_or(Json::Null)
}

/// A Claude Code transcript: waiting once the last reply ended its turn, running otherwise. Its title is the last
/// one the transcript records: the user's or the App's, else the one Claude Code made up.
fn claude(path: &str) -> Result<Option<Object>, FileError> {
    let (mut last, mut reply, mut folder) = (None::<Object>, None::<Object>, None::<Json>);
    let (mut named, mut made) = (None::<String>, None::<String>);
    let said = |row: &Object| {
        (kind_is(row, "assistant") || kind_is(row, "user")) && !row.get("isSidechain").is_some_and(Json::is_truthy)
    };
    for row in rows_back(path, TAIL, |rows| rows.iter().any(said))? {
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
    let marks = |row: &Object| {
        let kind = row.get("payload").and_then(|payload| payload.get("type")).and_then(Json::as_str);
        matches!(kind, Some("task_started" | "task_complete" | "turn_aborted"))
    };
    for row in rows_back(path, TAIL, |rows| rows.iter().any(marks))? {
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
    // A segment opens with the history it goes on from: its turn context can sit far before the end.
    if model.is_null() {
        for row in rows_at_start(path, TAIL)? {
            if kind_is(&row, "turn_context") {
                let data = row.get("payload").and_then(Json::as_object).cloned().unwrap_or_default();
                model = py::or(data.get("model"), model);
                effort = text(data.get("effort")).or(effort);
            }
        }
    }
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
    let own = own_id(path);
    for row in rows_at_end(path, DETAIL)? {
        if copied(&row, &own) {
            continue;
        }
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
/// A Codex session's last turns, newest first, across the files of a thread written in segments (oldest first): read
/// from the newest back until there are enough whole turns; a turn that runs on into the next segment is one turn.
fn codex_turns(files: &[String], price: &Price) -> Result<Vec<Json>, FileError> {
    let mut turns: Vec<Object> = Vec::new();
    for (index, file) in files.iter().rev().enumerate() {
        let mut earlier = match codex_file_turns(file, price) {
            Ok(turns) => turns,
            Err(error) if index == 0 => return Err(error),
            Err(_) => break,
        };
        // What the later segment did before its first task_started finishes the earlier segment's last turn.
        if index > 0
            && let Some(carried) = turns.first().filter(|first| first.get("at").is_none_or(Json::is_null)).cloned()
        {
            turns.remove(0);
            let last = earlier.last_mut().expect("a turn");
            for key in ["requests", "tokens", "cost", "unpriced"] {
                bump(last, key, carried.get(key).cloned().unwrap_or(Json::Int(0)));
            }
            for (name, count) in carried.get("tools").and_then(Json::as_object).cloned().unwrap_or_default() {
                let mut tools = last.get("tools").and_then(Json::as_object).cloned().unwrap_or_default();
                let current = tools.get(&name).cloned().unwrap_or(Json::Int(0));
                tools.insert(name.as_str(), add(&current, &count));
                last.insert("tools", tools);
            }
        }
        earlier.extend(turns);
        turns = earlier;
        if turns.iter().filter(|turn| !turn.get("at").is_none_or(Json::is_null)).count() >= TURNS {
            break;
        }
    }
    Ok(last_turns(turns))
}

/// One Codex file's turns, oldest first; the first holds what came before its first task_started.
fn codex_file_turns(path: &str, price: &Price) -> Result<Vec<Object>, FileError> {
    let mut turns = vec![turn(Json::Null)];
    let mut model = Json::Null;
    let mut total: Option<Option<Json>> = None;
    // Back far enough that a turn starts before the first request read: a long tool output can push a turn's
    // start out, and its requests would read as the turn before's.
    // Nothing at all in view (a long output since the turn began, before its first request) is not enough either:
    // the turn going on would be left out.
    let opened = |rows: &[Object]| {
        for row in rows {
            match row.get("payload").and_then(|payload| payload.get("type")).and_then(Json::as_str) {
                Some("task_started" | "task_complete" | "turn_aborted") => return true,
                Some("token_count") => return false,
                _ => {}
            }
        }
        false
    };
    for row in rows_back(path, DETAIL, opened)? {
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
    Ok(turns)
}

/// How many of a Claude session's last reply IDs are kept between readings, so a reply streamed across the point where
/// one reading stopped still counts once.
const SEEN_KEPT: usize = 256;

/// How far back each request's time and cost are kept beside a session's total: past the longest range but 全部
/// (a month, 30 days), which is the whole session's.
const SPENDS_KEPT: f64 = 40.0 * 86400.0;
/// The session-spent cache's own version, beside the schema 1 every cache record carries (a record of another schema
/// reads as empty): 2 added each request's time and cost and when the session began.
const SPENT_VERSION: i64 = 2;

/// What a whole session has cost: every request in its transcript, counted as the turns count them (a Claude reply
/// streamed over several lines once; each change of Codex's running token total once). A Claude fork's copied
/// history is left to the session it came from. With `kept`, the tally of the last reading (by inode and the offset it
/// stopped at), only what was written since is read: a long running session isn't read whole every half minute. The
/// tally also keeps each request since `since` as [its time, its cost or null], for what the session cost in a range.
/// Gives the cost and the tally to keep.
fn spent(
    path: &str,
    tool: &str,
    price: &Price,
    kept: Option<&Object>,
    since: f64,
) -> Result<(Json, Object), FileError> {
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
    let mut spends: Vec<Json> = field("spends").and_then(|value| value.as_array().cloned()).unwrap_or_default();
    let start = field("offset").and_then(|value| value.integer()).unwrap_or(0) as u64;
    let own = own_id(path);

    let mut file = File::open(path).map_err(|_| unreadable())?;
    file.seek(SeekFrom::Start(start)).map_err(|_| unreadable())?;
    let mut data = Vec::new();
    file.read_to_end(&mut data).map_err(|_| unreadable())?;
    // Complete lines only: one being written is read next time.
    let complete = data.iter().rposition(|&byte| byte == b'\n').map_or(0, |at| at + 1);
    let mut known: HashSet<String> = seen.iter().cloned().collect();
    let mut add_request = |model: Option<&Json>, usage: &Object, at: Json| {
        requests += 1;
        let priced = price(model, usage);
        match priced {
            Some(spent) => cost += spent,
            None => unpriced += 1,
        }
        if at.number().is_some_and(|at| at >= since) {
            spends.push(Json::Array(vec![at, priced.map_or(Json::Null, Json::Double)]));
        }
    };
    for line in data[..complete].split(|&byte| byte == b'\n') {
        let Some(row) =
            Json::parse_selected(line, &Selection::All, true).ok().and_then(|value| value.as_object().cloned())
        else {
            continue;
        };
        if tool == "Claude" {
            if !kind_is(&row, "assistant") || copied(&row, &own) {
                continue;
            }
            let message = row.get("message").and_then(Json::as_object).cloned().unwrap_or_default();
            let key = py::dumps(&py::or(message.get("id"), row.get("uuid").cloned().unwrap_or(Json::Null)));
            if let Some(usage) = message.get("usage").and_then(Json::as_object)
                && !known.contains(&key)
            {
                known.insert(key.clone());
                seen.push(key);
                add_request(message.get("model"), usage, stamp(row.get("timestamp")));
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
                add_request(Some(&model), &last, stamp(row.get("timestamp")));
            }
        }
    }
    let cut = seen.len().saturating_sub(SEEN_KEPT);
    seen.drain(..cut);
    // The oldest go as they pass out of every range a session's cost is shown in.
    spends.retain(|spend| spend_parts(spend).0.is_some_and(|at| at >= since));
    let tally = object! {
        "inode" => state.inode, "offset" => (start + complete as u64) as i64, "requests" => requests,
        "cost" => cost, "unpriced" => unpriced, "seen" => Json::Array(seen.into_iter().map(Json::from).collect()),
        "model" => model, "total" => total.map_or(Json::Null, |value| object! { "value" => value.unwrap_or(Json::Null) }.into()),
        "spends" => Json::Array(spends),
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

/// A session to read: its tool, its file (a long Codex thread's newest segment), all its files, its ID and its thread.
struct Candidate {
    tool: &'static str,
    path: String,
    files: Vec<String>,
    id: String,
    thread: Option<String>,
}

/// A Codex thread's files as found: its ID when the names give it, and each file with when it last changed.
type Thread = (Option<String>, Vec<(f64, String)>);

/// A kept request's time and cost (None when unpriced).
fn spend_parts(spend: &Json) -> (Option<f64>, Option<f64>) {
    let pair = spend.as_array();
    (
        pair.and_then(|pair| pair.first()).and_then(Json::number),
        pair.and_then(|pair| pair.get(1)).and_then(Json::python_float),
    )
}

/// What the requests between start and end (both counted) cost, of each range: [key, start, end]. The requests come
/// in the order the files hold them, so the sum is the one Python's reading makes.
fn range_costs<'a>(spends: impl Iterator<Item = &'a Json> + Clone, ranges: &[(String, f64, f64)]) -> Json {
    let mut out = Object::new();
    for (key, start, end) in ranges {
        let (mut requests, mut cost, mut unpriced) = (0i64, 0.0, 0i64);
        for spend in spends.clone() {
            let (Some(at), spent) = spend_parts(spend) else { continue };
            if at < *start || at > *end {
                continue;
            }
            requests += 1;
            match spent {
                Some(spent) => cost += spent,
                None => unpriced += 1,
            }
        }
        out.insert(key.as_str(), object! { "requests" => requests, "cost" => cost, "unpriced" => unpriced });
    }
    Json::Object(out)
}

/// Sessions changed in the last day, the running ones first, then the most recent; a running one silent for ten minutes
/// has stopped without saying so and is idle. A long Codex thread written in segments is one session. With prices,
/// each carries its last turns and what the whole session has cost, and of each range in `ranges` ([key, start, end];
/// 全部 is the whole session) what it cost then. `tallies`, kept between readings by session, lets the cost be read
/// from where the last reading stopped, and a transcript unchanged since (a pinned panel reads every second or two, a
/// day of sessions is many files) is not read again: what was read then serves.
pub fn active(
    claude_roots: &[String],
    codex_root: Option<&str>,
    now: f64,
    prices: Option<&HashMap<&str, Price>>,
    claude_app: &HashMap<String, (Option<String>, Option<String>)>,
    tallies: Option<&mut Object>,
    ranges: &[(String, f64, f64)],
) -> Vec<Json> {
    let previous = tallies.as_deref().cloned().unwrap_or_default();
    let mut kept = Object::new();
    let hash = |text: &str| py::sha256_hex(text.as_bytes())[..16].to_string();
    let mut candidates: Vec<Candidate> = Vec::new();
    for root in claude_roots {
        for path in recent(root, 1, now - ACTIVE, &|name: &str| name.ends_with(".jsonl")) {
            if !path.split('/').any(|part| part == "subagents") {
                candidates.push(Candidate {
                    tool: "Claude",
                    path: path.clone(),
                    files: vec![path.clone()],
                    id: hash(&path),
                    thread: None,
                });
            }
        }
    }
    if let Some(codex_root) = codex_root {
        // A rollout is filed under the day its thread started, and a thread can still be going days later. A long
        // thread is written in segments, each going on from the one before: one session, in the state of its newest
        // segment, its cost all of theirs.
        let matching = |name: &str| name.starts_with("rollout-") && name.ends_with(".jsonl");
        let mut threads: indexmap::IndexMap<String, Thread> = indexmap::IndexMap::new();
        for path in recent(&format!("{codex_root}/sessions"), 3, 0.0, &matching) {
            let Some(state) = state_of(Path::new(&path)) else { continue };
            let thread = codex_thread(&path);
            let group = threads.entry(thread.clone().unwrap_or_else(|| path.clone())).or_insert((thread, Vec::new()));
            group.1.push((state.mtime as f64 / 1e9, path));
        }
        for (key, (thread, mut segments)) in threads {
            // In the order they were written, by name (rollout-<when it began>-…): a segment put back from a copy
            // carries a new modification time, and would read as the newest.
            let name = |path: &str| Path::new(path).file_name().map(|name| name.to_string_lossy().into_owned());
            segments.sort_by(|a, b| name(&a.1).cmp(&name(&b.1)).then(a.1.cmp(&b.1)));
            let Some((changed, newest)) = segments.last().cloned() else { continue };
            if changed < now - ACTIVE {
                continue;
            }
            let files = segments.into_iter().map(|(_, path)| path).collect();
            candidates.push(Candidate { tool: "ChatGPT", path: newest, files, id: hash(&key), thread });
        }
    }
    let mut titles: Option<HashMap<String, String>> = None;
    let mut found: Vec<(f64, Object)> = Vec::new();
    for Candidate { tool, path, files, id, thread } in candidates {
        let Some(state) = state_of(Path::new(&path)) else { continue };
        let changed = state.mtime as f64 / 1e9;
        let print_of =
            |state: &claude_usage::FileState| Json::from(format!("{}:{}:{}", state.inode, state.size, state.mtime));
        let print = print_of(&state);
        let file = hash(&path);
        // Every file the session is read from as it is now: what was read before serves while none has changed (a
        // segment put back changes the turns, not the newest file).
        let prints = Json::Array(
            files
                .iter()
                .map(|segment| state_of(Path::new(segment)).map_or(Json::Null, |state| print_of(&state)))
                .collect(),
        );
        let unchanged = previous.get(&file).and_then(Json::as_object).filter(|before| {
            before.get("print") == Some(&print)
                && before.get("prints") == Some(&prints)
                && before.get("read").and_then(Json::as_object).is_some()
        });
        let read = match unchanged {
            Some(before) => before.get("read").and_then(Json::as_object).cloned(),
            None => {
                let read = if tool == "Claude" { claude(&path).ok().flatten() } else { codex(&path).ok().flatten() };
                // When it began, for telling apart sessions of one name: a file's start stays as it grows.
                let before = previous.get(&file).and_then(Json::as_object).filter(|before| {
                    before.get("inode").and_then(Json::integer) == Some(state.inode) && files.len() == 1
                });
                read.map(|mut read| {
                    let began = before
                        .and_then(|before| before.get("read"))
                        .and_then(|read| read.get("started"))
                        .cloned()
                        .unwrap_or_else(|| started(&files[0]));
                    read.insert("started", began);
                    read
                })
            }
        };
        let Some(raw) = read else { continue };
        let mut session = raw.clone();
        if session.get("state") == Some(&Json::from("running")) && now - changed > STALLED {
            session.insert("state", "waiting");
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
            session.insert("title", thread.as_ref().and_then(|thread| names.get(thread)).cloned());
        }
        session.insert("id", id.as_str());
        session.insert("at", changed);
        // The turns and the whole session's cost are read here, so the path itself never leaves this function.
        if let Some(price) = prices.and_then(|prices| prices.get(tool)) {
            let turns =
                match unchanged.and_then(|before| before.get("turns")).filter(|turns| turns.as_array().is_some()) {
                    Some(turns) => turns.clone(),
                    None => Json::Array(
                        if tool == "Claude" { claude_turns(&path, price) } else { codex_turns(&files, price) }
                            .unwrap_or_default(),
                    ),
                };
            session.insert("turns", turns.clone());
            // Each file's count, kept by file: an unchanged one is not read again, a grown one from where it stopped.
            let (mut requests, mut cost, mut unpriced, mut whole) = (Json::Int(0), 0.0, Json::Int(0), true);
            let mut spends: Vec<Json> = Vec::new();
            for segment in &files {
                let key = hash(segment);
                let before = previous.get(&key).and_then(Json::as_object);
                let Some(now_print) = state_of(Path::new(segment)).map(|state| print_of(&state)) else {
                    whole = false;
                    continue;
                };
                let mut tally = match before.filter(|before| before.get("print") == Some(&now_print)) {
                    Some(before) => before.clone(),
                    None => match spent(segment, tool, price, before, now - SPENDS_KEPT) {
                        Ok((_, mut tally)) => {
                            tally.insert("print", now_print);
                            tally
                        }
                        Err(_) => {
                            whole = false;
                            continue;
                        }
                    },
                };
                requests = add(&requests, tally.get("requests").unwrap_or(&Json::Int(0)));
                cost += tally.get("cost").and_then(Json::python_float).unwrap_or(0.0);
                unpriced = add(&unpriced, tally.get("unpriced").unwrap_or(&Json::Int(0)));
                spends.extend(tally.get("spends").and_then(Json::as_array).into_iter().flatten().cloned());
                if *segment == path {
                    tally.insert("read", Json::Object(raw.clone()));
                    tally.insert("turns", turns.clone());
                    tally.insert("prints", prints.clone());
                }
                kept.insert(key.as_str(), Json::Object(tally));
            }
            session.insert(
                "spent",
                if whole {
                    object! {
                        "requests" => requests, "cost" => cost, "unpriced" => unpriced,
                        "ranges" => range_costs(spends.iter(), ranges),
                    }
                    .into()
                } else {
                    Json::Null
                },
            );
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
            // Each session's cost read so far, by session, kept beside the sessions to go on from. Version 2 keeps each
            // request's time with its cost and when the session began: a tally from before is read again from the
            // start, once. (A schema 2 record was never read back, so every refresh read each session whole.)
            let record = self.read_cache("session-spent");
            let mut tallies = record
                .get("sessions")
                .and_then(Json::as_object)
                .filter(|_| record.get("version").and_then(Json::integer) == Some(SPENT_VERSION))
                .cloned()
                .unwrap_or_default();
            let before = tallies.clone();
            // What each session cost in every range the panel can show; 全部 is the whole session's.
            let zone = self.local_zone();
            let ranges: Vec<(String, f64, f64)> = crate::runtime::RANGES
                .iter()
                .filter(|(key, _)| *key != "all")
                .map(|(key, _)| {
                    let (start, end) = self.range_bounds(key, now, &zone);
                    (key.to_string(), start, end)
                })
                .collect();
            let found = active(&roots, Some(&codex_root), now, Some(&prices), &app, Some(&mut tallies), &ranges);
            // Written only when something changed: unchanged transcripts leave it as it was.
            if tallies != before {
                self.write_json_cache(
                    &self.cache_path("session-spent"),
                    &object! { "schema" => 1, "version" => SPENT_VERSION, "sessions" => tallies }.into(),
                );
            }
            if self.settings().get("sessionAlerts").is_some_and(Json::is_truthy) {
                for session in newly_waiting(&previous, &found, now, 180.0).into_iter().take(3) {
                    let name = crate::more_rows::session_name(&session);
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
    use super::{Price, active, range_costs, spent};
    use crate::json::{Json, Object};
    use std::collections::HashMap;

    fn line(id: &str, output: i64) -> String {
        format!(
            "{{\"type\":\"assistant\",\"timestamp\":\"2026-11-21T10:00:{output:02}Z\",\"message\":{{\"id\":\"{id}\",\"model\":\"m\",\"usage\":{{\"input_tokens\":10,\"output_tokens\":{output}}}}}}}\n"
        )
    }

    #[test]
    fn a_session_read_on_from_where_it_stopped_costs_what_a_whole_reading_does() {
        let folder = std::env::temp_dir().join(format!("starmeter-spent-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("session.jsonl");
        let price: Price =
            Box::new(|_, usage| Some(usage.get("output_tokens").and_then(Json::integer)? as f64 * 0.001));
        // A reply streamed over two lines, a second reply, and a line still being written.
        std::fs::write(&path, format!("{}{}{}{{\"type\":\"assist", line("a", 5), line("a", 5), line("b", 7))).unwrap();
        let path = path.to_string_lossy().into_owned();
        let (first, tally) = spent(&path, "Claude", &price, None, 0.0).unwrap();
        assert_eq!(first.get("requests").and_then(Json::integer), Some(2));
        // The half-written line finished, the streamed reply "b" continued across the stop, and a new reply.
        let mut whole = std::fs::read_to_string(&path).unwrap();
        whole.truncate(whole.rfind('\n').unwrap() + 1);
        whole += &line("b", 7);
        whole += &line("c", 11);
        std::fs::write(&path, &whole).unwrap();
        let (resumed, resumed_tally) = spent(&path, "Claude", &price, Some(&tally), 0.0).unwrap();
        let (fresh, fresh_tally) = spent(&path, "Claude", &price, None, 0.0).unwrap();
        assert_eq!(resumed.serialized(), fresh.serialized());
        assert_eq!(fresh.get("requests").and_then(Json::integer), Some(3));
        // Each request's time and cost, kept as a whole reading keeps them.
        assert_eq!(resumed_tally.get("spends"), fresh_tally.get("spends"));
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_session_costs_its_own_requests_and_each_range_counts_both_ends() {
        let folder = std::env::temp_dir().join(format!("starmeter-own-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let own = "7a5d1c2e-0000-4000-8000-000000000002";
        let path = folder.join(format!("{own}.jsonl"));
        let reply = |session: &str, id: &str, at: &str| {
            format!(
                "{{\"type\":\"assistant\",\"sessionId\":\"{session}\",\"timestamp\":\"{at}\",\"message\":{{\"id\":\"{id}\",\"model\":\"m\",\"usage\":{{\"input_tokens\":10,\"output_tokens\":5}}}}}}\n"
            )
        };
        // The history a hand-off copied from the session it came from, then two replies of its own.
        let first = "7a5d1c2e-0000-4000-8000-000000000001";
        std::fs::write(
            &path,
            reply(first, "a", "2026-11-20T10:00:00Z")
                + &reply(first, "b", "2026-11-20T11:00:00Z")
                + &reply(own, "c", "2026-11-21T09:00:00Z")
                + &reply(own, "d", "2026-11-21T10:00:00Z"),
        )
        .unwrap();
        let path = path.to_string_lossy().into_owned();
        let price: Price = Box::new(|_, _| Some(0.25));
        let (whole, tally) = spent(&path, "Claude", &price, None, 0.0).unwrap();
        assert_eq!(whole.get("requests").and_then(Json::integer), Some(2), "the copied history is not its own");
        let at = |text: &str| crate::time::IsoTime::parse(text).unwrap().timestamp(&jiff::tz::TimeZone::UTC);
        let spends = tally.get("spends").and_then(Json::as_array).cloned().unwrap();
        let ranges = vec![
            ("from 9".to_string(), at("2026-11-21T09:00:00Z"), at("2026-11-21T09:30:00Z")),
            ("to 10".to_string(), at("2026-11-21T09:30:00Z"), at("2026-11-21T10:00:00Z")),
            ("between".to_string(), at("2026-11-21T09:00:01Z"), at("2026-11-21T09:59:59Z")),
        ];
        let costs = range_costs(spends.iter(), &ranges);
        let requests = |key: &str| costs.get(key).and_then(|figure| figure.get("requests")).and_then(Json::integer);
        assert_eq!((requests("from 9"), requests("to 10"), requests("between")), (Some(1), Some(1), Some(0)));
        assert_eq!(costs.get("from 9").and_then(|figure| figure.get("cost")).and_then(Json::python_float), Some(0.25));
        // Kept only as far back as asked: the older go.
        let (_, recent) = spent(&path, "Claude", &price, None, at("2026-11-21T09:30:00Z")).unwrap();
        assert_eq!(recent.get("spends").and_then(Json::as_array).map(Vec::len), Some(1));
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn an_unchanged_transcript_is_not_read_again() {
        let folder = std::env::temp_dir().join(format!("starmeter-unchanged-{}", std::process::id()));
        let path = folder.join("p").join("session.jsonl");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let turn = |stop: &str| {
            format!(
                "{{\"type\":\"user\",\"message\":{{\"content\":\"go\"}}}}\n{{\"type\":\"assistant\",\"message\":{{\"id\":\"a\",\"stop_reason\":\"{stop}\"}}}}\n"
            )
        };
        std::fs::write(&path, turn("end_turn")).unwrap();
        let written = std::fs::metadata(&path).unwrap().modified().unwrap();
        let price: Price = Box::new(|_, _| Some(0.0));
        let prices = HashMap::from([("Claude", price)]);
        let roots = [folder.to_string_lossy().into_owned()];
        let now = crate::runtime::now();
        let state = |found: &[Json]| found[0].get("state").and_then(Json::as_str).map(String::from);
        let mut tallies = Object::new();
        let first = active(&roots, None, now, Some(&prices), &HashMap::new(), Some(&mut tallies), &[]);
        assert_eq!(state(&first).as_deref(), Some("waiting"));
        // Rewritten in place to the same length and time: taken for unchanged, what was read before serves.
        std::fs::write(&path, turn("tool_use")).unwrap();
        std::fs::File::options().write(true).open(&path).unwrap().set_modified(written).unwrap();
        let again = active(&roots, None, now, Some(&prices), &HashMap::new(), Some(&mut tallies), &[]);
        assert_eq!(state(&again).as_deref(), Some("waiting"));
        assert_eq!(again[0].get("spent"), first[0].get("spent"));
        let fresh = active(&roots, None, now, Some(&prices), &HashMap::new(), None, &[]);
        assert_eq!(state(&fresh).as_deref(), Some("running"));
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_segment_put_back_is_read_again_and_the_newest_goes_by_name() {
        let root = std::env::temp_dir().join(format!("starmeter-segments-{}", std::process::id()));
        let day = root.join("sessions").join("2026").join("11").join("20");
        std::fs::create_dir_all(&day).unwrap();
        let thread = "019c0000-0000-7000-8000-000000000001";
        let segment = |at: &str, part: &str| {
            day.join(format!("rollout-2026-11-20T{at}-{thread}_019c0000-0000-7000-8000-00000000000{part}.jsonl"))
        };
        let turn = |at: &str, total: i64, finished: bool| {
            let mut rows = vec![
                format!("{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{thread}\",\"cwd\":\"/w/x\"}}}}"),
                "{\"type\":\"turn_context\",\"payload\":{\"model\":\"m\"}}".to_string(),
                format!(
                    "{{\"type\":\"event_msg\",\"timestamp\":\"2026-11-20T{at}Z\",\"payload\":{{\"type\":\"task_started\"}}}}"
                ),
                format!(
                    "{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"token_count\",\"info\":{{\"total_token_usage\":{{\"t\":{total}}},\"last_token_usage\":{{\"input_tokens\":10,\"output_tokens\":1}}}}}}}}"
                ),
            ];
            if finished {
                rows.push("{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_complete\"}}".to_string());
            }
            rows.join("\n") + "\n"
        };
        let price: Price = Box::new(|_, _| Some(0.0));
        let prices = HashMap::from([("ChatGPT", price)]);
        let codex = root.to_string_lossy().into_owned();
        let now = crate::runtime::now();
        let mut tallies = Object::new();
        let read =
            |tallies: &mut Object| active(&[], Some(&codex), now, Some(&prices), &HashMap::new(), Some(tallies), &[]);
        // The later segment alone: one turn, one request.
        std::fs::write(segment("10-00-00", "b"), turn("10:00:00", 2, true)).unwrap();
        let first = read(&mut tallies);
        let turns = |found: &[Json]| found[0].get("turns").and_then(Json::as_array).map(Vec::len);
        let requests =
            |found: &[Json]| found[0].get("spent").and_then(|spent| spent.get("requests")).and_then(Json::integer);
        assert_eq!((turns(&first), requests(&first)), (Some(1), Some(1)));
        let touch = |path: &std::path::Path, at: f64| {
            let when = std::time::UNIX_EPOCH + std::time::Duration::from_secs_f64(at);
            std::fs::File::options().write(true).open(path).unwrap().set_modified(when).unwrap();
        };
        // The earlier one put back, older than the newest file by time too: both turns and both requests, though the
        // newest file has not changed.
        std::fs::write(segment("09-00-00", "a"), turn("09:00:00", 1, false)).unwrap();
        touch(&segment("09-00-00", "a"), now - 3600.0);
        let both = read(&mut tallies);
        assert_eq!((turns(&both), requests(&both)), (Some(2), Some(2)));
        // Touched since, it is the newest file by time but not by name: the session is as its last segment left it.
        touch(&segment("09-00-00", "a"), now + 60.0);
        let touched = read(&mut tallies);
        assert_eq!(touched[0].get("state").and_then(Json::as_str), Some("waiting"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
