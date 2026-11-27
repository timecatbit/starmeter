//! Sessions of the other tools on the homepage, beside Claude Code's and Codex's: Gemini (its CLI, Antigravity and
//! Google's Gemini app), OpenCode, Qoder, WorkBuddy and DeepSeek. Each is read from what that tool's usage reader keeps of it, so a session's
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
/// Requests further apart than this are told as two turns when a tool records no user prompt timestamps.
const TURN_GAP: f64 = 180.0;

/// The tools read here: module, the name a session carries, how its requests are priced when the record has no cost.
const TOOLS: [(&str, &str); 5] = [
    ("gemini", "Gemini"),
    ("opencode", "OpenCode"),
    ("qoder", "Qoder"),
    ("workbuddy", "WorkBuddy"),
    ("deepseek-harness", "DeepSeek Harness"),
];

/// How a request is priced when its record carries no cost.
type Pricing = dyn Fn(&Object) -> Option<f64>;
/// What is known of a transcript by its path: the file's print when read, its title, project and prompt timestamps.
type Names = HashMap<String, (String, Option<String>, Option<String>, Vec<f64>)>;

/// A session's file (or conversation) and what its record says beyond its requests.
#[derive(Clone)]
struct Record {
    key: String,
    changed: f64,
    events: Vec<Object>,
    title: Option<String>,
    project: Option<String>,
    /// The tool's own word on whether it is working, when it keeps one (Antigravity).
    running: Option<bool>,
    /// Real user prompt timestamps (oldest first) when the tool records them (Antigravity, OpenCode, WorkBuddy, Qoder).
    prompts: Option<Vec<f64>>,
}

fn hash(text: &str) -> String {
    py::sha256_hex(text.as_bytes())[..16].to_string()
}

fn number(event: &Object, key: &str) -> f64 {
    event.get(key).and_then(Json::python_float).unwrap_or(0.0)
}

/// What a transcript in Claude Code's shape (WorkBuddy's, Qoder's) names itself: the title the user gave it, else the
/// one its tool made, the folder it works in, and any user prompt timestamps. From its last half megabyte, else its
/// start; kept while the file is unchanged, as a pinned panel reads the sessions every second or two.
fn transcript_names(path: &str) -> (Option<String>, Option<String>, Vec<f64>) {
    static KNOWN: std::sync::Mutex<Option<Names>> = std::sync::Mutex::new(None);
    let Some(state) = state_of(Path::new(path)) else { return (None, None, Vec::new()) };
    let print = format!("{}:{}:{}", state.inode, state.size, state.mtime);
    if let Some((_, title, folder, prompts)) =
        KNOWN.lock().ok().and_then(|known| known.as_ref()?.get(path).filter(|(seen, _, _, _)| *seen == print).cloned())
    {
        return (title, folder, prompts);
    }
    let scan = |rows: Vec<Object>| {
        let (mut named, mut made, mut folder) = (None::<String>, None::<String>, None::<String>);
        let mut prompts = Vec::new();
        for row in rows {
            match row.get("type").and_then(Json::as_str) {
                Some("custom-title") => named = text_of(row.get("customTitle")).or(named),
                Some("ai-title") => made = text_of(row.get("aiTitle")).or(made),
                _ => {}
            }
            if let Some(cwd) = text_of(row.get("cwd")) {
                folder = Some(cwd);
            }
            if crate::sessions::is_prompt(&row) {
                if let Some(at) = py::stamp(row.get("timestamp")).ok().and_then(|v| v.number()) {
                    prompts.push(at);
                }
            } else if row.get("type").and_then(Json::as_str) == Some("message")
                && row.get("message").and_then(|m| m.get("role")).and_then(Json::as_str) == Some("user")
            {
                let content = row.get("message").and_then(|m| m.get("content"));
                let tool_only = content.and_then(Json::as_array).is_some_and(|arr| {
                    !arr.is_empty() && arr.iter().all(|item| item.get("type") == Some(&Json::from("tool_result")))
                });
                if !tool_only && let Some(at) = py::stamp(row.get("timestamp")).ok().and_then(|v| v.number()) {
                    prompts.push(at);
                }
            } else if row.get("type").and_then(Json::as_str) == Some("user/message")
                && let Some(at) = py::stamp(row.get("time")).ok().and_then(|v| v.number())
            {
                prompts.push(at);
            }
        }
        (named.or(made), folder, prompts)
    };
    let (mut title, mut folder, prompts) = scan(rows_at_end(path, 512 * 1024).unwrap_or_default());
    if title.is_none() || folder.is_none() {
        let (early_title, early_folder, _) = scan(rows_at_start(path, 64 * 1024).unwrap_or_default());
        title = title.or(early_title);
        folder = folder.or(early_folder);
    }
    let project =
        folder.and_then(|folder| py::project_name(Some(&Json::from(folder.as_str()))).as_str().map(String::from));
    if let Ok(mut known) = KNOWN.lock() {
        known
            .get_or_insert_with(HashMap::new)
            .insert(path.to_string(), (print, title.clone(), project.clone(), prompts.clone()));
    }
    (title, project, prompts)
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
                let (title, project, prompts) = transcript_names(&path);
                Record {
                    key: format!("{source}:{path}"),
                    changed,
                    events: indexed(&db, &path),
                    title,
                    project,
                    running: None,
                    prompts: (!prompts.is_empty()).then_some(prompts),
                }
            })
        })
        .collect()
}

/// What Antigravity's own list says of a conversation.
#[derive(Default)]
struct Summary {
    title: Option<String>,
    /// Still working.
    running: Option<bool>,
    /// Its workspace's name.
    project: Option<String>,
    /// The conversation it was started from: a subagent's (research and the like) is its task's.
    parent: Option<String>,
}

