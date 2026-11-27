//! Incremental, usage-only reader of Claude transcripts. It never reads credentials and never keeps message text,
//! paths or raw response IDs; of a project it keeps only the folder name. The index file it writes is the one 5.x
//! writes, row for row, so either version can pick up where the other left off.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use jiff::tz::TimeZone;
use sha2::{Digest, Sha256};

use crate::Fallible;
use crate::database::{self, Database, Mode, Value};
use crate::files::{self, paths};
use crate::json::{Format, Json, Selection};
use crate::lines::{ReadFailure, read_lines, without_byte_order_mark};
use crate::time::IsoTime;

pub type Digest16 = [u8; 16];

/// 128-bit hashed identifiers keep the index compact; raw IDs and paths are never stored.
pub fn digest(values: Vec<Json>) -> Digest16 {
    let text = Json::Array(values).serialized_with(Format { item: ",", key: ":", ..Format::default() });
    let hash = Sha256::digest(text.as_bytes());
    let mut digest = [0; 16];
    digest.copy_from_slice(&hash[..16]);
    digest
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// One counted response, as indexed.
#[derive(Clone, Debug)]
pub struct Record {
    pub key: Vec<u8>,
    pub message: Option<Vec<u8>>,
    pub request: Option<Vec<u8>>,
    pub session: Vec<u8>,
    pub ts: f64,
    pub model: String,
    /// input, output, 5-minute cache writes, 1-hour cache writes, cache reads.
    pub metrics: [i64; 5],
    pub cost: Option<f64>,
    pub sidechain: bool,
    pub speed: Option<String>,
    pub geo: String,
    pub tier: String,
    pub slot: i64,
    pub project: Option<String>,
    file: Vec<u8>,
    offset: i64,
}

/// A response once deduplicated: what range figures are made of.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub ts: f64,
    pub model: String,
    pub metrics: [i64; 5],
    pub cost: Option<f64>,
    pub speed: Option<String>,
    pub geo: Option<String>,
    pub tier: Option<String>,
    pub session: Option<Vec<u8>>,
    pub project: Option<String>,
}

/// A line, or one response in it, that is skipped.
#[derive(Debug)]
pub struct Skip;

fn counter(value: Option<&Json>) -> Result<i64, Skip> {
    match value {
        None | Some(Json::Null) => Ok(0),
        Some(Json::Int(number)) if *number >= 0 => Ok(*number),
        _ => Err(Skip),
    }
}

pub fn normalize(usage: Option<&Json>) -> Result<[i64; 5], Skip> {
    let usage = usage.and_then(Json::as_object).ok_or(Skip)?;
    let (w5, w1) = match usage.get("cache_creation").and_then(Json::as_object) {
        Some(breakdown) => {
            (counter(breakdown.get("ephemeral_5m_input_tokens"))?, counter(breakdown.get("ephemeral_1h_input_tokens"))?)
        }
        None => (counter(usage.get("cache_creation_input_tokens"))?, 0),
    };
    Ok([
        counter(usage.get("input_tokens"))?,
        counter(usage.get("output_tokens"))?,
        w5,
        w1,
        counter(usage.get("cache_read_input_tokens"))?,
    ])
}

/// The project of the folder a request ran in (see projects), by its name only.
pub fn project_name(value: Option<&Json>) -> Option<String> {
    crate::projects::name_of(value.and_then(Json::as_str)?)
}

