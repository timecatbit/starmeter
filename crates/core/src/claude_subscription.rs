//! Claude subscription usage: the usage response's windows and extra usage, Claude Code's OAuth login (read only,
//! tokens kept in memory), its statusline snapshot and the desktop app's own snapshot.

use std::collections::HashSet;
use std::path::Path;

use jiff::tz::TimeZone;

use crate::claude_usage::state_of;
use crate::files::{self, paths};
use crate::harness::is_absolute;
use crate::http::{self, Request};
use crate::json::{Json, Object};
use crate::runtime::{Loaded, Runtime, now};
use crate::time::IsoTime;
use crate::{claude_app, object, platform, py};

const FIVE_HOURS: f64 = 5.0 * 3600.0;
const WEEK: f64 = 7.0 * 86400.0;

#[derive(Clone, Debug)]
pub struct UsageError(pub String);

/// The windows of a usage response, newest plan format first, and any extra usage it carries.
pub fn parse_usage(data: &Json, now: f64) -> Result<Object, UsageError> {
    let fields = data.as_object().ok_or_else(|| UsageError("用量格式尚未支持".into()))?;
    let mut windows: Vec<Json> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    fn add(
        windows: &mut Vec<Json>,
        seen: &mut HashSet<String>,
        label: Option<&Json>,
        percent: Option<&Json>,
        reset: Option<&Json>,
        span: Option<f64>,
    ) {
        let Some(percent) = percent.filter(|p| p.as_bool().is_none()) else { return };
        if percent.number().filter(|v| v.is_finite() && (0.0..=100.0).contains(v)).is_none() {
            return;
        }
        if let Some(reset) = reset
            && !reset.is_null()
            && reset.as_str().is_none()
        {
            return;
        }
        let Some(text) = label.and_then(Json::as_str).filter(|text| !text.is_empty()) else { return };
        let key = format!("{text}\u{1F}{}", reset.and_then(Json::as_str).unwrap_or("\u{0}"));
        if !seen.insert(key) {
            return;
        }
        let mut window = object! {
            "label" => text.chars().take(80).collect::<String>(), "used" => percent.clone(), "resetsAt" => reset.cloned().unwrap_or(Json::Null),
        };
        if let Some(span) = span {
            // Python's 5*3600 and 7*86400 are integers.
            window.insert("span", span as i64);
        }
        windows.push(window.into());
    }
    // The plan API lists model and surface limits instead of fixed five_hour keys.
    if let Some(limits) = fields.get("limits").and_then(Json::as_array) {
        for row in limits {
            let Some(limit) = row.as_object() else { continue };
            let scope = limit.get("scope").and_then(Json::as_object).cloned().unwrap_or_default();
            let model = scope.get("model").and_then(Json::as_object).cloned().unwrap_or_default();
            let surface = scope.get("surface").and_then(Json::as_object).cloned().unwrap_or_default();
            let label = py::or(model.get("display_name"), surface.get("display_name").cloned().unwrap_or(Json::Null));
            // The window's length, for the sidebar's pace; a kind not recognised has none.
            let kind = format!(
                "{} {}",
                py::str(Some(limit.get("group").unwrap_or(&Json::from("")))),
                py::str(Some(limit.get("kind").unwrap_or(&Json::from(""))))
            )
            .to_lowercase();
            let span = if ["week", "seven_day"].iter().any(|word| kind.contains(word)) {
                Some(WEEK)
            } else if ["five_hour", "session"].iter().any(|word| kind.contains(word)) {
                Some(FIVE_HOURS)
            } else {
                None
            };
            let shown = if label.is_truthy() {
                label
            } else {
                Json::from(if span == Some(WEEK) { "本周" } else { "当前周期" })
            };
            add(&mut windows, &mut seen, Some(&shown), limit.get("percent"), limit.get("resets_at"), span);
        }
    }
    if windows.is_empty() {
        for (key, label) in [
            ("five_hour", "5 小时"),
            ("seven_day", "本周"),
            ("seven_day_opus", "Opus 本周"),
            ("seven_day_sonnet", "Sonnet 本周"),
        ] {
            if let Some(row) = fields.get(key).and_then(Json::as_object) {
                let span = if key == "five_hour" { FIVE_HOURS } else { WEEK };
                add(
                    &mut windows,
                    &mut seen,
                    Some(&Json::from(label)),
                    row.get("utilization"),
                    row.get("resets_at"),
                    Some(span),
                );
            }
        }
    }
    if windows.is_empty() {
        return Err(UsageError("尚未返回可识别的订阅用量".into()));
    }
    let mut result = object! { "windows" => Json::Array(windows), "updatedAt" => now, "stale" => false, "state" => "在线快照", "source" => "live" };
    if let Some(extra) = extra_usage(fields) {
        result.insert("extraUsage", extra);
    }
    Ok(result)
}

fn is_currency(value: Option<&Json>) -> bool {
    value
        .and_then(Json::as_str)
        .is_some_and(|text| text.len() == 3 && text.bytes().all(|byte| byte.is_ascii_uppercase()))
}

/// {amount_minor, exponent, currency} → amount and currency; anything else is unknown.
fn money(value: Option<&Json>) -> Option<(f64, Option<String>)> {
    let fields = value?.as_object()?;
    let (Some(Json::Int(amount)), Some(Json::Int(exponent))) = (fields.get("amount_minor"), fields.get("exponent"))
    else {
        return None;
    };
    if !(0..=6).contains(exponent) {
        return None;
    }
    let scale = (0..*exponent).fold(1.0, |scale, _| scale * 10.0);
    let currency = is_currency(fields.get("currency"))
        .then(|| fields.get("currency").and_then(Json::as_str).unwrap_or("").to_string());
    Some((*amount as f64 / scale, currency))
}

