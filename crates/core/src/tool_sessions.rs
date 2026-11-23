//! Sessions of the other tools on the homepage, beside Claude Code's and Codex's: Gemini (its CLI and Antigravity),
//! OpenCode, Qoder, WorkBuddy and DeepSeek. Each is read from what that tool's usage reader keeps of it, so a session's
//! requests are those its card counts: DeepSeek, WorkBuddy, Qoder and Gemini keep an index of every session file's
//! requests (written as the usage is read), OpenCode keeps its own database. Only a tool shown on the homepage is
//! looked at. A session is running while its record changed in the last minute (Antigravity says so itself); its turns
//! are runs of requests, a new one after three minutes without any, as these records keep no prompts to go by. As for
//! Claude and Codex, nothing is kept but the state, the title the tool gave, the project folder's name, the model and
//! the figures; never prompts, replies or paths.

use std::collections::HashMap;
use std::path::Path;

use jiff::tz::TimeZone;

use crate::claude_usage::state_of;
use crate::database::{self, Database, Mode, Value};
use crate::files::paths;
use crate::harness;
use crate::json::{Json, Object};
use crate::runtime::Runtime;
use crate::sessions::{
    SPENDS_KEPT, STALLED, add_to_day, bump, day_of, last_turns, range_costs, rows_at_end, rows_at_start, turn,
};
use crate::{object, pricing, py};

/// A session whose record changed this recently is running.
const RUNNING: f64 = 60.0;
/// Requests further apart than this are told as two turns.
const TURN_GAP: f64 = 180.0;

/// The tools read here: module, the name a session carries, how its requests are priced when the record has no cost.
const TOOLS: [(&str, &str); 5] = [
    ("gemini", "Gemini"),
    ("opencode", "OpenCode"),
    ("qoder", "Qoder"),
    ("workbuddy", "WorkBuddy"),
    ("deepseek-harness", "DeepSeek"),
];

/// How a request is priced when its record carries no cost.
type Pricing = dyn Fn(&Object) -> Option<f64>;
/// What is known of a transcript by its path: the file's print when read, its title and its project.
type Names = HashMap<String, (String, Option<String>, Option<String>)>;

/// A session's file (or conversation) and what its record says beyond its requests.
struct Record {
    key: String,
    changed: f64,
    events: Vec<Object>,
    title: Option<String>,
    project: Option<String>,
    /// The tool's own word on whether it is working, when it keeps one (Antigravity).
    running: Option<bool>,
}

fn hash(text: &str) -> String {
    py::sha256_hex(text.as_bytes())[..16].to_string()
}

fn number(event: &Object, key: &str) -> f64 {
    event.get(key).and_then(Json::python_float).unwrap_or(0.0)
}

/// What a transcript in Claude Code's shape (WorkBuddy's, Qoder's) names itself: the title the user gave it, else the
/// one its tool made, and the folder it works in. From its last half megabyte, else its start; kept while the file is
/// unchanged, as a pinned panel reads the sessions every second or two.
fn transcript_names(path: &str) -> (Option<String>, Option<String>) {
    static KNOWN: std::sync::Mutex<Option<Names>> = std::sync::Mutex::new(None);
    let Some(state) = state_of(Path::new(path)) else { return (None, None) };
    let print = format!("{}:{}:{}", state.inode, state.size, state.mtime);
    if let Some((_, title, folder)) =
        KNOWN.lock().ok().and_then(|known| known.as_ref()?.get(path).filter(|(seen, _, _)| *seen == print).cloned())
    {
        return (title, folder);
    }
    let scan = |rows: Vec<Object>| {
        let (mut named, mut made, mut folder) = (None::<String>, None::<String>, None::<String>);
        for row in rows {
            match row.get("type").and_then(Json::as_str) {
                Some("custom-title") => named = text_of(row.get("customTitle")).or(named),
                Some("ai-title") => made = text_of(row.get("aiTitle")).or(made),
                _ => {}
            }
            if let Some(cwd) = text_of(row.get("cwd")) {
                folder = Some(cwd);
            }
        }
        (named.or(made), folder)
    };
    let (mut title, mut folder) = scan(rows_at_end(path, 512 * 1024).unwrap_or_default());
    if title.is_none() || folder.is_none() {
        let (early_title, early_folder) = scan(rows_at_start(path, 64 * 1024).unwrap_or_default());
        title = title.or(early_title);
        folder = folder.or(early_folder);
    }
    let project =
        folder.and_then(|folder| py::project_name(Some(&Json::from(folder.as_str()))).as_str().map(String::from));
    if let Ok(mut known) = KNOWN.lock() {
        known.get_or_insert_with(HashMap::new).insert(path.to_string(), (print, title.clone(), project.clone()));
    }
    (title, project)
}