/// The responses one transcript line holds: the reply itself and any advisor calls inside it. Fails for a line
/// that must be skipped whole.
pub fn records(row: &Json, session_fallback: &str, location: (&str, i64)) -> Result<Vec<Record>, Skip> {
    let Some(row) = row.as_object().filter(|row| row.get("type") == Some(&Json::from("assistant"))) else {
        return Ok(vec![]);
    };
    let Some(message) = row.get("message").and_then(Json::as_object) else { return Ok(vec![]) };
    let Some(usage) = message.get("usage").filter(|usage| usage.as_object().is_some()) else { return Ok(vec![]) };
    let Some(model) = message
        .get("model")
        .and_then(Json::as_str)
        .filter(|m| !m.is_empty() && *m != "<synthetic>" && *m != "synthetic")
    else {
        return Ok(vec![]);
    };
    let session_value = match row.get("sessionId") {
        Some(given) if given.is_truthy() => given.clone(),
        _ => Json::from(session_fallback),
    };
    let Some(session) = session_value.as_str() else { return Ok(vec![]) };
    let text = match row.get("timestamp") {
        Some(Json::String(text)) => text.as_str(),
        None => "",
        Some(_) => return Err(Skip),
    };
    let time = IsoTime::parse(text).map_err(|_| Skip)?;
    if time.offset.is_none() {
        return Err(Skip);
    }
    let stamp = time.timestamp(&TimeZone::UTC);
    let request = row.get("requestId");
    let message_id = message.get("id");
    let blank =
        |value: Option<&Json>| value.is_some_and(|value| !value.is_null() && value.as_str().unwrap_or("").is_empty());
    if blank(request) || blank(message_id) {
        return Ok(vec![]);
    }
    let request_id = request.and_then(Json::as_str);
    let identifier_base = message_id.and_then(Json::as_str);

    let mut parts: Vec<(&str, &Json, Option<&Json>)> = vec![(model, usage, row.get("costUSD"))];
    if let Some(iterations) = usage.get("iterations").and_then(Json::as_array) {
        for item in iterations {
            if let Some(advisor) = item.as_object()
                && advisor.get("type") == Some(&Json::from("advisor_message"))
                && let Some(name) = advisor.get("model").and_then(Json::as_str).filter(|name| !name.is_empty())
            {
                parts.push((name, item, None));
            }
        }
    }
    let mut output = Vec::new();
    for (slot, (part_model, counts, cost)) in parts.into_iter().enumerate() {
        let Ok(metrics) = normalize(Some(counts)) else { continue };
        let stored = cost
            .filter(|cost| cost.as_bool().is_none())
            .and_then(Json::number)
            .filter(|number| number.is_finite() && *number >= 0.0);
        if metrics.iter().sum::<i64>() == 0 && stored.is_none() {
            continue;
        }
        let identifier =
            identifier_base.map(|base| if slot > 0 { format!("{base}:advisor:{slot}") } else { base.to_string() });
        let key = match (request_id, &identifier, identifier_base) {
            (Some(request_id), Some(identifier), _) => digest(vec![identifier.as_str().into(), request_id.into()]),
            (None, Some(identifier), Some(base)) => {
                // Without a request ID, gateway placeholders are scoped to session + time; msg_/resp_ IDs
                // represent one streamed response across content blocks.
                let when = if base.starts_with("msg_") || base.starts_with("resp_") {
                    Json::Null
                } else {
                    Json::Double(stamp)
                };
                digest(vec![identifier.as_str().into(), session.into(), when])
            }
            _ => digest(vec![
                "location".into(),
                Json::Array(vec![location.0.into(), Json::Int(location.1)]),
                Json::Int(slot as i64),
            ]),
        };
        let fields = counts.as_object().expect("normalize checked it");
        output.push(Record {
            key: key.to_vec(),
            message: identifier.as_ref().map(|id| digest(vec![id.as_str().into()]).to_vec()),
            request: request_id.map(|id| digest(vec![id.into()]).to_vec()),
            session: digest(vec![session.into()]).to_vec(),
            ts: stamp,
            model: part_model.to_string(),
            metrics,
            cost: stored,
            sidechain: row.get("isSidechain").and_then(Json::as_bool) == Some(true),
            speed: fields.get("speed").and_then(Json::as_str).map(String::from),
            geo: fields.get("inference_geo").and_then(Json::as_str).unwrap_or("global").to_string(),
            tier: fields.get("service_tier").and_then(Json::as_str).unwrap_or("standard").to_string(),
            slot: slot as i64,
            project: project_name(row.get("cwd")),
            file: Vec::new(),
            offset: 0,
        });
    }
    Ok(output)
}

/// Keeps complete responses; parent entries win over sidechain replays.
pub fn deduplicate(events: impl IntoIterator<Item = Record>) -> Vec<Record> {
    let mut output: Vec<Record> = Vec::new();
    let mut exact: HashMap<Vec<u8>, usize> = HashMap::new();
    let mut routes: HashMap<Vec<u8>, Vec<usize>> = HashMap::new();
    let route_key = |row: &Record| {
        let mut key = row.message.clone().unwrap_or_default();
        key.push(0xFF);
        key.extend_from_slice(&row.session);
        key
    };
    // Python compares (not sidechain, tokens, has speed, has cost, -ts) as a tuple.
    let rank = |row: &Record| {
        [
            if row.sidechain { 0.0 } else { 1.0 },
            row.metrics.iter().sum::<i64>() as f64,
            if row.speed.is_some() { 1.0 } else { 0.0 },
            if row.cost.is_some() { 1.0 } else { 0.0 },
            -row.ts,
        ]
    };
    let outranks = |a: &Record, b: &Record| {
        let (left, right) = (rank(a), rank(b));
        left.iter().zip(right.iter()).find(|(x, y)| x != y).is_some_and(|(x, y)| x > y)
    };
    for row in events {
        let mut found = exact.get(&row.key).copied();
        if found.is_none() && row.message.is_some() {
            for &index in routes.get(&route_key(&row)).into_iter().flatten() {
                let other = &output[index];
                if (row.sidechain || other.sidechain)
                    && (row.ts == other.ts || (row.request.is_none() && other.request.is_none()))
                {
                    found = Some(index);
                    break;
                }
            }
        }
        let index = match found {
            Some(index) => {
                if outranks(&row, &output[index]) {
                    output[index] = row.clone();
                }
                index
            }
            None => {
                output.push(row.clone());
                output.len() - 1
            }
        };
        exact.insert(row.key.clone(), index);
        if row.message.is_some() {
            let indexes = routes.entry(route_key(&row)).or_default();
            if !indexes.contains(&index) {
                indexes.push(index);
            }
        }
    }
    output
}