/// Extra usage from the same response: monthly spend, cap and, when reported, the credit balance.
fn extra_usage(data: &Object) -> Option<Object> {
    if let Some(spend) = data.get("spend").and_then(Json::as_object)
        && let Some(enabled) = spend.get("enabled").and_then(Json::as_bool)
    {
        if !enabled {
            return Some(object! { "enabled" => false });
        }
        let (used, limit, balance) = (money(spend.get("used")), money(spend.get("limit")), money(spend.get("balance")));
        if let Some((used, currency)) = used.filter(|(used, _)| *used >= 0.0) {
            let mut result = object! {
                "enabled" => true, "used" => used,
                "limit" => limit.filter(|(limit, _)| *limit > 0.0).map(|(limit, _)| limit),
                "currency" => currency.unwrap_or_else(|| "USD".into()),
            };
            if let Some((balance, _)) = balance {
                result.insert("balance", balance);
            }
            return Some(result);
        }
    }
    // Older responses only carry extra_usage, in minor units (cents unless decimal_places says otherwise).
    let value = data.get("extra_usage")?.as_object()?;
    let enabled = value.get("is_enabled")?.as_bool()?;
    if !enabled {
        return Some(object! { "enabled" => false });
    }
    let mut scale = 100.0;
    if let Some(Json::Int(places)) = value.get("decimal_places")
        && (0..=6).contains(places)
    {
        scale = (0..*places).fold(1.0, |scale, _| scale * 10.0);
    }
    let major = |raw: Option<&Json>| -> Option<f64> {
        raw.filter(|raw| raw.as_bool().is_none())
            .and_then(Json::number)
            .filter(|n| n.is_finite() && *n >= 0.0)
            .map(|n| n / scale)
    };
    let used = major(value.get("used_credits"))?;
    let limit = major(value.get("monthly_limit"));
    // A zero monthly limit means no cap rather than nothing left.
    let currency = if is_currency(value.get("currency")) {
        value.get("currency").and_then(Json::as_str).unwrap_or("USD")
    } else {
        "USD"
    };
    Some(
        object! { "enabled" => true, "used" => used, "limit" => limit.filter(|limit| *limit != 0.0), "currency" => currency },
    )
}

// Small state files

pub fn read_state(path: &str) -> Object {
    claude_app::read_object_limited(path, 2 * 1024 * 1024)
}

