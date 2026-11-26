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
use crate::range_usage::{Wall, zone_key};
use crate::runtime::{Loaded, Runtime, now};
use crate::tally::add;
use crate::time::IsoTime;
use crate::{claude_app, object, pricing};

/// A session stays listed a day after its last change.
pub const ACTIVE: f64 = 86400.0;
/// A running session silent this long has most likely stopped without saying so: it is idle.
pub(crate) const STALLED: f64 = 600.0;
const TAIL: usize = 512 * 1024;
/// How far back a session's state is looked for when the usual tail holds none (a long tool output can fill it).
const FAR: usize = 16 * 1024 * 1024;
const DETAIL: usize = 2 * 1024 * 1024;
/// How far back a Claude turn's start is looked for when the detail read holds none of it: a turn of many tool outputs
/// (screenshots above all) runs well past DETAIL, and its clock then had no start (the user: 正在运行只有运行中).
const PROMPT_FAR: u64 = 256 * 1024 * 1024;
pub(crate) const TURNS: usize = 8;
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
pub(crate) fn rows_at_end(path: &str, size: usize) -> Result<Vec<Object>, FileError> {
    rows_at_end_from(path, size).map(|(rows, _)| rows)
}

/// rows_at_end, and where in the file the part read begins.
fn rows_at_end_from(path: &str, size: usize) -> Result<(Vec<Object>, u64), FileError> {
    let unreadable = || FileError::of_kind("unreadable", "OSError");
    let mut file = File::open(path).map_err(|_| unreadable())?;
    let end = file.seek(SeekFrom::End(0)).map_err(|_| unreadable())?;
    let start = end.saturating_sub(size as u64);
    file.seek(SeekFrom::Start(start)).map_err(|_| unreadable())?;
    let mut data = Vec::new();
    file.read_to_end(&mut data).map_err(|_| unreadable())?;
    let mut lines: Vec<&[u8]> = data.split(|&byte| byte == b'\n').collect();
    if end > size as u64 && !lines.is_empty() {
        lines.remove(0);
    }
    let rows = lines
        .into_iter()
        .filter_map(|line| Json::parse_selected(line, &Selection::All, true).ok())
        .filter_map(|value| value.as_object().cloned())
        .collect();
    Ok((rows, start))
}

/// A row that begins a Claude turn: the user's own words (or a picture, a file), or a message from another session
/// (written as a note, `isMeta`, from a peer: it sets the session to work as a prompt does; its turn's clock said the
/// hours since the user last wrote, 用户：这个20小时3分钟是累积时间？); not a tool's result, not a subagent's, not
/// another note of Claude Code's.
fn prompt(row: &Object) -> bool {
    let from_peer = row.get("origin").and_then(|origin| origin.get("kind")).and_then(Json::as_str) == Some("peer");
    if !kind_is(row, "user")
        || row.get("isSidechain").is_some_and(Json::is_truthy)
        || (row.get("isMeta").is_some_and(Json::is_truthy) && !from_peer)
    {
        return false;
    }
    let content = row.get("message").and_then(|message| message.get("content"));
    let kinds: HashSet<String> = content
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("type").map(py::dumps))
        .collect();
    let spoken = content.and_then(Json::as_str).is_some_and(|text| !text.trim().is_empty());
    spoken || (!kinds.is_empty() && !kinds.contains(&py::dumps(&Json::from("tool_result"))))
}

/// Where the last prompt before byte `before` of a transcript was found, as far as `upto`: per file, so a transcript
/// read again is read back only from where it was read up to the last time (it only grows).
type Prompts = HashMap<String, (u64, Option<(u64, Json)>)>;
static PROMPTS: std::sync::Mutex<Option<Prompts>> = std::sync::Mutex::new(None);

/// The time of the last prompt in a transcript before byte `before` (the start of the part read for its turns), read
/// back from there in pieces, at most PROMPT_FAR: the start of a turn begun before that part. Null if none.
fn prompt_before(path: &str, before: u64, own: &str) -> Json {
    let length = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
    let known = {
        let mut kept = PROMPTS.lock().expect("the prompts");
        kept.get_or_insert_with(HashMap::new).get(path).cloned().filter(|(upto, _)| *upto <= before && *upto <= length)
    };
    let limit = before.saturating_sub(PROMPT_FAR);
    let floor = known.as_ref().map_or(limit, |(upto, _)| (*upto).max(limit));
    // Scan outside the PROMPTS lock and advance `upto` only on Ok: a transient I/O error must not skip this range
    // permanently (R10-12).
    match scan_prompt(path, floor, before, own) {
        Ok(scanned) => {
            let found = scanned.or_else(|| known.and_then(|(_, found)| found));
            let mut kept = PROMPTS.lock().expect("the prompts");
            kept.get_or_insert_with(HashMap::new).insert(path.to_string(), (before, found.clone()));
            found.map_or(Json::Null, |(_, at)| at)
        }
        Err(_) => known.and_then(|(_, found)| found).map_or(Json::Null, |(_, at)| at),
    }
}