fn antigravity_summary(
    store: &str,
    conversation: &str,
    lists: &mut HashMap<std::path::PathBuf, Option<Database>>,
) -> Summary {
    // <root>/antigravity…/conversations[/implicit]/<id>.db, its list beside conversations/.
    let folder =
        Path::new(store).ancestors().find(|folder| folder.file_name().is_some_and(|name| name == "conversations"));
    let Some(list) = folder.and_then(Path::parent).map(|parent| parent.join("conversation_summaries.db")) else {
        return Summary::default();
    };
    // Open each conversation_summaries.db once per Antigravity root instead of N+1 opens per conversation (PERF-5).
    let db_slot = lists.entry(list.clone()).or_insert_with(|| open_index(&list));
    let Some(db) = db_slot.as_ref() else { return Summary::default() };
    let full = "SELECT title, status, not_fully_idle, workspace_uris, parent_conversation_id FROM conversation_summaries \
                WHERE conversation_id=?";
    let short =
        "SELECT title, status, not_fully_idle, workspace_uris FROM conversation_summaries WHERE conversation_id=?";
    let id = [Value::Text(conversation.to_string())];
    // An older list keeps no parent: every conversation stands alone there.
    let row = db.first(full, &id).or_else(|_| db.first(short, &id)).ok().flatten();
    let Some(row) = row else { return Summary::default() };
    let parent =
        row.get(4).and_then(database::text).map(str::trim).filter(|parent| !parent.is_empty()).map(String::from);
    let title = row.first().and_then(database::text).map(str::trim).filter(|title| !title.is_empty()).map(String::from);
    let status = row.get(1).and_then(database::text).unwrap_or("");
    let busy = row.get(2).and_then(database::int).is_some_and(|flag| flag != 0);
    let running = Some(status.contains("RUNNING") || busy);
    let project = row.get(3).and_then(database::text).and_then(harness::workspace_project);
    Summary { title, running, project, parent }
}

/// Gemini's sessions: the CLI's chat files and Antigravity's conversations (one store each; a conversation kept in two
/// stores, the app's and the IDE's, is one session, as its usage counts once). The subagents a task starts (research
/// and the like, each a conversation of its own with no title) are part of that task's session, as Antigravity shows
/// them: their requests counted in it, and it running while any of them is (用户：反重力会启动很多会话).
fn gemini_records(cache: &Path, root: &str, since: f64) -> Vec<Record> {
    let index = cache.join(format!("harness-gemini-v5-{}.sqlite3", harness::root_identity(root)));
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
                prompts: None,
            })
        })
        .collect();
    // Each conversation's copies: the app's, the IDE's and the agy CLI's stores may each keep it.
    let mut copies: HashMap<String, Vec<(String, f64)>> = HashMap::new();
    for store in harness::antigravity_stores(root) {
        let Some((_, mtime)) = harness::store_state(&store) else { continue };
        let id = Path::new(&store).file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
        copies.entry(id).or_default().push((store, mtime as f64 / 1e9));
    }
    // A conversation's record: its title, project and state from its newest copy; its requests from all of them, each
    // response once, from the copy that dates it best (its timing), as the usage counts it, since a copy may hold
    // responses another lacks (AGY-6).
    let mut summary_dbs: HashMap<std::path::PathBuf, Option<Database>> = HashMap::new();
    let mut read = |id: &str| -> Option<(Record, Option<String>)> {
        let kept = copies.get(id)?;
        let (newest, changed) = kept.iter().max_by(|a, b| a.1.total_cmp(&b.1))?;
        let summary = antigravity_summary(newest, id, &mut summary_dbs);
        let mut best: HashMap<String, Object> = HashMap::new();
        let mut order: Vec<String> = Vec::new();
        for (store, _) in kept {
            for (index, event) in indexed(&db, store).into_iter().enumerate() {
                let key =
                    event.get("id").and_then(Json::as_str).map_or_else(|| format!("{store}#{index}"), String::from);
                let timing = |event: &Object| event.get("timing").and_then(Json::number).unwrap_or(0.0);
                if best.get(&key).is_some_and(|known| timing(known) >= timing(&event)) {
                    continue;
                }
                if !best.contains_key(&key) {
                    order.push(key.clone());
                }
                best.insert(key, event);
            }
        }
        let mut events: Vec<Object> = order.into_iter().filter_map(|key| best.remove(&key)).collect();
        events.sort_by(|a, b| number(a, "timestamp").total_cmp(&number(b, "timestamp")));
        let prompts = harness::antigravity_prompts(newest);
        let record = Record {
            key: format!("antigravity:{id}"),
            changed: *changed,
            events,
            title: summary.title,
            project: summary.project,
            running: summary.running,
            prompts: (!prompts.is_empty()).then_some(prompts),
        };
        Some((record, summary.parent))
    };
    let mut conversations: HashMap<String, Record> = HashMap::new();
    let mut parents: HashMap<String, String> = HashMap::new();
    let mut listed: Vec<&String> = copies.keys().collect();
    listed.sort();
    for id in listed {
        if !copies[id].iter().any(|(_, changed)| *changed >= since) {
            continue;
        }
        let Some((record, parent)) = read(id) else { continue };
        if let Some(parent) = parent.filter(|parent| parent != id) {
            parents.insert(id.clone(), parent);
        }
        conversations.insert(id.clone(), record);
    }
    // A task not read yet (only its subagents changed since): read the same way, with its own parent.
    fold_subagents(&mut conversations, parents, |parent| read(parent));
    let mut ids: Vec<String> = conversations.keys().cloned().collect();
    ids.sort();
    records.extend(ids.into_iter().filter_map(|id| conversations.remove(&id)));
    records
}