/// The requests the usage reader's index keeps for one file, oldest first.
fn indexed(db: &Database, path: &str) -> Vec<Object> {
    let file = py::identity(vec![path.into()]);
    db.rows("SELECT data FROM events WHERE file=? ORDER BY ts", &[Value::Text(file)])
        .unwrap_or_default()
        .into_iter()
        .filter_map(|row| row.first().and_then(database::text).and_then(|raw| Json::parse_str(raw).ok()))
        .filter_map(|value| value.as_object().cloned())
        .collect()
}

fn open_index(path: &Path) -> Option<Database> {
    if !path.is_file() {
        return None;
    }
    let db = Database::open(path, 2.0, Mode::ReadOnly).ok()?;
    db.execute("PRAGMA query_only=ON").ok()?;
    Some(db)
}

/// The sessions of a tool whose usage reader indexes JSON-lines files (DeepSeek, WorkBuddy, Qoder).
fn jsonl_records(cache: &Path, source: &str, root: &str, since: f64) -> Vec<Record> {
    let index = cache.join(format!(
        "harness-v{}-{source}-{}.sqlite3",
        harness::index_version(source),
        harness::root_identity(root)
    ));
    let Some(db) = open_index(&index) else { return vec![] };
    harness::jsonl_files(root, source)
        .into_iter()
        .filter_map(|path| {
            let changed = state_of(Path::new(&path))?.mtime as f64 / 1e9;
            (changed >= since).then(|| {
                let (title, project) = transcript_names(&path);
                Record {
                    key: format!("{source}:{path}"),
                    changed,
                    events: indexed(&db, &path),
                    title,
                    project,
                    running: None,
                }
            })
        })
        .collect()
}

/// What Antigravity's own list says of a conversation: its title, whether it is still working, its workspace's name.
fn antigravity_summary(store: &str, conversation: &str) -> (Option<String>, Option<bool>, Option<String>) {
    // <root>/antigravity…/conversations[/implicit]/<id>.db, its list beside conversations/.
    let folder =
        Path::new(store).ancestors().find(|folder| folder.file_name().is_some_and(|name| name == "conversations"));
    let Some(list) = folder.and_then(Path::parent).map(|parent| parent.join("conversation_summaries.db")) else {
        return (None, None, None);
    };
    let Some(db) = open_index(&list) else { return (None, None, None) };
    let row = db
        .first(
            "SELECT title, status, not_fully_idle, workspace_uris FROM conversation_summaries WHERE conversation_id=?",
            &[Value::Text(conversation.to_string())],
        )
        .ok()
        .flatten();
    let Some(row) = row else { return (None, None, None) };
    let title = row.first().and_then(database::text).map(str::trim).filter(|title| !title.is_empty()).map(String::from);
    let status = row.get(1).and_then(database::text).unwrap_or("");
    let busy = row.get(2).and_then(database::int).is_some_and(|flag| flag != 0);
    let running = Some(status.contains("RUNNING") || busy);
    // The first workspace: file:///Users/me/project → project.
    let project = row.get(3).and_then(database::text).and_then(|uris| {
        let start = uris.find("file://")? + "file://".len();
        let rest = &uris[start..];
        let end = rest.find(['"', ',', ' ', ']']).unwrap_or(rest.len());
        let folder = percent_decoded(&rest[..end]);
        py::project_name(Some(&Json::from(folder.trim_end_matches('/')))).as_str().map(String::from)
    });
    (title, running, project)
}

