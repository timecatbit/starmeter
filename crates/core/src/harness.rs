//! The Harness readers other than Claude's: Codex, DeepSeek Harness, WorkBuddy, Qoder CN, Magpie and Gemini CLI
//! logs, OpenCode's database, and the gateways' own logs. Only usage counters are kept, never message bodies. Each
//! event is kept as the JSON text 5.x writes, so their index files are interchangeable.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use jiff::tz::TimeZone;

use crate::claude_usage::state_of;
use crate::database::{self, Database, Mode, Value};
use crate::files::{self, paths};
use crate::json::{Json, Object, Selection};
use crate::lines::{ReadFailure, lines_in, read_lines};
use crate::py::{self, FileError, LineError, ReadError};
use crate::tally::{self, Carrier, Event, Price, archive, transaction};
use crate::{Fallible, object, pricing, sessions, zstd};

const SERVICE_TIERS: [(&str, &str); 6] = [
    ("default", "standard"),
    ("standard", "standard"),
    ("fast", "fast"),
    ("priority", "fast"),
    ("flex", "flex"),
    ("ultrafast", "ultrafast"),
];

fn service_tier(name: &str) -> Option<&'static str> {
    SERVICE_TIERS.iter().find(|(key, _)| *key == name).map(|(_, tier)| *tier)
}

/// A source's index version rises when what its events hold changes, so old indexes are rebuilt.
pub fn index_version(source: &str) -> i64 {
    match source {
        "magpie" | "codex" => 6,
        "qoder" => 5,
        _ => 4,
    }
}

/// A ChatGPT app chat with no project folder runs in a folder the app makes for it, Documents/Codex/<day>/<slug>,
/// named after the first words typed. Its requests keep the chat they belong to (a Guardian review its parent
/// chat) and take the chat's title when a range is read; ChatGPT 对话 until it has one.
pub const CHAT_MARK: &str = "\u{1}";

/// Whether a working directory is one the ChatGPT app made for a chat: …/Codex/YYYY-MM-DD/<slug>.
fn chat_folder(path: &str) -> bool {
    let path = path.strip_suffix('/').unwrap_or(path);
    let Some((head, slug)) = path.rsplit_once('/') else { return false };
    let Some((head, date)) = head.rsplit_once('/') else { return false };
    let day = date.as_bytes();
    !slug.is_empty()
        && day.len() == 10
        && day
            .iter()
            .enumerate()
            .all(|(index, byte)| if index == 4 || index == 7 { *byte == b'-' } else { byte.is_ascii_digit() })
        && head.ends_with("/Codex")
}

/// A ChatGPT project's own folder, .chatgpt-projects/g-p-<id>: its ID, whose name comes from the app.
fn project_folder(path: &str) -> Option<&str> {
    let path = path.strip_suffix('/').unwrap_or(path);
    let (head, id) = path.rsplit_once('/')?;
    let hex = id.strip_prefix("g-p-")?;
    (!hex.is_empty()
        && hex.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        && head.ends_with("/.chatgpt-projects"))
    .then_some(id)
}

fn codex_project(data: &Json, session: &str) -> Result<Json, LineError> {
    if let Some(cwd) = py::get(data, "cwd")?.and_then(Json::as_str) {
        let path = cwd.trim().replace('\\', "/");
        if chat_folder(&path) {
            let parent = py::or(py::get(data, "parent_thread_id")?, Json::from(session));
            return Ok(Json::from(format!("{CHAT_MARK}{}", py::str(Some(&parent)))));
        }
        if let Some(id) = project_folder(&path) {
            return Ok(Json::from(format!("{CHAT_MARK}{id}")));
        }
    }
    Ok(py::project_name(py::get(data, "cwd")?))
}

/// Chat titles by thread ID, and ChatGPT projects' names by project ID from the app's own state (only those).
pub fn codex_names(root: &Path) -> HashMap<String, String> {
    let mut names = sessions::codex_titles(root);
    let projects = std::fs::read(root.join(".codex-global-state.json"))
        .ok()
        .and_then(|data| Json::parse(&data).ok())
        .and_then(|value| value.get("local-projects").and_then(Json::as_object).cloned());
    for (key, project) in projects.into_iter().flatten() {
        let Some(name) = project.get("name").and_then(Json::as_str).map(str::trim).filter(|name| !name.is_empty())
        else {
            continue;
        };
        names.entry(key).or_insert_with(|| name.to_string());
    }
    names
}

pub fn chat_projects(events: Vec<Event>, names: &HashMap<String, String>) -> Vec<Event> {
    events
        .into_iter()
        .map(|mut event| {
            if let Some(key) =
                event.get("project").and_then(Json::as_str).and_then(|p| p.strip_prefix(CHAT_MARK)).map(String::from)
            {
                let name = names.get(&key).cloned().unwrap_or_else(|| {
                    if key.starts_with("g-p-") { "ChatGPT 项目".into() } else { "ChatGPT 对话".into() }
                });
                event.insert("project", name);
            }
            event
        })
        .collect()
}

// Events

/// The engine's `metrics()`: a model name and counters as `number()` makes them.
fn metrics(
    model: Option<&Json>,
    inputs: Option<&Json>,
    output: Option<&Json>,
    cached: Option<&Json>,
    writes: Option<&Json>,
    cost: Option<&Json>,
) -> Event {
    let mut event = Event::new();
    event.insert("model", py::str(Some(&py::or(model, Json::from("未知模型")))));
    event.insert("input", py::number(inputs));
    event.insert("output", py::number(output));
    event.insert("cached", py::number(cached));
    event.insert("writes", py::number(writes));
    let priced = cost.filter(|cost| cost.number().is_some() || cost.as_bool().is_some()).and_then(|cost| {
        cost.python_float().filter(|value| value.is_finite() && *value >= 0.0).map(|_| py::number(Some(cost)))
    });
    event.insert("cost", priced.unwrap_or(Json::Null));
    event
}

const ZERO: Json = Json::Int(0);

/// min(a, b) of two number() results: the first unless the second is smaller.
fn smaller(a: Json, b: Json) -> Json {
    if b.number().unwrap_or(0.0) < a.number().unwrap_or(0.0) { b } else { a }
}

/// a - b where both came from number(): an int only when both are ints.
fn minus(a: &Json, b: &Json) -> Json {
    match (a, b) {
        (Json::Int(x), Json::Int(y)) => Json::Int(x - y),
        _ => Json::Double(a.number().unwrap_or(0.0) - b.number().unwrap_or(0.0)),
    }
}

fn plus(a: &Json, b: &Json) -> Json {
    tally::add(a, b)
}

fn kind_is(row: &Object, kind: &str) -> bool {
    row.get("type") == Some(&Json::from(kind))
}