/// Each subagent into its task (by `parents`, subagent → task), a subagent's subagent too: up the chain of parents to
/// the top task, every conversation on the way folded into it once (its requests among the task's, by time; the task
/// running while any of them is). A task not read (only its subagents changed since) is read through `load`, with its
/// own parent, so the chain goes on above it; a chain that loops ends at its least id. A subagent whose task is gone
/// stays a session of its own, and so does one under it.
fn fold_subagents(
    conversations: &mut HashMap<String, Record>,
    mut parents: HashMap<String, String>,
    mut load: impl FnMut(&str) -> Option<(Record, Option<String>)>,
) {
    let mut children: Vec<String> = parents.keys().cloned().collect();
    children.sort();
    let mut tops: HashMap<String, String> = HashMap::new();
    for child in children {
        let mut chain = vec![child];
        let top = loop {
            let current = chain.last().expect("the chain").clone();
            let Some(parent) = parents.get(&current).cloned() else { break current };
            if let Some(at) = chain.iter().position(|id| *id == parent) {
                break chain[at..].iter().min().expect("the loop").clone();
            }
            if !conversations.contains_key(&parent) {
                let Some((record, above)) = load(&parent) else { break current };
                conversations.insert(parent.clone(), record);
                if let Some(above) = above.filter(|above| *above != parent) {
                    parents.insert(parent.clone(), above);
                }
            }
            chain.push(parent);
        };
        for id in chain.into_iter().filter(|id| *id != top) {
            tops.insert(id, top.clone());
        }
    }
    let mut folded: Vec<(String, String)> = tops.into_iter().collect();
    folded.sort();
    for (id, top) in folded {
        let Some(sub) = conversations.remove(&id) else { continue };
        let Some(task) = conversations.get_mut(&top) else {
            conversations.insert(id, sub);
            continue;
        };
        task.changed = task.changed.max(sub.changed);
        task.events.extend(sub.events);
        task.events.sort_by(|a, b| number(a, "timestamp").total_cmp(&number(b, "timestamp")));
        task.running = Some(task.running.unwrap_or(false) || sub.running.unwrap_or(false));
        if task.project.is_none() {
            task.project = sub.project;
        }
    }
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
        let prompts: Vec<f64> = db
            .rows(
                "SELECT time_created FROM message WHERE session_id=? AND json_extract(data,'$.role')='user' ORDER BY time_created",
                std::slice::from_ref(&session),
            )
            .unwrap_or_default()
            .into_iter()
            .filter_map(|row| row.first().and_then(database::double).map(|ms| ms / 1000.0))
            .collect();
        records.push(Record {
            key: format!("opencode:{key}"),
            changed,
            events,
            title,
            project: None,
            running: None,
            prompts: (!prompts.is_empty()).then_some(prompts),
        });
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
    // When real user prompt timestamps are known (Antigravity, OpenCode, WorkBuddy, Qoder), group events by those real
    // turn boundaries so a long command/test (> TURN_GAP) never falsely splits the ongoing turn or resets its clock.
    // Before the first prompt known (WorkBuddy's and Qoder's are read from the transcript's last half megabyte only),
    // and with none known, requests further apart than TURN_GAP are two turns, as before: a long history read without
    // its prompts would otherwise be one turn.
    let prompts: Vec<f64> = record.prompts.unwrap_or_default().into_iter().filter(|&at| at <= now).collect();
    let mut prompt_index = 0usize;
    for event in &events {
        let at = number(event, "timestamp");
        let mut advanced = None;
        while prompt_index < prompts.len() && prompts[prompt_index] <= at {
            advanced = Some(prompts[prompt_index]);
            prompt_index += 1;
        }
        if let Some(prompt_at) = advanced {
            turns.push(turn(Json::Double(prompt_at)));
        } else if prompt_index == 0 && previous.is_none_or(|before| at - before > TURN_GAP) {
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
    // If the session is actively running and a new user prompt arrived after the last completed request, start the
    // active turn from that prompt timestamp immediately so the live clock ticks from the real prompt time.
    if running && prompt_index < prompts.len() {
        turns.push(turn(Json::Double(*prompts.last().expect("prompt"))));
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

// Gemini App

/// Core Data's clock starts at 2001-01-01: its seconds plus this are Unix seconds.
const CORE_DATA_EPOCH: f64 = 978_307_200.0;
/// How much of the newest diagnostic log is read for whether a reply is being written.
const LOG_TAIL: u64 = 256 * 1024;

/// A conversation of Google's Gemini app, as its cache keeps it: when it began (its first message), when it was last
/// active, how many times the user spoke. Its title, its messages and the model are encrypted and never read.
struct AppChat {
    chat: String,
    conversation: String,
    started: Option<f64>,
    last_message: Option<f64>,
    at: f64,
    rounds: i64,
}

/// The Gemini app's conversations active since `since`, from every account's ChatInfo2.store (Core Data's SQLite)
/// under <cache>/Gemini/user*/: only the ids, the times and the count of the user's messages.
fn gemini_app_chats(cache: &str, since: f64) -> Vec<AppChat> {
    let mut accounts: Vec<std::path::PathBuf> = std::fs::read_dir(Path::new(cache).join("Gemini"))
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.file_name().to_string_lossy().starts_with("user"))
                .map(|entry| entry.path().join("ChatInfo2.store"))
                .collect()
        })
        .unwrap_or_default();
    accounts.sort();
    let mut chats = Vec::new();
    for store in accounts {
        let Some(db) = open_index(&store) else { continue };
        let rows = db
            .rows(
                "SELECT c.ZCHATUUID, c.ZROBINCONVERSATIONID, c.ZLASTUPDATEDTIME, MIN(m.ZCREATEDTIME), MAX(m.ZCREATEDTIME), \
                 COALESCE(SUM(m.ZISUSERTURN), 0) FROM ZCHATINFOSTOREDMODEL c LEFT JOIN ZCHATMESSAGESTOREDMODEL m \
                 ON m.ZCHATUUID = c.ZCHATUUID GROUP BY c.Z_PK",
                &[],
            )
            .unwrap_or_default();
        for row in rows {
            let text = |index: usize| row.get(index).and_then(database::text).unwrap_or("").to_string();
            let moment = |index: usize| row.get(index).and_then(database::double).map(|at| at + CORE_DATA_EPOCH);
            let (updated, last_message) = (moment(2), moment(4));
            let Some(at) = [updated, last_message].into_iter().flatten().reduce(f64::max) else { continue };
            if at < since || text(0).is_empty() {
                continue;
            }
            chats.push(AppChat {
                chat: text(0),
                conversation: text(1),
                started: moment(3),
                last_message,
                at,
                rounds: row.get(5).and_then(database::int).unwrap_or(0),
            });
        }
    }
    chats
}

/// A line's time in the Gemini app's diagnostic log (20261123-155101.847 …), on this computer's clock.
fn log_time(line: &str) -> Option<f64> {
    let stamp = line.get(..19)?;
    let number = |range: std::ops::Range<usize>| stamp.get(range)?.parse::<i32>().ok();
    let (year, month, day) = (number(0..4)?, number(4..6)?, number(6..8)?);
    let (hour, minute, second) = (number(9..11)?, number(11..13)?, number(13..15)?);
    let millis = number(16..19)?;
    if stamp.as_bytes().get(8) != Some(&b'-') || stamp.as_bytes().get(15) != Some(&b'.') {
        return None;
    }
    // A stamp out of range (a damaged or cut-off line, or another format) is no time, never a panic.
    let civil = jiff::civil::DateTime::new(
        i16::try_from(year).ok()?,
        i8::try_from(month).ok()?,
        i8::try_from(day).ok()?,
        i8::try_from(hour).ok()?,
        i8::try_from(minute).ok()?,
        i8::try_from(second).ok()?,
        millis.checked_mul(1_000_000)?,
    )
    .ok()?;
    let zoned = civil.to_zoned(TimeZone::system()).ok()?;
    Some(zoned.timestamp().as_millisecond() as f64 / 1000.0)
}

/// What the Gemini app's newest diagnostic log says at its end: each reply is opened with addStreamingConversation (a
/// few times) and closed with one removeStreamingConversation, and the conversation it syncs is named in
/// /bard-client-sync/<id>.
struct AppLog {
    /// A reply is being written (opened, not closed, and less than ten minutes ago: older, it stopped unsaid).
    streaming: bool,
    /// The conversation synced last, and when the last reply was opened or closed.
    conversation: Option<String>,
    at: Option<f64>,
}

/// The log's word, None when it says nothing either way (no log, no such event).
fn gemini_app_streaming(cache: &str, now: f64) -> Option<AppLog> {
    static SYNCED: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"/bard-client-sync/(c_[0-9A-Za-z]+)").expect("sync pattern"));
    let log = std::fs::read_dir(Path::new(cache).join("Logs"))
        .ok()?
        .flatten()
        .filter(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.starts_with("diagnostic") && name.ends_with(".log")
        })
        .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, path)| path)?;
    let text = {
        use std::io::{Read, Seek, SeekFrom};
        let mut file = std::fs::File::open(&log).ok()?;
        let size = file.metadata().ok()?.len();
        let start = size.saturating_sub(LOG_TAIL);
        file.seek(SeekFrom::Start(start)).ok()?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).ok()?;
        let mut text = String::from_utf8_lossy(&bytes).into_owned();
        // A tail read from the middle of a line starts at the next whole one.
        if start > 0 {
            text = text.split_once('\n').map(|(_, rest)| rest.to_string()).unwrap_or_default();
        }
        text
    };
    let (mut last, mut synced): (Option<(bool, Option<f64>)>, Option<String>) = (None, None);
    for line in text.lines() {
        if line.contains("addStreamingConversation") {
            last = Some((true, log_time(line)));
        } else if line.contains("removeStreamingConversation") {
            last = Some((false, log_time(line)));
        }
        if let Some(found) = SYNCED.captures(line).and_then(|found| found.get(1)) {
            synced = Some(found.as_str().to_string());
        }
    }
    let (opened, at) = last?;
    Some(AppLog { streaming: opened && at.is_some_and(|at| now - at <= STALLED), conversation: synced, at })
}