fn write_state(path: &str, value: &Object) {
    let path = Path::new(path);
    if let Some(parent) = path.parent() {
        let _ = files::make_directory(parent, 0o700);
    }
    let prefix = format!(".{}", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
    let _ = files::replace_atomically(path, Json::Object(value.clone()).serialized().as_bytes(), &prefix, || Ok(()));
}

// Claude Code's statusline snapshot

pub const SNAPSHOT_FILE: &str = "claude-code-usage.json";
pub const BACKUP_FILE: &str = "integrations/claude-statusline-original.json";

/// Whether the statusline bridge is what Claude Code runs now.
pub fn local_enabled(state: &str) -> bool {
    let meta = read_state(&format!("{state}/{BACKUP_FILE}"));
    let (Some(settings), Some(command)) =
        (meta.get("settings").filter(|s| s.is_truthy()), meta.get("managedCommand").filter(|c| c.is_truthy()))
    else {
        return false;
    };
    read_state(&py::str(Some(settings))).get("statusLine").and_then(|status| status.get("command")) == Some(command)
}

/// The bridge's last snapshot, or None when there is none worth reading.
pub fn local_snapshot(state: &str, now: f64) -> Option<Object> {
    let data = read_state(&format!("{state}/{SNAPSHOT_FILE}"));
    if data.get("schema") != Some(&Json::Int(1)) || data.get("source") != Some(&Json::from("claude-code")) {
        return None;
    }
    let stamp_value = data.get("updatedAt").filter(|v| v.as_bool().is_none())?;
    let stamp = stamp_value.number().filter(|stamp| stamp.is_finite() && *stamp <= now + 60.0)?;
    let rows = data.get("windows")?.as_array()?;
    let mut windows: Vec<Json> = Vec::new();
    let mut expired = false;
    for row in rows {
        let Some(fields) = row.as_object() else { continue };
        let (Some(label), Some(value), Some(reset_text)) =
            (fields.get("label"), fields.get("used"), fields.get("resetsAt").and_then(Json::as_str))
        else {
            continue;
        };
        let Ok(reset) = IsoTime::parse(reset_text) else { continue };
        let known = label == &Json::from("5 小时") || label == &Json::from("本周");
        let usable = value.as_bool().is_none()
            && value.number().is_some_and(|used| used.is_finite() && (0.0..=100.0).contains(&used));
        if !known || !usable {
            continue;
        }
        expired = expired || reset.timestamp(&TimeZone::system()) <= now;
        windows.push(object! { "label" => label.clone(), "used" => value.clone(), "resetsAt" => reset_text }.into());
    }
    let stale = expired || now - stamp > 600.0 || windows.is_empty();
    let state_text = if !windows.is_empty() && stale {
        "本机用量待更新"
    } else if !windows.is_empty() {
        "本机用量"
    } else {
        "等待客户端返回订阅用量"
    };
    Some(object! {
        "schema" => 1, "source" => "claude-code", "windows" => Json::Array(windows), "updatedAt" => stamp_value.clone(),
        "stale" => stale, "state" => state_text,
    })
}

// The desktop app's snapshot

const DESKTOP_WINDOWS: [(&str, &str); 6] = [
    ("fh", "5 小时"),
    ("sd", "本周"),
    ("so", "Opus 本周"),
    ("sn", "Sonnet 本周"),
    ("cw", "Cowork 本周"),
    ("oa", "其他应用本周"),
];

pub fn desktop(root: &str, now: f64) -> Object {
    let mut result = object! { "windows" => Json::Array(vec![]), "state" => "暂无本机用量快照", "stale" => true };
    let data = claude_app::read_object(&format!("{root}/plan-usage-history.json"));
    let Some(version) = data.get("version").filter(|v| **v == Json::Int(1) || **v == Json::Int(2)).cloned() else {
        return result;
    };
    let Some(samples) = data.get("samples").and_then(Json::as_array) else { return result };
    let moment = |row: &Object| row.get("t").filter(|t| t.as_bool().is_none()).and_then(Json::number);
    let mut rows: Vec<Object> = samples
        .iter()
        .filter_map(|row| row.as_object().cloned())
        .filter(|row| {
            moment(row).is_some_and(|value| value.is_finite() && 0.0 < value / 1000.0 && value / 1000.0 <= now + 60.0)
        })
        .collect();
    let mut orgs: HashSet<String> =
        rows.iter().filter_map(|row| row.get("org").and_then(Json::as_str).map(String::from)).collect();
    // A registry may prove an account boundary, but not which of several orgs is active.
    if let Some(account) =
        claude_app::read_object(&format!("{root}/config.json")).get("lastKnownAccountUuid").and_then(Json::as_str)
        && claude_app::is_uuid(account)
    {
        let base = format!("{root}/claude-code-sessions/{account}");
        let owned: HashSet<String> = std::fs::read_dir(&base)
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .filter(|name| Path::new(&format!("{base}/{name}")).is_dir() && claude_app::is_uuid(name))
                    .collect()
            })
            .unwrap_or_default();
        if !owned.is_empty() {
            rows.retain(|row| row.get("org").and_then(Json::as_str).is_some_and(|org| owned.contains(org)));
            orgs = rows.iter().map(|row| row.get("org").map_or_else(|| "null".to_string(), py::dumps)).collect();
        }
    }
    if orgs.len() > 1 {
        result.insert("state", "多个组织，请在 Claude 查看");
        return result;
    }
    let Some(row) = rows.iter().fold(None::<&Object>, |best, row| match best {
        Some(best) if moment(row).unwrap_or(0.0) <= moment(best).unwrap_or(0.0) => Some(best),
        _ => Some(row),
    }) else {
        return result;
    };
    let usage = if version == Json::Int(2) {
        row.get("u").and_then(Json::as_object).cloned()
    } else {
        Some(DESKTOP_WINDOWS.iter().map(|(key, _)| (*key, row.get(key).cloned().unwrap_or(Json::Null))).collect())
    };
    let Some(usage) = usage else { return result };
    let windows: Vec<Json> = DESKTOP_WINDOWS
        .iter()
        .filter_map(|(key, label)| {
            let value = usage.get(key).filter(|v| v.as_bool().is_none())?;
            value.number().filter(|used| used.is_finite() && (0.0..=100.0).contains(used))?;
            Some(object! { "label" => *label, "used" => value.clone() }.into())
        })
        .collect();
    if windows.is_empty() {
        return result;
    }
    let stamp = moment(row).unwrap_or(0.0) / 1000.0;
    let stale = now - stamp > 1800.0;
    result.insert("windows", Json::Array(windows));
    result.insert("updatedAt", stamp);
    result.insert("stale", stale);
    result.insert("state", if stale { "快照待更新" } else { "已读取本机快照" });
    result
}

// Claude Code's OAuth login

#[derive(Clone, Debug)]
pub struct OAuthError {
    pub code: &'static str,
    pub description: String,
    pub retry: f64,
}

fn oauth_error(code: &'static str, description: &str) -> OAuthError {
    OAuthError { code, description: description.to_string(), retry: 300.0 }
}

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const INTERVAL: f64 = 300.0;
const MAX_RESPONSE: usize = 1024 * 1024;
/// A run of failures the network clears by itself (just woken, no Wi-Fi yet, a timeout, the service briefly down)
/// keeps the last figures as they are for this long, unmarked; past it the row says the refresh failed.
const QUIET: f64 = 180.0;
/// Attempts further apart than this (the machine slept in between) start a new run.
const RUN_GAP: f64 = 600.0;

/// Offline, a timeout or the service briefly down: nothing the user has to do.
fn passing(code: &str) -> bool {
    code == "network" || code == "service"
}

/// After a passing failure the next try comes soon: 15 seconds, doubling up to five minutes.
fn passing_retry(failures: i64) -> f64 {
    (15.0 * 2f64.powi(failures.clamp(1, 6) as i32 - 1)).min(INTERVAL)
}

/// Waking from sleep: a run of passing failures (from before the sleep, or a maintenance wake while the lid was shut)
/// is forgotten and its backoff dropped, so the next read goes ahead now and starts a run of its own. A server's rate
/// limit, a paused login and a failed renewal stay as they were.
pub fn forget_passing_failures(state: &str) {
    let path = format!("{state}/claude-oauth-usage.json");
    let _lock = platform::lock(Path::new(&format!("{state}/claude-oauth.lock")), true);
    let mut cached = read_state(&path);
    if cached.get("failures").and_then(Json::number).unwrap_or(0.0) <= 0.0 {
        return;
    }
    cached.remove("failures");
    cached.remove("failingSince");
    cached.insert("retryAt", 0);
    write_state(&path, &cached);
}

/// Claude Code files its login under the login name, or claude-code-user when the name has other characters.
fn keychain_account() -> String {
    let name = std::env::var("USER").ok().filter(|name| !name.is_empty()).unwrap_or_default();
    let plain = !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte));
    if plain { name } else { "claude-code-user".into() }
}