fn codex_event(row: &Object, state: &mut Object) -> Result<Option<Event>, ReadError> {
    let data = py::dict(row.get("payload"));
    if kind_is(row, "event_msg") && py::get(&data, "type")? == Some(&Json::from("thread_settings_applied")) {
        // A settings event without a tier says nothing; an unknown tier must not inherit the old one.
        if let Some(settings) = py::get(&data, "thread_settings")?.and_then(Json::as_object)
            && let Some(tier) = settings.get("service_tier").filter(|tier| !tier.is_null())
        {
            state.insert("tier", service_tier(&py::str(Some(tier))).map_or(Json::Null, Json::from));
        }
        return Ok(None);
    }
    if kind_is(row, "session_meta") {
        let id = py::get(&data, "id")?;
        let fallback = py::get(&data, "session_id")?.cloned().unwrap_or(Json::Null);
        let session = py::or(Some(&py::or(id, fallback)), state.get("session").cloned().unwrap_or(Json::Null));
        let session = py::str(Some(&session));
        state.insert("session", session.as_str());
        state.insert("provider", py::str(Some(&py::or(py::get(&data, "model_provider")?, Json::from("")))));
        state.insert("project", codex_project(&data, &session)?);
    } else if kind_is(row, "turn_context") {
        let model = py::or(
            Some(&py::or(py::get(&data, "model")?, state.get("model").cloned().unwrap_or(Json::Null))),
            Json::from("未知模型"),
        );
        state.insert("model", py::str(Some(&model)));
    } else if kind_is(row, "event_msg") && py::get(&data, "type")? == Some(&Json::from("token_count")) {
        let info = py::dict(py::get(&data, "info")?);
        let total = py::dict(py::get(&info, "total_token_usage")?);
        let last = py::dict(py::get(&info, "last_token_usage")?);
        if !total.is_truthy() && !last.is_truthy() {
            return Ok(None);
        }
        let previous = state.get("total").cloned();
        if total.is_truthy() {
            state.insert("total", total.clone());
        }
        if total.is_truthy() && previous.as_ref() == Some(&total) {
            return Ok(None);
        }
        // ccusage prefers a recorded request delta; cumulative totals are a fallback, since their jump can include
        // inherited or compacted context, not just this request.
        let values = if last.is_truthy() {
            last.clone()
        } else {
            let before = py::dict(previous.as_ref());
            let mut delta = Object::new();
            for key in ["input_tokens", "cached_input_tokens", "output_tokens", "total_tokens"] {
                let difference = minus(&py::number(py::get(&total, key)?), &py::number(py::get(&before, key)?));
                delta.insert(key, if difference.number().unwrap_or(0.0) > 0.0 { difference } else { ZERO });
            }
            Json::Object(delta)
        };
        let inputs = py::number(py::get(&values, "input_tokens")?);
        let cached = smaller(inputs.clone(), py::number(py::get(&values, "cached_input_tokens")?));
        let mut result = metrics(
            state.get("model"),
            Some(&minus(&inputs, &cached)),
            py::get(&values, "output_tokens")?,
            Some(&cached),
            Some(&ZERO),
            None,
        );
        if inputs.number().unwrap_or(0.0) + result.get("output").and_then(Json::number).unwrap_or(0.0) <= 0.0 {
            return Ok(None);
        }
        let session = state.get("session").cloned().ok_or(LineError)?;
        let identifying = if total.is_truthy() {
            total
        } else {
            let time = row.get("timestamp").cloned().ok_or(LineError)?;
            Json::Array(vec![time, last])
        };
        result.insert("id", py::identity(vec!["codex".into(), session.clone(), identifying]));
        result.insert("session", py::identity(vec![session]));
        let time = row.get("timestamp").ok_or(LineError)?;
        result.insert("timestamp", py::stamp(Some(time))?);
        result.insert("provider", state.get("provider").cloned().unwrap_or(Json::from("")));
        result.insert("tier", state.get("tier").cloned().unwrap_or(Json::Null));
        result.insert("project", state.get("project").cloned().unwrap_or(Json::Null));
        return Ok(Some(result));
    }
    Ok(None)
}

fn deepseek_event(row: &Object, state: &mut Object) -> Result<Option<Event>, ReadError> {
    let data = py::dict(row.get("data"));
    if kind_is(row, "session") {
        let version = py::int(Some(row.get("version").unwrap_or(&ZERO)))?;
        if ![2, 3, 4].contains(&version) {
            return Err(FileError::new("不支持的 Harness 日志版本").into());
        }
        let seeded = row.get("isSeeded").is_some_and(Json::is_truthy);
        let session = py::or(row.get("id"), state.get("session").cloned().unwrap_or(Json::Null));
        state.insert("session", py::str(Some(&session)));
        state.insert("isSeeded", seeded);
        state.insert("seeded", seeded);
        state.insert("seedLength", py::int(Some(row.get("seedLength").unwrap_or(&ZERO)))?);
    } else if kind_is(row, "session/end-seed") && py::get(&data, "inherited")?.is_some_and(Json::is_truthy) {
        state.insert("seeded", false);
    } else if kind_is(row, "request/header") {
        let header = py::dict(py::get(&data, "header")?);
        let config = py::or(py::get(&header, "config")?, header.clone());
        let fields = config.as_object().ok_or(LineError)?;
        let model = fields.get("model").or(state.get("model")).cloned().unwrap_or(Json::Null);
        let provider = fields.get("provider").or(state.get("provider")).cloned().unwrap_or(Json::from(""));
        state.insert("model", model);
        state.insert("provider", provider);
    } else if kind_is(row, "llm/retry-started") {
        let attempt = py::int(Some(state.get("attempt").unwrap_or(&ZERO)))? + 1;
        state.insert("attempt", attempt);
    } else if kind_is(row, "assistant/message") || kind_is(row, "assistant/attempt") {
        if state.get("seeded").is_some_and(Json::is_truthy)
            || py::num(row.get("seq")) < state.get("seedLength").and_then(Json::number).unwrap_or(0.0)
        {
            return Ok(None);
        }
        let mut usage: Option<Json> =
            if kind_is(row, "assistant/message") { py::get(&data, "usage")?.cloned() } else { None };
        if usage.as_ref().is_none_or(Json::is_null) {
            usage = None;
            let stream = py::or(py::get(&data, "stream")?, Json::Array(vec![]));
            match &stream {
                Json::Array(chunks) => {
                    for item in chunks {
                        let chunk = match item.as_object() {
                            Some(object) => object.get("chunk").cloned().unwrap_or_else(|| item.clone()),
                            None => Json::Object(Object::new()),
                        };
                        if py::get(&chunk, "type")? == Some(&Json::from("usage")) {
                            usage = Some(py::get(&chunk, "usage")?.cloned().unwrap_or(Json::Null));
                        }
                    }
                }
                Json::String(_) | Json::Object(_) => {}
                _ => return Err(LineError.into()),
            }
        }
        let Some(usage) = usage.filter(|usage| usage.as_object().is_some()) else { return Ok(None) };
        let message = py::dict(py::get(&data, "message")?);
        let model = py::or(py::get(&message, "model")?, state.get("model").cloned().unwrap_or(Json::Null));
        let mut result = metrics(
            Some(&model),
            usage.get("inputTokens"),
            usage.get("outputTokens"),
            usage.get("cacheReadTokens"),
            usage.get("cacheWriteTokens"),
            None,
        );
        let session = state.get("session").cloned().ok_or(LineError)?;
        let turn = py::get(&data, "turn")?.cloned().unwrap_or(Json::Null);
        let step = py::get(&data, "step")?.cloned().unwrap_or(Json::Null);
        let attempt = state.get("attempt").cloned().unwrap_or(ZERO);
        result.insert("id", py::identity(vec!["dsh".into(), session.clone(), turn, step, attempt]));
        result.insert("session", py::identity(vec![session]));
        let time = row.get("time").ok_or(LineError)?;
        result.insert("timestamp", py::stamp(Some(time))?);
        result.insert(
            "provider",
            py::or(py::get(&message, "provider")?, state.get("provider").cloned().unwrap_or(Json::from(""))),
        );
        return Ok(Some(result));
    }
    Ok(None)
}