fn percent_decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let Ok(value) = u8::from_str_radix(&text[index + 1..index + 3], 16)
        {
            out.push(value);
            index += 3;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Gemini's sessions: the CLI's chat files and Antigravity's conversations (one store each; a conversation kept in two
/// stores, the app's and the IDE's, is one session, as its usage counts once).
fn gemini_records(cache: &Path, root: &str, since: f64) -> Vec<Record> {
    let index = cache.join(format!("harness-gemini-v4-{}.sqlite3", harness::root_identity(root)));
    let Some(db) = open_index(&index) else { return vec![] };
    let mut records: Vec<Record> = harness::gemini_files(root)
        .into_iter()
        .filter_map(|path| {
            let changed = state_of(Path::new(&path))?.mtime as f64 / 1e9;
            (changed >= since).then(|| Record {
                key: format!("gemini:{path}"),
                changed,
                events: indexed(&db, &path),
                title: None,
                project: None,
                running: None,
            })
        })
        .collect();
    let mut conversations: HashMap<String, Record> = HashMap::new();
    for store in harness::antigravity_stores(root) {
        let Some((_, mtime)) = harness::store_state(&store) else { continue };
        let changed = mtime as f64 / 1e9;
        if changed < since {
            continue;
        }
        let id = Path::new(&store).file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
        if conversations.get(&id).is_some_and(|known| known.changed >= changed) {
            continue;
        }
        let (title, running, project) = antigravity_summary(&store, &id);
        let events = indexed(&db, &store);
        conversations
            .insert(id.clone(), Record { key: format!("antigravity:{id}"), changed, events, title, project, running });
    }
    let mut ids: Vec<String> = conversations.keys().cloned().collect();
    ids.sort();
    records.extend(ids.into_iter().filter_map(|id| conversations.remove(&id)));
    records
}

/// OpenCode's sessions, from its own database: each with a message since `since`, its title if the database keeps one.
fn opencode_records(root: &str, since: f64) -> Vec<Record> {
    let database_path = if Path::new(root).is_file() { root.to_string() } else { format!("{root}/opencode.db") };
    let resolved = paths::resolve(Path::new(&database_path)).unwrap_or_else(|| database_path.clone().into());
    let Some(db) = open_index(&resolved) else { return vec![] };
    let low = Value::Integer((since * 1000.0).trunc() as i64);
    let recent = db
        .rows("SELECT session_id, MAX(time_created) FROM message WHERE time_created>=? GROUP BY session_id", &[low])
        .unwrap_or_default();
    let mut records = Vec::new();
    for row in recent {
        let Some(session) = row.first().cloned() else { continue };
        let changed = row.get(1).and_then(database::double).unwrap_or(0.0) / 1000.0;
        let events: Vec<Object> = db
            .rows(
                "SELECT id,session_id,time_created,data FROM message WHERE session_id=? AND json_extract(data,'$.role')='assistant' ORDER BY time_created",
                std::slice::from_ref(&session),
            )
            .unwrap_or_default()
            .iter()
            .filter_map(|row| harness::opencode_event(row))
            .collect();
        // Older databases have no session table, or no title in it.
        let title = db
            .first("SELECT title FROM session WHERE id=?", std::slice::from_ref(&session))
            .ok()
            .flatten()
            .and_then(|row| row.first().and_then(database::text).map(str::trim).map(String::from))
            .filter(|title| !title.is_empty());
        let key = match &session {
            Value::Text(text) => text.clone(),
            Value::Integer(number) => number.to_string(),
            _ => continue,
        };
        records.push(Record { key: format!("opencode:{key}"), changed, events, title, project: None, running: None });
    }
    records
}

/// A session as the 会话 tab reads it, from its record: its requests in turns, its cost whole and in each range.
fn session_of(
    tool: &str,
    module: &str,
    record: Record,
    now: f64,
    price: &Pricing,
    ranges: &[(String, f64, f64)],
    zone: &TimeZone,
) -> Option<Object> {
    // A request timed ahead of now (a clock running fast) is counted once its time comes, as the cards count it.
    let events: Vec<&Object> = record.events.iter().filter(|event| number(event, "timestamp") <= now).collect();
    if events.is_empty() && now - record.changed > RUNNING {
        return None;
    }
    let running = match record.running {
        Some(said) => said && now - record.changed <= STALLED,
        None => now - record.changed <= RUNNING,
    };
    let latest = events.last();
    let project = record
        .project
        .or_else(|| events.iter().rev().find_map(|event| text_of(event.get("project"))))
        .map(Json::from)
        .unwrap_or(Json::Null);
    let mut turns: Vec<Object> = Vec::new();
    let mut previous: Option<f64> = None;
    let (mut requests, mut cost, mut unpriced) = (0i64, 0.0, 0i64);
    let mut days = Object::new();
    let mut spends: Vec<Json> = Vec::new();
    for event in &events {
        let at = number(event, "timestamp");
        if previous.is_none_or(|before| at - before > TURN_GAP) {
            turns.push(turn(Json::Double(at)));
        }
        previous = Some(at);
        let priced =
            event.get("cost").filter(|cost| !cost.is_null()).and_then(Json::python_float).or_else(|| price(event));
        let tokens = ["input", "output", "cached", "writes"].iter().map(|key| number(event, key)).sum::<f64>();
        if let Some(current) = turns.last_mut() {
            bump(current, "requests", Json::Int(1));
            bump(current, "tokens", Json::Int(tokens as i64));
            match priced {
                Some(spent) => bump(current, "cost", Json::Double(spent)),
                None => bump(current, "unpriced", Json::Int(1)),
            }
        }
        requests += 1;
        match priced {
            Some(spent) => cost += spent,
            None => unpriced += 1,
        }
        add_to_day(&mut days, &day_of(at, zone), priced);
        if at >= now - SPENDS_KEPT {
            spends.push(Json::Array(vec![Json::Double(at), priced.map_or(Json::Null, Json::Double)]));
        }
    }
    let started = events.first().map_or(Json::Null, |event| Json::Double(number(event, "timestamp")));
    Some(object! {
        "tool" => tool, "module" => module, "id" => hash(&record.key),
        "title" => record.title.map_or(Json::Null, Json::from), "project" => project,
        "model" => latest.and_then(|event| text_of(event.get("model"))).map_or(Json::Null, Json::from),
        "effort" => Json::Null, "context" => Json::Null,
        "state" => if running { "running" } else { "waiting" }, "started" => started, "at" => record.changed,
        "turns" => Json::Array(last_turns(turns)),
        "spent" => object! {
            "requests" => requests, "cost" => cost, "unpriced" => unpriced,
            "ranges" => range_costs(&[(days, spends)], ranges, zone),
        },
    })
}

fn text_of(value: Option<&Json>) -> Option<String> {
    value.and_then(Json::as_str).map(str::trim).filter(|text| !text.is_empty() && *text != "未知模型").map(String::from)
}

impl Runtime {
    /// The other tools' sessions changed since `since`, each with the time it last changed.
    pub fn tool_sessions(
        &self,
        since: f64,
        now: f64,
        ranges: &[(String, f64, f64)],
        zone: &TimeZone,
    ) -> Vec<(f64, Object)> {
        let cache = Path::new(&self.cache_dir);
        let mut found = Vec::new();
        for (module, tool) in TOOLS {
            if !self.module_enabled(module) {
                continue;
            }
            let root = harness::normalize(&paths::expand_user(&self.source_path(module)).to_string_lossy());
            let (records, price): (Vec<Record>, &Pricing) = match module {
                "gemini" => (gemini_records(cache, &root, since), &harness::magpie_price),
                "opencode" => (opencode_records(&root, since), &pricing::model_cost),
                _ => (jsonl_records(cache, module, &root, since), &pricing::model_cost),
            };
            for record in records {
                let changed = record.changed;
                if let Some(session) = session_of(tool, module, record, now, price, ranges, zone) {
                    found.push((changed, session));
                }
            }
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(at: f64, model: &str, input: i64, cost: Option<f64>) -> Object {
        object! {
            "timestamp" => at, "model" => model, "input" => input, "output" => 10, "cached" => 0, "writes" => 0,
            "cost" => cost.map_or(Json::Null, Json::Double), "project" => "harbor",
        }
    }

    fn record(changed: f64, events: Vec<Object>) -> Record {
        Record { key: "t:1".into(), changed, events, title: None, project: None, running: None }
    }

    #[test]
    fn a_tools_session_has_its_turns_its_cost_and_its_state() {
        let now = 1_800_000_000.0;
        let ranges = vec![("today".to_string(), now - 3600.0, now), ("all".to_string(), 0.0, now)];
        let zone = TimeZone::UTC;
        let price = |_: &Object| -> Option<f64> { None };
        // Two turns (ten minutes apart), the last request a moment ago; one unpriced request, one ahead of now.
        let events = vec![
            event(now - 1200.0, "glm-5", 100, Some(0.5)),
            event(now - 1150.0, "glm-5", 100, Some(0.25)),
            event(now - 30.0, "glm-5.1", 200, None),
            event(now + 300.0, "glm-5.1", 999, Some(9.0)),
        ];
        let session =
            session_of("WorkBuddy", "workbuddy", record(now - 20.0, events.clone()), now, &price, &ranges, &zone)
                .expect("a session");
        assert_eq!(session.get("state"), Some(&Json::from("running")));
        assert_eq!(session.get("model"), Some(&Json::from("glm-5.1")));
        assert_eq!(session.get("project"), Some(&Json::from("harbor")));
        let turns = session.get("turns").and_then(Json::as_array).cloned().unwrap();
        assert_eq!(turns.len(), 2);
        // Newest first: one request, unpriced; then the two of the first turn.
        assert_eq!(turns[0].get("requests").and_then(Json::integer), Some(1));
        assert_eq!(turns[0].get("unpriced").and_then(Json::integer), Some(1));
        assert_eq!(turns[1].get("requests").and_then(Json::integer), Some(2));
        assert_eq!(turns[1].get("tokens").and_then(Json::integer), Some(220));
        let spent = session.get("spent").unwrap();
        assert_eq!(spent.get("requests").and_then(Json::integer), Some(3));
        assert_eq!(spent.get("cost").and_then(Json::python_float), Some(0.75));
        assert_eq!(spent.get("unpriced").and_then(Json::integer), Some(1));
        let today = spent.get("ranges").and_then(|ranges| ranges.get("today")).unwrap();
        assert_eq!(today.get("requests").and_then(Json::integer), Some(3));
        // Quiet for over a minute: idle.
        let session =
            session_of("WorkBuddy", "workbuddy", record(now - 90.0, events), now, &price, &ranges, &zone).unwrap();
        assert_eq!(session.get("state"), Some(&Json::from("waiting")));
    }

    #[test]
    fn antigravity_says_itself_whether_a_conversation_is_working() {
        let now = 1_800_000_000.0;
        let ranges = vec![("today".to_string(), now - 3600.0, now)];
        let price = |_: &Object| -> Option<f64> { Some(0.1) };
        let mut working = record(now - 200.0, vec![event(now - 200.0, "gemini-3-pro", 10, None)]);
        working.running = Some(true);
        working.title = Some("修订论文".into());
        let session = session_of("Gemini", "gemini", working, now, &price, &ranges, &TimeZone::UTC).unwrap();
        assert_eq!(session.get("state"), Some(&Json::from("running")));
        assert_eq!(session.get("title"), Some(&Json::from("修订论文")));
        // Said to be working but silent past ten minutes: stopped without saying so.
        let mut stalled = record(now - 900.0, vec![event(now - 900.0, "gemini-3-pro", 10, None)]);
        stalled.running = Some(true);
        let session = session_of("Gemini", "gemini", stalled, now, &price, &ranges, &TimeZone::UTC).unwrap();
        assert_eq!(session.get("state"), Some(&Json::from("waiting")));
    }

    #[test]
    fn a_workspace_uri_names_the_project() {
        assert_eq!(percent_decoded("/Users/me/My%20Paper"), "/Users/me/My Paper");
        assert_eq!(percent_decoded("/a/%E4%B8%AD"), "/a/中");
    }
}

#[cfg(test)]
mod reading {
    use crate::json::Json;
    use crate::object;
    use crate::runtime::{Runtime, now};

    /// End to end: a WorkBuddy conversation on disk, read for its usage (which indexes it), then listed in 会话 with the
    /// requests its card counts; nothing listed for a tool off the homepage.
    #[test]
    fn a_workbuddy_conversation_is_listed_with_the_requests_its_card_counts() {
        let home = std::env::temp_dir().join(format!("starmeter-tool-sessions-{}", crate::runtime::random_hex(6)));
        let project = home.join(".workbuddy/projects/harbor");
        std::fs::create_dir_all(&project).unwrap();
        let moment = now();
        let stamp = |ago: f64| jiff::Timestamp::from_second((moment - ago) as i64).unwrap().to_string();
        let mut lines: Vec<String> = [(300.0, "w1", 900), (240.0, "w2", 1100), (20.0, "w3", 500)]
            .iter()
            .map(|(ago, id, input)| {
                format!(
                    r#"{{"type":"message","timestamp":"{}","cwd":"/Users/demo/harbor","message":{{"role":"assistant","id":"{id}","model":"glm-5","usage":{{"input_tokens":{input},"output_tokens":10}}}}}}"#,
                    stamp(*ago)
                )
            })
            .collect();
        lines.push(r#"{"type":"ai-title","aiTitle":"整理发布说明","sessionId":"s1"}"#.to_string());
        std::fs::write(project.join("w.jsonl"), lines.join("\n") + "\n").unwrap();
        let state = home.join("state");
        std::fs::create_dir_all(&state).unwrap();
        let write_prefs = |visible: &[&str]| {
            let prefs = object! {
                "schema" => 1, "visible" => Json::from(visible.iter().map(|key| key.to_string()).collect::<Vec<_>>()),
            };
            std::fs::write(state.join("preferences.json"), Json::Object(prefs).serialized()).unwrap();
        };
        write_prefs(&["summary", "workbuddy", "sessions"]);
        let environment: std::collections::HashMap<String, String> = [
            ("HOME", home.to_string_lossy().into_owned()),
            ("OCX_STATE_DIR", state.to_string_lossy().into_owned()),
            ("OCX_CACHE_DIR", home.join("cache").to_string_lossy().into_owned()),
            ("OCX_TIMEZONE", "UTC".to_string()),
            // The home folder the engine reads is this machine's: the tool's folder is given.
            ("WORKBUDDY_USAGE_DIR", home.to_string_lossy().into_owned()),
            ("CLAUDE_CONFIG_DIR", home.join("claude").to_string_lossy().into_owned()),
            ("CLAUDE_APP_DATA_DIR", home.join("claude-app").to_string_lossy().into_owned()),
            ("OCX_CHATGPT_HOME", home.join("codex").to_string_lossy().into_owned()),
            ("CODEX_HOME", home.join("codex").to_string_lossy().into_owned()),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect();
        let runtime = Runtime::new(environment.clone(), "starmeter");
        let (usage, _) = runtime.range_usage(Some("today"), Some(0.0));
        let card =
            usage.as_ref().and_then(|usage| usage.get("workbuddy")).and_then(|card| card.get("summary")).cloned();
        assert_eq!(card.as_ref().and_then(|summary| summary.get("requests")).and_then(Json::integer), Some(3));
        let (payload, _) = runtime.sessions_cached(Some(0.0));
        let sessions = payload
            .and_then(|payload| payload.get("sessions").cloned())
            .and_then(|list| list.as_array().cloned())
            .unwrap();
        let session =
            sessions.iter().find(|session| session.get("tool") == Some(&Json::from("WorkBuddy"))).expect("listed");
        assert_eq!(session.get("state"), Some(&Json::from("running")));
        assert_eq!(session.get("model"), Some(&Json::from("glm-5")));
        assert_eq!(session.get("title"), Some(&Json::from("整理发布说明")));
        assert_eq!(session.get("project"), Some(&Json::from("harbor")));
        assert_eq!(session.get("spent").and_then(|spent| spent.get("requests")).and_then(Json::integer), Some(3));
        // Two turns: the first two requests a minute apart, the last one four minutes on.
        assert_eq!(session.get("turns").and_then(Json::as_array).map(Vec::len), Some(2));
        // Its row in 会话 carries WorkBuddy's own mark.
        let data = object! { "sessions" => object! { "sessions" => Json::Array(sessions.clone()) } };
        let lines = runtime.sessions_lines_in(&data, &object! { "symbol" => "$", "rate" => 1 }.into(), "today");
        assert!(lines.iter().any(|line| line.contains("templateImage=") || line.contains("image=")), "{lines:?}");
        // Off the homepage, the tool's sessions aren't looked at.
        write_prefs(&["summary", "sessions"]);
        let runtime = Runtime::new(environment, "starmeter");
        let (payload, _) = runtime.sessions_cached(Some(0.0));
        let sessions = payload
            .and_then(|payload| payload.get("sessions").cloned())
            .and_then(|list| list.as_array().cloned())
            .unwrap();
        assert!(sessions.iter().all(|session| session.get("tool") != Some(&Json::from("WorkBuddy"))));
        let _ = std::fs::remove_dir_all(&home);
    }
}