const COLUMNS: &str = "key,message,request,session,ts,model,i,o,w5,w1,r,cost,sidechain,speed,geo,tier,project";

fn blob(value: &Value) -> Option<Vec<u8>> {
    database::blob(value).map(<[u8]>::to_vec)
}

fn text(value: &Value) -> Option<String> {
    database::text(value).map(String::from)
}

fn stored_record(row: &[Value]) -> Record {
    Record {
        key: blob(&row[0]).unwrap_or_default(),
        message: blob(&row[1]),
        request: blob(&row[2]),
        session: blob(&row[3]).unwrap_or_default(),
        ts: database::double(&row[4]).unwrap_or(0.0),
        model: text(&row[5]).unwrap_or_default(),
        metrics: std::array::from_fn(|index| database::int(&row[6 + index]).unwrap_or(0)),
        cost: database::double(&row[11]),
        sidechain: database::int(&row[12]).unwrap_or(0) != 0,
        speed: text(&row[13]),
        geo: text(&row[14]).unwrap_or_default(),
        tier: text(&row[15]).unwrap_or_default(),
        slot: 0,
        project: text(&row[16]),
        file: Vec::new(),
        offset: 0,
    }
}

/// Appended responses sharing no ID or route with indexed ones form their own dedupe groups.
fn isolated(db: &Database, added: &[Record], allowed: Option<&HashSet<Vec<u8>>>) -> Fallible<bool> {
    let fresh: HashSet<(Vec<u8>, i64, i64)> =
        added.iter().map(|row| (row.file.clone(), row.offset, row.slot)).collect();
    let clash = |sql: &str, arguments: &[Value]| -> Fallible<bool> {
        for row in db.rows(sql, arguments)? {
            let slot = (
                blob(&row[0]).unwrap_or_default(),
                database::int(&row[1]).unwrap_or(0),
                database::int(&row[2]).unwrap_or(0),
            );
            if !fresh.contains(&slot)
                && allowed.is_none_or(|allowed| allowed.contains(&blob(&row[3]).unwrap_or_default()))
            {
                return Ok(true);
            }
        }
        Ok(false)
    };
    for row in added {
        if clash("SELECT file,offset,slot,session FROM everything WHERE key=?", &[Value::Blob(row.key.clone())])? {
            return Ok(false);
        }
        if let Some(message) = &row.message
            && clash(
                "SELECT file,offset,slot,session FROM everything WHERE message=? AND session=?",
                &[Value::Blob(message.clone()), Value::Blob(row.session.clone())],
            )?
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn record_outranks(a: &Record, b: &Record) -> bool {
    let rank = |row: &Record| {
        [
            if row.sidechain { 0.0 } else { 1.0 },
            row.metrics.iter().sum::<i64>() as f64,
            if row.speed.is_some() { 1.0 } else { 0.0 },
            if row.cost.is_some() { 1.0 } else { 0.0 },
            -row.ts,
        ]
    };
    let (left, right) = (rank(a), rank(b));
    left.iter().zip(right.iter()).find(|(x, y)| x != y).is_some_and(|(x, y)| x > y)
}

fn best_of(rows: &[Record]) -> &Record {
    let mut winner = &rows[0];
    for row in &rows[1..] {
        if record_outranks(row, winner) {
            winner = row;
        }
    }
    winner
}

/// Incremental in-place update for `canonical` when appended rows only touch existing `key`s without cross-key
/// `(message, session)` route clashes (the 99.9% Claude streaming-chunk hot path). Returns `Ok(true)` if applied
/// cleanly (preserving exact `canonical.id` values), or `Ok(false)` to fall back to a full rebuild (PERF-5).
fn patch_canonical(db: &Database, added: &[Record], allowed: Option<&HashSet<Vec<u8>>>) -> Fallible<bool> {
    let fresh: HashSet<(Vec<u8>, i64, i64)> =
        added.iter().map(|row| (row.file.clone(), row.offset, row.slot)).collect();
    let mut checked_routes: HashSet<(Vec<u8>, Vec<u8>)> = HashSet::new();
    let mut route_clashes = |message: &Vec<u8>, session: &Vec<u8>, expected_key: &Vec<u8>| -> Fallible<bool> {
        if !checked_routes.insert((message.clone(), session.clone())) {
            return Ok(false);
        }
        for hit in db.rows(
            "SELECT session,key FROM everything WHERE message=? AND session=?",
            &[Value::Blob(message.clone()), Value::Blob(session.clone())],
        )? {
            let hit_session = blob(&hit[0]).unwrap_or_default();
            let hit_key = blob(&hit[1]).unwrap_or_default();
            if allowed.is_none_or(|a| a.contains(&hit_session)) && &hit_key != expected_key {
                return Ok(true);
            }
        }
        Ok(false)
    };
    // Bail out to full rebuild if any appended row shares a (message, session) route with another key (pre-existing
    // or in the same appended batch, such as a parent + subagent copy).
    for row in added {
        if let Some(message) = &row.message
            && route_clashes(message, &row.session, &row.key)?
        {
            return Ok(false);
        }
    }
    let mut keys: Vec<Vec<u8>> = Vec::new();
    let mut seen_keys: HashSet<Vec<u8>> = HashSet::new();
    for row in added {
        if seen_keys.insert(row.key.clone()) {
            keys.push(row.key.clone());
        }
    }
    let same_canonical = |a: &Record, b: &Record| {
        a.ts == b.ts
            && a.model == b.model
            && a.metrics == b.metrics
            && a.cost == b.cost
            && a.speed == b.speed
            && a.geo == b.geo
            && a.tier == b.tier
            && a.session == b.session
            && a.project == b.project
    };
    let mut updates: Vec<(i64, Record)> = Vec::new();
    let mut updated_ids: HashSet<i64> = HashSet::new();
    let mut fresh_only: Vec<Record> = Vec::new();
    for key in &keys {
        let mut old_rows: Vec<Record> = Vec::new();
        let mut all_rows: Vec<Record> = Vec::new();
        db.each(
            &format!("SELECT file,offset,slot,{COLUMNS} FROM everything WHERE key=? ORDER BY file,offset,slot"),
            &[Value::Blob(key.clone())],
            |row| {
                let session = blob(&row[6]).unwrap_or_default();
                if allowed.is_some_and(|a| !a.contains(&session)) {
                    return Ok(());
                }
                let slot = (
                    blob(&row[0]).unwrap_or_default(),
                    database::int(&row[1]).unwrap_or(0),
                    database::int(&row[2]).unwrap_or(0),
                );
                let rec = stored_record(&row[3..]);
                if !fresh.contains(&slot) {
                    old_rows.push(rec.clone());
                }
                all_rows.push(rec);
                Ok(())
            },
        )?;
        for rec in &all_rows {
            if let Some(message) = &rec.message
                && route_clashes(message, &rec.session, key)?
            {
                return Ok(false);
            }
        }
        if old_rows.is_empty() {
            fresh_only.extend(all_rows);
            continue;
        }
        let (old_w, new_w) = (best_of(&old_rows), best_of(&all_rows));
        if same_canonical(old_w, new_w) {
            continue;
        }
        // Locate the unique canonical row for `old_w` using the `canonical_ts` index.
        let candidates = db.rows(
            "SELECT id,cost,speed,project FROM canonical WHERE ts=? AND model=? AND i=? AND o=? AND w5=? AND w1=? AND r=? AND geo=? AND tier=? AND session=?",
            &[
                Value::Real(old_w.ts),
                Value::Text(old_w.model.clone()),
                Value::Integer(old_w.metrics[0]),
                Value::Integer(old_w.metrics[1]),
                Value::Integer(old_w.metrics[2]),
                Value::Integer(old_w.metrics[3]),
                Value::Integer(old_w.metrics[4]),
                Value::Text(old_w.geo.clone()),
                Value::Text(old_w.tier.clone()),
                Value::Blob(old_w.session.clone()),
            ],
        )?;
        let matches: Vec<i64> = candidates
            .iter()
            .filter(|r| {
                database::double(&r[1]) == old_w.cost && text(&r[2]) == old_w.speed && text(&r[3]) == old_w.project
            })
            .filter_map(|r| database::int(&r[0]))
            .collect();
        if matches.len() != 1 || !updated_ids.insert(matches[0]) {
            return Ok(false);
        }
        updates.push((matches[0], new_w.clone()));
    }
    for (id, row) in updates {
        let mut values = vec![Value::Real(row.ts), Value::Text(row.model.clone())];
        values.extend(row.metrics.iter().map(|&count| Value::Integer(count)));
        values.extend([
            optional_real(row.cost),
            optional_text(&row.speed),
            Value::Text(row.geo.clone()),
            Value::Text(row.tier.clone()),
            Value::Blob(row.session.clone()),
            optional_text(&row.project),
            Value::Integer(id),
        ]);
        db.run(
            "UPDATE canonical SET ts=?,model=?,i=?,o=?,w5=?,w1=?,r=?,cost=?,speed=?,geo=?,tier=?,session=?,project=? WHERE id=?",
            &values,
        )?;
    }
    if !fresh_only.is_empty() {
        let next = db
            .first("SELECT COALESCE(MAX(id)+1,0) FROM canonical", &[])?
            .and_then(|row| database::int(&row[0]))
            .unwrap_or(0);
        store_canonical(db, &deduplicate(fresh_only), next)?;
    }
    Ok(true)
}

fn optional_text(value: &Option<String>) -> Value {
    value.clone().map_or(Value::Null, Value::Text)
}

fn optional_blob(value: &Option<Vec<u8>>) -> Value {
    value.clone().map_or(Value::Null, Value::Blob)
}

fn optional_real(value: Option<f64>) -> Value {
    value.map_or(Value::Null, Value::Real)
}

fn store_canonical(db: &Database, rows: &[Record], first: i64) -> Fallible<()> {
    for (index, row) in rows.iter().enumerate() {
        let mut values =
            vec![Value::Integer(first + index as i64), Value::Real(row.ts), Value::Text(row.model.clone())];
        values.extend(row.metrics.iter().map(|&count| Value::Integer(count)));
        values.extend([
            optional_real(row.cost),
            optional_text(&row.speed),
            Value::Text(row.geo.clone()),
            Value::Text(row.tier.clone()),
            Value::Blob(row.session.clone()),
            optional_text(&row.project),
        ]);
        db.run("INSERT INTO canonical VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?)", &values)?;
    }
    Ok(())
}

/// The file facts the index compares: inode, size and modification time in nanoseconds, as os.stat gives them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileState {
    pub inode: i64,
    pub size: i64,
    pub mtime: i64,
}

pub fn state_of(path: &Path) -> Option<FileState> {
    let metadata = std::fs::metadata(path).ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some(FileState {
            inode: metadata.ino() as i64,
            size: metadata.size() as i64,
            mtime: metadata.mtime() * 1_000_000_000 + metadata.mtime_nsec(),
        })
    }
    #[cfg(not(unix))]
    {
        let mtime = metadata.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_nanos() as i64;
        // Windows has no inode here: when the file was made stands in, so a file put in another's place (larger, at
        // the same path) is read again from the start rather than on from the old one's offset.
        #[cfg(windows)]
        let inode = {
            use std::os::windows::fs::MetadataExt;
            metadata.creation_time() as i64
        };
        #[cfg(not(windows))]
        let inode = 0;
        Some(FileState { inode, size: metadata.len() as i64, mtime })
    }
}

