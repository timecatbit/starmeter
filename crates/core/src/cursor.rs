//! Cursor usage from the account's own usage records on cursor.com, read only after the user turns it on. The login
//! the desktop app or CLI saved is kept in memory and goes to cursor.com only; it is never stored, refreshed or
//! written back.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use jiff::tz::TimeZone;

use crate::database::{Database, Mode, Value};
use crate::files::paths;
use crate::http::{self, Request};
use crate::json::{Format, Json, Object};
use crate::runtime::{Runtime, now};
use crate::tally::{self, Carrier};
use crate::time::IsoTime;
use crate::{object, pricing, py};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure(pub String);

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn fail<T>(message: &str) -> Result<T, Failure> {
    Err(Failure(message.to_string()))
}

const ORIGIN: &str = "https://cursor.com";
const PAGE_SIZE: usize = 1000;
const MAX_PAGES: usize = 50;
const ENCRYPTED: &str = "cursor-safe-storage:";
/// Read again on every refresh: requests still running and records that land late.
const OVERLAP: f64 = 3600.0;

fn events_url() -> String {
    format!("{ORIGIN}/api/dashboard/get-filtered-usage-events")
}

fn summary_url() -> String {
    format!("{ORIGIN}/api/usage-summary")
}

pub fn app_root(home: &str, environment: &HashMap<String, String>) -> String {
    if cfg!(windows) {
        let roaming = environment.get("APPDATA").cloned().unwrap_or_else(|| format!("{home}\\AppData\\Roaming"));
        format!("{roaming}\\Cursor\\User\\globalStorage")
    } else if cfg!(target_os = "linux") {
        let config = environment
            .get("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .cloned()
            .unwrap_or_else(|| format!("{home}/.config"));
        format!("{config}/Cursor/User/globalStorage")
    } else {
        format!("{home}/Library/Application Support/Cursor/User/globalStorage")
    }
}

pub fn cli_auth(home: &str, environment: &HashMap<String, String>) -> String {
    let config = environment
        .get("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .cloned()
        .unwrap_or_else(|| format!("{home}/.config"));
    format!("{config}/cursor/auth.json")
}

/// The desktop app's access token, read only; never recreates Cursor's own files.
fn app_token(root: &str) -> Option<String> {
    let path = format!("{root}/state.vscdb");
    if !Path::new(&path).is_file() {
        return None;
    }
    let resolved = paths::resolve(Path::new(&path)).map_or_else(|| path.clone(), |p| p.to_string_lossy().into_owned());
    let read = |immutable: bool| -> Option<Option<Value>> {
        let uri = crate::database::file_uri(&resolved, if immutable { "immutable=1" } else { "mode=ro" });
        let db = Database::open(Path::new(&uri), 2.0, Mode::ReadOnlyUri).ok()?;
        let row = db.first("SELECT value FROM ItemTable WHERE key='cursorAuth/accessToken' LIMIT 1", &[]).ok()?;
        Some(row.and_then(|row| row.into_iter().next()))
    };
    let value = match read(false) {
        Some(value) => value,
        None => {
            // An idle database can keep WAL mode with no sidecars; immutable reads it without creating them.
            if ["-wal", "-shm"].iter().any(|suffix| Path::new(&format!("{path}{suffix}")).exists()) {
                return None;
            }
            read(true)?
        }
    };
    match value? {
        Value::Text(text) => Some(text.trim().to_string()),
        Value::Blob(bytes) => {
            let text = if bytes.len() > 1 && bytes[1] == 0 {
                let units: Vec<u16> = bytes
                    .chunks(2)
                    .filter(|pair| pair.len() == 2)
                    .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                    .collect();
                String::from_utf16_lossy(&units)
            } else {
                String::from_utf8_lossy(&bytes).into_owned()
            };
            Some(text.trim().to_string())
        }
        _ => None,
    }
}

fn file_token(path: &str) -> Option<String> {
    let value = Json::parse(&std::fs::read(path).ok()?).ok()?;
    value.get("accessToken").and_then(Json::as_str).map(|text| text.trim().to_string())
}

/// Standard or URL-safe base64, padding optional.
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut bits: u32 = 0;
    let mut count = 0;
    let mut output = Vec::new();
    for byte in text.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            _ => return None,
        };
        bits = bits << 6 | u32::from(value);
        count += 6;
        if count >= 8 {
            count -= 8;
            output.push((bits >> count) as u8);
            bits &= (1 << count) - 1;
        }
    }
    Some(output)
}

