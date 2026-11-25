//! OpenCodex's management API (health, readiness, accounts, quota, its usage and timeline), Komari's public API,
//! and DeepSeek's balance through OpenCodex or directly.

use indexmap::IndexMap;

use crate::claude_app;
use crate::files::paths;
use crate::http::{self, Request, Unreachable};
use crate::json::{Format, Json, Object};
use crate::py::{self, FileError};
use crate::runtime::{HOURLY_RANGES, Loaded, Runtime, SHORT_RANGES, Thrown};
use crate::tally::add;
use crate::{object, shell};

/// An answer that was not the data: the HTTP status (0 when there was no answer), what went wrong, and the body.
#[derive(Clone, Debug)]
pub struct RequestError {
    pub code: u16,
    pub message: String,
    pub body: Option<Json>,
}

pub const ENDPOINTS: [(&str, &str, bool); 5] = [
    ("healthz", "/healthz", false),
    ("ready", "/readyz", false),
    ("quota", "/api/codex-auth/quota", true),
    ("accounts", "/api/codex-auth/accounts", true),
    ("active", "/api/codex-auth/active", true),
];

fn reason(status: u16) -> Option<&'static str> {
    Some(match status {
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        409 => "Conflict",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => return None,
    })
}

fn ascii(value: &Json) -> Vec<u8> {
    value.serialized_with(Format { ensure_ascii: true, ..Format::default() }).into_bytes()
}

impl Runtime {
    pub fn opencodex_configured(&self) -> bool {
        self.env("OCX_BASE_URL").is_some()
            || std::path::Path::new(&self.token_file).exists()
            || std::path::Path::new(&format!("{}/runtime-port.json", self.ocx_home)).exists()
    }

    pub fn admin_token(&self) -> Option<String> {
        for key in ["OPENCODEX_ADMIN_AUTH_TOKEN", "OCX_ADMIN_TOKEN"] {
            if let Some(value) = self.environment.get(key).map(|value| value.trim()).filter(|value| !value.is_empty()) {
                return Some(value.to_string());
            }
        }
        let text = std::fs::read_to_string(&self.token_file).ok()?;
        let token = text.trim();
        (!token.is_empty()).then(|| token.to_string())
    }

    /// OpenCodex's management API: the data, or what went wrong. The admin token only travels to this machine or
    /// over HTTPS.
    pub fn opencodex_request(
        &self,
        path: &str,
        method: &str,
        body: Option<&Json>,
        timeout: f64,
        auth: bool,
    ) -> (Option<Object>, Option<RequestError>) {
        let mut headers = vec![("Accept", "application/json".to_string())];
        if body.is_some() {
            headers.push(("Content-Type", "application/json".to_string()));
        }
        let local = self.base_is_local();
        if auth {
            let https = reqwest::Url::parse(&self.base).is_ok_and(|url| url.scheme() == "https");
            if !local && !https {
                return (
                    None,
                    Some(RequestError {
                        code: 0, message: "管理令牌只发送到本机或 HTTPS 地址".into(), body: None
                    }),
                );
            }
            if let Some(token) = self.admin_token() {
                headers.push(("x-opencodex-api-key", token));
            }
        }
        // Never followed: a redirect to another host would carry the admin token with it (reqwest drops only the
        // standard credential headers on the way, not x-opencodex-api-key).
        let request = Request {
            method,
            headers,
            body: body.map(ascii),
            timeout,
            direct: local,
            follow_redirects: false,
            ..Request::default()
        };
        let response = match http::send(&format!("{}{path}", self.base), request) {
            Ok(response) => response,
            Err(Unreachable(message)) => return (None, Some(RequestError { code: 0, message, body: None })),
        };
        if !(200..300).contains(&response.status) {
            let mut parsed = None;
            let mut message = None;
            if let Ok(value) = Json::parse(&response.body) {
                if let Some(object) = value.as_object() {
                    let found = py::or(object.get("error"), object.get("message").cloned().unwrap_or(Json::Null));
                    if let Some(inner) = found.as_object() {
                        let text = py::or(inner.get("message"), Json::from(Json::Object(inner.clone()).serialized()));
                        message = Some(py::str(Some(&text)));
                    } else if found.is_truthy() {
                        message = Some(py::str(Some(&found)));
                    }
                }
                parsed = Some(value);
            }
            let message = message
                .or_else(|| reason(response.status).map(String::from))
                .unwrap_or_else(|| format!("HTTP {}", response.status));
            return (None, Some(RequestError { code: response.status, message, body: parsed }));
        }
        if response.body.is_empty() {
            return (Some(Object::new()), None);
        }
        let Ok(parsed) = Json::parse(&response.body) else {
            return (None, Some(RequestError { code: 0, message: "JSONDecodeError".into(), body: None }));
        };
        if let Some(error) = parsed.get("error").filter(|error| error.is_truthy()) {
            let message = py::str(Some(error));
            return (None, Some(RequestError { code: response.status, message, body: Some(parsed) }));
        }
        // A list or a bare value is no error, just not data: the cache calls it 数据格式异常.
        (parsed.as_object().cloned(), None)
    }