/// WorkBuddy project transcripts, including tool calls with the same response ID.
fn workbuddy_event(row: &Object, state: &mut Object) -> Result<Option<Event>, ReadError> {
    if !["message", "function_call", "assistant"].iter().any(|kind| kind_is(row, kind)) {
        return Ok(None);
    }
    let message = py::dict(row.get("message"));
    let provider = py::dict(row.get("providerData"));
    let message_role = py::get(&message, "role")?;
    let role = row.get("role").or(message_role).cloned().unwrap_or(Json::Null);
    if !(role.is_null() || role.as_str() == Some("assistant")) {
        return Ok(None);
    }
    let message_usage = py::get(&message, "usage")?;
    let usage = match message_usage {
        Some(usage) if usage.is_truthy() => usage.clone(),
        _ => py::or(py::get(&provider, "usage")?, Json::Object(Object::new())),
    };
    let Some(fields) = usage.as_object().filter(|fields| !fields.is_empty()) else { return Ok(None) };
    let inputs = py::number(fields.get("input_tokens"));
    let cached = smaller(inputs.clone(), py::number(fields.get("cache_read_input_tokens")));
    // WorkBuddy normalizes inclusive prompt/output counters; cache and reasoning are already part of them.
    let model = py::or(py::get(&provider, "model")?, py::get(&message, "model")?.cloned().unwrap_or(Json::Null));
    let mut event = metrics(
        Some(&model),
        Some(&minus(&inputs, &cached)),
        fields.get("output_tokens"),
        Some(&cached),
        Some(&ZERO),
        None,
    );
    let message_id = py::get(&message, "id")?.cloned().unwrap_or(Json::Null);
    let response = py::or(
        Some(&py::or(py::get(&provider, "messageId")?, message_id)),
        row.get("id").cloned().unwrap_or(Json::Null),
    );
    if !response.is_truthy() {
        return Ok(None);
    }
    event.insert("id", py::identity(vec!["workbuddy".into(), response]));
    event.insert(
        "session",
        py::identity(vec![py::or(row.get("sessionId"), state.get("session").cloned().unwrap_or(Json::Null))]),
    );
    let time = row.get("timestamp").ok_or(LineError)?;
    event.insert("timestamp", py::stamp(Some(time))?);
    event.insert("provider", py::get(&provider, "provider")?.cloned().unwrap_or(Json::from("")));
    Ok(Some(event))
}

/// Qoder CN transcripts: Claude-style assistant rows. Plan-billed requests record Qoder Credits with every token
/// count at zero; Credits are Qoder's own unit, never USD and never converted into tokens.
fn qoder_event(row: &Object, state: &mut Object) -> Result<Option<Event>, ReadError> {
    if !kind_is(row, "assistant") {
        return Ok(None);
    }
    let message = py::dict(row.get("message"));
    let Some(usage) = py::get(&message, "usage")?.and_then(Json::as_object).cloned() else { return Ok(None) };
    let credits = if usage.contains_key("credits") {
        py::number(usage.get("credits"))
    } else {
        py::number(usage.get("original_credits"))
    };
    let mut model = py::str(Some(&py::or(py::get(&message, "model")?, Json::from(""))));
    // Bring-your-own-key models carry a qoder-custom-<id>/ prefix before the provider's model name.
    if model.starts_with("qoder-custom-")
        && let Some(slash) = model.find('/')
    {
        model = model[slash + 1..].to_string();
    }
    let cached =
        py::or(usage.get("cache_read_input_tokens"), usage.get("cached_tokens").cloned().unwrap_or(Json::Null));
    let mut event = metrics(
        Some(&Json::from(model)),
        usage.get("input_tokens"),
        usage.get("output_tokens"),
        Some(&cached),
        usage.get("cache_creation_input_tokens"),
        None,
    );
    let counted: f64 = ["input", "output", "cached", "writes"]
        .iter()
        .map(|key| event.get(key).and_then(Json::number).unwrap_or(0.0))
        .sum();
    if counted <= 0.0 && credits.number().unwrap_or(0.0) <= 0.0 {
        return Ok(None);
    }
    let session = py::str(Some(&py::or(row.get("sessionId"), state.get("session").cloned().unwrap_or(Json::Null))));
    let response = py::or(py::get(&message, "id")?, row.get("uuid").cloned().unwrap_or(Json::Null));
    let given = py::or(row.get("timestamp"), py::get(&message, "timestamp")?.cloned().unwrap_or(Json::Null));
    let when = match py::stamp(Some(&given)) {
        Ok(when) => when,
        Err(ReadError::File(_)) => return Ok(None),
        Err(error) => return Err(error),
    };
    if !response.is_truthy() {
        return Ok(None);
    }
    // A streamed reply is rewritten under the same message ID; the newest copy in a file wins.
    event.insert("id", py::identity(vec!["qoder".into(), session.as_str().into(), response]));
    event.insert("session", py::identity(vec![session.as_str().into()]));
    event.insert("credits", credits);
    event.insert("timestamp", when);
    event.insert("project", py::project_name(row.get("cwd")));
    Ok(Some(event))
}