/// The only parts of a transcript line the reader looks at; message text and the rest are checked, not kept.
fn wanted() -> Selection {
    Selection::Keys(vec![
        ("type", Selection::All),
        ("sessionId", Selection::All),
        ("timestamp", Selection::All),
        ("requestId", Selection::All),
        ("costUSD", Selection::All),
        ("isSidechain", Selection::All),
        ("cwd", Selection::All),
        (
            "message",
            Selection::Keys(vec![("model", Selection::All), ("id", Selection::All), ("usage", Selection::All)]),
        ),
    ])
}

/// Reindexes only changed files and keeps only normalized counters and hashed IDs. `archive` is a durable file
/// that keeps the records of transcripts that disappear (Claude Code cleans up after 30 days); they are
/// deduplicated together with the live ones, so a response that survives in another file still counts once.
/// Returns the deduplicated responses between start and end, and how many files could not be read.
pub fn read_events(
    files: &[String],
    database_path: &Path,
    allowed_sessions: Option<&HashSet<String>>,
    start: f64,
    end: f64,
    archive: Option<&Path>,
) -> Fallible<(Vec<Event>, usize)> {
    let archive = archive.filter(|path| !path.as_os_str().is_empty());
    if let Some(parent) = database_path.parent() {
        files::make_directory(parent, 0o700)?;
    }
    let db = Database::open(database_path, 10.0, Mode::ReadWrite)?;
    files::owner_only(database_path);
    let result = index(&db, files, allowed_sessions, start, end, archive);
    if result.is_err() {
        let _ = db.execute("ROLLBACK");
    }
    result
}