    fn opencodex_endpoint(&self, name: &str) -> Loaded {
        let Some((_, path, auth)) = ENDPOINTS.iter().find(|(key, _, _)| *key == name) else {
            return (None, Some("unknown endpoint".into()));
        };
        let (value, error) =
            self.opencodex_request(path, "GET", None, if name == "healthz" { 1.5 } else { 3.0 }, *auth);
        // /readyz answers 503 while starting, with the real state in its body.
        if name == "ready"
            && let Some(body) = error.as_ref().and_then(|error| error.body.as_ref()).and_then(Json::as_object)
        {
            return (Some(body.clone()), None);
        }
        (value, error.map(|error| error.message))
    }

    /// The health check first; while it fails, the other endpoints wait for the next one.
    pub fn opencodex_refresh(&self) {
        let ttl = self.refresh_ttl(30.0);
        let (health, error) = self.cached_fetch("healthz", ttl, 60.0, || Ok(self.opencodex_endpoint("healthz")));
        if error.is_some() || health.is_none() {
            return;
        }
        std::thread::scope(|scope| {
            for (name, _, _) in ENDPOINTS.iter().filter(|(name, _, _)| *name != "healthz") {
                scope.spawn(move || self.cached_fetch(name, ttl, 60.0, || Ok(self.opencodex_endpoint(name))));
            }
        });
    }

    fn usage_window(&self, start: f64, end: f64) -> Result<Object, FileError> {
        let query =
            format!("range=all&since={}&until={}", (start * 1000.0).trunc() as i64, (end * 1000.0).trunc() as i64);
        let (value, error) = self.opencodex_request(&format!("/api/usage?{query}"), "GET", None, 12.0, true);
        match (value, error) {
            (Some(value), None) if !value.get("error").is_some_and(Json::is_truthy) => Ok(value),
            _ => Err(FileError::new("OpenCodex 用量读取失败")),
        }
    }

    fn timeline(&self, hours: i64, minutes: i64) -> Option<Object> {
        let path = format!("/api/usage/timeline?hours={hours}&bucketMinutes={minutes}&metric=total&aggregation=sum");
        let (value, error) = self.opencodex_request(&path, "GET", None, 12.0, true);
        value.filter(|value| error.is_none() && !value.get("truncated").is_some_and(Json::is_truthy))
    }

    /// OpenCodex's own figures for a range, with its timeline for the hourly charts and heatmap.
    pub fn opencodex_range_usage_live(&self, key: &str, now: f64) -> Object {
        if !self.opencodex_configured() {
            return object! { "error" => "未配置 OpenCodex" };
        }
        self.opencodex_range(key, now).unwrap_or_else(|_| object! { "error" => "记录暂不可用" })
    }