/// Counters from Magpie's log as Magpie totals them: its gateway's own calls and the calls a client reported to it
/// (source 'reported'), but not its internal probes or refused and handed-on calls.
pub fn magpie_event(row: &Object, offset: i64) -> Result<Option<Event>, ReadError> {
    let source = row.get("source");
    if source.is_some_and(|source| source.is_truthy() && source != &Json::from("reported"))
        || row.get("rejected").is_some_and(Json::is_truthy)
        || row.get("handed_on").is_some_and(Json::is_truthy)
    {
        return Ok(None);
    }
    let (Some(_), Some(model)) =
        (row.get("provider").filter(|p| p.is_truthy()), row.get("model").filter(|m| m.is_truthy()))
    else {
        return Ok(None);
    };
    // A request the upstream refused before replying produced no usage to count.
    if py::num(row.get("status")) >= 400.0 && py::num(row.get("out")) == 0.0 {
        return Ok(None);
    }
    // Magpie already splits fresh input, cache reads and writes; price the actual model, not a routing group.
    let model = py::or(row.get("served"), model.clone());
    let mut event = metrics(
        Some(&model),
        row.get("in"),
        row.get("out"),
        row.get("cache_read"),
        row.get("cache_write"),
        row.get("cost"),
    );
    let time = row.get("t").ok_or(LineError)?;
    let timestamp = py::stamp(Some(time))?;
    // The tier the upstream reported, else the one Magpie asked for: the local client prices by its request too.
    let tier = py::or(row.get("tier_served"), row.get("tier_sent").cloned().unwrap_or(Json::Null));
    if tier.as_array().is_some() || tier.as_object().is_some() {
        return Err(LineError.into());
    }
    event.insert("id", py::identity(vec!["magpie".into(), Json::Int(offset)]));
    event.insert("timestamp", timestamp);
    event.insert("agent", py::str(Some(&py::or(row.get("agent"), Json::from("")))));
    event.insert("reported", source.and_then(Json::as_str) == Some("reported"));
    match row.get("session").filter(|session| session.is_truthy()) {
        Some(session) => event.insert("session", py::identity(vec![session.clone()])),
        None => event.insert("session", Json::Null),
    }
    let writes = event.get("writes").cloned().unwrap_or(ZERO);
    event.insert("write1h", smaller(writes, py::number(row.get("cache_write_1h"))));
    event.insert("cacheKnown", !row.get("cache_unknown").is_some_and(Json::is_truthy));
    event.insert("tier", tier.as_str().and_then(service_tier).map_or(Json::Null, Json::from));
    event.insert("fast", tier.as_str() == Some("fast"));
    Ok(Some(event))
}

fn gemini_event(row: &Object, state: &mut Object) -> Result<Option<Event>, ReadError> {
    if let Some(session) = row.get("sessionId").filter(|session| session.is_truthy()) {
        state.insert("session", py::str(Some(session)));
    }
    let (true, Some(usage), Some(id)) = (
        kind_is(row, "gemini"),
        row.get("tokens").and_then(Json::as_object),
        row.get("id").filter(|id| id.is_truthy()),
    ) else {
        return Ok(None);
    };
    let inputs = py::number(usage.get("input"));
    let cached = smaller(inputs.clone(), py::number(usage.get("cached")));
    let mut output = plus(&py::number(usage.get("output")), &py::number(usage.get("thoughts")));
    let total = py::number(usage.get("total"));
    let mut tool = py::number(usage.get("tool"));
    // Gemini returns thoughts separately from candidate output; its own total keeps a token from counting twice.
    let total_number = total.number().unwrap_or(0.0);
    if total_number >= inputs.number().unwrap_or(0.0) && total_number > 0.0 {
        tool = smaller(tool, minus(&total, &inputs));
        output = minus(&minus(&total, &inputs), &tool);
    }
    let mut result = metrics(
        row.get("model"),
        Some(&plus(&minus(&inputs, &cached), &tool)),
        Some(&output),
        Some(&cached),
        Some(&ZERO),
        None,
    );
    result.insert("id", py::identity(vec!["gemini".into(), id.clone()]));
    result.insert("session", py::identity(vec![state.get("session").cloned().unwrap_or(Json::Null)]));
    let time = row.get("timestamp").ok_or_else(|| FileError::new("KeyError: timestamp"))?;
    result.insert("timestamp", py::stamp(Some(time))?);
    result.insert("provider", "google");
    Ok(Some(result))
}

// Files

pub fn sorted_paths(mut paths: Vec<String>) -> Vec<String> {
    paths.sort_by(|a, b| a.split('/').cmp(b.split('/')));
    paths
}

/// Every file under root, as root + relative path, without following linked folders.
pub fn walk(root: &str) -> Vec<String> {
    // Joined with '/', on Windows too (which takes either separator), so a file's name and its parent folders are
    // always the parts after the last slashes.
    let join = |folder: &str, name: &str| {
        let base = folder.trim_end_matches(['/', '\\']);
        format!("{base}/{name}")
    };
    let mut found = Vec::new();
    let mut pending = vec![root.to_string()];
    while let Some(folder) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else { continue };
        let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = join(&folder, &entry.file_name().to_string_lossy());
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() {
                pending.push(path);
            } else if std::fs::metadata(&path).is_ok_and(|metadata| !metadata.is_dir()) {
                found.push(path);
            }
        }
    }
    found
}

fn is_directory(path: &str) -> bool {
    Path::new(path).is_dir()
}