fn claims(token: &str) -> Object {
    let parts: Vec<&str> = token.split('.').collect();
    let Some(part) = parts.get(1) else { return Object::new() };
    base64_decode(part)
        .and_then(|data| Json::parse(&data).ok())
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default()
}

fn numeric(value: Option<&Json>) -> Option<f64> {
    match value? {
        Json::Bool(flag) => Some(if *flag { 1.0 } else { 0.0 }),
        other => other.number(),
    }
}

/// The user ID, token and where it came from, for the first usable login: the desktop app first, then the CLI.
fn session(home: &str, environment: &HashMap<String, String>, now: f64) -> Result<(String, String, String), Failure> {
    let mut problems: HashSet<&str> = HashSet::new();
    let found = [
        ("Cursor 桌面版", app_token(&app_root(home, environment))),
        ("Cursor CLI", file_token(&cli_auth(home, environment))),
    ];
    for (place, token) in found {
        let Some(token) = token.filter(|token| !token.is_empty()) else { continue };
        if token.starts_with(ENCRYPTED) {
            problems.insert("encrypted");
            continue;
        }
        let info = claims(&token);
        let subject =
            py::str(Some(&py::or(info.get("sub"), Json::from("")))).rsplit('|').next().unwrap_or("").to_string();
        if subject.is_empty() {
            problems.insert("format");
            continue;
        }
        if !numeric(info.get("exp")).is_some_and(|seconds| seconds > now + 60.0) {
            problems.insert("expired");
            continue;
        }
        return Ok((subject, token, place.to_string()));
    }
    if problems.contains("expired") {
        return fail("Cursor 登录已过期，请打开 Cursor 重新登录");
    }
    if problems.contains("encrypted") {
        return fail("Cursor 登录已加密保存，暂时无法读取");
    }
    if problems.contains("format") {
        return fail("无法识别 Cursor 登录信息");
    }
    fail("未找到 Cursor 登录，请先登录 Cursor 桌面版或 Cursor CLI")
}

/// POSTs the body to a cursor.com endpoint (GET when there is none) with the session cookie; no redirects.
fn post(body: Option<&Json>, user: &str, token: &str, url: &str) -> Result<Object, Failure> {
    let mut headers = vec![
        ("Accept", "application/json".to_string()),
        ("Origin", ORIGIN.to_string()),
        ("Cookie", format!("WorkosCursorSessionToken={user}%3A%3A{token}")),
    ];
    if body.is_some() {
        headers.push(("Content-Type", "application/json".to_string()));
    }
    let request = Request {
        method: if body.is_some() { "POST" } else { "GET" },
        headers,
        body: body.map(|body| body.serialized_with(Format { ensure_ascii: true, ..Format::default() }).into_bytes()),
        timeout: 20.0,
        follow_redirects: false,
        limit: 32 * 1024 * 1024 + 1,
        ..Request::default()
    };
    let Ok(response) = http::send(url, request) else { return fail("无法连接 cursor.com") };
    match response.status {
        200..=299 => {}
        301 | 302 | 303 | 307 | 308 | 401 => return fail("Cursor 登录已失效，请打开 Cursor 重新登录"),
        403 => return fail("cursor.com 拒绝读取用量记录"),
        429 => return fail("cursor.com 请求过于频繁，稍后重试"),
        status => return Err(Failure(format!("cursor.com 暂时不可用（HTTP {status}）"))),
    }
    if response.body.len() > 32 * 1024 * 1024 {
        return fail("用量记录过大");
    }
    Json::parse(&response.body)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .ok_or_else(|| Failure("用量记录格式异常".into()))
}