/// The last prompt in bytes `floor`..`before` of a transcript (and in the line running on past `before`), read back in
/// pieces of 4 MB: its offset and time.
fn scan_prompt(path: &str, floor: u64, before: u64, own: &str) -> std::io::Result<Option<(u64, Json)>> {
    const PIECE: u64 = 4 * 1024 * 1024;
    let mut file = File::open(path)?;
    // The line `before` falls in, whole: its start is in the pieces read back, its end after `before`.
    let mut tail = Vec::new();
    file.seek(SeekFrom::Start(before))?;
    let mut buffer = vec![0u8; 256 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        match memchr::memchr(b'\n', &buffer[..read]) {
            Some(at) => {
                tail.extend_from_slice(&buffer[..at]);
                break;
            }
            None => tail.extend_from_slice(&buffer[..read]),
        }
    }
    let mut end = before;
    while end > floor {
        let start = end.saturating_sub(PIECE).max(floor);
        let mut piece = vec![0u8; (end - start) as usize];
        file.seek(SeekFrom::Start(start))?;
        file.read_exact(&mut piece)?;
        piece.extend_from_slice(&tail);
        // The piece's first line may begin before it: kept for the next piece back (unless the file begins here).
        let (head, body) = match (start > 0).then(|| memchr::memchr(b'\n', &piece)).flatten() {
            Some(at) => (piece[..at].to_vec(), &piece[at + 1..]),
            None if start > 0 => (piece.clone(), &piece[piece.len()..]),
            None => (Vec::new(), &piece[..]),
        };
        let body_start = start + (piece.len() - body.len()) as u64;
        let mut line_end = body.len();
        for line_start in memchr::memrchr_iter(b'\n', body).map(|at| at + 1).chain(std::iter::once(0)) {
            let line = &body[line_start..line_end];
            line_end = line_start.saturating_sub(1);
            if memchr::memmem::find(line, b"\"user\"").is_none() {
                continue;
            }
            let Some(row) =
                Json::parse_selected(line, &Selection::All, true).ok().and_then(|value| value.as_object().cloned())
            else {
                continue;
            };
            if !copied(&row, own) && prompt(&row) {
                return Ok(Some((body_start + line_start as u64, stamp(row.get("timestamp")))));
            }
        }
        tail = head;
        end = start;
    }
    Ok(None)
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
pub(crate) fn rows_at_start(path: &str, size: usize) -> Result<Vec<Object>, FileError> {
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

pub(crate) fn turn(at: Json) -> Object {
    object! { "at" => at, "requests" => 0, "tokens" => 0, "cost" => 0.0, "unpriced" => 0, "tools" => Object::new() }
}

pub(crate) fn bump(turn: &mut Object, key: &str, amount: Json) {
    let current = turn.get(key).cloned().unwrap_or(Json::Int(0));
    turn.insert(key, add(&current, &amount));
}

fn count_tool(turn: &mut Object, name: &str) {
    let mut tools = turn.get("tools").and_then(Json::as_object).cloned().unwrap_or_default();
    let current = tools.get(name).cloned().unwrap_or(Json::Int(0));
    tools.insert(name, add(&current, &Json::Int(1)));
    turn.insert("tools", tools);
}

pub(crate) fn last_turns(turns: Vec<Object>) -> Vec<Json> {
    let kept: Vec<Object> = turns
        .into_iter()
        .filter(|turn| !turn.get("at").is_none_or(Json::is_null) || turn.get("requests").is_some_and(Json::is_truthy))
        .collect();
    let start = kept.len().saturating_sub(TURNS);
    kept[start..].iter().rev().cloned().map(Json::Object).collect()
}

/// A Claude reply's tokens, or None for a line that is no request: Claude Code's own notices (model <synthetic>) and
/// lines with no tokens, as the usage leaves them out.
fn reply_tokens(message: &Object) -> Option<Json> {
    let model = message.get("model").and_then(Json::as_str)?;
    if model.is_empty() || model == "<synthetic>" || model == "synthetic" {
        return None;
    }
    let usage = message.get("usage").and_then(Json::as_object)?;
    let tokens = ["input_tokens", "cache_read_input_tokens", "cache_creation_input_tokens", "output_tokens"]
        .iter()
        .fold(Json::Int(0), |sum, key| add(&sum, &number(usage.get(key))));
    (tokens.number().unwrap_or(0.0) != 0.0).then_some(tokens)
}

/// A count taken back: -n, of the same kind.
fn negated(value: &Json) -> Json {
    match value {
        Json::Int(n) => Json::Int(-n),
        other => Json::Double(-other.number().unwrap_or(0.0)),
    }
}

/// What tells a Claude reply's lines apart: its message's id, else the row's; None for a line with neither, a reply
/// of its own.
fn reply_key(message: &Object, row: &Object) -> Option<String> {
    py::or(message.get("id"), row.get("uuid").cloned().unwrap_or(Json::Null))
        .is_truthy()
        .then(|| py::dumps(&py::or(message.get("id"), row.get("uuid").cloned().unwrap_or(Json::Null))))
}

/// The last turns of a Claude Code session, newest first: from one prompt to the next. A reply streamed over several
/// lines (its output written as it goes) counts once, in the turn of its first line, with the most any of its lines
/// says, as the usage takes it; a subagent's work belongs to the turn that started it.
fn claude_turns(path: &str, price: &Price) -> Result<Vec<Json>, FileError> {
    let mut turns = vec![turn(Json::Null)];
    // Each reply counted: the turn it is in, its tokens and its cost.
    let mut best: HashMap<String, (usize, Json, Option<f64>)> = HashMap::new();
    let own = own_id(path);
    let (rows, start) = rows_at_end_from(path, DETAIL)?;
    for row in rows {
        if copied(&row, &own) {
            continue;
        }
        let message = row.get("message").and_then(Json::as_object).cloned().unwrap_or_default();
        let content = message.get("content");
        if prompt(&row) {
            turns.push(turn(stamp(row.get("timestamp"))));
        } else if kind_is(&row, "assistant") {
            for item in content.and_then(Json::as_array).into_iter().flatten() {
                if let Some(fields) = item.as_object()
                    && fields.get("type") == Some(&Json::from("tool_use"))
                    && let Some(name) = fields.get("name").filter(|name| name.is_truthy())
                {
                    count_tool(turns.last_mut().expect("a turn"), &py::str(Some(name)));
                }
            }
            let Some(tokens) = reply_tokens(&message) else { continue };
            let usage = message.get("usage").and_then(Json::as_object).cloned().unwrap_or_default();
            let key = reply_key(&message, &row);
            let before = key.as_ref().and_then(|key| best.get(key)).cloned();
            let size = |tokens: &Json| tokens.number().unwrap_or(0.0);
            if before.as_ref().is_some_and(|(_, counted, _)| size(&tokens) <= size(counted)) {
                continue;
            }
            let cost = price(message.get("model"), &usage);
            let at = match before {
                // A later line of a reply counted already: its turn has the reply as this line tells it.
                Some((at, counted, was)) => {
                    let where_ = &mut turns[at];
                    bump(where_, "tokens", add(&tokens, &negated(&counted)));
                    match was {
                        Some(was) => bump(where_, "cost", Json::Double(-was)),
                        None => bump(where_, "unpriced", Json::Int(-1)),
                    }
                    at
                }
                None => {
                    let at = turns.len() - 1;
                    bump(&mut turns[at], "requests", Json::Int(1));
                    bump(&mut turns[at], "tokens", tokens.clone());
                    at
                }
            };
            match cost {
                Some(cost) => bump(&mut turns[at], "cost", Json::Double(cost)),
                None => bump(&mut turns[at], "unpriced", Json::Int(1)),
            }
            if let Some(key) = key {
                best.insert(key, (at, tokens, cost));
            }
        }
    }
    // No prompt in the part read: the turn going on began before it (a turn of many tool outputs), and its start is the
    // last prompt further back, so its clock has a start like any other's.
    if turns.len() == 1 && start > 0 {
        let at = prompt_before(path, start, &own);
        if !at.is_null() {
            turns[0].insert("at", at);
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

/// How far back each request's own time and cost are kept beside a session's total: past the last 24 hours, the one
/// range that doesn't begin at a midnight. The others are summed from the session's days.
pub(crate) const SPENDS_KEPT: f64 = 26.0 * 3600.0;
/// How many days of a session's cost by day are kept: past the longest range but 全部 (a month, 30 days), which is
/// the whole session's.
const DAYS_KEPT: i64 = 40;
/// The session-spent cache's own version, beside the schema 1 every cache record carries (a record of another schema
/// reads as empty): 3 keeps each session's cost by day in the counting zone and its last day of requests one by one,
/// and when it began; 4 keeps the prices it was counted with; 5 a digest of everything each file's reading read,
/// and the requests timed ahead of the clock apart; 6 each Claude reply once with its fullest line, kept in `best`,
/// its own notices and lines with no tokens left out (a reading of 5 counted the first line).
const SPENT_VERSION: i64 = 6;

/// A digest of a file's first `offset` bytes, all that the last reading read: when it differs, something already
/// counted was written over, wherever in the file (SESSION-6), and the file is read again from the start. It reads
/// those bytes again each time a grown file is read on, so it is no cryptographic digest but four lanes of multiply
/// and rotate that keep pace with the reading itself (a 257 MB transcript in about 22 ms, where SHA-256 takes 450):
/// a change by accident, not by design, is all it has to catch.
fn read_digest(path: &str, offset: u64) -> Option<String> {
    const STEP: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut file = File::open(path).ok()?;
    let mut lanes: [u64; 4] =
        [0x243F_6A88_85A3_08D3, 0x1319_8A2E_0370_7344, 0xA409_3822_299F_31D0, 0x082E_FA98_EC4E_6C89];
    let mut buffer = vec![0u8; 1 << 20];
    let mut left = offset;
    while left > 0 {
        let size = left.min(buffer.len() as u64) as usize;
        file.read_exact(&mut buffer[..size]).ok()?;
        let (blocks, rest) = buffer[..size].as_chunks::<32>();
        for block in blocks {
            for (lane, word) in lanes.iter_mut().zip(block.as_chunks::<8>().0) {
                *lane = (*lane ^ u64::from_le_bytes(*word)).wrapping_mul(STEP).rotate_left(29);
            }
        }
        for (k, byte) in rest.iter().enumerate() {
            lanes[k % 4] = (lanes[k % 4] ^ u64::from(*byte)).wrapping_mul(STEP).rotate_left(29);
        }
        left -= size as u64;
    }
    let fold = |a: u64, b: u64| (a ^ b.rotate_left(32)).wrapping_mul(STEP);
    Some(format!("{:016x}{:016x}", fold(lanes[0], lanes[2]), fold(lanes[1], lanes[3]) ^ offset))
}

/// A moment's day in a zone, as 2026-11-21.
pub(crate) fn day_of(at: f64, zone: &TimeZone) -> String {
    let wall = Wall::at(at, zone);
    format!("{:04}-{:02}-{:02}", wall.year, wall.month, wall.day)
}

/// One request more in a day's figure ([requests, cost, unpriced]).
pub(crate) fn add_to_day(days: &mut Object, date: &str, priced: Option<f64>) {
    if !days.contains_key(date) {
        days.insert(date, Json::Array(vec![0.into(), 0.0.into(), 0.into()]));
    }
    if let Some(Json::Array(figure)) = days.get_mut(date) {
        figure[0] = Json::Int(figure[0].integer().unwrap_or(0) + 1);
        match priced {
            Some(spent) => figure[1] = Json::Double(figure[1].python_float().unwrap_or(0.0) + spent),
            None => figure[2] = Json::Int(figure[2].integer().unwrap_or(0) + 1),
        }
    }
}

/// One request less on a day; a day left with none goes, as a reading afresh wouldn't have it.
fn take_from_day(days: &mut Object, date: &str, priced: Option<f64>) {
    let Some(Json::Array(figure)) = days.get_mut(date) else { return };
    let left = figure[0].integer().unwrap_or(0) - 1;
    figure[0] = Json::Int(left);
    match priced {
        Some(spent) => figure[1] = Json::Double(figure[1].python_float().unwrap_or(0.0) - spent),
        None => figure[2] = Json::Int(figure[2].integer().unwrap_or(0) - 1),
    }
    if left <= 0 {
        days.remove(date);
    }
}

/// The requests waiting in a tally's `later` whose time has come by `until`, counted now as a reading would have:
/// in the session's total, its day and, from `since`, one by one. A transcript timed by a clock running ahead is
/// counted as that time arrives, unchanged or not, as the usage itself is (SESSION-8). The other way too: a request
/// counted already whose time is ahead of `until` again (the clock was set back) leaves the total and its day and
/// waits in `later` until its time comes once more, as a reading afresh would have it (SESSION-9). Each request of
/// the last day and more is in `spends` with its time, which is all a clock set back can reach.
fn settle(tally: &mut Object, until: f64, since: f64, zone: &TimeZone) {
    let spends = tally.get("spends").and_then(Json::as_array).cloned().unwrap_or_default();
    let (ahead, behind): (Vec<Json>, Vec<Json>) =
        spends.into_iter().partition(|spend| spend_parts(spend).0.is_some_and(|at| at > until));
    if !ahead.is_empty() {
        let mut requests = tally.get("requests").and_then(Json::integer).unwrap_or(0);
        let mut cost = tally.get("cost").and_then(Json::python_float).unwrap_or(0.0);
        let mut unpriced = tally.get("unpriced").and_then(Json::integer).unwrap_or(0);
        let mut days = tally.get("days").and_then(Json::as_object).cloned().unwrap_or_default();
        let mut later = tally.get("later").and_then(Json::as_array).cloned().unwrap_or_default();
        for spend in &ahead {
            let (Some(at), priced) = spend_parts(spend) else { continue };
            requests -= 1;
            match priced {
                Some(spent) => cost -= spent,
                None => unpriced -= 1,
            }
            take_from_day(&mut days, &day_of(at, zone), priced);
        }
        later.extend(ahead);
        tally.insert("requests", requests);
        tally.insert("cost", cost);
        tally.insert("unpriced", unpriced);
        tally.insert("days", Json::Object(days));
        tally.insert("spends", Json::Array(behind));
        tally.insert("later", Json::Array(later));
    }
    let Some(later) = tally.get("later").and_then(Json::as_array).cloned() else { return };
    let (arrived, waiting): (Vec<Json>, Vec<Json>) =
        later.into_iter().partition(|spend| spend_parts(spend).0.is_some_and(|at| at <= until));
    if arrived.is_empty() {
        return;
    }
    let mut requests = tally.get("requests").and_then(Json::integer).unwrap_or(0);
    let mut cost = tally.get("cost").and_then(Json::python_float).unwrap_or(0.0);
    let mut unpriced = tally.get("unpriced").and_then(Json::integer).unwrap_or(0);
    let mut days = tally.get("days").and_then(Json::as_object).cloned().unwrap_or_default();
    let mut spends = tally.get("spends").and_then(Json::as_array).cloned().unwrap_or_default();
    for spend in arrived {
        let (Some(at), priced) = spend_parts(&spend) else { continue };
        requests += 1;
        match priced {
            Some(spent) => cost += spent,
            None => unpriced += 1,
        }
        add_to_day(&mut days, &day_of(at, zone), priced);
        if at >= since {
            spends.push(spend);
        }
    }
    tally.insert("requests", requests);
    tally.insert("cost", cost);
    tally.insert("unpriced", unpriced);
    tally.insert("days", Json::Object(days));
    tally.insert("spends", Json::Array(spends));
    tally.insert("later", Json::Array(waiting));
}

/// A reading's sums as it goes: the session's total, its days, its requests since `since` and those waiting in
/// `later` (timed after `until`).
struct Ledger<'a> {
    requests: i64,
    cost: f64,
    unpriced: i64,
    days: Object,
    spends: Vec<Json>,
    later: Vec<Json>,
    since: f64,
    until: f64,
    zone: &'a TimeZone,
}

impl Ledger<'_> {
    fn add(&mut self, priced: Option<f64>, at: Json) {
        if at.number().is_some_and(|moment| moment > self.until) {
            self.later.push(Json::Array(vec![at, priced.map_or(Json::Null, Json::Double)]));
            return;
        }
        self.requests += 1;
        match priced {
            Some(spent) => self.cost += spent,
            None => self.unpriced += 1,
        }
        let Some(moment) = at.number() else { return };
        add_to_day(&mut self.days, &day_of(moment, self.zone), priced);
        if moment >= self.since {
            self.spends.push(Json::Array(vec![at, priced.map_or(Json::Null, Json::Double)]));
        }
    }

    /// A request counted at `at` costing `was`, better told by a later line (a streamed reply's): `now` instead, in its
    /// total, its day and its place among the requests since `since`, or among those waiting, where it waits.
    fn correct(&mut self, at: &Json, was: Option<f64>, now: Option<f64>) {
        let entry = |priced: Option<f64>| Json::Array(vec![at.clone(), priced.map_or(Json::Null, Json::Double)]);
        if let Some(waiting) = self.later.iter_mut().rev().find(|spend| **spend == entry(was)) {
            *waiting = entry(now);
            return;
        }
        let change = |cost: &mut f64, unpriced: &mut i64| {
            match was {
                Some(spent) => *cost -= spent,
                None => *unpriced -= 1,
            }
            match now {
                Some(spent) => *cost += spent,
                None => *unpriced += 1,
            }
        };
        change(&mut self.cost, &mut self.unpriced);
        let Some(moment) = at.number() else { return };
        if let Some(Json::Array(figure)) = self.days.get_mut(&day_of(moment, self.zone)) {
            let (mut spent, mut left) = (figure[1].python_float().unwrap_or(0.0), figure[2].integer().unwrap_or(0));
            change(&mut spent, &mut left);
            figure[1] = Json::Double(spent);
            figure[2] = Json::Int(left);
        }
        if let Some(spend) = self.spends.iter_mut().rev().find(|spend| **spend == entry(was)) {
            *spend = entry(now);
        }
    }
}

/// What a whole session has cost: every request in its transcript, counted as the turns count them (a Claude reply
/// streamed over several lines once; each change of Codex's running token total once). A Claude fork's copied
/// history is left to the session it came from. With `kept`, the tally of the last reading (by inode and the offset it
/// stopped at), only what was written since is read: a long running session isn't read whole every half minute. A
/// file that changed without growing, or whose bytes up to that offset are no longer those read (read_digest), was
/// written over in place and is read again from the start. The
/// tally also keeps, for what the session cost in a range, its cost by day in `zone` ({day: [requests, cost,
/// unpriced]}, in the order the days come) and each request since `since` as [its time, its cost or null]; a tally of
/// another zone is read afresh. A request timed after `until` (a clock running ahead wrote it) waits in `later`,
/// counted nowhere until settle() finds its time has come (SESSION-8). Gives the cost and the tally to keep.
fn spent(
    path: &str,
    tool: &str,
    price: &Price,
    kept: Option<&Object>,
    since: f64,
    until: f64,
    zone: &TimeZone,
) -> Result<(Json, Object), FileError> {
    let unreadable = || FileError::of_kind("unreadable", "OSError");
    let state = state_of(Path::new(path)).ok_or_else(unreadable)?;
    let counted = zone_key(zone, since);
    // Only read when the file changed: grown, it goes on from where the last reading stopped, provided what was read
    // is still there as it was.
    let resumed = kept.filter(|kept| {
        let offset = kept.get("offset").and_then(Json::integer).filter(|offset| (0..state.size).contains(offset));
        kept.get("inode").and_then(Json::integer) == Some(state.inode)
            && kept.get("zone").and_then(Json::as_str) == Some(counted.as_str())
            && offset.is_some_and(|offset| {
                kept.get("digest")
                    .and_then(Json::as_str)
                    .is_some_and(|digest| read_digest(path, offset as u64).as_deref() == Some(digest))
            })
    });
    let field = |name: &str| resumed.and_then(|kept| kept.get(name)).cloned();
    let requests = field("requests").and_then(|value| value.integer()).unwrap_or(0);
    let cost = field("cost").and_then(|value| value.python_float()).unwrap_or(0.0);
    let unpriced = field("unpriced").and_then(|value| value.integer()).unwrap_or(0);
    let mut seen: Vec<String> = field("seen")
        .and_then(|value| value.as_array().cloned())
        .map(|keys| keys.iter().filter_map(|key| key.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let mut model = field("model").unwrap_or(Json::Null);
    // None until a running total has been seen: a reading that saw none kept null, not a total that was missing.
    let mut total: Option<Option<Json>> =
        field("total").filter(|total| !total.is_null()).map(|total| total.get("value").cloned());
    let spends: Vec<Json> = field("spends").and_then(|value| value.as_array().cloned()).unwrap_or_default();
    let days: Object = field("days").and_then(|value| value.as_object().cloned()).unwrap_or_default();
    let later: Vec<Json> = field("later").and_then(|value| value.as_array().cloned()).unwrap_or_default();
    let start = field("offset").and_then(|value| value.integer()).unwrap_or(0) as u64;
    let own = own_id(path);

    let mut file = File::open(path).map_err(|_| unreadable())?;
    file.seek(SeekFrom::Start(start)).map_err(|_| unreadable())?;
    let mut data = Vec::new();
    file.read_to_end(&mut data).map_err(|_| unreadable())?;
    // Complete lines only: one being written is read next time.
    let complete = data.iter().rposition(|&byte| byte == b'\n').map_or(0, |at| at + 1);
    let mut known: HashSet<String> = seen.iter().cloned().collect();
    // Each Claude reply counted, by its key: its tokens, the time it was counted at and its cost then; a later line
    // with more (a reply streamed over several lines) is the reply as counted, at the same time.
    let mut best: HashMap<String, (Json, Json, Option<f64>)> = field("best")
        .and_then(|value| value.as_object().cloned())
        .map(|best| {
            best.iter()
                .filter_map(|(key, value)| {
                    let parts = value.as_array()?;
                    let priced = parts.get(2).filter(|priced| !priced.is_null()).and_then(Json::number);
                    Some((key.clone(), (parts.first()?.clone(), parts.get(1)?.clone(), priced)))
                })
                .collect()
        })
        .unwrap_or_default();
    let mut ledger = Ledger { requests, cost, unpriced, days, spends, later, since, until, zone };
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
            let Some(tokens) = reply_tokens(&message) else { continue };
            let usage = message.get("usage").and_then(Json::as_object).cloned().unwrap_or_default();
            let priced = price(message.get("model"), &usage);
            let Some(key) = reply_key(&message, &row) else {
                ledger.add(priced, stamp(row.get("timestamp")));
                continue;
            };
            if !known.contains(&key) {
                let at = stamp(row.get("timestamp"));
                known.insert(key.clone());
                seen.push(key.clone());
                best.insert(key, (tokens, at.clone(), priced));
                ledger.add(priced, at);
            } else if let Some((counted, at, was)) = best.get(&key).cloned()
                && tokens.number().unwrap_or(0.0) > counted.number().unwrap_or(0.0)
            {
                ledger.correct(&at, was, priced);
                best.insert(key, (tokens, at, priced));
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
                ledger.add(price(Some(&model), &last), stamp(row.get("timestamp")));
            }
        }
    }
    let Ledger { requests, cost, unpriced, days, mut spends, later, .. } = ledger;
    let cut = seen.len().saturating_sub(SEEN_KEPT);
    seen.drain(..cut);
    let kept: HashSet<&String> = seen.iter().collect();
    let best: Object = best
        .into_iter()
        .filter(|(key, _)| kept.contains(key))
        .map(|(key, (tokens, at, priced))| {
            (key, Json::Array(vec![tokens, at, priced.map_or(Json::Null, Json::Double)]))
        })
        .collect();
    // The oldest go as they pass out of every range a session's cost is shown in.
    spends.retain(|spend| spend_parts(spend).0.is_some_and(|at| at >= since));
    let oldest = day_of(since - DAYS_KEPT as f64 * 86400.0, zone);
    let days: Object = days
        .iter()
        .filter(|(day, _)| day.as_str() >= oldest.as_str())
        .map(|(day, figure)| (day.as_str(), figure.clone()))
        .collect();
    let offset = start + complete as u64;
    let tally = object! {
        "inode" => state.inode, "offset" => offset as i64, "digest" => read_digest(path, offset).map_or(Json::Null, Json::from),
        "requests" => requests,
        "cost" => cost, "unpriced" => unpriced, "seen" => Json::Array(seen.into_iter().map(Json::from).collect()),
        "model" => model, "total" => total.map_or(Json::Null, |value| object! { "value" => value.unwrap_or(Json::Null) }.into()),
        "zone" => counted, "days" => Json::Object(days), "spends" => Json::Array(spends), "later" => Json::Array(later),
        "best" => Json::Object(best),
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

/// What the session cost in each range ([key, start, end], both ends counted), from its files' tallies in order: the
/// last 24 hours request by request, the others (each from a midnight in `zone`) day by day. Python sums in the same
/// order, so both give the same figure.
pub(crate) fn range_costs(parts: &[(Object, Vec<Json>)], ranges: &[(String, f64, f64)], zone: &TimeZone) -> Json {
    let mut out = Object::new();
    for (key, start, end) in ranges {
        let (mut requests, mut cost, mut unpriced) = (0i64, 0.0, 0i64);
        if key == "24h" {
            for spend in parts.iter().flat_map(|(_, spends)| spends) {
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
        } else {
            // Yesterday ends a hair before midnight, which a day's rounding can carry over: it is one day.
            let first = day_of(*start, zone);
            let last = if key == "yesterday" { first.clone() } else { day_of(*end, zone) };
            for (day, figure) in parts.iter().flat_map(|(days, _)| days.iter()) {
                if day.as_str() < first.as_str() || day.as_str() > last.as_str() {
                    continue;
                }
                let field = |index: usize| figure.as_array().and_then(|figure| figure.get(index));
                requests += field(0).and_then(Json::integer).unwrap_or(0);
                cost += field(1).and_then(Json::python_float).unwrap_or(0.0);
                unpriced += field(2).and_then(Json::integer).unwrap_or(0);
            }
        }
        out.insert(key.as_str(), object! { "requests" => requests, "cost" => cost, "unpriced" => unpriced });
    }
    Json::Object(out)
}

/// The ranges a session's cost is told in ([key, start, end]) and the zone their days begin in.
#[derive(Clone, Copy)]
pub struct Counting<'a> {
    pub ranges: &'a [(String, f64, f64)],
    pub zone: &'a TimeZone,
}

/// From when sessions are listed: the earliest range's start (a month, 30 days), else the last day.
pub fn listed_since(ranges: &[(String, f64, f64)], now: f64) -> f64 {
    ranges.iter().map(|(_, start, _)| *start).fold(now - ACTIVE, f64::min)
}

/// Sessions changed since the earliest range in `ranges` began (the last day without ranges), the running ones first,
/// then the most recent; a running one silent for ten minutes has stopped without saying so and is idle. A long Codex
/// thread written in segments is one session. With prices, each carries its last turns and what the whole session has
/// cost, and of each range in `ranges` ([key, start, end]; 全部 is the whole session) what it cost then, its days in
/// `zone`. `tallies`, kept between readings by session, lets the cost be read
/// from where the last reading stopped, and a transcript unchanged since (a pinned panel reads every second or two, a
/// day of sessions is many files) is not read again: what was read then serves.
pub fn active(
    claude_roots: &[String],
    codex_root: Option<&str>,
    now: f64,
    prices: Option<&HashMap<&str, Price>>,
    claude_app: &HashMap<String, (Option<String>, Option<String>)>,
    tallies: Option<&mut Object>,
    counting: Counting,
) -> Vec<Json> {
    let Counting { ranges, zone } = counting;
    let since = listed_since(ranges, now);
    let previous = tallies.as_deref().cloned().unwrap_or_default();
    let mut kept = Object::new();
    let hash = |text: &str| py::sha256_hex(text.as_bytes())[..16].to_string();
    let mut candidates: Vec<Candidate> = Vec::new();
    for root in claude_roots {
        for path in recent(root, 1, since, &|name: &str| name.ends_with(".jsonl")) {
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
            if changed < since {
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
            let mut parts: Vec<(Object, Vec<Json>)> = Vec::new();
            for segment in &files {
                let key = hash(segment);
                let before = previous.get(&key).and_then(Json::as_object);
                let Some(now_print) = state_of(Path::new(segment)).map(|state| print_of(&state)) else {
                    whole = false;
                    continue;
                };
                // Unchanged, and counted in the zone counted in now: as it was. Otherwise read (on) again.
                let mut tally = match before.filter(|before| {
                    before.get("print") == Some(&now_print)
                        && before.get("zone").and_then(Json::as_str) == Some(zone_key(zone, now - SPENDS_KEPT).as_str())
                }) {
                    Some(before) => before.clone(),
                    None => match spent(segment, tool, price, before, now - SPENDS_KEPT, now, zone) {
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
                settle(&mut tally, now, now - SPENDS_KEPT, zone);
                requests = add(&requests, tally.get("requests").unwrap_or(&Json::Int(0)));
                cost += tally.get("cost").and_then(Json::python_float).unwrap_or(0.0);
                unpriced = add(&unpriced, tally.get("unpriced").unwrap_or(&Json::Int(0)));
                parts.push((
                    tally.get("days").and_then(Json::as_object).cloned().unwrap_or_default(),
                    tally.get("spends").and_then(Json::as_array).cloned().unwrap_or_default(),
                ));
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
                        "ranges" => range_costs(&parts, ranges, zone),
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

    /// Claude Code's, Codex's and the other tools' sessions running or waiting now; with the alert on, one that has
    /// just finished its turn is told of.
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
            // Every range the panel can show, for what each session cost then; 全部 is the whole session's. Sessions are
            // listed from the earliest range's start.
            let zone = self.local_zone();
            let ranges: Vec<(String, f64, f64)> = crate::runtime::RANGES
                .iter()
                .filter(|(key, _)| *key != "all")
                .map(|(key, _)| {
                    let (start, end) = self.range_bounds(key, now, &zone);
                    (key.to_string(), start, end)
                })
                .collect();
            let app = claude_app::session_settings(&self.source_path("claude"), listed_since(&ranges, now));
            let prices = self.session_prices();
            let codex_root = self.codex_home_absolute();
            // Each session's cost read so far, by session, kept beside the sessions to go on from. A tally of another
            // version, or counted with other prices (a price table updated, a model priced for the first time, this
            // app's own prices changed), is read again from the start, once. (A schema 2 record was never read back,
            // so every refresh read each session whole.)
            let record = self.read_cache("session-spent");
            let priced_with = crate::pricing::fingerprint();
            let mut tallies = record
                .get("sessions")
                .and_then(Json::as_object)
                .filter(|_| record.get("version").and_then(Json::integer) == Some(SPENT_VERSION))
                .filter(|_| record.get("prices").and_then(Json::as_str) == Some(priced_with.as_str()))
                .cloned()
                .unwrap_or_default();
            let before = tallies.clone();
            let found = active(
                &roots,
                Some(&codex_root),
                now,
                Some(&prices),
                &app,
                Some(&mut tallies),
                Counting { ranges: &ranges, zone: &zone },
            );
            // The other tools' sessions beside them, the running ones first, then the most recent.
            let others = self.tool_sessions(listed_since(&ranges, now), now, &ranges, &zone);
            let found = if others.is_empty() {
                found
            } else {
                let mut all: Vec<(f64, Json)> = found
                    .into_iter()
                    .map(|session| (session.get("at").and_then(Json::number).unwrap_or(0.0), session))
                    .chain(others.into_iter().map(|(at, session)| (at, Json::Object(session))))
                    .collect();
                let running = |session: &Json| session.get("state") == Some(&Json::from("running"));
                all.sort_by(|(a, a_session), (b, b_session)| running(b_session).cmp(&running(a_session)).then(b.total_cmp(a)));
                all.into_iter().map(|(_, session)| session).collect()
            };
            // Written only when something changed: unchanged transcripts leave it as it was.
            if tallies != before {
                self.write_json_cache(
                    &self.cache_path("session-spent"),
                    &object! { "schema" => 1, "version" => SPENT_VERSION, "prices" => priced_with, "sessions" => tallies }.into(),
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
    use super::{Price, active, claude_turns, range_costs, read_digest, spent};
    use crate::json::{Json, Object};
    use std::collections::{HashMap, HashSet};
    use std::io::Write;

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
        let (first, tally) =
            spent(&path, "Claude", &price, None, 0.0, f64::INFINITY, &jiff::tz::TimeZone::UTC).unwrap();
        assert_eq!(first.get("requests").and_then(Json::integer), Some(2));
        // The half-written line finished, the streamed reply "b" continued across the stop, and a new reply.
        let mut whole = std::fs::read_to_string(&path).unwrap();
        whole.truncate(whole.rfind('\n').unwrap() + 1);
        whole += &line("b", 7);
        whole += &line("c", 11);
        std::fs::write(&path, &whole).unwrap();
        let (resumed, resumed_tally) =
            spent(&path, "Claude", &price, Some(&tally), 0.0, f64::INFINITY, &jiff::tz::TimeZone::UTC).unwrap();
        let (fresh, fresh_tally) =
            spent(&path, "Claude", &price, None, 0.0, f64::INFINITY, &jiff::tz::TimeZone::UTC).unwrap();
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
        // The history a hand-off copied from the session it came from, then three replies of its own over two days.
        let first = "7a5d1c2e-0000-4000-8000-000000000001";
        std::fs::write(
            &path,
            reply(first, "a", "2026-11-20T10:00:00Z")
                + &reply(first, "b", "2026-11-20T11:00:00Z")
                + &reply(own, "c", "2026-11-20T23:59:59.9997Z")
                + &reply(own, "d", "2026-11-21T09:00:00Z")
                + &reply(own, "e", "2026-11-21T10:00:00Z"),
        )
        .unwrap();
        let path = path.to_string_lossy().into_owned();
        let price: Price = Box::new(|_, _| Some(0.25));
        let zone = jiff::tz::TimeZone::UTC;
        let at = |text: &str| crate::time::IsoTime::parse(text).unwrap().timestamp(&zone);
        let (whole, tally) =
            spent(&path, "Claude", &price, None, at("2026-11-21T00:00:00Z"), f64::INFINITY, &zone).unwrap();
        assert_eq!(whole.get("requests").and_then(Json::integer), Some(3), "the copied history is not its own");
        // By day in the zone; one by one only since the time asked.
        let days = tally.get("days").and_then(Json::as_object).cloned().unwrap();
        assert_eq!(days.keys().cloned().collect::<Vec<_>>(), ["2026-11-20", "2026-11-21"]);
        assert_eq!(tally.get("spends").and_then(Json::as_array).map(Vec::len), Some(2));
        let parts = vec![(days, tally.get("spends").and_then(Json::as_array).cloned().unwrap())];
        let ranges = vec![
            ("today".to_string(), at("2026-11-21T00:00:00Z"), at("2026-11-21T12:00:00Z")),
            ("yesterday".to_string(), at("2026-11-20T00:00:00Z"), at("2026-11-21T00:00:00Z").next_down()),
            ("24h".to_string(), at("2026-11-21T09:00:00Z"), at("2026-11-21T09:30:00Z")),
        ];
        let costs = range_costs(&parts, &ranges, &zone);
        let requests = |key: &str| costs.get(key).and_then(|figure| figure.get("requests")).and_then(Json::integer);
        // The last moment before midnight is yesterday's; 09:00 is the 24 hours' very start.
        assert_eq!((requests("today"), requests("yesterday"), requests("24h")), (Some(2), Some(1), Some(1)));
        assert_eq!(costs.get("today").and_then(|figure| figure.get("cost")).and_then(Json::python_float), Some(0.5));
        // A tally of another zone is read afresh: the reply at 23:59:59.9997 UTC is the next day in Taipei.
        let taipei = jiff::tz::TimeZone::get("Asia/Taipei").unwrap();
        let (_, again) =
            spent(&path, "Claude", &price, Some(&tally), at("2026-11-21T00:00:00Z"), f64::INFINITY, &taipei).unwrap();
        let days = again.get("days").and_then(Json::as_object).cloned().unwrap();
        assert_eq!(days.get("2026-11-21").and_then(|day| day.as_array()).and_then(|day| day[0].integer()), Some(3));
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_transcript_written_over_in_place_is_counted_again() {
        // SESSION-6: changed without growing, or changed above the new lines, it is read again from the start; grown
        // only, it is read on from where it stopped.
        let folder = std::env::temp_dir().join(format!("starmeter-rewritten-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("session.jsonl");
        let path = file.to_string_lossy().into_owned();
        let reply = |id: &str, input: i64| {
            format!(
                "{{\"type\":\"assistant\",\"timestamp\":\"2026-11-21T10:00:00Z\",\"message\":{{\"id\":\"{id}\",\"model\":\"m\",\"usage\":{{\"input_tokens\":{input},\"output_tokens\":0}}}}}}\n"
            )
        };
        let price: Price =
            Box::new(|_, usage| Some(usage.get("input_tokens").and_then(Json::integer)? as f64 / 1_000_000.0 * 3.0));
        let utc = jiff::tz::TimeZone::UTC;
        let cost = |figure: &Json| figure.get("cost").and_then(Json::python_float).unwrap();
        let fresh = || spent(&path, "Claude", &price, None, 0.0, f64::INFINITY, &utc).unwrap().0;
        std::fs::write(&file, reply("a", 1_000_000)).unwrap();
        let (first, tally) = spent(&path, "Claude", &price, None, 0.0, f64::INFINITY, &utc).unwrap();
        assert_eq!(cost(&first), 3.0);
        // 1,000,000 input tokens written over as 2,000,000: the same file, the same length.
        std::fs::write(&file, reply("a", 2_000_000)).unwrap();
        let (again, tally) = spent(&path, "Claude", &price, Some(&tally), 0.0, f64::INFINITY, &utc).unwrap();
        assert_eq!(cost(&again), 6.0);
        assert_eq!(again.serialized(), fresh().serialized());
        // Written over again and grown: the new line alone would keep the old count above it.
        std::fs::write(&file, reply("a", 3_000_000) + &reply("b", 1_000_000)).unwrap();
        let (grown, tally) = spent(&path, "Claude", &price, Some(&tally), 0.0, f64::INFINITY, &utc).unwrap();
        assert_eq!((grown.get("requests").and_then(Json::integer), cost(&grown)), (Some(2), 12.0));
        // Only grown: read on, and the same as a whole reading.
        std::fs::OpenOptions::new()
            .append(true)
            .open(&file)
            .unwrap()
            .write_all(reply("c", 1_000_000).as_bytes())
            .unwrap();
        let (resumed, _) = spent(&path, "Claude", &price, Some(&tally), 0.0, f64::INFINITY, &utc).unwrap();
        assert_eq!(cost(&resumed), 15.0);
        assert_eq!(resumed.serialized(), fresh().serialized());
        // Written over far above where it stopped, past any last few kilobytes (Codex's 13,334 bytes and more below
        // the change), and grown in the same round: read again from the start.
        let filler: String = (0..120).map(|k| reply(&format!("f{k:03}"), 0)).collect();
        assert!(filler.len() > 13_334);
        std::fs::write(&file, reply("a", 1_000_000) + &filler).unwrap();
        let (_, tally) = spent(&path, "Claude", &price, None, 0.0, f64::INFINITY, &utc).unwrap();
        std::fs::write(&file, reply("a", 2_000_000) + &filler + &reply("z", 1_000_000)).unwrap();
        let (far, _) = spent(&path, "Claude", &price, Some(&tally), 0.0, f64::INFINITY, &utc).unwrap();
        // The filler's lines have no tokens: no requests, as the usage leaves them out.
        assert_eq!((far.get("requests").and_then(Json::integer), cost(&far)), (Some(2), 9.0));
        assert_eq!(far.serialized(), fresh().serialized());
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_streamed_reply_counts_once_with_its_fullest_line() {
        // Claude Code writes a reply over several lines, its output as it goes: once, at its first line's time, with
        // the most any line says, whether the last line came in the same reading or a later one. Its own notices
        // (<synthetic>) and lines with no tokens are no requests; lines with neither id are each a reply.
        let folder = std::env::temp_dir().join(format!("starmeter-streamed-{}", crate::runtime::random_hex(6)));
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("session.jsonl");
        let path = file.to_string_lossy().into_owned();
        let line = |id: Option<&str>, model: &str, at: &str, output: i64| {
            let id = id.map_or(String::new(), |id| format!("\"id\":\"{id}\","));
            format!(
                "{{\"type\":\"assistant\",\"timestamp\":\"2026-11-21T10:00:{at}Z\",\"message\":{{{id}\"model\":\"{model}\",\"usage\":{{\"input_tokens\":100,\"output_tokens\":{output}}}}}}}\n"
            )
        };
        let price: Price =
            Box::new(|_, usage| Some(usage.get("output_tokens").and_then(Json::integer)? as f64 / 100.0));
        let utc = jiff::tz::TimeZone::UTC;
        let figures = |figure: &Json| {
            (figure.get("requests").and_then(Json::integer), figure.get("cost").and_then(Json::python_float))
        };
        std::fs::write(&file, line(Some("m1"), "claude-opus-5-5", "01", 1)).unwrap();
        let (first, tally) = spent(&path, "Claude", &price, None, 0.0, f64::INFINITY, &utc).unwrap();
        assert_eq!(figures(&first), (Some(1), Some(0.01)));
        let rest = line(Some("m1"), "claude-opus-5-5", "02", 400)
            + &line(Some("m1"), "claude-opus-5-5", "03", 300)
            + &line(Some("n1"), "<synthetic>", "04", 0)
            + &line(None, "claude-opus-5-5", "05", 100)
            + &line(None, "claude-opus-5-5", "06", 100);
        std::fs::OpenOptions::new().append(true).open(&file).unwrap().write_all(rest.as_bytes()).unwrap();
        let (resumed, _) = spent(&path, "Claude", &price, Some(&tally), 0.0, f64::INFINITY, &utc).unwrap();
        let (fresh, kept) = spent(&path, "Claude", &price, None, 0.0, f64::INFINITY, &utc).unwrap();
        assert_eq!(figures(&fresh), (Some(3), Some(6.0)));
        assert_eq!(resumed.serialized(), fresh.serialized());
        // Its day and its place among the requests since `since` follow the fullest line too, at the first line's time.
        let day = kept.get("days").and_then(|days| days.get("2026-11-21")).cloned().unwrap();
        assert_eq!(day.serialized(), "[3, 6.0, 0]");
        let first_spend =
            kept.get("spends").and_then(Json::as_array).and_then(|spends| spends.first()).cloned().unwrap();
        assert_eq!(first_spend.as_array().and_then(|spend| spend.get(1)).and_then(Json::number), Some(4.0));
        // The turns: one reply with its 500 tokens, then the two of their own.
        let turns = claude_turns(&path, &price).unwrap();
        let figure = |key: &str| turns[0].get(key).cloned().unwrap();
        assert_eq!(
            (figure("requests"), figure("tokens"), figure("cost")),
            (Json::Int(3), Json::Int(900), Json::Double(6.0))
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_turn_begun_before_the_part_read_starts_at_its_prompt_further_back() {
        // A turn of many tool outputs (screenshots above all) runs past the 2 MB read for the turns: its start is the
        // last prompt further back, not none (its clock said only 运行中); read again as it grows, the same, and a new
        // prompt starts a new turn at its own time.
        let folder = std::env::temp_dir().join(format!("starmeter-turn-start-{}", crate::runtime::random_hex(6)));
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("session.jsonl");
        let path = file.to_string_lossy().into_owned();
        let prompt = |at: &str| {
            format!(
                "{{\"type\":\"user\",\"timestamp\":\"2026-11-26T{at}Z\",\"message\":{{\"role\":\"user\",\"content\":\"go on\"}}}}\n"
            )
        };
        let work = |at: &str, n: usize| {
            let mut lines = String::new();
            for i in 0..n {
                lines += &format!(
                    "{{\"type\":\"assistant\",\"timestamp\":\"2026-11-26T{at}Z\",\"message\":{{\"id\":\"m{at}{i}\",\"model\":\"claude-opus-5-5\",\"content\":[{{\"type\":\"tool_use\",\"name\":\"Bash\"}}],\"usage\":{{\"input_tokens\":100,\"output_tokens\":10}}}}}}\n"
                );
                lines += &format!(
                    "{{\"type\":\"user\",\"timestamp\":\"2026-11-26T{at}Z\",\"message\":{{\"role\":\"user\",\"content\":[{{\"type\":\"tool_result\",\"content\":\"{}\"}}]}}}}\n",
                    "x".repeat(100_000)
                );
            }
            lines
        };
        let price: Price = Box::new(|_, _| Some(0.01));
        let started = |turns: &[Json]| turns[0].get("at").and_then(Json::number);
        let at = |hms: &str| {
            crate::time::IsoTime::parse(&format!("2026-11-26T{hms}Z")).unwrap().timestamp(&jiff::tz::TimeZone::system())
        };
        std::fs::write(&file, prompt("09:00:00") + &work("09:00:05", 40)).unwrap();
        let turns = claude_turns(&path, &price).unwrap();
        assert!(std::fs::metadata(&file).unwrap().len() > 3 * 1024 * 1024);
        assert_eq!(started(&turns), Some(at("09:00:00")), "its prompt 4 MB back");
        // Read again as it grows (from where it was read up to the last time): the same start.
        std::fs::OpenOptions::new()
            .append(true)
            .open(&file)
            .unwrap()
            .write_all(work("09:10:00", 30).as_bytes())
            .unwrap();
        assert_eq!(started(&claude_turns(&path, &price).unwrap()), Some(at("09:00:00")));
        // A new prompt: a new turn at its time, also once it is pushed out of the part read.
        std::fs::OpenOptions::new()
            .append(true)
            .open(&file)
            .unwrap()
            .write_all((prompt("09:20:00") + &work("09:20:05", 30)).as_bytes())
            .unwrap();
        assert_eq!(started(&claude_turns(&path, &price).unwrap()), Some(at("09:20:00")));
        // A tool's result, a subagent's line or a note is no prompt.
        let note = "{\"type\":\"user\",\"isMeta\":true,\"timestamp\":\"2026-11-26T08:35:00Z\",\"message\":{\"content\":\"[Image: source: shot.png]\"}}\n";
        std::fs::write(&file, prompt("08:00:00") + "{\"type\":\"user\",\"isSidechain\":true,\"timestamp\":\"2026-11-26T08:30:00Z\",\"message\":{\"content\":\"sub\"}}\n" + note + &work("08:40:00", 40)).unwrap();
        super::PROMPTS.lock().unwrap().take();
        assert_eq!(started(&claude_turns(&path, &price).unwrap()), Some(at("08:00:00")));
        // A message from another session sets it to work as a prompt does: its turn begins there, not at the user's
        // last words hours before (用户：这个20小时3分钟是累积时间？).
        let peer = "{\"type\":\"user\",\"isMeta\":true,\"origin\":{\"kind\":\"peer\",\"name\":\"main\"},\"timestamp\":\"2026-11-26T09:30:00Z\",\"message\":{\"role\":\"user\",\"content\":\"Another Claude session sent a message\"}}\n";
        std::fs::write(&file, prompt("08:00:00") + &work("08:00:05", 5) + peer + &work("09:30:05", 40)).unwrap();
        super::PROMPTS.lock().unwrap().take();
        assert_eq!(started(&claude_turns(&path, &price).unwrap()), Some(at("09:30:00")));
        // R10-12: a transient I/O error (missing file) must not poison PROMPTS by advancing `upto` past unread bytes.
        let missing = folder.join("missing.jsonl");
        let missing_path = missing.to_string_lossy().to_string();
        assert_eq!(super::prompt_before(&missing_path, 4096, ""), Json::Null);
        std::fs::write(&missing, prompt("10:00:00") + &work("10:00:05", 5)).unwrap();
        assert_eq!(super::prompt_before(&missing_path, 4096, ""), Json::Double(at("10:00:00")));
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_read_digest_follows_every_byte_read() {
        // Any byte changed, wherever it is, changes it; the same bytes give the same digest at every length.
        let folder = std::env::temp_dir().join(format!("starmeter-digest-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("bytes");
        let path = file.to_string_lossy().into_owned();
        let bytes: Vec<u8> = (0..3_000_000u32).map(|k| (k * 7 % 251) as u8).collect();
        std::fs::write(&file, &bytes).unwrap();
        let lengths = [0, 1, 31, 32, 33, 1 << 20, (1 << 20) + 5, bytes.len() as u64];
        let before: Vec<_> = lengths.iter().map(|&n| read_digest(&path, n).unwrap()).collect();
        assert_eq!(before.iter().collect::<HashSet<_>>().len(), lengths.len());
        for at in [0, 17, 1 << 20, (1 << 20) + 3, bytes.len() - 1] {
            let mut changed = bytes.clone();
            changed[at] ^= 1;
            std::fs::write(&file, &changed).unwrap();
            assert_ne!(read_digest(&path, bytes.len() as u64), before.last().cloned(), "byte {at}");
        }
        std::fs::write(&file, &bytes).unwrap();
        assert_eq!(read_digest(&path, bytes.len() as u64), before.last().cloned());
        assert_eq!(read_digest(&path, bytes.len() as u64 + 1), None);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_new_counting_zone_puts_a_kept_sessions_cost_in_its_own_days() {
        // SESSION-5: $3 before Taipei's midnight, $6 after. Taipei's today is $6; in UTC both are today's, $9, though the
        // transcript is unchanged and its tally kept.
        let folder = std::env::temp_dir().join(format!("starmeter-zones-{}", std::process::id()));
        let file = folder.join("p").join("session.jsonl");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let reply = |id: &str, at: &str, output: i64| {
            format!(
                "{{\"type\":\"assistant\",\"timestamp\":\"{at}\",\"message\":{{\"id\":\"{id}\",\"model\":\"m\",\"stop_reason\":\"end_turn\",\"usage\":{{\"input_tokens\":1,\"output_tokens\":{output}}}}}}}\n"
            )
        };
        std::fs::write(&file, reply("a", "2026-11-20T15:30:00Z", 3) + &reply("b", "2026-11-20T16:30:00Z", 6)).unwrap();
        let utc = jiff::tz::TimeZone::UTC;
        let taipei = jiff::tz::TimeZone::get("Asia/Taipei").unwrap();
        let at = |text: &str| crate::time::IsoTime::parse(text).unwrap().timestamp(&utc);
        let now = at("2026-11-20T20:00:00Z");
        let written = std::time::UNIX_EPOCH + std::time::Duration::from_secs_f64(now - 60.0);
        std::fs::File::options().write(true).open(&file).unwrap().set_modified(written).unwrap();
        let price: Price = Box::new(|_, usage| Some(usage.get("output_tokens").and_then(Json::integer)? as f64));
        let prices = HashMap::from([("Claude", price)]);
        let roots = [folder.to_string_lossy().into_owned()];
        let today = |found: &[Json]| {
            found[0]
                .get("spent")
                .and_then(|spent| spent.get("ranges"))
                .and_then(|ranges| ranges.get("today"))
                .and_then(|today| today.get("cost"))
                .and_then(Json::python_float)
        };
        let in_taipei = [("today".to_string(), at("2026-11-20T16:00:00Z"), now)];
        let in_utc = [("today".to_string(), at("2026-11-20T00:00:00Z"), now)];
        let mut tallies = Object::new();
        let first = active(
            &roots,
            None,
            now,
            Some(&prices),
            &HashMap::new(),
            Some(&mut tallies),
            super::Counting { ranges: &in_taipei, zone: &taipei },
        );
        assert_eq!(today(&first), Some(6.0));
        let again = active(
            &roots,
            None,
            now,
            Some(&prices),
            &HashMap::new(),
            Some(&mut tallies),
            super::Counting { ranges: &in_utc, zone: &utc },
        );
        assert_eq!(today(&again), Some(9.0));
        let fresh = active(
            &roots,
            None,
            now,
            Some(&prices),
            &HashMap::new(),
            None,
            super::Counting { ranges: &in_utc, zone: &utc },
        );
        assert_eq!(today(&fresh), Some(9.0));
        // Back in Taipei, from the UTC tally: $6 again.
        let back = active(
            &roots,
            None,
            now,
            Some(&prices),
            &HashMap::new(),
            Some(&mut tallies),
            super::Counting { ranges: &in_taipei, zone: &taipei },
        );
        assert_eq!(today(&back), Some(6.0));
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_request_timed_ahead_of_the_clock_counts_once_its_time_comes() {
        // SESSION-8: a computer whose clock runs five minutes ahead wrote $6 at 20:05; it is 20:00 here. Every range
        // ends now, so today, the last 24 hours and the last 7 days all say $3, as the usage does; at 20:10 the same
        // transcript, unchanged and its tally kept, says $9 in each, as a reading afresh does.
        let folder = std::env::temp_dir().join(format!("starmeter-ahead-{}", std::process::id()));
        let file = folder.join("p").join("session.jsonl");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let reply = |id: &str, at: &str, output: i64| {
            format!(
                "{{\"type\":\"assistant\",\"timestamp\":\"{at}\",\"message\":{{\"id\":\"{id}\",\"model\":\"m\",\"stop_reason\":\"end_turn\",\"usage\":{{\"input_tokens\":1,\"output_tokens\":{output}}}}}}}\n"
            )
        };
        std::fs::write(&file, reply("a", "2026-11-20T19:59:00Z", 3) + &reply("b", "2026-11-20T20:05:00Z", 6)).unwrap();
        let utc = jiff::tz::TimeZone::UTC;
        let at = |text: &str| crate::time::IsoTime::parse(text).unwrap().timestamp(&utc);
        let now = at("2026-11-20T20:00:00Z");
        let written = std::time::UNIX_EPOCH + std::time::Duration::from_secs_f64(now - 30.0);
        std::fs::File::options().write(true).open(&file).unwrap().set_modified(written).unwrap();
        let price: Price = Box::new(|_, usage| Some(usage.get("output_tokens").and_then(Json::integer)? as f64));
        let prices = HashMap::from([("Claude", price)]);
        let roots = [folder.to_string_lossy().into_owned()];
        let ranges = |now: f64| {
            vec![
                ("today".to_string(), at("2026-11-20T00:00:00Z"), now),
                ("24h".to_string(), now - 86400.0, now),
                ("7d".to_string(), at("2026-11-14T00:00:00Z"), now),
            ]
        };
        let figures = |found: &[Json]| {
            let spent = found[0].get("spent").cloned().unwrap();
            let range = |key: &str| {
                let figure = spent.get("ranges").and_then(|ranges| ranges.get(key)).cloned().unwrap();
                (figure.get("requests").and_then(Json::integer), figure.get("cost").and_then(Json::python_float))
            };
            let whole = (spent.get("requests").and_then(Json::integer), spent.get("cost").and_then(Json::python_float));
            [whole, range("today"), range("24h"), range("7d")]
        };
        let read = |now: f64, tallies: Option<&mut Object>| {
            let ranges = ranges(now);
            active(
                &roots,
                None,
                now,
                Some(&prices),
                &HashMap::new(),
                tallies,
                super::Counting { ranges: &ranges, zone: &utc },
            )
        };
        let mut tallies = Object::new();
        assert_eq!(figures(&read(now, Some(&mut tallies))), [(Some(1), Some(3.0)); 4]);
        assert_eq!(figures(&read(now, None)), [(Some(1), Some(3.0)); 4]);
        let later = at("2026-11-20T20:10:00Z");
        assert_eq!(figures(&read(later, Some(&mut tallies))), [(Some(2), Some(9.0)); 4]);
        assert_eq!(figures(&read(later, None)), [(Some(2), Some(9.0)); 4]);
        // Counted once: read again with the settled tally, still $9.
        assert_eq!(figures(&read(later, Some(&mut tallies))), [(Some(2), Some(9.0)); 4]);
        // SESSION-9: the clock set back to 20:04 puts the $6 of 20:05 ahead again: $3 everywhere, with the tally kept
        // as afresh, and $9 again once 20:05 has passed.
        let back = at("2026-11-20T20:04:00Z");
        assert_eq!(figures(&read(back, Some(&mut tallies))), [(Some(1), Some(3.0)); 4]);
        assert_eq!(figures(&read(back, None)), [(Some(1), Some(3.0)); 4]);
        assert_eq!(figures(&read(back, Some(&mut tallies))), [(Some(1), Some(3.0)); 4]);
        assert_eq!(figures(&read(later, Some(&mut tallies))), [(Some(2), Some(9.0)); 4]);
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
        let first = active(
            &roots,
            None,
            now,
            Some(&prices),
            &HashMap::new(),
            Some(&mut tallies),
            super::Counting { ranges: &[], zone: &jiff::tz::TimeZone::UTC },
        );
        assert_eq!(state(&first).as_deref(), Some("waiting"));
        // Rewritten in place to the same length and time: taken for unchanged, what was read before serves.
        std::fs::write(&path, turn("tool_use")).unwrap();
        std::fs::File::options().write(true).open(&path).unwrap().set_modified(written).unwrap();
        let again = active(
            &roots,
            None,
            now,
            Some(&prices),
            &HashMap::new(),
            Some(&mut tallies),
            super::Counting { ranges: &[], zone: &jiff::tz::TimeZone::UTC },
        );
        assert_eq!(state(&again).as_deref(), Some("waiting"));
        assert_eq!(again[0].get("spent"), first[0].get("spent"));
        let fresh = active(
            &roots,
            None,
            now,
            Some(&prices),
            &HashMap::new(),
            None,
            super::Counting { ranges: &[], zone: &jiff::tz::TimeZone::UTC },
        );
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
        let read = |tallies: &mut Object| {
            active(
                &[],
                Some(&codex),
                now,
                Some(&prices),
                &HashMap::new(),
                Some(tallies),
                super::Counting { ranges: &[], zone: &jiff::tz::TimeZone::UTC },
            )
        };
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