    fn opencodex_range(&self, key: &str, now: f64) -> Result<Object, FileError> {
        let zone = self.local_zone();
        let (start, end) = self.range_bounds(key, now, &zone);
        let mut payload = self.usage_window(start, end)?;
        let points = |line: &Object, body: &mut dyn FnMut(f64, f64)| {
            let begin = line.get("start").and_then(Json::number).unwrap_or(0.0);
            let step = line.get("bucketSeconds").and_then(Json::number).unwrap_or(0.0);
            for series in line.get("series").and_then(Json::as_array).into_iter().flatten() {
                let values = series.get("points").and_then(Json::as_array).cloned().unwrap_or_default();
                for (index, value) in values.iter().enumerate() {
                    body(begin + index as f64 * step, py::or(Some(value), Json::Int(0)).python_float().unwrap_or(0.0));
                }
            }
        };
        if SHORT_RANGES.contains(&key) {
            // The timeline counts back from now, so 48 hours always reach yesterday's midnight.
            let Some(line) =
                self.timeline(if key == "yesterday" { 48 } else { 24 }, if key == "today" { 1 } else { 60 })
            else {
                payload.insert("chartUnavailable", true);
                return Ok(payload);
            };
            let begin = (start / 3600.0).floor() as i64 * 3600;
            let last = (end / 3600.0).floor() as i64 * 3600;
            let mut buckets: IndexMap<i64, Json> = IndexMap::new();
            let mut stamp = begin;
            while stamp <= last {
                buckets.insert(stamp, Json::Int(0));
                stamp += 3600;
            }
            let mut activity: Vec<f64> = Vec::new();
            points(&line, &mut |stamp, value| {
                let bucket = (stamp / 3600.0).floor() as i64 * 3600;
                if start <= stamp && stamp <= end && value > 0.0 {
                    activity.push(stamp);
                }
                if let Some(current) = buckets.get(&bucket) {
                    let sum = add(current, &Json::Double(value));
                    buckets.insert(bucket, sum);
                }
            });
            // Timeline is hour-aligned. Query both partial edges with the exact same bounds as the totals.
            let mut edges = vec![begin, last];
            edges.sort();
            edges.dedup();
            let line_start = line.get("start").and_then(Json::number).unwrap_or(0.0);
            for stamp in edges {
                let low = start.max(stamp as f64);
                let high = end.min(stamp as f64 + 3600.0 - 0.001);
                if low != stamp as f64 || stamp == last || (stamp as f64) < line_start {
                    let total = self
                        .usage_window(low, high)?
                        .get("summary")
                        .and_then(|summary| summary.get("totalTokens"))
                        .cloned()
                        .unwrap_or(Json::Null);
                    buckets.insert(stamp, total);
                }
            }
            buckets.sort_keys();
            let any = buckets.values().any(Json::is_truthy);
            payload.insert(
                "hours",
                Json::Array(
                    buckets
                        .into_iter()
                        .map(|(stamp, tokens)| object! { "timestamp" => stamp, "totalTokens" => tokens }.into())
                        .collect(),
                ),
            );
            if key == "today" && any {
                let minute = (end / 60.0).floor() as i64 * 60;
                let first =
                    activity.iter().copied().reduce(f64::min).map(Json::Double).unwrap_or_else(|| {
                        if start >= minute as f64 { Json::Double(start) } else { Json::Int(minute) }
                    });
                payload.insert("firstActivityAt", first);
            }
            payload.insert("chartIncomplete", line.get("missingMeasurements").is_some_and(Json::is_truthy));
        } else if HOURLY_RANGES.contains(&key)
            && let Some(line) = self.timeline(168, 60)
        {
            // The chart goes by day here; these hours only fill the heatmap.
            let mut buckets: IndexMap<i64, f64> = IndexMap::new();
            points(&line, &mut |stamp, value| {
                if start <= stamp && stamp <= end {
                    *buckets.entry((stamp / 3600.0).floor() as i64 * 3600).or_default() += value;
                }
            });
            buckets.sort_keys();
            payload.insert(
                "hours",
                Json::Array(
                    buckets
                        .into_iter()
                        .map(|(stamp, tokens)| object! { "timestamp" => stamp, "totalTokens" => tokens }.into())
                        .collect(),
                ),
            );
        }
        Ok(payload)
    }

    // Komari

    pub fn komari_base_url(&self) -> String {
        monitor_url(self.environment.get("KOMARI_BASE_URL").map(String::as_str).unwrap_or(""))
    }

