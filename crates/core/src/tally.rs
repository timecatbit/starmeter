//! Totals for a source's events, and the gateways (OpenCodex, Magpie) that may have logged the same requests: a
//! request a gateway logged is counted once, by the gateway. Plus the archive that keeps usage after its logs go.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use jiff::Timestamp;
use jiff::tz::TimeZone;

use crate::database::{self, Database, Mode, Value};
use crate::files::{self, paths};
use crate::json::{Json, Object};
use crate::lines::{ReadFailure, read_lines};
use crate::py::{self, LineError, ReadError};
use crate::{Fallible, harness, object};

pub type Event = Object;
pub type Price<'a> = &'a dyn Fn(&Event) -> Option<f64>;

/// A request's token split: input with cache reads, cache reads, cache writes, output.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Split {
    pub input: i64,
    pub cached: i64,
    pub writes: i64,
    pub output: i64,
}

/// The requests one gateway logged, by token split, each with the times it finished. Matching uses them up, so
/// every source gets a fresh copy from `requests_from`.
#[derive(Clone, Debug, Default)]
pub struct GatewayRequests {
    times: IndexMap<Split, Vec<f64>>,
}

impl GatewayRequests {
    fn is_empty(&self) -> bool {
        self.times.is_empty()
    }
}

/// A gateway that may have logged the same requests as a source: its name and its requests, or None when its log
/// can't be read here (nothing is matched against it then).
pub type Carrier = (String, Option<GatewayRequests>);
/// What a gateway's log holds around a range: per client, in the order they first appear, each split's finish times.
pub type GatewayLog = IndexMap<String, IndexMap<Split, Vec<f64>>>;

/// A local record and a gateway's record of one request usually finish within seconds of each other; Codex's
/// auto-review writes its record a few minutes late. The exact token split is what identifies the call.
pub const MATCH_WINDOW: f64 = 600.0;

/// One request a gateway logged: id, agent, finish time, input with cache reads, cache reads, cache writes, output.
#[allow(clippy::type_complexity)]
fn gateway_row(kind: &str, row: &Object, offset: i64) -> Result<Option<(String, String, f64, [f64; 4])>, ReadError> {
    if kind == "magpie" {
        let Some(event) = harness::magpie_event(row, offset)? else { return Ok(None) };
        let n = |key: &str| event.get(key).and_then(Json::number).unwrap_or(0.0);
        if n("input") + n("cached") + n("writes") + n("output") <= 0.0 {
            return Ok(None);
        }
        let stamp = event.get("timestamp");
        let when =
            stamp.and_then(Json::number).or_else(|| stamp.and_then(Json::integer).map(|n| n as f64)).unwrap_or(0.0);
        return Ok(Some((
            event.get("id").and_then(Json::as_str).unwrap_or("").to_string(),
            py::str(Some(&py::or(row.get("agent"), Json::from("")))),
            when + py::num(row.get("ms")) / 1000.0,
            [n("input") + n("cached"), n("cached"), n("writes"), n("output")],
        )));
    }
    let Some(usage) = row.get("usage").and_then(Json::as_object) else { return Ok(None) };
    let Some(time) = row.get("timestamp") else { return Err(LineError.into()) };
    Ok(Some((
        py::identity(vec![py::or(row.get("requestId"), Json::Int(offset))]),
        String::new(),
        (py::num(Some(time)) + py::num(row.get("durationMs"))) / 1000.0,
        [
            py::num(usage.get("inputTokens")),
            py::num(usage.get("cachedInputTokens")),
            py::num(usage.get("cacheCreationInputTokens")),
            py::num(usage.get("outputTokens")),
        ],
    )))
}

/// Runs body in a transaction, rolled back when it fails.
pub fn transaction<T>(db: &Database, body: impl FnOnce() -> Fallible<T>) -> Fallible<T> {
    db.execute("BEGIN")?;
    match body() {
        Ok(value) => {
            db.execute("COMMIT")?;
            Ok(value)
        }
        Err(error) => {
            let _ = db.execute("ROLLBACK");
            Err(error)
        }
    }
}