/// One keychain item, through the security tool so the credential never lands in an argument.
fn keychain_data(account: Option<&str>) -> Result<Vec<u8>, OAuthError> {
    if !cfg!(target_os = "macos") {
        return Err(oauth_error("missing", "请先在 Claude Code 登录订阅账户"));
    }
    let mut command = std::process::Command::new("/usr/bin/security");
    command.arg("find-generic-password");
    if let Some(account) = account {
        command.args(["-a", account]);
    }
    command.args(["-s", "Claude Code-credentials", "-w"]).stderr(std::process::Stdio::null());
    let output = command.output().map_err(|_| oauth_error("keychain", "钥匙串读取未完成，请手动重试授权"))?;
    match output.status.code() {
        Some(0) => {}
        Some(44) => return Err(oauth_error("missing", "请先在 Claude Code 登录订阅账户")),
        _ => return Err(oauth_error("keychain", "钥匙串未授权，请点击授权读取登录")),
    }
    // security prints the value as bare hex digits when any byte is not printable ASCII; JSON is never all hex.
    let data = output.stdout;
    let trimmed = data.trim_ascii();
    if !trimmed.is_empty() && trimmed.len().is_multiple_of(2) && trimmed.iter().all(u8::is_ascii_hexdigit) {
        let bytes =
            trimmed.chunks(2).filter_map(|pair| u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()).collect();
        return Ok(bytes);
    }
    Ok(data)
}

fn login_document(raw: &[u8]) -> Option<Object> {
    if raw.len() > MAX_RESPONSE {
        return None;
    }
    Json::parse(raw).ok().and_then(|value| value.as_object().cloned())
}

/// The access token, its expiry and whose login it is, read only; the refresh token is never kept.
fn credentials() -> Result<(String, f64, String), OAuthError> {
    let configured = std::env::var("CLAUDE_CONFIG_DIR").unwrap_or_default();
    if configured.contains(',') {
        return Err(oauth_error("profile", "多个 Claude 目录，请指定单一登录来源"));
    }
    let raw = configured.trim();
    let fallback = format!("{}/.claude", paths::home().to_string_lossy());
    let root = if raw.is_empty() {
        fallback.clone()
    } else {
        let expanded = paths::expand_user(raw);
        paths::resolve(&expanded).unwrap_or(expanded).to_string_lossy().into_owned()
    };
    let (mut data, mut unreadable, mut legacy) = (None::<Object>, None::<&str>, false);
    if cfg!(target_os = "macos") && root == fallback {
        match keychain_data(Some(&keychain_account())) {
            Ok(raw) => {
                data = login_document(&raw);
                if data.is_none() {
                    unreadable = Some("Claude Code 钥匙串里的登录无法解析");
                }
            }
            Err(error) if error.code == "missing" => legacy = true,
            Err(_) => {}
        }
    }
    if data.is_none() {
        let path = format!("{root}/.credentials.json");
        if Path::new(&path).exists() {
            let content =
                std::fs::read(&path).map_err(|_| oauth_error("credential", "Claude Code 登录文件无法读取"))?;
            data = login_document(&content);
            if data.is_none() {
                return Err(oauth_error("credential", "Claude Code 登录文件无法解析"));
            }
        }
    }
    if data.is_none() && legacy {
        // Older installs may have used another account name; only without a local file.
        if let Ok(raw) = keychain_data(None) {
            data = login_document(&raw);
            if data.is_none() {
                unreadable = Some("Claude Code 钥匙串里的登录无法解析");
            }
        }
    }
    let Some(data) = data else {
        return Err(oauth_error(
            if unreadable.is_some() { "credential" } else { "missing" },
            unreadable.unwrap_or("请先在 Claude Code 登录订阅账户"),
        ));
    };
    // Signed in with an API key, or only MCP logins stored: no subscription login to read.
    let Some(oauth_value) = data.get("claudeAiOauth").filter(|value| !value.is_null()) else {
        return Err(oauth_error("missing", "请先在 Claude Code 登录订阅账户"));
    };
    let oauth = oauth_value.as_object().ok_or_else(|| oauth_error("credential", "Claude Code 登录内容无法识别"))?;
    let token = oauth.get("accessToken");
    // Claude Code empties the stored token once the server rejects its refresh token.
    if token == Some(&Json::from("")) {
        return Err(oauth_error("missing", "Claude Code 登录已失效，请在 Claude Code 运行 /login"));
    }
    let text = token
        .and_then(Json::as_str)
        .filter(|text| {
            (16..=4096).contains(&text.len())
                && text.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"._~-".contains(&byte))
        })
        .ok_or_else(|| oauth_error("credential", "Claude Code 登录令牌格式无法识别"))?;
    let expires = oauth
        .get("expiresAt")
        .filter(|expiry| expiry.as_bool().is_none())
        .and_then(Json::number)
        .filter(|expires| expires.is_finite())
        .ok_or_else(|| oauth_error("credential", "Claude Code 登录缺少有效期"))?;
    let scoped =
        oauth.get("scopes").and_then(Json::as_array).is_some_and(|scopes| scopes.contains(&Json::from("user:profile")));
    if !scoped {
        return Err(oauth_error("scope", "Claude Code 登录缺少用量权限，请重新登录"));
    }
    Ok((text.to_string(), expires / 1000.0, py::sha256_hex(text.as_bytes())))
}