    /// A public Komari endpoint through the system curl, which keeps the system's TLS and proxy settings.
    fn komari_public_json(&self, path: &str, body: Option<&Json>) -> Loaded {
        let curl = if cfg!(windows) {
            shell::which("curl", &self.environment).unwrap_or_else(|| {
                format!(
                    "{}\\System32\\curl.exe",
                    self.environment.get("SystemRoot").cloned().unwrap_or_else(|| "C:\\Windows".into())
                )
            })
        } else {
            "/usr/bin/curl".to_string()
        };
        let url = format!("{}{path}", self.komari_base_url());
        let mut command: Vec<String> = [
            curl.as_str(),
            "--disable",
            "--silent",
            "--show-error",
            "--fail",
            "--connect-timeout",
            "2",
            "--max-time",
            "5",
            "--max-filesize",
            "4194304",
            "--header",
            "Accept: application/json",
            url.as_str(),
        ]
        .iter()
        .map(|part| part.to_string())
        .collect();
        if let Some(body) = body {
            command.extend(["--header".into(), "Content-Type: application/json".into(), "--data-binary".into()]);
            command.push(String::from_utf8_lossy(&ascii(body)).into_owned());
        }
        let arguments: Vec<&str> = command.iter().map(String::as_str).collect();
        let Some(output) = shell::run_data(&arguments, 6.0, None, None) else {
            return (None, Some("Komari 连接失败".into()));
        };
        let Some(payload) = Json::parse(&output).ok().and_then(|value| value.as_object().cloned()) else {
            return (None, Some("Komari 请求超时或数据异常".into()));
        };
        if payload.get("error").is_some_and(Json::is_truthy) || payload.get("status") == Some(&Json::from("error")) {
            return (None, Some("Komari 接口暂不可用".into()));
        }
        (Some(payload), None)
    }

    fn komari_snapshot_live(&self) -> Result<Object, Thrown> {
        let rpc = Json::Object(object! {
            "jsonrpc" => "2.0", "method" => "common:getNodesLatestStatus", "params" => Object::new(), "id" => 1,
        });
        let (nodes, latest) = std::thread::scope(|scope| {
            let nodes = scope.spawn(|| self.komari_public_json("/api/nodes", None));
            let latest = scope.spawn(|| self.komari_public_json("/api/rpc2", Some(&rpc)));
            (nodes.join().unwrap_or((None, Some(String::new()))), latest.join().unwrap_or((None, Some(String::new()))))
        });
        if nodes.1.is_some() {
            return Err(FileError::new("Komari 节点读取失败").into());
        }
        let rows = nodes.0.as_ref().and_then(|nodes| nodes.get("data")).and_then(Json::as_array).cloned();
        let Some(rows) = rows else { return Err(FileError::new("Komari 节点格式异常").into()) };
        let states = latest.0.as_ref().and_then(|latest| latest.get("result")).and_then(Json::as_object).cloned();
        let status_valid = latest.1.is_none() && states.is_some();
        let mut servers = Vec::new();
        for row in &rows {
            let Some(fields) = row.as_object() else { continue };
            if !fields.get("uuid").is_some_and(Json::is_truthy) || fields.get("hidden").is_some_and(Json::is_truthy) {
                continue;
            }
            let state = if status_valid {
                fields
                    .get("uuid")
                    .and_then(Json::as_str)
                    .and_then(|uuid| states.as_ref().and_then(|states| states.get(uuid)))
                    .and_then(Json::as_object)
                    .cloned()
            } else {
                None
            }
            .unwrap_or_default();
            let mut server = Object::new();
            for key in ["uuid", "name", "weight", "expired_at"] {
                server.insert(key, fields.get(key).cloned().unwrap_or(Json::Null));
            }
            for key in ["online", "cpu", "ram", "ram_total", "time"] {
                server.insert(key, state.get(key).cloned().unwrap_or(Json::Null));
            }
            // An omitted online flag is unknown, never silently offline.
            if server.get("online").and_then(Json::as_bool).is_none() {
                server.insert("online", Json::Null);
            }
            servers.push(Json::Object(server));
        }
        Ok(object! { "servers" => Json::Array(servers), "statusAvailable" => status_valid })
    }

    pub fn komari_snapshot(&self) -> Loaded {
        self.cached_fetch("komari", 30.0, 60.0, || {
            if self.komari_base_url().is_empty() {
                return Ok((
                    Some(
                        object! { "servers" => Json::Array(vec![]), "statusAvailable" => false, "configured" => false },
                    ),
                    None,
                ));
            }
            Ok((Some(self.komari_snapshot_live()?), None))
        })
    }