/// The requests a gateway (OpenCodex or Magpie) logged around start...end, by client; the client is "" when the
/// gateway doesn't name it. The log is appended to; when it is rewritten (new file, shorter, or another first line)
/// the index starts over. Only these counters are kept, never request content.
pub fn gateway_requests(path: &Path, cache: &Path, start: f64, end: f64, kind: &str) -> Fallible<GatewayLog> {
    let Some(state) = crate::claude_usage::state_of(path) else { return Err(crate::Error("unreadable".into())) };
    let first = first_line(path).ok_or_else(|| crate::Error("unreadable".into()))?;
    let head = py::identity(vec![Json::from(String::from_utf8_lossy(&first).into_owned())]);
    let resolved = paths::resolve(path).unwrap_or_else(|| path.to_path_buf());
    let identity = py::identity(vec![Json::from(resolved.to_string_lossy().into_owned())]);
    let database_path = cache.join(format!("harness-gateway-v2-{kind}-{}.sqlite3", &identity[..12]));
    files::make_directory(cache, 0o700)?;
    let db = Database::open(&database_path, 10.0, Mode::ReadWrite)?;
    files::owner_only(&database_path);
    db.execute(
        "CREATE TABLE IF NOT EXISTS log(id INTEGER PRIMARY KEY CHECK(id=1), inode INTEGER, head TEXT, offset INTEGER); CREATE TABLE IF NOT EXISTS requests(id TEXT PRIMARY KEY, agent TEXT, ts REAL, input INTEGER, cached INTEGER, writes INTEGER, output INTEGER); CREATE INDEX IF NOT EXISTS requests_ts ON requests(ts);",
    )?;
    transaction(&db, || {
        let known = db.first("SELECT inode,head,offset FROM log", &[])?;
        let mut offset: i64 = 0;
        if let Some(known) = &known
            && database::int(&known[0]) == Some(state.inode)
            && database::text(&known[1]) == Some(head.as_str())
            && let Some(saved) = database::int(&known[2]).filter(|saved| *saved <= state.size)
        {
            offset = saved;
        }
        if offset == 0 {
            db.run("DELETE FROM requests", &[])?;
        }
        let read = read_lines(path, offset, |_, line| {
            offset += line.len() as i64;
            let Ok(row) = Json::parse_selected(line, &crate::json::Selection::All, true) else { return Ok(()) };
            let Some(fields) = row.as_object() else { return Ok(()) };
            let Ok(Some((id, agent, when, split))) = gateway_row(kind, fields, offset) else { return Ok(()) };
            let mut values = vec![Value::Text(id), Value::Text(agent), Value::Real(when)];
            values.extend(split.iter().map(|count| Value::Integer(count.trunc() as i64)));
            db.run("INSERT OR REPLACE INTO requests VALUES (?,?,?,?,?,?,?)", &values)?;
            Ok(())
        });
        match read {
            Ok(_) | Err(ReadFailure::Unreadable | ReadFailure::File(_)) => {}
            Err(ReadFailure::Other(error)) => return Err(error),
        }
        db.run(
            "INSERT OR REPLACE INTO log VALUES (1,?,?,?)",
            &[Value::Integer(state.inode), Value::Text(head.clone()), Value::Integer(offset)],
        )?;
        Ok(())
    })?;
    let mut clients = GatewayLog::new();
    db.each(
        "SELECT agent,ts,input,cached,writes,output FROM requests WHERE ts>=? AND ts<=?",
        &[Value::Real(start - MATCH_WINDOW), Value::Real(end + MATCH_WINDOW)],
        |row| {
            let int = |index: usize| database::int(&row[index]).unwrap_or(0);
            let split = Split { input: int(2), cached: int(3), writes: int(4), output: int(5) };
            let agent = database::text(&row[0]).unwrap_or("").to_string();
            clients.entry(agent).or_default().entry(split).or_default().push(database::double(&row[1]).unwrap_or(0.0));
            Ok(())
        },
    )?;
    Ok(clients)
}

/// The first line of a file (up to 4096 bytes, with its newline), as the gateway index compares it.
fn first_line(path: &Path) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let mut first = Vec::new();
    let mut byte = [0u8; 1];
    while first.len() < 4096 && file.read(&mut byte).ok()? == 1 {
        first.push(byte[0]);
        if byte[0] == b'\n' {
            break;
        }
    }
    Some(first)
}

/// A fresh copy of the requests these clients sent (all of them when agents is None): matching uses them up.
pub fn requests_from(clients: &GatewayLog, agents: Option<&HashSet<String>>) -> GatewayRequests {
    let mut carried = GatewayRequests::default();
    for (agent, requests) in clients {
        if agents.is_none_or(|agents| agents.contains(agent)) {
            for (split, times) in requests {
                carried.times.entry(*split).or_default().extend(times);
            }
        }
    }
    carried
}