/// Retry-After as seconds or an HTTP date, at least a minute; five minutes when it can't be read.
fn retry_seconds(header: Option<&str>, now: f64) -> f64 {
    let raw = header.unwrap_or("");
    let seconds = raw
        .trim_matches([' ', '\t'])
        .parse::<f64>()
        .ok()
        .or_else(|| http_date(raw).map(|moment| moment - now))
        .unwrap_or(300.0);
    if seconds.is_finite() { seconds.max(60.0) } else { 300.0 }
}

/// An HTTP date ("Wed, 21 Oct 2015 07:28:00 GMT") as seconds since 1970.
fn http_date(text: &str) -> Option<f64> {
    let parts: Vec<&str> = text.split_whitespace().collect();
    let [_, day, month, year, time, zone] = parts.as_slice() else { return None };
    if !["GMT", "UTC"].contains(zone) {
        return None;
    }
    let months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let month = months.iter().position(|name| name == month)? as i64 + 1;
    let clock: Vec<i64> = time.split(':').map(|part| part.parse().ok()).collect::<Option<Vec<i64>>>()?;
    let [hour, minute, second] = clock.as_slice() else { return None };
    let days = crate::time::days_from_civil(year.parse().ok()?, month, day.parse().ok()?);
    Some((days * 86400 + hour * 3600 + minute * 60 + second) as f64)
}

fn query_usage(token: &str) -> Result<Object, OAuthError> {
    let request = Request {
        headers: vec![
            ("Authorization", format!("Bearer {token}")),
            ("anthropic-beta", "oauth-2025-04-20".into()),
            ("Accept", "application/json".into()),
            ("User-Agent", "Starmeter".into()),
        ],
        timeout: 12.0,
        follow_redirects: false,
        limit: MAX_RESPONSE + 1,
        ..Request::default()
    };
    let response =
        http::send(USAGE_URL, request).map_err(|_| oauth_error("network", "Claude 用量连接失败，稍后重试"))?;
    match response.status {
        200..=299 => {}
        401 => return Err(oauth_error("expired", "Claude Code 登录失效，请刷新登录")),
        403 => return Err(oauth_error("forbidden", "用量接口拒绝访问，请检查 Claude Code 登录")),
        429 => {
            return Err(OAuthError {
                code: "limited",
                description: "请求频繁，稍后自动重试".into(),
                retry: retry_seconds(response.header("Retry-After"), now()),
            });
        }
        300..=399 => return Err(oauth_error("redirect", "用量接口发生跳转，已停止查询")),
        _ => return Err(oauth_error("service", "Claude 用量服务暂不可用")),
    }
    if !response.header("Content-Type").unwrap_or("").contains("application/json") {
        return Err(oauth_error("format", "用量接口返回格式异常"));
    }
    if response.body.len() > MAX_RESPONSE {
        return Err(oauth_error("format", "用量响应过大"));
    }
    let parsed = Json::parse(&response.body).map_err(|_| oauth_error("format", "用量格式尚未支持"))?;
    let mut result = parse_usage(&parsed, now()).map_err(|_| oauth_error("format", "用量格式尚未支持"))?;
    result.insert("source", "claude-oauth");
    result.insert("state", "已同步");
    Ok(result)
}

#[cfg(target_os = "macos")]
const RENEW_SECONDS: f64 = 30.0;

#[cfg(target_os = "macos")]
fn renewed(before: &str) -> bool {
    credentials().is_ok_and(|(_, expires, owner)| owner != before && expires > now() + 30.0)
}

