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
    if rebuild
        || old_scope.as_deref() != Some(scope.as_str())
        || (!added.is_empty() && !isolated(db, &added, allowed.as_ref())?)
    {
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
    } else if !added.is_empty() {
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