fn name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn jsonl_files(root: &str, source: &str) -> Vec<String> {
    match source {
        "magpie" => {
            let log = format!("{root}/usage.jsonl");
            if Path::new(&log).exists() && !is_directory(&log) { vec![log] } else { vec![] }
        }
        "codex" => {
            let roots: Vec<String> = ["sessions", "archived_sessions"]
                .iter()
                .map(|part| format!("{root}/{part}"))
                .filter(|p| is_directory(p))
                .collect();
            let folders = if roots.is_empty() { vec![root.to_string()] } else { roots };
            let mut chosen: HashMap<String, String> = HashMap::new();
            let mut order: Vec<String> = Vec::new();
            // Active copy wins over archive; plain copy wins over compressed in each root.
            for folder in folders {
                for path in sorted_paths(walk(&folder)) {
                    if !(name(&path).contains(".jsonl") && (path.ends_with(".jsonl") || path.ends_with(".jsonl.zst"))) {
                        continue;
                    }
                    let mut relative = path[folder.len() + 1..].to_string();
                    if relative.ends_with(".zst") {
                        relative.truncate(relative.len() - 4);
                    }
                    if !chosen.contains_key(&relative) {
                        chosen.insert(relative.clone(), path);
                        order.push(relative);
                    }
                }
            }
            order.into_iter().map(|relative| chosen.remove(&relative).expect("chosen")).collect()
        }
        "qoder" => {
            // Main transcripts sort before their subagents/ folder, so a main copy is indexed first.
            let projects = format!("{root}/projects");
            let base = if is_directory(&projects) { projects } else { root.to_string() };
            sorted_paths(walk(&base).into_iter().filter(|path| path.ends_with(".jsonl")).collect())
        }
        "workbuddy" => {
            let mut roots: Vec<String> = [".workbuddy-ai", ".workbuddy"]
                .iter()
                .map(|part| format!("{root}/{part}/projects"))
                .filter(|path| is_directory(path))
                .collect();
            if roots.is_empty() && paths::resolve(Path::new(root)) == paths::resolve(&paths::home()) {
                return vec![];
            }
            if roots.is_empty() {
                let projects = format!("{root}/projects");
                roots = vec![if is_directory(&projects) { projects } else { root.to_string() }];
            }
            let all: HashSet<String> =
                roots.iter().flat_map(|root| walk(root)).filter(|path| path.ends_with(".jsonl")).collect();
            sorted_paths(all.into_iter().collect())
        }
        _ => {
            // Only one physical generation of each DSH session is authoritative.
            let mut chosen: HashMap<String, (i64, String)> = HashMap::new();
            let mut order: Vec<String> = Vec::new();
            for path in walk(root) {
                let Some(generation) = dsh_generation(name(&path)) else { continue };
                let parent = path.rsplit_once('/').map_or("", |(parent, _)| parent).to_string();
                match chosen.get(&parent) {
                    None => {
                        order.push(parent.clone());
                        chosen.insert(parent, (generation, path));
                    }
                    Some((best, _)) if generation > *best => {
                        chosen.insert(parent, (generation, path));
                    }
                    Some(_) => {}
                }
            }
            order.into_iter().map(|parent| chosen.remove(&parent).expect("chosen").1).collect()
        }
    }
}

/// session.jsonl, session.v<N>.jsonl, optionally .zstd: the generation number, or None for other names.
fn dsh_generation(name: &str) -> Option<i64> {
    let rest = name.strip_prefix("session")?;
    let rest = rest.strip_suffix(".zstd").unwrap_or(rest);
    let rest = rest.strip_suffix(".jsonl")?;
    if rest.is_empty() {
        return Some(0);
    }
    let digits = rest.strip_prefix(".v").filter(|digits| !digits.is_empty() && digits.is_ascii())?;
    digits.parse().ok()
}

fn suffix(path: &str) -> &str {
    let last = name(path);
    match last.rfind('.') {
        Some(dot) if dot > 0 => &last[dot..],
        _ => "",
    }
}

fn bind(value: Option<&Json>) -> Value {
    match value {
        Some(Json::Int(number)) => Value::Integer(*number),
        Some(Json::Double(number)) => Value::Real(*number),
        Some(Json::Bool(flag)) => Value::Integer(i64::from(*flag)),
        Some(Json::String(text)) => Value::Text(text.clone()),
        _ => Value::Null,
    }
}

fn root_identity(root: &str) -> String {
    let resolved =
        paths::resolve(Path::new(root)).map_or_else(|| root.to_string(), |path| path.to_string_lossy().into_owned());
    py::identity(vec![resolved.into()])[..12].to_string()
}

fn requests_of(result: &Object) -> f64 {
    result.get("summary").and_then(|summary| summary.get("requests")).and_then(Json::number).unwrap_or(0.0)
}

/// Where a read of one log stopped, when it did.
enum Stop {
    Counted,
    Other(crate::Error),
}