fn index(
    db: &Database,
    files: &[String],
    allowed_sessions: Option<&HashSet<String>>,
    start: f64,
    end: f64,
    archive: Option<&Path>,
) -> Fallible<(Vec<Event>, usize)> {
    let mut failures = 0;
    let mut rebuild = false;
    let mut appended: Vec<Record> = Vec::new();
    db.execute(
        "CREATE TABLE IF NOT EXISTS files(path BLOB PRIMARY KEY,inode INTEGER,size INTEGER,mtime INTEGER,offset INTEGER);
        CREATE TABLE IF NOT EXISTS records(file BLOB,offset INTEGER,slot INTEGER,key BLOB,message BLOB,request BLOB,session BLOB,ts REAL,model TEXT,
          i INTEGER,o INTEGER,w5 INTEGER,w1 INTEGER,r INTEGER,cost REAL,sidechain INTEGER,speed TEXT,geo TEXT,tier TEXT,project TEXT,PRIMARY KEY(file,offset,slot));
        CREATE INDEX IF NOT EXISTS records_key ON records(key);
        CREATE INDEX IF NOT EXISTS records_route ON records(message,session);
        CREATE TABLE IF NOT EXISTS metadata(key TEXT PRIMARY KEY,value TEXT);
        CREATE TABLE IF NOT EXISTS canonical(id INTEGER PRIMARY KEY,ts REAL,model TEXT,i INTEGER,o INTEGER,w5 INTEGER,w1 INTEGER,r INTEGER,cost REAL,speed TEXT,geo TEXT,tier TEXT,session BLOB,project TEXT);
        CREATE INDEX IF NOT EXISTS canonical_ts ON canonical(ts);",
    )?;
    if let Some(archive) = archive {
        if let Some(parent) = archive.parent() {
            files::make_directory(parent, 0o700)?;
        }
        db.run("ATTACH DATABASE ? AS kept", &[Value::Text(archive.to_string_lossy().into_owned())])?;
        db.execute(
            "CREATE TABLE IF NOT EXISTS kept.records(file BLOB,offset INTEGER,slot INTEGER,key BLOB,message BLOB,request BLOB,session BLOB,ts REAL,model TEXT,
              i INTEGER,o INTEGER,w5 INTEGER,w1 INTEGER,r INTEGER,cost REAL,sidechain INTEGER,speed TEXT,geo TEXT,tier TEXT,project TEXT,PRIMARY KEY(file,offset,slot))",
        )?;
        // Archives written before records knew their project gain the column, empty for what they hold.
        let columns = db.rows("PRAGMA kept.table_info(records)", &[])?;
        if !columns.iter().any(|column| database::text(&column[1]) == Some("project")) {
            db.execute("ALTER TABLE kept.records ADD COLUMN project TEXT")?;
        }
        files::owner_only(archive);
        // A transcript that comes back reads as itself again.
        db.execute(
            "CREATE TEMP VIEW everything AS SELECT * FROM main.records UNION ALL SELECT * FROM kept.records WHERE file NOT IN (SELECT path FROM main.files)",
        )?;
    } else {
        db.execute("CREATE TEMP VIEW everything AS SELECT * FROM main.records")?;
    }
    db.execute("BEGIN")?;
    // In pathlib's order: by path components (Python orders str by code point, which is UTF-8 byte order).
    let mut paths: Vec<String> = files
        .iter()
        .map(|file| {
            paths::resolve(Path::new(file)).map_or_else(|| file.clone(), |path| path.to_string_lossy().into_owned())
        })
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    paths.sort_by(|a, b| a.split('/').cmp(b.split('/')));
    let active: HashSet<Vec<u8>> = paths.iter().map(|path| digest(vec![path.as_str().into()]).to_vec()).collect();
    for row in db.rows("SELECT path FROM files", &[])? {
        let Some(key) = blob(&row[0]).filter(|key| !active.contains(key)) else { continue };
        rebuild = true;
        if archive.is_some() {
            db.run(
                "INSERT OR IGNORE INTO kept.records SELECT * FROM main.records WHERE file=?",
                &[Value::Blob(key.clone())],
            )?;
        }
        db.run("DELETE FROM records WHERE file=?", &[Value::Blob(key.clone())])?;
        db.run("DELETE FROM files WHERE path=?", &[Value::Blob(key)])?;
    }
    for path in &paths {
        let key = digest(vec![path.as_str().into()]).to_vec();
        let Some(state) = state_of(Path::new(path)) else {
            failures += 1;
            continue;
        };
        let old = db.first("SELECT inode,size,mtime,offset FROM files WHERE path=?", &[Value::Blob(key.clone())])?;
        // A transcript the archive holds has come back: its records replace the archived ones.
        if archive.is_some()
            && old.is_none()
            && db.first("SELECT 1 FROM kept.records WHERE file=? LIMIT 1", &[Value::Blob(key.clone())])?.is_some()
        {
            rebuild = true;
        }
        let previous = old.as_ref().map(|row| FileState {
            inode: database::int(&row[0]).unwrap_or(-1),
            size: database::int(&row[1]).unwrap_or(-1),
            mtime: database::int(&row[2]).unwrap_or(-1),
        });
        if previous == Some(state) {
            continue;
        }
        let incremental = previous.is_some_and(|previous| previous.inode == state.inode && state.size > previous.size);
        let offset = if incremental { old.as_ref().and_then(|row| database::int(&row[3])).unwrap_or(0) } else { 0 };
        let stem = Path::new(path).file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
        let fallback = allowed_sessions
            .and_then(|allowed| path.split(['/', '\\']).rev().find(|part| allowed.contains(*part)).map(String::from))
            .unwrap_or(stem);
        let location = hex(&key);
        // The folder Claude files the conversation under is its project, wherever it went on to work.
        let project = crate::projects::transcript_project(path);
        let folder = crate::projects::transcript_folder(path);
        let mut added: Vec<Record> = Vec::new();
        db.execute("SAVEPOINT source")?;
        let read = (|| -> Result<i64, ReadFailure> {
            if !incremental {
                db.run("DELETE FROM records WHERE file=?", &[Value::Blob(key.clone())])?;
            }
            let selection = wanted();
            let end = read_lines(Path::new(path), offset, |position, line| {
                if memchr::memmem::find(line, b"\"usage\"").is_none()
                    || memchr::memmem::find(line, b"\"assistant\"").is_none()
                {
                    return Ok(());
                }
                let Ok(row) = Json::parse_selected(without_byte_order_mark(line), &selection, true) else {
                    return Ok(());
                };
                let Ok(found) = records(&row, &fallback, (&location, position)) else { return Ok(()) };
                for mut event in found {
                    if project.is_some() {
                        event.project = project.clone();
                    } else if let (Some(folder), Some(cwd)) = (folder, row.get("cwd").and_then(Json::as_str))
                        && let Some(name) = crate::projects::within(cwd, folder)
                    {
                        event.project = Some(name);
                    }
                    let mut values = vec![
                        Value::Blob(key.clone()),
                        Value::Integer(position),
                        Value::Integer(event.slot),
                        Value::Blob(event.key.clone()),
                        optional_blob(&event.message),
                        optional_blob(&event.request),
                        Value::Blob(event.session.clone()),
                        Value::Real(event.ts),
                        Value::Text(event.model.clone()),
                    ];
                    values.extend(event.metrics.iter().map(|&count| Value::Integer(count)));
                    values.extend([
                        optional_real(event.cost),
                        Value::Integer(i64::from(event.sidechain)),
                        optional_text(&event.speed),
                        Value::Text(event.geo.clone()),
                        Value::Text(event.tier.clone()),
                        optional_text(&event.project),
                    ]);
                    db.run("INSERT OR REPLACE INTO records VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)", &values)?;
                    event.file = key.clone();
                    event.offset = position;
                    added.push(event);
                }
                Ok(())
            })?;
            db.run(
                "INSERT OR REPLACE INTO files VALUES (?,?,?,?,?)",
                &[
                    Value::Blob(key.clone()),
                    Value::Integer(state.inode),
                    Value::Integer(state.size),
                    Value::Integer(state.mtime),
                    Value::Integer(end),
                ],
            )?;
            Ok(end)
        })();
        match read {
            Ok(_) => {
                db.execute("RELEASE source")?;
                // A rewritten file replaces earlier rows; a new or grown file only adds rows.
                if old.is_some() && !incremental {
                    rebuild = true;
                }
                appended.extend(added);
            }
            Err(ReadFailure::Unreadable | ReadFailure::File(_)) => {
                db.execute("ROLLBACK TO source")?;
                db.execute("RELEASE source")?;
                failures += 1;
            }
            Err(ReadFailure::Other(error)) => {
                let _ = db.execute("ROLLBACK TO source");
                let _ = db.execute("RELEASE source");
                return Err(error);
            }
        }
    }
    let allowed: Option<HashSet<Vec<u8>>> =
        allowed_sessions.map(|sessions| sessions.iter().map(|s| digest(vec![s.as_str().into()]).to_vec()).collect());
    // Switching the archive on or off changes what is deduplicated, so it starts the dedupe over.
    let scope_sessions = match allowed_sessions {
        Some(sessions) => {
            let mut sorted: Vec<&String> = sessions.iter().collect();
            sorted.sort();
            Json::Array(sorted.into_iter().map(|s| Json::from(s.as_str())).collect())
        }
        None => Json::Null,
    };
    let scope = hex(&digest(vec![scope_sessions, Json::Bool(archive.is_some())]));
    let old_scope = db.first("SELECT value FROM metadata WHERE key='dedupe-scope'", &[])?.and_then(|row| text(&row[0]));
    let mut added: Vec<Record> = appended
        .into_iter()
        .filter(|row| allowed.as_ref().is_none_or(|allowed| allowed.contains(&row.session)))
        .collect();
    added.sort_by(|a, b| a.file.cmp(&b.file).then(a.offset.cmp(&b.offset)).then(a.slot.cmp(&b.slot)));
    let is_isolated = !added.is_empty() && isolated(db, &added, allowed.as_ref())?;
    let needs_rebuild = rebuild
        || old_scope.as_deref() != Some(scope.as_str())
        || (!added.is_empty() && !is_isolated && !patch_canonical(db, &added, allowed.as_ref())?);
    if needs_rebuild {
        // Deduplicate before date filtering, once per file/ownership change.
        let mut stored = Vec::new();
        db.each(&format!("SELECT {COLUMNS} FROM everything ORDER BY file,offset,slot"), &[], |row| {
            if allowed.as_ref().is_none_or(|allowed| allowed.contains(&blob(&row[3]).unwrap_or_default())) {
                stored.push(stored_record(&row));
            }
            Ok(())
        })?;
        let unique = deduplicate(stored);
        db.run("DELETE FROM canonical", &[])?;
        store_canonical(db, &unique, 0)?;
        db.run("INSERT OR REPLACE INTO metadata VALUES ('dedupe-scope',?)", &[Value::Text(scope)])?;
    } else if is_isolated {
        let next = db
            .first("SELECT COALESCE(MAX(id)+1,0) FROM canonical", &[])?
            .and_then(|row| database::int(&row[0]))
            .unwrap_or(0);
        store_canonical(db, &deduplicate(added), next)?;
    }
    db.execute("COMMIT")?;
    let rows = db.rows(
        "SELECT ts,model,i,o,w5,w1,r,cost,speed,geo,tier,session,project FROM canonical WHERE ts>=? AND ts<=?",
        &[Value::Real(start), Value::Real(end)],
    )?;
    let events = rows
        .iter()
        .map(|row| Event {
            ts: database::double(&row[0]).unwrap_or(0.0),
            model: text(&row[1]).unwrap_or_default(),
            metrics: std::array::from_fn(|index| database::int(&row[2 + index]).unwrap_or(0)),
            cost: database::double(&row[7]),
            speed: text(&row[8]),
            geo: text(&row[9]),
            tier: text(&row[10]),
            session: blob(&row[11]),
            project: text(&row[12]),
        })
        .collect();
    Ok((events, failures))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_chunk_append_patches_canonical_in_place() {
        let dir = std::env::temp_dir().join(format!("starmeter-claude-patch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("s1.jsonl");
        let db = dir.join("claude.sqlite3");
        let chunk1 = r#"{"type":"assistant","sessionId":"s1","timestamp":"2026-11-20T10:00:00Z","requestId":"req_1","message":{"id":"msg_1","model":"claude-opus-5-5","usage":{"input_tokens":100,"output_tokens":10}}}"#;
        let chunk2 = r#"{"type":"assistant","sessionId":"s1","timestamp":"2026-11-20T10:00:01Z","requestId":"req_1","message":{"id":"msg_1","model":"claude-opus-5-5","usage":{"input_tokens":100,"output_tokens":250}}}"#;
        std::fs::write(&log, format!("{chunk1}\n")).unwrap();
        let files = vec![log.to_string_lossy().into_owned()];
        let (first, failures) = read_events(&files, &db, None, 0.0, f64::INFINITY, None).unwrap();
        assert_eq!(failures, 0);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].metrics[1], 10);
        // Append streaming update for the same (msg_1, req_1): should update canonical row id=0 in place.
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
        writeln!(f, "{chunk2}").unwrap();
        drop(f);
        let (second, failures) = read_events(&files, &db, None, 0.0, f64::INFINITY, None).unwrap();
        assert_eq!(failures, 0);
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].metrics[1], 250);
        let conn = Database::open(&db, 5.0, Mode::ReadOnly).unwrap();
        let ids = conn.rows("SELECT id, o FROM canonical", &[]).unwrap();
        assert_eq!(ids.len(), 1);
        assert_eq!(database::int(&ids[0][0]), Some(0));
        assert_eq!(database::int(&ids[0][1]), Some(250));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn intra_batch_subagent_route_clash_does_not_duplicate_canonical_rows() {
        let dir = std::env::temp_dir().join(format!("starmeter-claude-clash-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("s1.jsonl");
        let db = dir.join("claude.sqlite3");
        let initial = r#"{"type":"assistant","sessionId":"s1","timestamp":"2026-11-20T10:00:00Z","requestId":"req_1","message":{"id":"msg_1","model":"claude-opus-5-5","usage":{"input_tokens":100,"output_tokens":10}}}"#;
        // Same batch appends both a streaming chunk for req_1 AND a sidechain replay of msg_1 with a different requestId.
        let update = r#"{"type":"assistant","sessionId":"s1","timestamp":"2026-11-20T10:00:00Z","requestId":"req_1","message":{"id":"msg_1","model":"claude-opus-5-5","usage":{"input_tokens":100,"output_tokens":120}}}"#;
        let sidechain = r#"{"type":"assistant","isSidechain":true,"sessionId":"s1","timestamp":"2026-11-20T10:00:00Z","requestId":"req_sub","message":{"id":"msg_1","model":"claude-opus-5-5","usage":{"input_tokens":100,"output_tokens":120}}}"#;
        std::fs::write(&log, format!("{initial}\n")).unwrap();
        let files = vec![log.to_string_lossy().into_owned()];
        let (first, _) = read_events(&files, &db, None, 0.0, f64::INFINITY, None).unwrap();
        assert_eq!(first.len(), 1);
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
        writeln!(f, "{update}").unwrap();
        writeln!(f, "{sidechain}").unwrap();
        drop(f);
        let (second, _) = read_events(&files, &db, None, 0.0, f64::INFINITY, None).unwrap();
        assert_eq!(second.len(), 1, "sidechain replay with same (message, session) must not duplicate canonical row");
        assert_eq!(second[0].metrics[1], 120);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