/// Whether a gateway logged this very request: the same token split, finished within the match window. Each logged
/// request accounts for one local record only.
fn carried_by(carried: &mut GatewayRequests, event: &Event) -> bool {
    let whole = |key: &str| py::num(event.get(key)).trunc() as i64;
    let (inputs, cached, writes, output) = (whole("input"), whole("cached"), whole("writes"), whole("output"));
    // Gateways report input with cache reads, and some with cache writes as well.
    let mut split = Split { input: inputs + cached, cached, writes, output };
    if carried.times.get(&split).is_none_or(Vec::is_empty) && writes != 0 {
        split = Split { input: inputs + cached + writes, cached, writes, output };
    }
    let Some(times) = carried.times.get_mut(&split).filter(|times| !times.is_empty()) else { return false };
    let when = event.get("timestamp").and_then(Json::number).unwrap_or(0.0);
    let mut nearest = 0;
    for index in 0..times.len() {
        if (times[index] - when).abs() < (times[nearest] - when).abs() {
            nearest = index;
        }
    }
    if (times[nearest] - when).abs() > MATCH_WINDOW {
        return false;
    }
    times.remove(nearest);
    true
}

/// The events no gateway carried, and those each gateway did.
pub fn split_carried(
    events: &[Event],
    carriers: &mut [Carrier],
    exclude_proxy: bool,
) -> (Vec<Event>, Vec<(String, Vec<Event>)>) {
    let mut counted = Vec::new();
    let mut carried: Vec<(String, Vec<Event>)> = Vec::new();
    for event in events {
        let provider = py::str(Some(event.get("provider").unwrap_or(&Json::from("")))).to_lowercase();
        let by = if exclude_proxy && (provider == "opencodex" || provider == "ocx") {
            Some("OpenCodex".to_string())
        } else {
            carriers.iter_mut().find_map(|(name, requests)| {
                requests
                    .as_mut()
                    .filter(|requests| !requests.is_empty())
                    .and_then(|requests| carried_by(requests, event).then(|| name.clone()))
            })
        };
        match by {
            Some(by) => match carried.iter_mut().find(|(name, _)| *name == by) {
                Some((_, list)) => list.push(event.clone()),
                None => carried.push((by, vec![event.clone()])),
            },
            None => counted.push(event.clone()),
        }
    }
    (counted, carried)
}

/// Summary's share of a source, plus each gateway's share, which Summary adds back when that gateway has nothing
/// to show (its card can't count those requests then).
fn summary_parts(counted: &[Event], carried: &[(String, Vec<Event>)], tally: impl Fn(&[Event]) -> Object) -> Object {
    let part = |events: &[Event]| -> Json {
        let result = tally(events);
        let kept: Object = ["summary", "models", "days", "hours", "firstActivityAt"]
            .into_iter()
            .map(|key| (key, result.get(key).cloned().unwrap_or(Json::Null)))
            .collect();
        kept.into()
    };
    let counts: Object = carried.iter().map(|(name, events)| (name.clone(), Json::Int(events.len() as i64))).collect();
    let parts: Object = carried.iter().map(|(name, events)| (name.clone(), part(events))).collect();
    object! { "counted" => part(counted), "carried" => counts, "carriedParts" => parts }
}

/// A source's own card covers all its records. When a gateway logged some of the same requests, "counted" holds
/// only the rest, which is what Summary adds for this source.
pub fn aggregate(
    all: &[Event],
    start: f64,
    end: f64,
    zone: &TimeZone,
    exclude_proxy: bool,
    price: Option<Price>,
    carriers: Option<&mut [Carrier]>,
) -> Object {
    let events: Vec<Event> = all
        .iter()
        .filter(|event| {
            let when = event.get("timestamp").and_then(Json::number).unwrap_or(f64::NEG_INFINITY);
            let used: f64 = ["input", "output", "cached", "writes"].iter().map(|key| py::num(event.get(key))).sum();
            start <= when && when <= end && (used > 0.0 || py::num(event.get("credits")) > 0.0)
        })
        .cloned()
        .collect();
    let mut result = tally(&events, zone, price);
    let mut empty: Vec<Carrier> = Vec::new();
    let carriers = carriers.unwrap_or(&mut empty);
    if exclude_proxy || !carriers.is_empty() {
        let (counted, carried) = split_carried(&events, carriers, exclude_proxy);
        for (key, value) in summary_parts(&counted, &carried, |events| tally(events, zone, price)) {
            result.insert(key, value);
        }
    }
    let matched =
        carriers.iter().rev().find(|(name, _)| name == "OpenCodex").is_some_and(|(_, requests)| requests.is_some());
    result.insert("proxyMatched", matched);
    result
}