/// Every usage event between start and end, page by page; an event ID seen before (at a page seam) is skipped.
fn events(user: &str, token: &str, start: f64, end: f64) -> Result<(Vec<Object>, bool), Failure> {
    let mut rows = Vec::new();
    let mut seen = HashSet::new();
    for page in 1..=MAX_PAGES {
        let body = Json::Object(object! {
            "page" => page as i64, "pageSize" => PAGE_SIZE as i64,
            "startDate" => ((start.max(0.0) * 1000.0) as i64).to_string(), "endDate" => ((end * 1000.0) as i64).to_string(),
        });
        let value = post(Some(&body), user, token, &events_url())?;
        let batch_value = py::or(value.get("usageEventsDisplay"), Json::Array(vec![]));
        let Some(batch) = batch_value.as_array() else { return fail("用量记录格式异常") };
        for row in batch {
            let Some(fields) = row.as_object() else { continue };
            if let Some(key) = fields.get("usageEventId").filter(|key| key.is_truthy())
                && !seen.insert(py::dumps(key))
            {
                continue;
            }
            rows.push(fields.clone());
        }
        let total = value.get("totalUsageEventsCount");
        let counted = total
            .filter(|total| total.as_bool().is_none())
            .and_then(Json::integer)
            .is_some_and(|total| rows.len() as i64 >= total);
        if batch.len() < PAGE_SIZE || counted {
            return Ok((rows, true));
        }
    }
    Ok((rows, false))
}

/// One event in the shape the shared tally reads. Cursor's input excludes cache reads and writes, and totalCents
/// is the request at the model's list price.
fn normalize(row: &Object) -> Option<Object> {
    let usage = row.get("tokenUsage").and_then(Json::as_object).cloned().unwrap_or_default();
    let stamp = py::str(row.get("timestamp"));
    let whole: i64 = stamp.trim_matches(' ').parse().ok()?;
    let cost = match usage.get("totalCents") {
        Some(cents) if cents.as_bool().is_none() && cents.number().is_some_and(|value| value >= 0.0) => {
            Json::Double(py::number(Some(cents)).number().unwrap_or(0.0) / 100.0)
        }
        _ => Json::Null,
    };
    Some(object! {
        "timestamp" => whole as f64 / 1000.0,
        "model" => py::str(Some(&py::or(row.get("model"), Json::from("default")))),
        "session" => py::or(row.get("conversationId"), Json::Null),
        "input" => py::number(usage.get("inputTokens")),
        "output" => py::number(usage.get("outputTokens")),
        "cached" => py::number(usage.get("cacheReadTokens")),
        "writes" => py::number(usage.get("cacheWriteTokens")),
        "cost" => cost,
    })
}