/// The Gemini app's conversations as sessions. Its cache keeps no tokens and no cost (ZBYTESIZE is the encrypted
/// message's size, not tokens): each shows whether it is running, when it was last active and its rounds (the user's
/// messages). Running is the log's word; without one, a message in the last minute. The conversation the log last
/// names was active when its reply last opened or closed, so a reply's end is told even before its message is kept.
fn gemini_app_sessions(cache: &str, since: f64, now: f64) -> Vec<(f64, Object)> {
    let chats = gemini_app_chats(cache, since);
    if chats.is_empty() {
        return vec![];
    }
    let streaming = gemini_app_streaming(cache, now);
    chats
        .into_iter()
        .filter_map(|mut chat| {
            let named =
                streaming.as_ref().filter(|log| log.conversation.as_deref() == Some(chat.conversation.as_str()));
            let running = match &streaming {
                Some(log) => log.streaming && named.is_some(),
                None => chat.last_message.is_some_and(|at| now - at <= RUNNING),
            };
            // A conversation with nothing said in it yet (the app keeps some empty) isn't a session, unless a reply is
            // being written in it now.
            if chat.rounds == 0 && !running {
                return None;
            }
            if let Some(at) = named.and_then(|log| log.at).filter(|at| *at <= now) {
                chat.at = chat.at.max(at);
            }
            let session = object! {
                "tool" => "Gemini", "module" => "gemini", "id" => hash(&format!("gemini-app:{}", chat.chat)),
                "title" => "Gemini App", "project" => Json::Null, "model" => Json::Null, "effort" => Json::Null,
                "context" => Json::Null, "state" => if running { "running" } else { "waiting" },
                "started" => chat.started.map_or(Json::Null, Json::Double), "at" => chat.at, "rounds" => chat.rounds,
                "turns" => Json::Array(vec![]),
                "spent" => object! { "requests" => 0, "cost" => 0.0, "unpriced" => 0, "ranges" => Object::new() },
            };
            Some((chat.at, session))
        })
        .collect()
}