    // DeepSeek through OpenCodex

    /// OpenCodex drops negative balances; when it has none to show, DeepSeek is asked directly and a negative kept.
    /// The balance asked of DeepSeek itself; Err says why not: no key set up to ask with, or the asking failed (said
    /// as such, not as 未配置).
    fn deepseek_balance_direct(&self) -> Result<Object, String> {
        let unset = || "未配置 DeepSeek API 凭据".to_string();
        let failed = |why: String| format!("DeepSeek 余额查询失败{why}");
        let mut key = self.environment.get("DEEPSEEK_API_KEY").map(|key| key.trim().to_string()).unwrap_or_default();
        if key.is_empty()
            && let Some(file) = self.env("DEEPSEEK_API_KEY_FILE")
        {
            key = std::fs::read_to_string(paths::expand_user(file)).map_err(|_| unset())?.trim().to_string();
        }
        let provider = if !key.is_empty() {
            object! { "baseUrl" => "https://api.deepseek.com", "apiKey" => key }
        } else {
            let config = claude_app::read_object(&format!("{}/config.json", self.ocx_home));
            if config.is_empty() && !std::path::Path::new(&format!("{}/config.json", self.ocx_home)).exists() {
                return Err(unset());
            }
            py::or(config.get("providers"), Json::Object(Object::new()))
                .get("deepseek")
                .filter(|provider| provider.is_truthy())
                .and_then(Json::as_object)
                .cloned()
                .unwrap_or_default()
        };
        let base = py::str(Some(&py::or(provider.get("baseUrl"), Json::from("")))).trim_end_matches('/').to_string();
        if !["https://api.deepseek.com", "https://api.deepseek.com/v1"].contains(&base.as_str()) {
            return Err(unset());
        }
        let secret = py::str(Some(&py::or(provider.get("apiKey"), Json::from("")))).trim().to_string();
        if secret.is_empty() {
            return Err(unset());
        }
        let request = Request {
            headers: vec![("Accept", "application/json".into()), ("Authorization", format!("Bearer {secret}"))],
            timeout: 4.0,
            follow_redirects: false,
            ..Request::default()
        };
        let response =
            http::send("https://api.deepseek.com/user/balance", request).map_err(|_| failed(String::new()))?;
        if !(200..300).contains(&response.status) {
            return Err(failed(format!("（HTTP {}）", response.status)));
        }
        Json::parse(&response.body).ok().and_then(|body| balance_report(&body)).ok_or_else(|| failed(String::new()))
    }