fn empty_totals() -> Object {
    object! {
        "totalTokens" => 0, "inputTokens" => 0, "outputTokens" => 0, "cachedInputTokens" => 0,
        "requests" => 0, "estimatedCostUsd" => 0, "unpricedRequests" => 0, "credits" => 0,
    }
}

/// Totals for the summary, each model, project, day and hour, and Magpie's clients.
pub fn tally(events: &[Event], zone: &TimeZone, price: Option<Price>) -> Object {
    let mut summary = empty_totals();
    let mut models: IndexMap<String, Object> = IndexMap::new();
    let mut projects: IndexMap<String, Object> = IndexMap::new();
    let mut days: IndexMap<String, Object> = IndexMap::new();
    let mut hours: IndexMap<i64, Object> = IndexMap::new();
    let mut clients: IndexMap<String, Object> = IndexMap::new();
    let mut sessions: HashSet<String> = HashSet::new();
    let mut first = Json::Null;
    for event in events {
        let ts = event.get("timestamp").cloned().unwrap_or(Json::Null);
        let when = ts.number().or_else(|| ts.integer().map(|n| n as f64)).unwrap_or(0.0);
        let i = py::number(event.get("input"));
        let o = py::number(event.get("output"));
        let r = py::number(event.get("cached"));
        let w = py::number(event.get("writes"));
        let credits = py::number(event.get("credits"));
        let mut cost = event.get("cost").filter(|cost| !cost.is_null()).cloned();
        if cost.is_none()
            && let Some(price) = price
        {
            cost = price(event).map(Json::Double);
        }
        let total = add(&add(&add(&i, &o), &r), &w);
        // A gateway names the client that sent each request (Magpie's agent).
        if let Some(agent) = event.get("agent") {
            let client =
                clients.entry(py::str(Some(agent))).or_insert_with(|| object! { "totalTokens" => 0, "requests" => 0 });
            bump(client, "totalTokens", &total);
            bump(client, "requests", &Json::Int(1));
        }
        first = if first.is_null() || ts.number().unwrap_or(0.0) < first.number().unwrap_or(0.0) {
            ts.clone()
        } else {
            first
        };
        let day = local_day(when, zone);
        if let Some(session) = event.get("session").filter(|session| !session.is_null()) {
            sessions.insert(py::dumps(session));
        }
        let hour = (when / 3600.0).floor() as i64 * 3600;
        let count = |target: &mut Object| {
            bump(target, "totalTokens", &total);
            bump(target, "inputTokens", &add(&add(&i, &r), &w));
            bump(target, "outputTokens", &o);
            bump(target, "cachedInputTokens", &r);
            bump(target, "requests", &Json::Int(1));
            bump(target, "credits", &credits);
            if let Some(known) = event.get("cacheKnown") {
                let eligible = if known.is_truthy() { add(&add(&i, &r), &w) } else { Json::Int(0) };
                let current = target.get("cacheEligibleInputTokens").cloned().unwrap_or(Json::Int(0));
                target.insert("cacheEligibleInputTokens", add(&current, &eligible));
            }
            match &cost {
                Some(cost) => bump(target, "estimatedCostUsd", cost),
                None => bump(target, "unpricedRequests", &Json::Int(1)),
            }
        };
        count(&mut summary);
        count(models.entry(py::str(event.get("model"))).or_insert_with(empty_totals));
        let project =
            event.get("project").filter(|project| project.is_truthy()).map(|p| py::str(Some(p))).unwrap_or_default();
        count(projects.entry(project).or_insert_with(empty_totals));
        count(days.entry(day).or_insert_with(empty_totals));
        count(hours.entry(hour).or_insert_with(empty_totals));
    }
    summary.insert("sessions", sessions.len() as i64);
    let requests = summary.get("requests").and_then(Json::number).unwrap_or(0.0);
    let unpriced = summary.get("unpricedRequests").and_then(Json::number).unwrap_or(0.0);
    summary.insert("coverageRatio", (requests - unpriced) / requests.max(1.0));
    let named = |rows: IndexMap<String, Object>, field: &str| -> Json {
        Json::Array(
            rows.into_iter()
                .map(|(key, mut row)| {
                    row.insert(field, key);
                    row.into()
                })
                .collect(),
        )
    };
    days.sort_keys();
    hours.sort_keys();
    let mut result = object! {
        "summary" => summary,
        "models" => named(models, "model"),
        "projects" => named(projects, "project"),
        "days" => named(days, "date"),
        "hours" => Json::Array(hours.into_iter().map(|(hour, mut row)| { row.insert("timestamp", hour); row.into() }).collect()),
        "firstActivityAt" => first,
    };
    if !clients.is_empty() {
        result.insert("clients", named(clients, "agent"));
    }
    result
}