/// Lets the CLI's own startup auth check rotate its token. Nothing is typed into it; it stops as soon as the login
/// changes, or after 30 seconds, and its whole process group goes with it.
#[cfg(target_os = "macos")]
fn renew_with_claude(before: Option<&str>) -> Result<(), OAuthError> {
    use std::os::unix::process::CommandExt;
    use std::time::{Duration, Instant};

    use crate::json::Format;
    use crate::shell;
    let environment: std::collections::HashMap<String, String> = std::env::vars().collect();
    let binary = shell::which("claude", &environment)
        .ok_or_else(|| oauth_error("expired", "登录已过期，请安装并登录 Claude Code"))?;
    let directory = std::env::temp_dir().join(format!("swiftbar-claude-auth-{}", crate::runtime::random_hex(32)));
    std::fs::create_dir_all(&directory)
        .map_err(|_| oauth_error("expired", "登录仍未续期，请在 Claude Code 检查登录"))?;
    let settings = Json::Object(object! {
        "disableAllHooks" => true, "remoteControlAtStartup" => false,
        "statusLine" => object! { "type" => "command", "command" => "/usr/bin/true" },
    })
    .serialized_with(Format { ensure_ascii: true, ..Format::default() });
    let (mut master, mut slave) = (0, 0);
    // SAFETY: openpty fills in two descriptors; null name, termios and winsize are allowed.
    let opened = unsafe {
        libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut())
    };
    if opened != 0 {
        let _ = std::fs::remove_dir_all(&directory);
        return Err(oauth_error("expired", "登录仍未续期，请在 Claude Code 检查登录"));
    }
    let stdio = |fd: i32| -> std::process::Stdio {
        // SAFETY: each Stdio takes its own duplicate of the terminal's slave side.
        unsafe { std::os::fd::FromRawFd::from_raw_fd(libc::dup(fd)) }
    };
    let mut command = std::process::Command::new(&binary);
    command
        .args(["--strict-mcp-config", "--tools", "", "--settings", &settings])
        .current_dir(&directory)
        .env("TERM", "xterm-256color")
        .env("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1")
        .env("DISABLE_AUTOUPDATER", "1")
        .stdin(stdio(slave))
        .stdout(stdio(slave))
        .stderr(stdio(slave))
        .process_group(0);
    let child = command.spawn();
    // SAFETY: the slave side is only needed by the child now.
    unsafe { libc::close(slave) };
    let Ok(mut child) = child else {
        unsafe { libc::close(master) };
        let _ = std::fs::remove_dir_all(&directory);
        return Err(oauth_error("expired", "登录仍未续期，请在 Claude Code 检查登录"));
    };
    // SAFETY: the master descriptor is ours; non-blocking reads drain the CLI's screen, which is never kept.
    unsafe { libc::fcntl(master, libc::F_SETFL, libc::fcntl(master, libc::F_GETFL) | libc::O_NONBLOCK) };
    let deadline = Instant::now() + Duration::from_secs_f64(RENEW_SECONDS);
    let mut check = Instant::now() + Duration::from_secs(3);
    let mut buffer = [0u8; 65536];
    while child.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
        unsafe { libc::read(master, buffer.as_mut_ptr().cast(), buffer.len()) };
        if let Some(before) = before
            && Instant::now() >= check
        {
            check += Duration::from_secs(2);
            if renewed(before) {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    if child.try_wait().ok().flatten().is_none() {
        let group = child.id() as i32;
        unsafe { libc::kill(-group, libc::SIGTERM) };
        let wait = Instant::now() + Duration::from_secs(2);
        while child.try_wait().ok().flatten().is_none() && Instant::now() < wait {
            std::thread::sleep(Duration::from_millis(50));
        }
        if child.try_wait().ok().flatten().is_none() {
            unsafe { libc::kill(-group, libc::SIGKILL) };
            let _ = child.wait();
        }
    }
    unsafe { libc::close(master) };
    let _ = std::fs::remove_dir_all(&directory);
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn renew_with_claude(_before: Option<&str>) -> Result<(), OAuthError> {
    Err(oauth_error("expired", "请在 Claude Code 中重新登录，再刷新用量"))
}

fn unavailable(message: &str) -> Object {
    object! { "source" => "claude-oauth", "windows" => Json::Array(vec![]), "state" => message, "stale" => true }
}

/// One read at a time; the server's backoff always holds, our own only until the user retries. Credentials stay
/// read only, and another login's figures are never shown.
pub fn oauth_fetch(state: &str, consent: bool, force: bool, requested_at: f64) -> (Object, Option<String>) {
    if !consent {
        return (unavailable("未启用 Claude Code 登录查询"), None);
    }
    let path = format!("{state}/claude-oauth-usage.json");
    let _ = files::make_directory(Path::new(state), 0o700);
    let _lock = platform::lock(Path::new(&format!("{state}/claude-oauth.lock")), true);
    let mut cached = read_state(&path);
    let moment = now();
    let number = |cached: &Object, key: &str| cached.get(key).and_then(Json::python_float).unwrap_or(0.0);
    let forced = force || requested_at > cached.get("checkedAt").and_then(Json::number).unwrap_or(0.0);
    // The run of passing failures so far: how many, since when, and when the last attempt was.
    let (failures, failing_since, last_attempt) =
        (number(&cached, "failures") as i64, number(&cached, "failingSince"), number(&cached, "checkedAt"));
    // Revoked keychain permission must not produce repeated background prompts.
    if cached.get("paused").is_some_and(Json::is_truthy) && !force {
        let error = py::str(Some(cached.get("error").unwrap_or(&Json::Null)));
        return (unavailable(&error), Some(error));
    }
    // An explicit retry skips our own backoff, but a server rate limit always holds.
    if moment < number(&cached, "retryAt") && (!force || cached.get("limited").is_some_and(Json::is_truthy)) {
        let payload = cached.get("payload").and_then(Json::as_object).filter(|payload| !payload.is_empty()).cloned();
        if failures > 0 && last_attempt - failing_since < QUIET {
            return (payload.unwrap_or_else(|| unavailable("待同步")), None);
        }
        let error = cached.get("error").map_or_else(|| "稍后重试".to_string(), |error| py::str(Some(error)));
        return (payload.unwrap_or_else(|| unavailable(&error)), Some(error));
    }
    let mut owner: Option<String> = None;
    let outcome = (|| -> Result<Object, OAuthError> {
        let (mut token, mut expires, first) = credentials()?;
        owner = Some(first.clone());
        if cached.get("owner") != Some(&Json::from(first.as_str())) {
            // Never another login's quota.
            cached = object! { "lastRenewAt" => cached.get("lastRenewAt").cloned().unwrap_or(Json::Int(0)) };
        }
        if cached.get("renewFailedOwner") == Some(&Json::from(first.as_str())) && !force {
            return Err(oauth_error("expired", "登录续期未完成，请在 Claude Code 检查登录"));
        }
        let renew = |cached: &mut Object, owner: &mut Option<String>| -> Result<(String, f64), OAuthError> {
            cached.insert("lastRenewAt", moment);
            cached.insert("checkedAt", moment);
            write_state(&path, cached);
            renew_with_claude(owner.as_deref())?;
            let (token, expires, next) = credentials()?;
            *owner = Some(next);
            cached.remove("payload");
            Ok((token, expires))
        };
        if expires <= moment + 30.0 {
            if moment - number(&cached, "lastRenewAt") < 300.0 {
                return Err(oauth_error("expired", "登录仍未续期，请在 Claude Code 检查登录"));
            }
            (token, expires) = renew(&mut cached, &mut owner)?;
            if expires <= now() + 30.0 {
                return Err(oauth_error("expired", "登录仍未续期，请在 Claude Code 检查登录"));
            }
        }
        let payload = cached.get("payload").and_then(Json::as_object).cloned();
        let age = moment - payload.as_ref().and_then(|p| p.get("updatedAt")).and_then(Json::number).unwrap_or(0.0);
        if let Some(payload) = payload.filter(|p| !p.is_empty())
            && 0.0 <= age
            && age < if forced { 30.0 } else { INTERVAL }
        {
            return Ok(payload);
        }
        let payload = match query_usage(&token) {
            Ok(payload) => payload,
            Err(error) if error.code == "expired" && moment - number(&cached, "lastRenewAt") >= 300.0 => {
                (token, expires) = renew(&mut cached, &mut owner)?;
                if expires <= now() {
                    return Err(error);
                }
                query_usage(&token)?
            }
            Err(error) => return Err(error),
        };
        let current_owner = owner.clone().unwrap_or_default();
        write_state(
            &path,
            &object! {
                "schema" => 1, "owner" => current_owner, "payload" => payload.clone(), "checkedAt" => now(),
                "lastRenewAt" => cached.get("lastRenewAt").cloned().unwrap_or(Json::Int(0)),
            },
        );
        Ok(payload)
    })();
    match outcome {
        Ok(payload) => (payload, None),
        Err(error) => {
            let same_owner =
                owner.as_ref().is_some_and(|owner| cached.get("owner") == Some(&Json::from(owner.as_str())));
            // A passing failure continues the run when it came about when the last one said to try again, and the
            // run stays quiet for its first three minutes, unless the user asked for this read.
            let passing = passing(error.code);
            let run_on = passing && same_owner && failures > 0 && moment - last_attempt <= RUN_GAP;
            let failures = match (passing, run_on) {
                (false, _) => 0,
                (true, true) => failures + 1,
                (true, false) => 1,
            };
            let since = if run_on { failing_since } else { moment };
            let quiet = passing && !force && moment - since < QUIET;
            let payload = if same_owner { cached.get("payload").and_then(Json::as_object).cloned() } else { None }
                .filter(|kept| !kept.is_empty())
                .map(|mut kept| {
                    if !quiet {
                        kept.insert("stale", true);
                        kept.insert("state", "刷新失败");
                    }
                    kept
                });
            let paused = ["keychain", "missing", "scope", "credential", "profile"].contains(&error.code);
            let renew_failed =
                if error.code == "expired" { owner.clone().map(Json::from).unwrap_or(Json::Null) } else { Json::Null };
            let retry = if passing { passing_retry(failures) } else { error.retry };
            let mut record = object! {
                "schema" => 1, "owner" => owner.clone(), "payload" => payload.clone(), "checkedAt" => moment,
                "lastRenewAt" => cached.get("lastRenewAt").cloned().unwrap_or(Json::Int(0)), "error" => error.description.as_str(),
                "paused" => paused, "renewFailedOwner" => renew_failed, "limited" => error.code == "limited",
                "retryAt" => if paused { Json::Int(0) } else { Json::Double(now() + retry) },
            };
            if passing {
                record.insert("failures", failures);
                record.insert("failingSince", since);
            }
            write_state(&path, &record);
            if quiet {
                return (payload.unwrap_or_else(|| unavailable("待同步")), None);
            }
            (payload.unwrap_or_else(|| unavailable(&error.description)), Some(error.description))
        }
    }
}

impl Runtime {
    pub fn invalidate_caches(&self, names: &[&str]) {
        for name in names {
            let mut record = self.read_cache(name);
            record.insert("force", true);
            record.insert("retryAt", 0);
            record.insert("schema", 1);
            self.write_json_cache(&self.cache_path(name), &Json::Object(record));
        }
    }

    /// What the Claude subscription depends on: its sources' settings and the files they read.
    pub fn claude_subscription_connection(&self) -> Vec<Json> {
        let settings = self.settings();
        let root = self.source_path("claude");
        let mut parts = vec![
            Json::from(root.as_str()),
            settings.get("claudeLiveUsage").cloned().unwrap_or(Json::Bool(false)),
            settings.get("claudeOAuthUsage").cloned().unwrap_or(Json::Bool(false)),
            Json::from(self.environment.get("CLAUDE_CONFIG_DIR").cloned().unwrap_or_default()),
            Json::from("oauth-v1"),
        ];
        let snapshot = format!("{}/{SNAPSHOT_FILE}", self.state_dir);
        for file in ["plan-usage-history.json", "config.json", snapshot.as_str()] {
            let path = if is_absolute(file) { file.to_string() } else { format!("{root}/{file}") };
            match state_of(Path::new(&path)) {
                Some(state) => parts.extend([Json::Int(state.mtime), Json::Int(state.size)]),
                None => parts.push("unavailable".into()),
            }
        }
        parts
    }

    /// A retry the user asked for (立即刷新用量) that no result has acknowledged yet.
    pub fn claude_retry_pending(&self) -> f64 {
        if !self.settings().get("claudeOAuthUsage").is_some_and(Json::is_truthy) {
            return 0.0;
        }
        let requested = self.read_cache("claude-retry").get("requestedAt").and_then(Json::python_float).unwrap_or(0.0);
        let handled = self
            .read_cache("claude_subscription")
            .get("payload")
            .and_then(|payload| payload.get("handledRetryAt"))
            .and_then(Json::python_float)
            .unwrap_or(0.0);
        if requested > handled { requested } else { 0.0 }
    }

    pub fn claude_subscription_load(&self) -> Loaded {
        let settings = self.settings();
        // Code OAuth is independent of browser cookies and does not invoke a model.
        if settings.get("claudeOAuthUsage").is_some_and(Json::is_truthy) {
            let requested = self
                .read_cache("resource-checks")
                .get("claude-subscription")
                .and_then(Json::python_float)
                .unwrap_or(0.0);
            // 立即刷新用量 is an explicit retry: renew once even after a failed renewal for this login.
            let retry = self.claude_retry_pending();
            let (mut payload, error) = oauth_fetch(&self.state_dir, true, retry != 0.0, requested.max(retry));
            let handled = if retry != 0.0 {
                Json::Double(retry)
            } else {
                self.read_cache("claude_subscription")
                    .get("payload")
                    .and_then(|payload| payload.get("handledRetryAt"))
                    .cloned()
                    .unwrap_or(Json::Int(0))
            };
            if handled.is_truthy() {
                payload.insert("handledRetryAt", handled);
            }
            if let Some(error) = error {
                payload.insert("fetchError", error);
            }
            // Publish errors as their own state; don't reuse a different App account.
            return (Some(payload), None);
        }
        let mut app = desktop(&self.source_path("claude"), now());
        app.insert("source", "app-snapshot");
        let stale = claude_usage_stale(&app);
        app.insert("stale", stale);
        let code = if local_enabled(&self.state_dir) { local_snapshot(&self.state_dir, now()) } else { None };
        let mut candidates: Vec<Object> = [Some(app.clone()), code.clone()]
            .into_iter()
            .flatten()
            .filter(|item| item.get("windows").is_some_and(Json::is_truthy))
            .collect();
        let manual = self.read_cache("claude_subscription_manual");
        if settings.get("claudeLiveUsage").is_some_and(Json::is_truthy)
            && manual.get("root") == Some(&Json::from(self.source_path("claude")))
            && let Some(mut payload) = manual.get("payload").and_then(Json::as_object).cloned()
            && payload.get("windows").is_some_and(Json::is_truthy)
        {
            let stale = claude_usage_stale(&payload);
            payload.insert("stale", stale);
            candidates.push(payload);
        }
        let newest = |items: Vec<&Object>| -> Option<Object> {
            let mut best: Option<&Object> = None;
            for item in items {
                let stamp = |item: &Object| item.get("updatedAt").and_then(Json::python_float).unwrap_or(0.0);
                if best.is_none_or(|best| stamp(item) > stamp(best)) {
                    best = Some(item);
                }
            }
            best.cloned()
        };
        if let Some(fresh) =
            newest(candidates.iter().filter(|item| !item.get("stale").is_some_and(Json::is_truthy)).collect())
        {
            return (Some(fresh), None);
        }
        if let Some(any) = newest(candidates.iter().collect()) {
            return (Some(any), None);
        }
        (Some(code.unwrap_or(app)), None)
    }

    pub fn claude_subscription_cached(&self) -> Loaded {
        // An older request can finish after the user clicks retry: keep it pending until a result acknowledges it.
        if self.claude_retry_pending() != 0.0 {
            self.invalidate_caches(&["claude_subscription"]);
        }
        self.cached_fetch("claude_subscription", 60.0, 60.0, || Ok(self.claude_subscription_load()))
    }
}

/// Too old to show as current: marked stale, older than ten minutes, or past a window's reset.
pub fn claude_usage_stale(payload: &Object) -> bool {
    let moment = now();
    if payload.get("stale").is_none_or(Json::is_truthy)
        || moment - payload.get("updatedAt").and_then(Json::python_float).unwrap_or(0.0) > 600.0
    {
        return true;
    }
    for row in payload.get("windows").and_then(Json::as_array).into_iter().flatten() {
        let Some(reset) = row.get("resetsAt").filter(|reset| reset.is_truthy()) else { continue };
        let future = reset
            .as_str()
            .and_then(|text| IsoTime::parse(text).ok())
            .is_some_and(|time| time.timestamp(&TimeZone::system()) > moment);
        if !future {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with(record: Object) -> String {
        let state = std::env::temp_dir().join(format!("starmeter-oauth-{}", crate::runtime::random_hex(16)));
        std::fs::create_dir_all(&state).unwrap();
        let state = state.to_string_lossy().into_owned();
        write_state(&format!("{state}/claude-oauth-usage.json"), &record);
        state
    }

    #[test]
    fn passing_failures_try_again_soon_then_less_often() {
        let delays: Vec<f64> = (1..=8).map(passing_retry).collect();
        assert_eq!(delays, vec![15.0, 30.0, 60.0, 120.0, 240.0, 300.0, 300.0, 300.0]);
        assert!(passing("network") && passing("service") && !passing("limited") && !passing("expired"));
    }

    #[test]
    fn waking_forgets_a_run_of_passing_failures_and_its_backoff() {
        let later = now() + 200.0;
        let state = state_with(object! {
            "schema" => 1, "owner" => "a", "error" => "Claude 用量连接失败，稍后重试", "checkedAt" => now(),
            "failures" => 4, "failingSince" => now() - 500.0, "retryAt" => later, "limited" => false,
        });
        forget_passing_failures(&state);
        let kept = read_state(&format!("{state}/claude-oauth-usage.json"));
        assert_eq!(kept.get("retryAt"), Some(&Json::Int(0)));
        assert!(kept.get("failures").is_none() && kept.get("failingSince").is_none());
        assert_eq!(kept.get("owner"), Some(&Json::from("a")));
        let _ = std::fs::remove_dir_all(&state);
    }

    #[test]
    fn waking_leaves_a_rate_limit_and_other_failures_alone() {
        let later = now() + 900.0;
        for record in [
            object! { "schema" => 1, "error" => "请求频繁，稍后自动重试", "limited" => true, "retryAt" => later },
            object! { "schema" => 1, "error" => "登录仍未续期", "renewFailedOwner" => "a", "retryAt" => later },
        ] {
            let state = state_with(record.clone());
            forget_passing_failures(&state);
            assert_eq!(read_state(&format!("{state}/claude-oauth-usage.json")), record);
            let _ = std::fs::remove_dir_all(&state);
        }
    }
}