    pub fn provider_quotas(&self) -> Loaded {
        self.cached_fetch("provider_quotas", 300.0, 60.0, || {
            if self.env("DEEPSEEK_API_KEY").is_some()
                || self.env("DEEPSEEK_API_KEY_FILE").is_some()
                || !self.opencodex_configured()
            {
                return Ok(match self.deepseek_balance_direct() {
                    Ok(direct) => (Some(direct), None),
                    Err(why) => (None, Some(why)),
                });
            }
            let (payload, error) = self.opencodex_request("/api/provider-quotas", "GET", None, 4.0, true);
            let mut reports: Vec<Object> = payload
                .as_ref()
                .and_then(|payload| payload.get("reports"))
                .and_then(Json::as_array)
                .map(|rows| rows.iter().filter_map(|row| row.as_object().cloned()).collect())
                .unwrap_or_default();
            let mut failure = error.map(|error| error.message);
            let is_deepseek = |row: &Object| {
                py::str(Some(row.get("provider").unwrap_or(&Json::from("")))).to_lowercase() == "deepseek"
            };
            let existing = reports.iter().find(|row| is_deepseek(row)).cloned();
            let value = report_balance(existing.as_ref()).map(|(_, value)| value);
            // Only call DeepSeek when the proxy has no usable balance, including a negative one.
            if (value.is_none() || value == Some(0.0))
                && let Ok(direct) = self.deepseek_balance_direct()
            {
                reports.retain(|row| !is_deepseek(row));
                reports.extend(
                    direct
                        .get("reports")
                        .and_then(Json::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(|row| row.as_object().cloned()),
                );
                failure = None;
            }
            if let Some(failure) = failure {
                return Ok((None, Some(failure)));
            }
            if payload.is_none() && reports.is_empty() {
                return Ok((None, Some("余额暂不可用".into())));
            }
            // Keep only display fields; no API credentials or provider configuration are cached.
            let kept: Vec<Json> = reports
                .iter()
                .map(|row| {
                    let mut kept = Object::new();
                    for key in ["provider", "label", "source", "quota"] {
                        if let Some(value) = row.get(key) {
                            kept.insert(key, value.clone());
                        }
                    }
                    Json::Object(kept)
                })
                .collect();
            Ok((Some(object! { "reports" => Json::Array(kept) }), None))
        })
    }
}

/// A Komari address the user set, or "" for anything that isn't a plain http(s) address (examples included).
pub fn monitor_url(value: &str) -> String {
    let text = value.trim().trim_end_matches('/').to_string();
    let Ok(url) = reqwest::Url::parse(&text) else { return String::new() };
    let host = url.host_str().unwrap_or("").to_lowercase();
    let plain = ["http", "https"].contains(&url.scheme())
        && !host.is_empty()
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && !host.ends_with(".example")
        && !["example.com", "example.org", "your-komari.example"].contains(&host.as_str());
    if plain { text } else { String::new() }
}

/// DeepSeek's /user/balance answer as a provider report: the account's balance in its currency, negative when it owes.
fn balance_report(body: &Json) -> Option<Object> {
    let rows: Vec<Object> =
        body.get("balance_infos")?.as_array()?.iter().filter_map(|row| row.as_object().cloned()).collect();
    let balance = |row: &Object| -> Option<f64> {
        let mut values: IndexMap<&str, f64> = IndexMap::new();
        for key in ["total_balance", "granted_balance", "topped_up_balance"] {
            // A negative total is kept: an account in debt says so (OpenCodex drops it, which is why this asks).
            if let Some(value) = row.get(key).and_then(Json::python_float).filter(|value| value.is_finite()) {
                values.insert(key, value);
            }
        }
        // total_balance first; when only the split fields are filled, the largest one actually available; else the
        // total as it is, owed money included.
        if values.get("total_balance").copied().unwrap_or(0.0) > 0.0 {
            return values.get("total_balance").copied();
        }
        if let Some(largest) = values.values().copied().filter(|value| *value > 0.0).reduce(f64::max) {
            return Some(largest);
        }
        values.get("total_balance").copied()
    };
    let funded: Vec<usize> = (0..rows.len()).filter(|&index| balance(&rows[index]).unwrap_or(0.0) > 0.0).collect();
    let currency_of = |index: usize| py::str(Some(rows[index].get("currency").unwrap_or(&Json::Null))).to_uppercase();
    let usd = (0..rows.len()).find(|&index| currency_of(index) == "USD");
    let cny = (0..rows.len()).find(|&index| currency_of(index) == "CNY");
    // USD with money in it first; a CNY account skips an empty USD row.
    let preferred = usd
        .filter(|index| funded.contains(index))
        .or_else(|| funded.first().copied())
        .or(usd)
        .or(cny)
        .or(if rows.is_empty() { None } else { Some(0) })?;
    let amount = balance(&rows[preferred])?;
    let currency = py::str(Some(&py::or(rows[preferred].get("currency"), Json::from("")))).to_uppercase();
    let symbol = match currency.as_str() {
        "CNY" => "¥".to_string(),
        "USD" => "$".to_string(),
        "EUR" => "€".to_string(),
        "GBP" => "£".to_string(),
        _ => format!("{currency} "),
    };
    let now = (crate::runtime::now() * 1000.0) as i64;
    let window = object! { "label" => format!("API balance ({symbol}{amount:.2})"), "percent" => 0 };
    let report = object! {
        "provider" => "deepseek", "label" => "DeepSeek", "source" => "swiftbar:direct-balance",
        "quota" => object! { "customWindows" => Json::Array(vec![window.into()]), "updatedAt" => now },
    };
    Some(object! { "generatedAt" => now, "reports" => Json::Array(vec![report.into()]) })
}

/// The first currency sign and amount in a report's labels, as the Python regex finds them.
pub fn report_balance(report: Option<&Object>) -> Option<(String, f64)> {
    let quota = py::dict(report.and_then(|report| report.get("quota")));
    let windows = py::or(quota.get("customWindows"), Json::Array(vec![]));
    for window in windows.as_array().into_iter().flatten() {
        let label: Vec<char> =
            py::str(Some(&py::or(py::dict(Some(window)).get("label"), Json::from("")))).chars().collect();
        for (index, character) in label.iter().enumerate() {
            if !"¥$€£".contains(*character) {
                continue;
            }
            let mut at = index + 1;
            while at < label.len() && label[at].is_whitespace() {
                at += 1;
            }
            let mut text = String::new();
            if at < label.len() && label[at] == '-' {
                text.push('-');
                at += 1;
            }
            if !(at < label.len() && label[at].is_ascii_digit()) {
                continue;
            }
            while at < label.len() && (label[at].is_ascii_digit() || label[at] == ',') {
                if label[at] != ',' {
                    text.push(label[at]);
                }
                at += 1;
            }
            if at + 1 < label.len() && label[at] == '.' && label[at + 1].is_ascii_digit() {
                text.push('.');
                at += 1;
                while at < label.len() && label[at].is_ascii_digit() {
                    text.push(label[at]);
                    at += 1;
                }
            }
            if let Ok(value) = text.parse() {
                return Some((character.to_string(), value));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn an_account_in_debt_shows_its_negative_balance() {
        let body = Json::parse(br#"{"balance_infos":[{"currency":"CNY","total_balance":"-3.20","granted_balance":"0.00","topped_up_balance":"-3.20"}]}"#).unwrap();
        let report = balance_report(&body).expect("a report");
        let first = report
            .get("reports")
            .and_then(Json::as_array)
            .and_then(|rows| rows.first())
            .and_then(Json::as_object)
            .cloned();
        assert_eq!(report_balance(first.as_ref()), Some(("¥".to_string(), -3.2)));
        // Money in the account reads as before.
        let body = Json::parse(br#"{"balance_infos":[{"currency":"USD","total_balance":"12.50"}]}"#).unwrap();
        let report = balance_report(&body).expect("a report");
        let first = report
            .get("reports")
            .and_then(Json::as_array)
            .and_then(|rows| rows.first())
            .and_then(Json::as_object)
            .cloned();
        assert_eq!(report_balance(first.as_ref()), Some(("$".to_string(), 12.5)));
    }

    #[test]
    fn a_redirect_from_opencodex_is_not_followed_with_the_admin_token() {
        // Where the redirect points: it must never be asked.
        let elsewhere = TcpListener::bind("127.0.0.1:0").unwrap();
        elsewhere.set_nonblocking(true).unwrap();
        let target = format!("http://127.0.0.1:{}/steal", elsewhere.local_addr().unwrap().port());
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://127.0.0.1:{}", server.local_addr().unwrap().port());
        let answer = std::thread::spawn(move || {
            let (mut stream, _) = server.accept().unwrap();
            let mut request = [0u8; 4096];
            let read = stream.read(&mut request).unwrap();
            let text = String::from_utf8_lossy(&request[..read]).to_lowercase();
            let reply =
                format!("HTTP/1.1 302 Found\r\nLocation: {target}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            stream.write_all(reply.as_bytes()).unwrap();
            text.contains("x-opencodex-api-key: secret-token")
        });
        let home = std::env::temp_dir().join(format!("starmeter-ocx-redirect-{}", crate::runtime::random_hex(6)));
        std::fs::create_dir_all(&home).unwrap();
        let environment: std::collections::HashMap<String, String> = [
            ("HOME", home.to_string_lossy().into_owned()),
            ("OCX_STATE_DIR", home.join("state").to_string_lossy().into_owned()),
            ("OCX_BASE_URL", base),
            ("OCX_ADMIN_TOKEN", "secret-token".to_string()),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect();
        let runtime = Runtime::new(environment, "starmeter");
        let (data, error) = runtime.opencodex_request("/api/provider-quotas", "GET", None, 4.0, true);
        assert!(answer.join().unwrap(), "the token went to OpenCodex itself");
        assert!(data.is_none());
        assert_eq!(error.map(|error| error.code), Some(302));
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(elsewhere.accept().is_err(), "the redirect was followed");
        let _ = std::fs::remove_dir_all(&home);
    }
}