impl Runtime {
    /// Where Google's Gemini app (com.google.GeminiMacOS) keeps its cache: GEMINI_APP_CACHE_DIR, else its folder in
    /// ~/Library/Caches. A cache: only conversations opened in the app lately, which it may clear at any time.
    pub fn gemini_app_cache(&self) -> String {
        self.env("GEMINI_APP_CACHE_DIR")
            .map(String::from)
            .unwrap_or_else(|| format!("{}/Library/Caches/com.google.GeminiMacOS", self.home))
    }

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
        // Google's Gemini app, with Gemini or Gemini 订阅 on the homepage (both are its).
        if self.module_enabled("gemini") || self.module_enabled("gemini-subscription") {
            found.extend(gemini_app_sessions(&self.gemini_app_cache(), since, now));
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
        Record { key: "t:1".into(), changed, events, title: None, project: None, running: None, prompts: None }
    }

    /// A Gemini app conversation: its chat, its conversation, how long ago it was updated, its messages (how long ago,
    /// whether the user's).
    type AppChatRow<'a> = (&'a str, &'a str, f64, &'a [(f64, bool)]);

    /// A Gemini app cache as the app keeps it: Core Data's two tables (with the encrypted column left empty) and its
    /// diagnostic log.
    fn gemini_app_cache(folder: &Path, now: f64, chats: &[AppChatRow]) {
        let account = folder.join("Gemini/user1");
        std::fs::create_dir_all(&account).unwrap();
        let _ = std::fs::remove_file(account.join("ChatInfo2.store"));
        let db = Database::open(&account.join("ChatInfo2.store"), 2.0, Mode::ReadWrite).unwrap();
        db.execute(
            "CREATE TABLE ZCHATINFOSTOREDMODEL (Z_PK INTEGER PRIMARY KEY, Z_ENT INTEGER, Z_OPT INTEGER, \
             ZLASTUPDATEDTIME TIMESTAMP, ZCHATUUID VARCHAR, ZROBINCONVERSATIONID VARCHAR, ZENCRYPTEDPROTOBYTES BLOB);
             CREATE TABLE ZCHATMESSAGESTOREDMODEL (Z_PK INTEGER PRIMARY KEY, Z_ENT INTEGER, Z_OPT INTEGER, \
             ZBYTESIZE INTEGER, ZHASTASKARTIFACTS INTEGER, ZISUSERTURN INTEGER, ZMESSAGEINDEX INTEGER, \
             ZCREATEDTIME TIMESTAMP, ZCHATUUID VARCHAR, ZMESSAGEUUID VARCHAR, ZENCRYPTEDPROTOBYTES BLOB);",
        )
        .unwrap();
        for (chat, conversation, updated, messages) in chats {
            db.run(
                "INSERT INTO ZCHATINFOSTOREDMODEL (ZLASTUPDATEDTIME, ZCHATUUID, ZROBINCONVERSATIONID) VALUES (?,?,?)",
                &[
                    Value::Real(now - updated - CORE_DATA_EPOCH),
                    Value::Text(chat.to_string()),
                    Value::Text(conversation.to_string()),
                ],
            )
            .unwrap();
            for (index, (ago, user)) in messages.iter().enumerate() {
                db.run(
                    "INSERT INTO ZCHATMESSAGESTOREDMODEL (ZBYTESIZE, ZISUSERTURN, ZMESSAGEINDEX, ZCREATEDTIME, ZCHATUUID) \
                     VALUES (4096, ?, ?, ?, ?)",
                    &[
                        Value::Integer(i64::from(*user)),
                        Value::Integer(index as i64),
                        Value::Real(now - ago - CORE_DATA_EPOCH),
                        Value::Text(chat.to_string()),
                    ],
                )
                .unwrap();
            }
        }
    }

    /// The diagnostic log's lines as the app writes them, on this computer's clock.
    fn log_line(now: f64, ago: f64, text: &str) -> String {
        let moment = jiff::Timestamp::from_millisecond(((now - ago) * 1000.0) as i64).unwrap();
        format!("{} [com.apple.main-thread] {text}", moment.to_zoned(TimeZone::system()).strftime("%Y%m%d-%H%M%S%.3f"))
    }

    #[test]
    fn a_log_stamp_out_of_range_is_no_time_and_no_panic() {
        let line = |stamp: &str| {
            format!("{stamp} [com.apple.main-thread] ConversationUpdateMonitor: addStreamingConversation() called.")
        };
        assert!(log_time(&line("20261123-093905.820")).is_some());
        for stamp in [
            "20261323-093905.820", // month 13
            "20260231-093905.820", // 31 February
            "20261100-093905.820", // day 0
            "20261123-253905.820", // hour 25
            "20261123-096005.820", // minute 60
            "20261123-093975.820", // second 75
            "2026-1-3-093905.820", // a negative month
            "20261123-093905.-20", // negative milliseconds
            "20261123-09390",      // cut off
        ] {
            assert_eq!(log_time(&line(stamp)), None, "{stamp}");
        }
    }

    #[test]
    fn the_gemini_apps_conversations_are_sessions_running_as_its_log_says() {
        let folder = std::env::temp_dir().join(format!("starmeter-gemini-app-{}", crate::runtime::random_hex(6)));
        let now = crate::runtime::now();
        // A: two rounds, the last five minutes ago. B: a message half a minute ago. C: untouched for a month. D: touched a
        // minute ago with nothing said in it (the app keeps empty ones): no session.
        let chats: [AppChatRow; 4] = [
            ("A", "c_aaaaaaaaaaaaaaaa", 600.0, &[(900.0, true), (890.0, false), (300.0, true), (290.0, false)]),
            ("B", "c_bbbbbbbbbbbbbbbb", 30.0, &[(40.0, true)]),
            ("C", "c_cccccccccccccccc", 30.0 * 86400.0, &[]),
            ("D", "c_dddddddddddddddd", 60.0, &[]),
        ];
        gemini_app_cache(&folder, now, &chats);
        let cache = folder.to_string_lossy().into_owned();
        let state = |sessions: &[(f64, Object)], title: &str| -> Vec<String> {
            sessions
                .iter()
                .filter(|(_, session)| session.get("title") == Some(&Json::from(title)))
                .map(|(_, session)| py::str(session.get("state")))
                .collect()
        };
        let found = gemini_app_sessions(&cache, now - 86400.0, now);
        assert_eq!(found.len(), 2, "the month-old conversation is outside the day, the empty one no session");
        let a = found.iter().find(|(_, session)| session.get("rounds") == Some(&Json::Int(2))).expect("A").1.clone();
        assert_eq!(a.get("title").and_then(Json::as_str), Some("Gemini App"));
        assert_eq!(a.get("tool").and_then(Json::as_str), Some("Gemini"));
        assert!((a.get("started").and_then(Json::number).unwrap() - (now - 900.0)).abs() < 0.01, "its first message");
        assert!((a.get("at").and_then(Json::number).unwrap() - (now - 290.0)).abs() < 0.01, "its last activity");
        assert_eq!(a.get("spent").and_then(|spent| spent.get("requests")), Some(&Json::Int(0)), "no tokens, no cost");
        // No log: a message in the last minute is running.
        let states: Vec<String> = found.iter().map(|(_, session)| py::str(session.get("state"))).collect();
        assert_eq!(states.iter().filter(|state| *state == "running").count(), 1);
        assert_eq!(
            py::str(found.iter().find(|(_, s)| s.get("rounds") == Some(&Json::Int(1))).unwrap().1.get("state")),
            "running"
        );
        // The log says a reply is being written for A: A runs, B (a message half a minute ago) doesn't.
        let logs = folder.join("Logs");
        std::fs::create_dir_all(&logs).unwrap();
        let write_log = |lines: &[String]| {
            std::fs::write(logs.join("diagnostic20261123-093905-820F.log"), lines.join("\n") + "\n").unwrap()
        };
        let mut lines = vec![
            log_line(now, 200.0, "ConversationUpdateMonitor: addStreamingConversation() called."),
            log_line(
                now,
                190.0,
                "ChatStore: Punctual objectId=/bard-client-sync/c_bbbbbbbbbbbbbbbb/r_01, payloadSize=72",
            ),
            log_line(now, 150.0, "ConversationUpdateMonitor: removeStreamingConversation() called."),
            log_line(now, 20.0, "ConversationUpdateMonitor: addStreamingConversation() called."),
            log_line(
                now,
                19.0,
                "ChatStore: Punctual objectId=/bard-client-sync/c_aaaaaaaaaaaaaaaa/r_02, payloadSize=72",
            ),
            log_line(now, 18.0, "ConversationUpdateMonitor: addStreamingConversation() called."),
        ];
        write_log(&lines);
        let found = gemini_app_sessions(&cache, now - 86400.0, now);
        let by_rounds = |rounds: i64| {
            py::str(found.iter().find(|(_, s)| s.get("rounds") == Some(&Json::Int(rounds))).unwrap().1.get("state"))
        };
        assert_eq!((by_rounds(2), by_rounds(1)), ("running".to_string(), "waiting".to_string()));
        assert_eq!(state(&found, "Gemini App").len(), 2);
        // The reply done: nothing runs, and A's turn finishing is told.
        lines.push(log_line(now, 2.0, "ConversationUpdateMonitor: removeStreamingConversation() called."));
        write_log(&lines);
        let done = gemini_app_sessions(&cache, now - 86400.0, now);
        assert!(done.iter().all(|(_, session)| session.get("state") == Some(&Json::from("waiting"))));
        let json =
            |sessions: &[(f64, Object)]| sessions.iter().map(|(_, s)| Json::Object(s.clone())).collect::<Vec<_>>();
        let told = crate::sessions::newly_waiting(&json(&found), &json(&done), now, 180.0);
        assert_eq!(told.len(), 1);
        assert_eq!(told[0].get("rounds"), Some(&Json::Int(2)));
        // A reply opened eleven minutes ago and never closed has stopped without saying so.
        write_log(&[
            log_line(now, 660.0, "addStreamingConversation() called."),
            log_line(now, 659.0, "objectId=/bard-client-sync/c_aaaaaaaaaaaaaaaa/r_03"),
        ]);
        assert!(
            gemini_app_sessions(&cache, now - 86400.0, now)
                .iter()
                .all(|(_, s)| s.get("state") == Some(&Json::from("waiting")))
        );

        // Read with Gemini 订阅 alone on the homepage too, and listed in 会话 by when each was last active.
        let state_dir = folder.join("state");
        std::fs::create_dir_all(&state_dir).unwrap();
        std::fs::write(
            state_dir.join("preferences.json"),
            r#"{"schema": 1, "visible": ["summary", "gemini-subscription", "sessions"]}"#,
        )
        .unwrap();
        let environment: HashMap<String, String> = [
            ("HOME", folder.to_string_lossy().into_owned()),
            ("OCX_STATE_DIR", state_dir.to_string_lossy().into_owned()),
            ("OCX_CACHE_DIR", folder.join("cache").to_string_lossy().into_owned()),
            ("GEMINI_APP_CACHE_DIR", cache.clone()),
            ("OCX_TIMEZONE", "UTC".to_string()),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect();
        let runtime = Runtime::new(environment, "starmeter");
        let found = runtime.tool_sessions(now - 86400.0, now, &[], &TimeZone::UTC);
        assert_eq!(found.len(), 2);
        let sessions: Vec<Json> = found.into_iter().map(|(_, session)| Json::Object(session)).collect();
        let data = object! { "sessions" => object! { "sessions" => Json::Array(sessions) } };
        let lines = runtime.sessions_lines_in(&data, &Json::Null, "today").join("\n");
        // Two sessions of one name are told apart by when each began; each says its rounds, never $0.00.
        assert_eq!(lines.matches("Gemini App · ").count(), 2, "{lines}");
        assert!(lines.contains("空闲 4 分钟 · 2 轮") || lines.contains("空闲 5 分钟 · 2 轮"), "{lines}");
        assert!(lines.contains("· 1 轮"), "{lines}");
        assert!(!lines.contains("$0.00"), "{lines}");
        let _ = std::fs::remove_dir_all(&folder);
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

    /// 用户：反重力会启动很多会话. A task's research subagents are part of its session: their requests in it, by time, it
    /// running while one is; the task read when only its subagents changed; a subagent whose task is gone stands alone.
    #[test]
    fn antigravitys_subagents_are_part_of_their_tasks_session() {
        let now = 1_800_000_000.0;
        let mut task = record(now - 300.0, vec![event(now - 300.0, "gemini-4-carbon", 10, Some(0.2))]);
        task.key = "antigravity:task".into();
        task.title = Some("Repository Bug Analysis".into());
        task.project = Some("swiftbar-token-monitor".into());
        task.running = Some(false);
        let mut research = record(now - 30.0, vec![event(now - 200.0, "gemini-3.5-flash", 5, Some(0.05))]);
        research.running = Some(true);
        let mut conversations: HashMap<String, Record> =
            [("task".to_string(), task), ("r1".to_string(), research)].into_iter().collect();
        let mut asked = Vec::new();
        fold_subagents(&mut conversations, [("r1".to_string(), "task".to_string())].into_iter().collect(), |id| {
            asked.push(id.to_string());
            None
        });
        assert!(asked.is_empty(), "the task was already read");
        assert_eq!(conversations.len(), 1);
        let task = &conversations["task"];
        assert_eq!(task.events.len(), 2);
        assert!(number(&task.events[0], "timestamp") < number(&task.events[1], "timestamp"), "by time");
        assert_eq!((task.running, task.changed), (Some(true), now - 30.0));
        // Only the subagent changed in the window: the task is read for it; an orphan stays.
        let mut late = record(now - 10.0, vec![event(now - 10.0, "gemini-3.5-flash", 3, None)]);
        late.running = Some(false);
        let orphan = record(now - 20.0, vec![]);
        let mut conversations: HashMap<String, Record> =
            [("r2".to_string(), late), ("r3".to_string(), orphan)].into_iter().collect();
        let parents =
            [("r2".to_string(), "old".to_string()), ("r3".to_string(), "gone".to_string())].into_iter().collect();
        fold_subagents(&mut conversations, parents, |id| {
            (id == "old").then(|| (record(now - 7200.0, vec![event(now - 7200.0, "gemini-4-carbon", 1, None)]), None))
        });
        let mut ids: Vec<&String> = conversations.keys().collect();
        ids.sort();
        assert_eq!(ids, ["old", "r3"]);
        assert_eq!((conversations["old"].events.len(), conversations["old"].changed), (2, now - 10.0));
    }

    #[test]
    fn a_subagents_subagent_is_part_of_the_top_task_once() {
        // grandchild → child → root, the child's id sorting first: every request counted once, in root, and the child
        // not listed again of its own.
        let now = 1_800_000_000.0;
        let one = |at: f64| record(at, vec![event(at, "gemini-4-carbon", 1, None)]);
        let mut conversations: HashMap<String, Record> =
            [("a-child", one(now - 300.0)), ("b-grandchild", one(now - 200.0)), ("root", one(now - 400.0))]
                .into_iter()
                .map(|(id, record)| (id.to_string(), record))
                .collect();
        let parents = [("a-child", "root"), ("b-grandchild", "a-child")]
            .into_iter()
            .map(|(child, parent)| (child.to_string(), parent.to_string()))
            .collect();
        fold_subagents(&mut conversations, parents, |_| None);
        assert_eq!(conversations.keys().collect::<Vec<_>>(), ["root"]);
        assert_eq!(conversations["root"].events.len(), 3);
        // Only the grandchild changed since: the child is read with its own parent, so it and its task are read and
        // all three are one session.
        let mut conversations: HashMap<String, Record> = [("b-grandchild".to_string(), one(now - 200.0))].into();
        let mut asked = Vec::new();
        fold_subagents(&mut conversations, [("b-grandchild".to_string(), "a-child".to_string())].into(), |id| {
            asked.push(id.to_string());
            match id {
                "a-child" => Some((one(now - 300.0), Some("root".to_string()))),
                "root" => Some((one(now - 400.0), None)),
                _ => None,
            }
        });
        assert_eq!(asked, ["a-child", "root"]);
        assert_eq!(conversations.keys().collect::<Vec<_>>(), ["root"]);
        assert_eq!((conversations["root"].events.len(), conversations["root"].changed), (3, now - 200.0));
        // A loop (x → y → x) is one session, under its least id; nothing is lost.
        let mut conversations: HashMap<String, Record> =
            [("x".to_string(), one(now - 50.0)), ("y".to_string(), one(now - 60.0))].into();
        fold_subagents(
            &mut conversations,
            [("x".to_string(), "y".to_string()), ("y".to_string(), "x".to_string())].into(),
            |_| None,
        );
        assert_eq!(conversations.keys().collect::<Vec<_>>(), ["x"]);
        assert_eq!(conversations["x"].events.len(), 2);
    }

    #[test]
    fn a_workspace_uri_names_the_project() {
        use crate::harness::workspace_project;
        let name = |uris: &str| workspace_project(uris);
        assert_eq!(name(r#"["file:///Users/me/My%20Paper"]"#).as_deref(), Some("My Paper"));
        assert_eq!(name(r#"["file:///a/%E4%B8%AD"]"#).as_deref(), Some("中"));
        // A space, a comma or a bracket in the folder's name: kept, the list read whole.
        assert_eq!(name(r#"["file:///Users/me/Q1 report, final [v2]"]"#).as_deref(), Some("Q1 report, final [v2]"));
        // A bare % before a multi-byte character, or at the end: as it is, no panic.
        assert_eq!(name(r#"["file:///Users/me/100%中文"]"#).as_deref(), Some("100%中文"));
        assert_eq!(name(r#"["file:///a/b%"]"#).as_deref(), Some("b%"));
        assert_eq!(name("not a list"), None);
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

    #[test]
    fn real_prompt_boundaries_survive_long_command_gaps_and_subagent_launches() {
        use super::{Record, fold_subagents, session_of};
        use jiff::tz::TimeZone;
        use std::collections::HashMap;
        let ev = |at: f64, model: &str, input: i64, cost: Option<f64>| {
            object! {
                "timestamp" => at, "model" => model, "input" => input, "output" => 10, "cached" => 0, "writes" => 0,
                "cost" => cost.map_or(Json::Null, Json::Double), "project" => "repo",
            }
        };
        let now = 1_800_000_000.0;
        let ranges = vec![("today".to_string(), now - 3600.0, now)];
        let zone = TimeZone::UTC;
        let price = |_: &crate::json::Object| -> Option<f64> { None };
        // One user prompt 20 minutes ago, then an 8-minute command gap between requests (would have been falsely
        // split by TURN_GAP=180s), plus a running active prompt.
        let mut rec = Record {
            key: "antigravity:conv1".into(),
            changed: now - 5.0,
            events: vec![
                ev(now - 1180.0, "gemini-4-neon", 1000, Some(0.1)),
                ev(now - 700.0, "gemini-4-neon", 2000, Some(0.2)),
                ev(now - 10.0, "gemini-4-neon", 3000, Some(0.3)),
            ],
            title: Some("Long task".into()),
            project: Some("repo".into()),
            running: Some(true),
            prompts: Some(vec![now - 1200.0]),
        };
        // Fold a subagent whose own prompt occurred mid-turn: parent keeps ONLY its own prompt timestamp.
        let mut conversations = HashMap::from([
            ("parent".to_string(), rec.clone()),
            (
                "sub".to_string(),
                Record {
                    key: "antigravity:sub".into(),
                    changed: now - 50.0,
                    events: vec![ev(now - 60.0, "gemini-4-neon", 500, Some(0.05))],
                    title: None,
                    project: None,
                    running: Some(false),
                    prompts: Some(vec![now - 65.0]),
                },
            ),
        ]);
        fold_subagents(&mut conversations, HashMap::from([("sub".into(), "parent".into())]), |_| None);
        rec = conversations.remove("parent").unwrap();
        let session = session_of("Gemini", "gemini", rec, now, &price, &ranges, &zone).expect("session");
        let turns = session.get("turns").and_then(Json::as_array).unwrap();
        assert_eq!(turns.len(), 1, "8-minute command gap and subagent launch must NOT reset the turn timer");
        assert_eq!(turns[0].get("at").and_then(Json::number), Some(now - 1200.0));
        assert_eq!(turns[0].get("requests").and_then(Json::integer), Some(4));
    }

    #[test]
    fn requests_before_the_first_prompt_known_keep_the_gap_split() {
        use super::{Record, session_of};
        use jiff::tz::TimeZone;
        let ev = |at: f64| {
            object! {
                "timestamp" => at, "model" => "hy4-flash", "input" => 100, "output" => 10, "cached" => 0, "writes" => 0,
                "cost" => 0.01, "project" => "repo",
            }
        };
        let now = 1_800_000_000.0;
        let ranges = vec![("today".to_string(), now - 7200.0, now)];
        let price = |_: &crate::json::Object| -> Option<f64> { None };
        // A long transcript whose prompts are read from its end only: two earlier turns (20 minutes apart) with no
        // prompt known, then one prompt known and its two requests 8 minutes apart.
        let record = Record {
            key: "workbuddy:t".into(),
            changed: now - 600.0,
            events: vec![ev(now - 3000.0), ev(now - 1800.0), ev(now - 1180.0), ev(now - 700.0)],
            title: None,
            project: None,
            running: Some(false),
            prompts: Some(vec![now - 1200.0]),
        };
        let session =
            session_of("WorkBuddy", "workbuddy", record, now, &price, &ranges, &TimeZone::UTC).expect("session");
        let turns = session.get("turns").and_then(Json::as_array).unwrap();
        let starts: Vec<f64> = turns.iter().filter_map(|turn| turn.get("at").and_then(Json::number)).collect();
        assert_eq!(turns.len(), 3, "{starts:?}");
        assert!(
            starts.contains(&(now - 1200.0)) && starts.contains(&(now - 3000.0)) && starts.contains(&(now - 1800.0))
        );
    }
}