/// The usage of one log source in start...end, indexing only what changed since the last read. `carriers` are the
/// gateways that may have logged the same requests; `archive` keeps the events of log files that disappear.
#[allow(clippy::too_many_arguments)]
pub fn jsonl_usage(
    source: &str,
    root_path: &str,
    cache: &Path,
    start: f64,
    end: f64,
    zone: &TimeZone,
    exclude_proxy: bool,
    carriers: Option<&mut [Carrier]>,
    price: Option<Price>,
    archive: Option<&Path>,
) -> Fallible<Object> {
    let root = normalize(&paths::expand_user(root_path).to_string_lossy());
    if !is_directory(&root) {
        return Ok(object! { "error" => "未找到记录目录" });
    }
    let archive = archive.filter(|path| !path.as_os_str().is_empty());
    let database_path =
        cache.join(format!("harness-v{}-{source}-{}.sqlite3", index_version(source), root_identity(&root)));
    files::make_directory(cache, 0o700)?;
    let db = Database::open(&database_path, 10.0, Mode::ReadWrite)?;
    files::owner_only(&database_path);
    let mut errors = 0;
    let paths = jsonl_files(&root, source);
    // Every copy of an event is kept per file and deduplicated when read, so a vanished file (archive or
    // compression move) only drops its own rows.
    db.execute(
        "CREATE TABLE IF NOT EXISTS files(path TEXT PRIMARY KEY, inode INTEGER, size INTEGER, mtime INTEGER, offset INTEGER, state TEXT); CREATE TABLE IF NOT EXISTS events(id TEXT, file TEXT, ts REAL, data TEXT, PRIMARY KEY(id, file)); CREATE INDEX IF NOT EXISTS events_ts ON events(ts); CREATE INDEX IF NOT EXISTS events_file ON events(file);",
    )?;
    transaction(&db, || {
        let active: HashSet<String> = paths.iter().map(|path| py::identity(vec![path.as_str().into()])).collect();
        forget_missing(&db, &active, archive)?;
        for path in &paths {
            let Some(state) = state_of(Path::new(path)) else {
                errors += 1;
                continue;
            };
            let file_key = py::identity(vec![path.as_str().into()]);
            let previous = db.first(
                "SELECT inode,size,mtime,offset,state FROM files WHERE path=?",
                &[Value::Text(file_key.clone())],
            )?;
            let int = |row: &Vec<Value>, index: usize| database::int(&row[index]);
            if let Some(previous) = &previous
                && int(previous, 0) == Some(state.inode)
                && int(previous, 1) == Some(state.size)
                && int(previous, 2) == Some(state.mtime)
            {
                continue;
            }
            let compressed = [".zst", ".zstd"].contains(&suffix(path));
            let incremental = previous.as_ref().is_some_and(|previous| {
                int(previous, 0) == Some(state.inode)
                    && int(previous, 1).unwrap_or(i64::MAX) < state.size
                    && !compressed
            });
            let saved = previous
                .as_ref()
                .filter(|_| incremental)
                .and_then(|previous| database::text(&previous[4]))
                .and_then(|text| Json::parse_str(text).ok())
                .and_then(|value| value.as_object().cloned());
            let mut file_state = saved.unwrap_or_else(|| {
                let mut session = name(path).to_string();
                if session.ends_with(".zst") {
                    session.truncate(session.len() - 4);
                }
                if session.ends_with(".jsonl") {
                    session.truncate(session.len() - 6);
                }
                object! { "session" => session }
            });
            let mut offset = if incremental { previous.as_ref().and_then(|p| int(p, 3)).unwrap_or(0) } else { 0 };
            db.execute("SAVEPOINT source_file")?;
            let outcome = (|| -> Result<(), Stop> {
                if !incremental {
                    db.run("DELETE FROM events WHERE file=?", &[Value::Text(file_key.clone())])
                        .map_err(|e| Stop::Other(e.into()))?;
                }
                let begin = offset;
                let mut read = |_: i64, line: &[u8]| -> Result<(), ReadFailure> {
                    offset += line.len() as i64;
                    // Avoid parsing full tool/message content unless it carries usage or metadata.
                    if source == "codex" && !contains_any(line, &CODEX_WORDS) {
                        return Ok(());
                    }
                    let row = match Json::parse_selected(line, &Selection::All, true) {
                        Ok(row) => row,
                        Err(error) if error.encoding => return Err(FileError::new("UnicodeDecodeError").into()),
                        Err(_) => return Ok(()),
                    };
                    let Some(fields) = row.as_object() else { return Ok(()) };
                    let step = (|| -> Result<Option<Event>, ReadError> {
                        if source != "codex"
                            && file_state.get("isSeeded").is_some_and(Json::is_truthy)
                            && kind_is(fields, "session/end-seed")
                            && py::get(&py::dict(fields.get("data")), "inherited")?.is_some_and(Json::is_truthy)
                        {
                            db.run("DELETE FROM events WHERE file=?", &[Value::Text(file_key.clone())])
                                .map_err(|error| ReadError::File(FileError::new(error.to_string())))?;
                        }
                        match source {
                            "codex" => codex_event(fields, &mut file_state),
                            "deepseek-harness" => deepseek_event(fields, &mut file_state),
                            "workbuddy" => workbuddy_event(fields, &mut file_state),
                            "qoder" => qoder_event(fields, &mut file_state),
                            _ => magpie_event(fields, offset),
                        }
                    })();
                    match step {
                        Ok(Some(event)) => {
                            db.run(
                                "INSERT OR REPLACE INTO events VALUES (?,?,?,?)",
                                &[
                                    bind(event.get("id")),
                                    Value::Text(file_key.clone()),
                                    bind(event.get("timestamp")),
                                    Value::Text(py::dumps(&Json::Object(event.clone()))),
                                ],
                            )?;
                            Ok(())
                        }
                        Ok(None) | Err(ReadError::Line) => Ok(()),
                        Err(ReadError::File(error)) => Err(error.into()),
                    }
                };
                let result = if compressed {
                    match zstd::decompress_file(Path::new(path)) {
                        Ok(data) => lines_in(&data, begin as usize, &mut read),
                        Err(_) => return Err(Stop::Counted),
                    }
                } else {
                    read_lines(Path::new(path), begin, &mut read).map(|_| ())
                };
                match result {
                    Ok(()) => {}
                    Err(ReadFailure::Unreadable | ReadFailure::File(_)) => return Err(Stop::Counted),
                    Err(ReadFailure::Other(error)) => return Err(Stop::Other(error)),
                }
                db.run(
                    "INSERT OR REPLACE INTO files VALUES (?,?,?,?,?,?)",
                    &[
                        Value::Text(file_key.clone()),
                        Value::Integer(state.inode),
                        Value::Integer(state.size),
                        Value::Integer(state.mtime),
                        Value::Integer(offset),
                        Value::Text(py::dumps(&Json::Object(file_state.clone()))),
                    ],
                )
                .map_err(|error| Stop::Other(error.into()))?;
                Ok(())
            })();
            match outcome {
                Ok(()) => db.execute("RELEASE source_file")?,
                Err(Stop::Counted) => {
                    db.execute("ROLLBACK TO source_file")?;
                    db.execute("RELEASE source_file")?;
                    errors += 1;
                }
                Err(Stop::Other(error)) => return Err(error),
            }
        }
        Ok(())
    })?;
    let mut events = archive::merged(unique_events(&db, start, end)?, archive, start, end);
    if source == "codex" {
        events = chat_projects(events, &codex_names(Path::new(&root)));
    }
    let default_price: Price = if source == "codex" { &pricing::codex_cost } else { &pricing::model_cost };
    let mut result =
        tally::aggregate(&events, start, end, zone, exclude_proxy, Some(price.unwrap_or(default_price)), carriers);
    if errors > 0 {
        result.insert("warning", "部分记录无法读取");
        result.insert("partial", true);
        if requests_of(&result) == 0.0 {
            result.insert("error", "记录格式或权限待检查");
        }
    }
    if paths.is_empty() && !archive.is_some_and(Path::exists) {
        return Ok(object! { "error" => "暂无用量记录" });
    }
    Ok(result)
}

const CODEX_WORDS: [&[u8]; 4] =
    [b"\"token_count\"", b"\"session_meta\"", b"\"turn_context\"", b"\"thread_settings_applied\""];

/// Whether any of the words occurs, in one pass: every word has an underscore, so only the places around each
/// underscore are compared, instead of a full search per word.
fn contains_any(line: &[u8], words: &[&[u8]]) -> bool {
    let anchors: Vec<usize> =
        words.iter().map(|word| word.iter().position(|&byte| byte == b'_').expect("underscore")).collect();
    memchr::memchr_iter(b'_', line).any(|underscore| {
        words.iter().zip(&anchors).any(|(word, &anchor)| {
            underscore >= anchor && line.get(underscore - anchor..underscore - anchor + word.len()) == Some(*word)
        })
    })
}

/// Drops what the index holds for files that are gone; with an archive, their events move there first.
fn forget_missing(db: &Database, active: &HashSet<String>, archive: Option<&Path>) -> Fallible<()> {
    for row in db.rows("SELECT path FROM files", &[])? {
        let Some(key) = database::text(&row[0]).filter(|key| !active.contains(*key)).map(String::from) else {
            continue;
        };
        if let Some(archive) = archive {
            archive::keep(
                archive,
                db.rows("SELECT id,ts,data FROM events WHERE file=?", &[Value::Text(key.clone())])?,
            )?;
        }
        db.run("DELETE FROM events WHERE file=?", &[Value::Text(key.clone())])?;
        db.run("DELETE FROM files WHERE path=?", &[Value::Text(key)])?;
    }
    Ok(())
}

/// One copy per event ID, whichever file was indexed first.
fn unique_events(db: &Database, start: f64, end: f64) -> Fallible<Vec<Event>> {
    let rows = db.rows(
        "SELECT data FROM events WHERE rowid IN (SELECT MIN(rowid) FROM events GROUP BY id) AND ts>=? AND ts<=?",
        &[Value::Real(start), Value::Real(end)],
    )?;
    Ok(rows
        .iter()
        .filter_map(|row| database::text(&row[0]).and_then(|text| Json::parse_str(text).ok()))
        .filter_map(|value| value.as_object().cloned())
        .collect())
}