/// This billing cycle's included plan usage, as Cursor's dashboard shows it: used ÷ limit, its own percent fields
/// only when there is no limit. Amounts are in cents.
pub fn plan(home: &str, environment: &HashMap<String, String>, now: f64) -> Result<Object, Failure> {
    let (user, token, place) = session(home, environment, now)?;
    let value = post(None, &user, &token, &summary_url())?;
    let usage = value.get("individualUsage").and_then(Json::as_object).cloned().unwrap_or_default();
    let part = |name: &str| usage.get(name).and_then(Json::as_object).cloned().unwrap_or_default();
    let (plan_part, overall, extra) = (part("plan"), part("overall"), part("onDemand"));
    let (mut used, mut limit, mut percent) = (Json::Null, Json::Null, Json::Null);
    for block in [&plan_part, &overall] {
        if py::num(block.get("limit")) > 0.0 {
            used = py::number(block.get("used"));
            limit = py::number(block.get("limit"));
            percent = Json::Double(100.0 * py::num(block.get("used")) / py::num(block.get("limit")));
            break;
        }
    }
    if percent.is_null()
        && let Some(total) = plan_part.get("totalPercentUsed").filter(|t| t.as_bool().is_none() && t.number().is_some())
    {
        percent = py::number(Some(total));
    }
    let stamp = |key: &str| -> Json {
        value
            .get(key)
            .and_then(Json::as_str)
            .and_then(|text| IsoTime::parse(text).ok())
            .map_or(Json::Null, |time| Json::Double(time.timestamp(&TimeZone::system())))
    };
    Ok(object! {
        "membership" => py::str(Some(&py::or(value.get("membershipType"), Json::from("")))),
        "unlimited" => value.get("isUnlimited").and_then(Json::as_bool) == Some(true),
        "percent" => percent, "usedCents" => used, "limitCents" => limit,
        "cycleStart" => stamp("billingCycleStart"), "cycleEnd" => stamp("billingCycleEnd"),
        "login" => place,
        "onDemand" => object! {
            "enabled" => extra.get("enabled").and_then(Json::as_bool) == Some(true),
            "usedCents" => py::number(extra.get("used")),
            "limitCents" => if py::num(extra.get("limit")) > 0.0 { py::number(extra.get("limit")) } else { Json::Null },
        },
    })
}

/// Which account a kept history belongs to, without keeping the account ID itself.
fn account(user: &str) -> String {
    py::sha256_hex(format!("cursor:{user}").as_bytes())[..16].to_string()
}

fn plain(value: Option<&Json>) -> Option<f64> {
    value.filter(|value| value.as_bool().is_none()).and_then(Json::number)
}

fn when(event: &Object) -> f64 {
    event.get("timestamp").and_then(Json::number).unwrap_or(0.0)
}

/// The account's events from start to until, in one store every range shares. Only what the store lacks is asked
/// for; a store for another account, or one a read could not finish, starts over.
pub fn refresh(
    store: Option<&Object>,
    home: &str,
    environment: &HashMap<String, String>,
    start: f64,
    until: f64,
    now: f64,
) -> Result<Object, Failure> {
    let (user, token, place) = session(home, environment, now)?;
    let owner = account(&user);
    let held = store.cloned().unwrap_or_default();
    let (mut low, mut high) = (held.get("from").cloned(), held.get("through").cloned());
    let mut kept: Vec<Object> = held
        .get("events")
        .and_then(Json::as_array)
        .map(|events| events.iter().filter_map(|e| e.as_object().cloned()).collect())
        .unwrap_or_default();
    let invalid = held.get("account") != Some(&Json::from(owner.as_str()))
        || held.get("complete").and_then(Json::as_bool) != Some(true)
        || held.get("events").and_then(Json::as_array).is_none()
        || plain(low.as_ref()).is_none()
        || plain(high.as_ref()).is_none()
        || plain(low.as_ref()) > plain(high.as_ref());
    if invalid {
        low = None;
        high = None;
        kept.clear();
    }
    let mut complete = true;
    let mut read = |begin: f64, end: f64| -> Result<Vec<Object>, Failure> {
        let (rows, done) = events(&user, &token, begin, end)?;
        complete = complete && done;
        Ok(rows.iter().filter_map(normalize).collect())
    };
    match plain(low.as_ref()) {
        None => {
            kept = read(start, until)?;
            low = Some(Json::Double(start));
        }
        Some(mut edge) => {
            // Each read runs a second past its seam and each side keeps its own events, so whether Cursor counts the
            // ends of a window in or out never loses or doubles one.
            if start < edge {
                let mut earlier: Vec<Object> =
                    read(start, edge + 1.0)?.into_iter().filter(|event| when(event) < edge).collect();
                earlier.extend(kept);
                kept = earlier;
                low = Some(Json::Double(start));
                edge = start;
            }
            let seam = edge.max(plain(high.as_ref()).unwrap_or(0.0) - OVERLAP);
            if until > seam {
                let mut before: Vec<Object> = kept.into_iter().filter(|event| when(event) < seam).collect();
                before.extend(read(seam - 1.0, until)?.into_iter().filter(|event| when(event) >= seam));
                kept = before;
            }
        }
    }
    let mut indexed: Vec<(usize, Object)> = kept.into_iter().enumerate().collect();
    indexed.sort_by(|(a_index, a), (b_index, b)| when(a).total_cmp(&when(b)).then(a_index.cmp(b_index)));
    let through = match plain(high.as_ref()) {
        Some(previous) if until < previous => high.clone().unwrap_or(Json::Double(until)),
        _ => Json::Double(until),
    };
    Ok(object! {
        "account" => owner,
        "events" => Json::Array(indexed.into_iter().map(|(_, event)| event.into()).collect()),
        "from" => low.unwrap_or(Json::Double(start)),
        "through" => through,
        "complete" => complete,
        "login" => place,
    })
}