fn bump(target: &mut Object, key: &str, amount: &Json) {
    let current = target.get(key).cloned().unwrap_or(Json::Int(0));
    target.insert(key, add(&current, amount));
}

/// a + b as Python adds them: ints stay ints, anything with a float is a float.
pub fn add(a: &Json, b: &Json) -> Json {
    match (a, b) {
        (Json::Int(x), Json::Int(y)) => Json::Int(x + y),
        _ => Json::Double(a.number().unwrap_or(0.0) + b.number().unwrap_or(0.0)),
    }
}

/// The offset from UTC in a zone at a moment, in seconds.
pub fn offset_at(zone: &TimeZone, seconds: f64) -> i64 {
    Timestamp::from_second(seconds.floor() as i64)
        .map(|moment| i64::from(zone.to_offset(moment).seconds()))
        .unwrap_or(0)
}

/// The local calendar day of a moment, as `datetime.fromtimestamp(ts, zone).date().isoformat()`.
pub fn local_day(seconds: f64, zone: &TimeZone) -> String {
    let offset = offset_at(zone, seconds) as f64;
    let (year, month, day) = py::civil_from_days(((seconds + offset) / 86400.0).floor() as i64);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Usage that outlives its logs: when a log file disappears, the counters the index held for it move to a durable
/// archive, and the readers add them back, the index winning for any request it still has. Counters, model names,
/// times and hashed IDs only, never content.
pub mod archive {
    use super::*;

    pub fn location(state: &Path, source: &str, root: &str) -> PathBuf {
        state.join("archive").join(format!("{source}-v1-{root}.sqlite3"))
    }

    /// Turning the setting off forgets everything kept so far.
    pub fn clear(state: &Path) {
        let _ = std::fs::remove_dir_all(state.join("archive"));
    }

    fn connect(path: &Path) -> Fallible<Database> {
        if let Some(parent) = path.parent() {
            files::make_directory(parent, 0o700)?;
        }
        let db = Database::open(path, 10.0, Mode::ReadWrite)?;
        files::owner_only(path);
        db.execute("CREATE TABLE IF NOT EXISTS events(id TEXT PRIMARY KEY, ts REAL, data TEXT)")?;
        db.execute("CREATE INDEX IF NOT EXISTS events_ts ON events(ts)")?;
        Ok(db)
    }

    /// Archives (id, timestamp, event JSON) rows; the first copy of an event stays.
    pub fn keep(path: &Path, rows: Vec<Vec<Value>>) -> Fallible<()> {
        if rows.is_empty() {
            return Ok(());
        }
        let db = connect(path)?;
        transaction(&db, || {
            for row in &rows {
                db.run("INSERT OR IGNORE INTO events VALUES (?,?,?)", row)?;
            }
            Ok(())
        })
    }

    pub fn events(path: Option<&Path>, start: f64, end: f64) -> Vec<Event> {
        let Some(path) = path.filter(|path| path.exists()) else { return vec![] };
        let Ok(db) = connect(path) else { return vec![] };
        let Ok(rows) =
            db.rows("SELECT data FROM events WHERE ts>=? AND ts<=?", &[Value::Real(start), Value::Real(end)])
        else {
            return vec![];
        };
        rows.iter()
            .filter_map(|row| database::text(&row[0]).and_then(|text| Json::parse_str(text).ok()))
            .filter_map(|value| value.as_object().cloned())
            .collect()
    }

    /// The index's events plus the archived ones it no longer has.
    pub fn merged(live: Vec<Event>, path: Option<&Path>, start: f64, end: f64) -> Vec<Event> {
        let seen: HashSet<String> =
            live.iter().map(|event| py::dumps(event.get("id").unwrap_or(&Json::Null))).collect();
        let mut all = live;
        all.extend(
            events(path, start, end)
                .into_iter()
                .filter(|event| !seen.contains(&py::dumps(event.get("id").unwrap_or(&Json::Null)))),
        );
        all
    }
}