// Gemini CLI and OpenCode

/// JSON and JSONL Gemini CLI sessions, usage only, including spent rewinds.
#[allow(clippy::too_many_arguments)]
pub fn gemini_usage(
    root_path: &str,
    cache: &Path,
    start: f64,
    end: f64,
    zone: &TimeZone,
    exclude_proxy: bool,
    carriers: Option<&mut [Carrier]>,
    archive: Option<&Path>,
) -> Fallible<Object> {
    let root = normalize(&paths::expand_user(root_path).to_string_lossy());
    if !is_directory(&root) {
        return Ok(object! { "error" => "未找到 Gemini CLI 记录" });
    }
    let archive = archive.filter(|path| !path.as_os_str().is_empty());
    let tmp = format!("{root}/tmp");
    let folders = if is_directory(&tmp) { vec![tmp] } else { vec![root.clone()] };
    let mut found: HashSet<String> = HashSet::new();
    for folder in &folders {
        for path in walk(folder) {
            let file = name(&path);
            if file.starts_with("session-") && file.contains(".json") && [".json", ".jsonl"].contains(&suffix(&path)) {
                found.insert(path.clone());
            }
            // New subagents may use UUID filenames instead of session-*.
            let relative: Vec<&str> = path[folder.len() + 1..].split('/').collect();
            if file.ends_with(".jsonl")
                && let Some(chats) = relative[..relative.len() - 1].iter().position(|part| *part == "chats")
                && chats < relative.len() - 1
            {
                found.insert(path.clone());
            }
        }
    }
    let paths = sorted_paths(found.into_iter().collect());
    if paths.is_empty() && !archive.is_some_and(Path::exists) {
        return Ok(object! { "error" => "暂无 Gemini CLI 记录" });
    }
    let target = cache.join(format!("harness-gemini-v2-{}.sqlite3", root_identity(&root)));
    files::make_directory(cache, 0o700)?;
    let db = Database::open(&target, 10.0, Mode::ReadWrite)?;
    files::owner_only(&target);
    db.execute(
        "CREATE TABLE IF NOT EXISTS files(path TEXT PRIMARY KEY,size INTEGER,mtime INTEGER); CREATE TABLE IF NOT EXISTS events(id TEXT,file TEXT,ts REAL,data TEXT,PRIMARY KEY(id,file)); CREATE INDEX IF NOT EXISTS events_ts ON events(ts); CREATE INDEX IF NOT EXISTS events_file ON events(file);",
    )?;
    let mut errors = 0;
    // Deleted sessions keep contributing only through the archive, when there is one.
    let active: HashSet<String> = paths.iter().map(|path| py::identity(vec![path.as_str().into()])).collect();
    transaction(&db, || forget_missing(&db, &active, archive))?;
    for path in &paths {
        let file_id = py::identity(vec![path.as_str().into()]);
        let Some(state) = state_of(Path::new(path)) else {
            errors += 1;
            continue;
        };
        if let Some(known) = db.first("SELECT size,mtime FROM files WHERE path=?", &[Value::Text(file_id.clone())])?
            && database::int(&known[0]) == Some(state.size)
            && database::int(&known[1]) == Some(state.mtime)
        {
            continue;
        }
        if (state.mtime as f64) / 1e9 < start {
            continue;
        }
        let stem = Path::new(name(path)).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let mut file_state = object! { "session" => stem };
        let events = match gemini_file(path, &mut file_state) {
            Ok(events) => events,
            Err(_) => {
                errors += 1;
                continue;
            }
        };
        transaction(&db, || {
            db.run("DELETE FROM events WHERE file=?", &[Value::Text(file_id.clone())])?;
            for event in &events {
                db.run(
                    "INSERT OR REPLACE INTO events VALUES (?,?,?,?)",
                    &[
                        bind(event.get("id")),
                        Value::Text(file_id.clone()),
                        bind(event.get("timestamp")),
                        Value::Text(py::dumps(&Json::Object(event.clone()))),
                    ],
                )?;
            }
            db.run(
                "INSERT OR REPLACE INTO files VALUES (?,?,?)",
                &[Value::Text(file_id.clone()), Value::Integer(state.size), Value::Integer(state.mtime)],
            )?;
            Ok(())
        })?;
    }
    let events = archive::merged(unique_events(&db, start, end)?, archive, start, end);
    let mut result = tally::aggregate(&events, start, end, zone, exclude_proxy, Some(&pricing::model_cost), carriers);
    if errors > 0 {
        result.insert("partial", true);
        if requests_of(&result) == 0.0 {
            result.insert("error", "Gemini 记录格式待检查");
        }
    }
    Ok(result)
}

/// One Gemini session file's events; a FileError fails the whole file.
fn gemini_file(path: &str, file_state: &mut Object) -> Result<Vec<Event>, FileError> {
    let data = std::fs::read(path).map_err(|_| FileError::new("unreadable"))?;
    let mut records: Vec<Json> = Vec::new();
    if suffix(path) == ".json" {
        let value = Json::parse(&data).map_err(|_| FileError::new("bad JSON"))?;
        let object = value.as_object().ok_or_else(|| FileError::new("not an object"))?;
        let session = py::or(object.get("sessionId"), file_state.get("session").cloned().unwrap_or(Json::Null));
        file_state.insert("session", py::str(Some(&session)));
        match py::or(object.get("messages"), Json::Array(vec![])) {
            Json::Array(items) => records = items,
            Json::String(_) | Json::Object(_) => {}
            _ => return Err(FileError::new("messages not iterable")),
        }
    } else {
        let text = String::from_utf8(data).map_err(|_| FileError::new("UnicodeDecodeError"))?;
        let mut lines: Vec<&str> = text.split('\n').collect();
        // The part after the last newline is still being written.
        lines.pop();
        for line in lines {
            let trimmed = line.strip_suffix('\r').unwrap_or(line);
            if let Ok(row) = Json::parse_str(trimmed) {
                records.push(row);
            }
        }
    }
    let mut events = Vec::new();
    for row in &records {
        let Some(fields) = row.as_object() else { continue };
        if let Some(session) = fields.get("sessionId").filter(|session| session.is_truthy()) {
            file_state.insert("session", py::str(Some(session)));
        }
        // Legacy full-history checkpoints and repeated message records share IDs. Rewinds don't refund spent tokens.
        let checkpoint = fields
            .get("$set")
            .and_then(Json::as_object)
            .map(|set| set.get("messages").cloned().unwrap_or(Json::Array(vec![])))
            .unwrap_or(Json::Array(vec![]));
        let list = checkpoint.as_array().ok_or_else(|| FileError::new("checkpoint is not a list"))?;
        for item in std::iter::once(row).chain(list.iter()) {
            let item_fields = item.as_object().ok_or_else(|| FileError::new("AttributeError"))?;
            match gemini_event(item_fields, file_state) {
                Ok(Some(event)) => events.push(event),
                Ok(None) => {}
                Err(ReadError::Line) => return Err(FileError::new("TypeError")),
                Err(ReadError::File(error)) => return Err(error),
            }
        }
    }
    Ok(events)
}