/// The card's figures from the kept events; a call Cursor also reported to Magpie counts once, from Magpie.
pub fn tally(
    events: &[Object],
    complete: bool,
    login: Json,
    start: f64,
    end: f64,
    zone: &TimeZone,
    carriers: &mut [Carrier],
) -> Object {
    let carriers = if carriers.is_empty() { None } else { Some(carriers) };
    let mut result = tally::aggregate(events, start, end, zone, false, Some(&pricing::model_cost), carriers);
    result.insert("source", "Cursor 账户用量记录");
    result.insert("login", login);
    if !complete {
        result.insert("partial", true);
        result.insert("warning", "用量记录过多，只读取了一部分");
    }
    result
}

impl Runtime {
    /// Cursor's records for a range: every range shares one store of the account's events, so switching ranges
    /// reads nothing new unless a range reaches further back, and a refresh asks only for the time since.
    pub fn cursor_range_usage_live(&self, start: f64, end: f64, now_moment: f64, carriers: &mut [Carrier]) -> Object {
        if !self.settings().get("cursorUsage").is_some_and(Json::is_truthy) {
            return object! { "error" => "未开启用量读取" };
        }
        let name = "cursor_usage_events";
        let held = self.read_cache(name).get("payload").and_then(Json::as_object).cloned().unwrap_or_default();
        let covered = plain(held.get("from")).is_some_and(|from| from <= start);
        // Due when the store ends more than five minutes before this range does; yesterday's range never is.
        let fresh =
            covered && plain(held.get("through")).is_some_and(|through| end - through < self.refresh_ttl(300.0));
        let ttl = if fresh { self.refresh_ttl(300.0) } else { 0.0 };
        let (payload, error) = self.cached_fetch(name, ttl, 60.0, || {
            match refresh(Some(&held), &self.home, &self.environment, start, now_moment, now()) {
                Ok(store) => Ok((Some(store), None)),
                Err(failure) => Ok((None, Some(failure.0))),
            }
        });
        // A failed read reports the reason; the range bundle keeps the last good figures with their own time.
        let usable = payload.filter(|payload| {
            payload.get("events").and_then(Json::as_array).is_some()
                && plain(payload.get("from")).is_some_and(|from| from <= start)
        });
        let (Some(payload), None) = (usable, error.as_ref()) else {
            return object! { "error" => error.unwrap_or_else(|| "用量记录暂不可用".into()) };
        };
        let events: Vec<Object> = payload
            .get("events")
            .and_then(Json::as_array)
            .map(|events| events.iter().filter_map(|event| event.as_object().cloned()).collect())
            .unwrap_or_default();
        tally(
            &events,
            payload.get("complete").is_none_or(Json::is_truthy),
            payload.get("login").cloned().unwrap_or(Json::Null),
            start,
            end,
            &self.local_zone(),
            carriers,
        )
    }
}