/// OpenCode's own database, read only.
pub fn opencode_usage(
    root_path: &str,
    start: f64,
    end: f64,
    zone: &TimeZone,
    exclude_proxy: bool,
    carriers: Option<&mut [Carrier]>,
) -> Fallible<Object> {
    let root = normalize(&paths::expand_user(root_path).to_string_lossy());
    let database_path =
        if Path::new(&root).exists() && !is_directory(&root) { root.clone() } else { format!("{root}/opencode.db") };
    if !Path::new(&database_path).exists() || is_directory(&database_path) {
        return Ok(object! { "error" => "未找到 OpenCode 数据库" });
    }
    let resolved = paths::resolve(Path::new(&database_path)).unwrap_or_else(|| PathBuf::from(&database_path));
    let db = Database::open(&resolved, 4.0, Mode::ReadOnly)?;
    db.execute("PRAGMA query_only=ON")?;
    let mut events: Vec<Event> = Vec::new();
    let low = Value::Integer((start * 1000.0).trunc() as i64);
    let high = if end.is_finite() { Value::Integer((end * 1000.0).trunc() as i64) } else { Value::Real(f64::INFINITY) };
    db.each(
        "SELECT id,session_id,time_created,data FROM message WHERE time_created>=? AND time_created<=? AND json_extract(data,'$.role')='assistant'",
        &[low, high],
        |row| {
            let Some(fields) = database::text(&row[3]).and_then(|raw| Json::parse_str(raw).ok()).and_then(|v| v.as_object().cloned())
            else {
                return Ok(());
            };
            let usage = py::dict(fields.get("tokens"));
            let Some(usage_fields) = usage.as_object() else { return Ok(()) };
            let cached = py::dict(usage_fields.get("cache"));
            let Some(cached_fields) = cached.as_object() else { return Ok(()) };
            let mut event = metrics(
                fields.get("modelID"),
                usage_fields.get("input"),
                usage_fields.get("output"),
                cached_fields.get("read"),
                cached_fields.get("write"),
                fields.get("cost"),
            );
            let folder = fields.get("path").and_then(Json::as_object).cloned().unwrap_or_default();
            event.insert("id", py::identity(vec![sql_json(&row[0])]));
            event.insert("session", py::identity(vec![sql_json(&row[1])]));
            let Some(created) = database::double(&row[2]) else { return Ok(()) };
            event.insert("timestamp", created / 1000.0);
            event.insert("provider", fields.get("providerID").cloned().unwrap_or(Json::from("")));
            event.insert("project", py::project_name(Some(&py::or(folder.get("cwd"), folder.get("root").cloned().unwrap_or(Json::Null)))));
            events.push(event);
            Ok(())
        },
    )?;
    Ok(tally::aggregate(&events, start, end, zone, exclude_proxy, Some(&pricing::model_cost), carriers))
}

fn sql_json(value: &Value) -> Json {
    match value {
        Value::Integer(number) => Json::Int(*number),
        Value::Real(number) => Json::Double(*number),
        Value::Text(text) => Json::from(text.as_str()),
        _ => Json::Null,
    }
}

/// What pathlib keeps of a path: no repeated or trailing slashes and no "." parts.
pub fn normalize(path: &str) -> String {
    if path.is_empty() {
        return ".".into();
    }
    if cfg!(windows) {
        // WindowsPath: either slash separates, the drive or UNC share stays in front, backslashes come out.
        let unified = path.replace('\\', "/");
        let (prefix, rest) = if unified.len() >= 2 && unified.as_bytes()[1] == b':' {
            (unified[..2].to_string(), &unified[2..])
        } else if let Some(rest) = unified.strip_prefix("//") {
            ("\\\\".to_string(), rest)
        } else {
            (String::new(), unified.as_str())
        };
        let rooted = rest.starts_with('/');
        let joined = rest.split('/').filter(|part| !part.is_empty() && *part != ".").collect::<Vec<_>>().join("\\");
        if prefix.is_empty() && !rooted {
            return if joined.is_empty() { ".".into() } else { joined };
        }
        let separator = if rooted && !prefix.starts_with("\\\\") { "\\" } else { "" };
        return format!("{prefix}{separator}{joined}");
    }
    let absolute = path.starts_with('/');
    let joined = path.split('/').filter(|part| !part.is_empty() && *part != ".").collect::<Vec<_>>().join("/");
    if absolute {
        format!("/{joined}")
    } else if joined.is_empty() {
        ".".into()
    } else {
        joined
    }
}

/// Whether a path names its place from the root: a leading slash, or on Windows a drive or a share.
pub fn is_absolute(path: &str) -> bool {
    if cfg!(windows) {
        let bytes = path.as_bytes();
        path.starts_with('/')
            || path.starts_with('\\')
            || (bytes.len() >= 3 && bytes[1] == b':' && (bytes[2] == b'\\' || bytes[2] == b'/'))
    } else {
        path.starts_with('/')
    }
}

/// Magpie's requests priced by the model actually served: Claude's own prices (with its one-hour cache writes and
/// fast mode), the Codex table for GPT and Codex models, and the shared table for the rest.
pub fn magpie_price(event: &Event) -> Option<f64> {
    let lowered = py::str(event.get("model")).to_lowercase();
    let model = lowered.rsplit('/').next().unwrap_or("").to_string();
    if model.starts_with("claude-") {
        let one_hour = event.get("write1h").and_then(Json::number).unwrap_or(0.0);
        let metrics = [
            py::num(event.get("input")),
            py::num(event.get("output")),
            py::num(event.get("writes")) - one_hour,
            one_hour,
            py::num(event.get("cached")),
        ];
        let speed = if event.get("fast").is_some_and(Json::is_truthy) { "fast" } else { "standard" };
        return pricing::claude::cost(&model, &metrics, speed, "global", "standard");
    }
    // Codex's own reviewer (codex-auto-review) bills as a GPT model too.
    if ["gpt-", "o1", "o3", "o4", "codex-"].iter().any(|prefix| model.starts_with(prefix)) {
        let mut renamed = event.clone();
        renamed.insert("model", model.as_str());
        if let Some(cost) = pricing::codex_cost(&renamed) {
            return Some(cost);
        }
    }
    pricing::model_cost(event)
}
