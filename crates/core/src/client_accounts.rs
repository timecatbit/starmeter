//! Accounts read from the clients themselves: Qoder CN's plan Credits from its desktop app's log, and ChatGPT's
//! rate-limit windows from Codex's official read-only App Server.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use jiff::tz::TimeZone;

use crate::accounts::numeric;
use crate::claude_usage::state_of;
use crate::harness::walk;
use crate::json::{Format, Json, Object};
use crate::time::IsoTime;
use crate::{object, py, shell};

pub mod qoder {
    use super::*;

    const MAX_AGE: f64 = 7.0 * 86400.0;
    const MAX_FILES: usize = 8;
    const SCAN_BYTES: u64 = 4 * 1024 * 1024;
    /// A far-future expiry (Qoder uses 9999-12-31) means no reset date.
    const NO_RESET: f64 = 4102444800.0;

    pub fn log_roots(home: &str, environment: &HashMap<String, String>) -> Vec<String> {
        let base = if cfg!(windows) {
            environment.get("APPDATA").cloned().unwrap_or_else(|| format!("{home}\\AppData\\Roaming"))
        } else {
            format!("{home}/Library/Application Support")
        };
        vec![format!("{base}/com.qodercn.app.stable/logs"), format!("{base}/QoderCN/logs")]
    }

    fn log_files(home: &str, environment: &HashMap<String, String>, now: f64) -> Vec<String> {
        let mut found: Vec<(f64, String)> = Vec::new();
        for root in log_roots(home, environment) {
            if !Path::new(&root).is_dir() {
                continue;
            }
            for path in walk(&root) {
                if path.rsplit('/').next() != Some("main.log") {
                    continue;
                }
                let Some(state) = state_of(Path::new(&path)) else { continue };
                let mtime = state.mtime as f64 / 1e9;
                if now - mtime <= MAX_AGE {
                    found.push((mtime, path));
                }
            }
        }
        found.sort_by(|(a_time, a), (b_time, b)| b_time.total_cmp(a_time).then(b.cmp(a)));
        found.into_iter().take(MAX_FILES).map(|(_, path)| path).collect()
    }

    fn amount(value: Option<&Json>) -> Option<f64> {
        value.filter(|v| v.as_bool().is_none()).and_then(Json::number).filter(|n| *n >= 0.0 && *n < 1e12)
    }

    /// Seconds since 1970 from seconds or milliseconds; None when unusable.
    fn epoch(value: Option<&Json>) -> Option<f64> {
        let number =
            value.filter(|v| v.as_bool().is_none()).and_then(Json::number).filter(|n| *n > 0.0 && *n < 1e15)?;
        Some(if number > 1e10 { number / 1000.0 } else { number })
    }

    /// {used,total,remaining} in Credits, or None when the block is missing or malformed.
    fn quota(value: Option<&Json>) -> Option<Json> {
        let fields = value?.as_object()?;
        let used = amount(fields.get("used"))?;
        let total = amount(fields.get("total"))?;
        let remaining = amount(fields.get("remaining")).unwrap_or((total - used).max(0.0));
        Some(object! { "used" => used, "total" => total, "remaining" => remaining }.into())
    }

    /// The Credits fields of one AccountUsage line, with the time the app logged it.
    fn summary(line: &str) -> Option<Object> {
        let rest = line.strip_prefix('[')?;
        let close = rest.find(']')?;
        if close == 0 {
            return None;
        }
        let start = line.find("{\"")?;
        let last = line.rfind('}')?;
        if start > last {
            return None;
        }
        let observed = IsoTime::parse(&rest[..close]).ok()?.timestamp(&TimeZone::system());
        let data = Json::parse_str(&line[start..=last]).ok()?;
        let usage = data.get("qoderUsage")?.as_object()?;
        let plan = quota(usage.get("userQuota"));
        let extra = quota(usage.get("addOnQuota"));
        if plan.is_none() && extra.is_none() {
            return None;
        }
        let resets = epoch(usage.get("expiresAt")).filter(|resets| *resets < NO_RESET);
        Some(object! {
            "observedAt" => observed, "plan" => plan, "addOn" => extra, "resetsAt" => resets,
            "exceeded" => usage.get("isQuotaExceeded").and_then(Json::as_bool) == Some(true),
        })
    }

    /// The newest summary across recent logs, or None. Lines are read from the end of each file.
    pub fn snapshot(home: &str, environment: &HashMap<String, String>, now: f64) -> Option<Object> {
        let mut best: Option<Object> = None;
        for path in log_files(home, environment, now) {
            let Ok(mut file) = std::fs::File::open(&path) else { continue };
            let size = file.seek(SeekFrom::End(0)).unwrap_or(0);
            let _ = file.seek(SeekFrom::Start(size.saturating_sub(SCAN_BYTES)));
            let mut data = Vec::new();
            let _ = file.read_to_end(&mut data);
            let text = String::from_utf8_lossy(&data);
            for line in text.split(['\n', '\r']).rev() {
                if !line.contains("\"qoderUsage\"") {
                    continue;
                }
                if let Some(row) = summary(line)
                    && now - row.get("observedAt").and_then(Json::number).unwrap_or(0.0) <= MAX_AGE
                {
                    let observed = row.get("observedAt").and_then(Json::number).unwrap_or(0.0);
                    if best
                        .as_ref()
                        .is_none_or(|best| observed > best.get("observedAt").and_then(Json::number).unwrap_or(0.0))
                    {
                        best = Some(row);
                    }
                    break;
                }
            }
        }
        best
    }
}

pub mod chatgpt {
    use super::*;

    /// ChatGPT's rate-limit windows from the App Server: it never refreshes or logs in, never calls a model.
    pub fn account(binary: Option<&str>, home: Option<&str>, environment: &HashMap<String, String>) -> Object {
        let Some(binary) = binary.map(String::from).or_else(|| shell::which("codex", environment)) else {
            return object! { "state" => "未安装 ChatGPT 编程客户端", "windows" => Json::Array(vec![]) };
        };
        let mut command = Command::new(&binary);
        command.arg("app-server").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        command.env_clear().envs(environment);
        if let Some(home) = home {
            command.env("CODEX_HOME", home);
        }
        let Ok(mut child) = command.spawn() else {
            return object! { "error" => "无法启动 ChatGPT 编程客户端" };
        };
        let result = talk(&mut child);
        let _ = child.kill();
        let _ = child.wait();
        result
    }

    fn talk(child: &mut std::process::Child) -> Object {
        let (Some(mut input), Some(output)) = (child.stdin.take(), child.stdout.take()) else {
            return object! { "error" => "客户端接口不可用" };
        };
        // One deadline for the whole handshake; frames are read on their own thread.
        let deadline = Instant::now() + Duration::from_secs(12);
        let (sender, receiver) = mpsc::channel::<Vec<u8>>();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(output);
            loop {
                let mut line = Vec::new();
                match reader.read_until(b'\n', &mut line) {
                    Ok(0) | Err(_) => return,
                    Ok(_) => {
                        if line.len() > 1024 * 1024 {
                            line.clear();
                        }
                        if sender.send(line).is_err() {
                            return;
                        }
                    }
                }
            }
        });
        let mut sequence: i64 = 0;
        let send = |input: &mut std::process::ChildStdin, value: Json| -> bool {
            let text = value.serialized_with(Format { ensure_ascii: true, ..Format::default() }) + "\n";
            input.write_all(text.as_bytes()).and_then(|()| input.flush()).is_ok()
        };
        let mut call = |input: &mut std::process::ChildStdin, method: &str, params: Json| -> Option<Json> {
            sequence += 1;
            let id = sequence;
            if !send(input, object! { "id" => id, "method" => method, "params" => params }.into()) {
                return None;
            }
            loop {
                let left = deadline.checked_duration_since(Instant::now())?;
                let line = receiver.recv_timeout(left).ok()?;
                let Some(response) = Json::parse(&line).ok().and_then(|value| value.as_object().cloned()) else {
                    continue;
                };
                if response.get("id") == Some(&Json::Int(id)) {
                    return Some(response.get("result").cloned().unwrap_or(Json::Null));
                }
            }
        };
        let client =
            object! { "clientInfo" => object! { "name" => "starmeter", "title" => "Starmeter", "version" => "5.0.0" } };
        if !call(&mut input, "initialize", client.into()).is_some_and(|result| !result.is_null()) {
            return object! { "error" => "客户端接口不可用" };
        }
        let notify = |input: &mut std::process::ChildStdin| {
            let text = Json::Object(object! { "method" => "initialized" })
                .serialized_with(Format { ensure_ascii: true, ..Format::default() });
            let _ = input.write_all(format!("{text}\n").as_bytes());
        };
        notify(&mut input);
        let Some(account) = call(&mut input, "account/read", object! { "refreshToken" => false }.into())
            .and_then(|v| v.as_object().cloned())
        else {
            return object! { "error" => "无法读取客户端登录状态" };
        };
        let kind = py::dict(account.get("account")).get("type").cloned();
        if kind != Some(Json::from("chatgpt")) && kind != Some(Json::from("chatgptAuthTokens")) {
            return object! { "state" => "需 ChatGPT 账户登录", "windows" => Json::Array(vec![]) };
        }
        let Some(result) =
            call(&mut input, "account/rateLimits/read", Object::new().into()).and_then(|v| v.as_object().cloned())
        else {
            return object! { "error" => "额度暂不可用" };
        };
        let mut buckets = py::or(result.get("rateLimitsByLimitId"), Json::Object(Object::new()))
            .as_object()
            .cloned()
            .unwrap_or_default();
        if buckets.is_empty()
            && let Some(limits) = result.get("rateLimits").filter(|limits| limits.is_truthy())
        {
            buckets = object! { "codex" => limits.clone() };
        }
        let mut windows: Vec<Json> = Vec::new();
        for (key, bucket) in buckets.iter() {
            let Some(fields) = bucket.as_object() else { continue };
            for slot in ["primary", "secondary"] {
                let Some(window) = fields.get(slot).and_then(Json::as_object) else { continue };
                let Some(used) = window.get("usedPercent") else { continue };
                let Some(value) = numeric(Some(used)) else { continue };
                let used_value = if used.as_bool().is_some() { Json::Int(value as i64) } else { used.clone() };
                let left = match &used_value {
                    Json::Int(number) => Json::Int(100 - number),
                    other => Json::Double(100.0 - other.number().unwrap_or(0.0)),
                };
                let capped = if left.number().unwrap_or(0.0) >= 100.0 { Json::Int(100) } else { left };
                let remaining = if capped.number().unwrap_or(0.0) <= 0.0 { Json::Int(0) } else { capped };
                windows.push(
                    object! {
                        "name" => py::str(Some(&py::or(fields.get("limitName"), Json::from(key.as_str())))), "slot" => slot,
                        "remaining" => remaining, "minutes" => py::number(window.get("windowDurationMins")),
                        "resetsAt" => py::number(window.get("resetsAt")),
                    }
                    .into(),
                );
            }
        }
        let state = if windows.is_empty() { "暂无可读取额度" } else { "已同步" };
        let plan = py::str(Some(&py::or(py::dict(account.get("account")).get("planType"), Json::from(""))));
        let mut out = object! { "state" => state, "windows" => Json::Array(windows), "plan" => plan };
        // Beside the windows (订阅额度框架): the Credits spent once they are used up, the free resets on offer, and
        // whether ordinary use has stopped.
        let snapshot = result
            .get("rateLimits")
            .filter(|limits| limits.is_truthy())
            .cloned()
            .or_else(|| buckets.iter().next().map(|(_, bucket)| bucket.clone()));
        if let Some(credits) = snapshot.as_ref().and_then(|limits| limits.get("credits")).and_then(Json::as_object) {
            let unlimited = credits.get("unlimited") == Some(&Json::Bool(true));
            if unlimited || credits.get("hasCredits") == Some(&Json::Bool(true)) {
                let balance = credits.get("balance").and_then(|balance| {
                    balance.as_str().and_then(|text| text.trim().parse::<f64>().ok()).or_else(|| balance.number())
                });
                out.insert(
                    "credits",
                    object! { "balance" => balance.map_or(Json::Null, Json::Double), "unlimited" => unlimited },
                );
            }
        }
        let offered: Vec<Option<f64>> = py::dict(result.get("rateLimitResetCredits"))
            .get("credits")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .filter(|credit| credit.get("status").and_then(Json::as_str) == Some("available"))
            .map(|credit| credit.get("expiresAt").and_then(Json::number))
            .collect();
        if !offered.is_empty() {
            let soonest = offered
                .iter()
                .flatten()
                .copied()
                .fold(None, |best: Option<f64>, at| Some(best.map_or(at, |best| best.min(at))));
            out.insert(
                "resets",
                object! { "count" => offered.len() as i64, "expiresAt" => soonest.map_or(Json::Null, Json::Double) },
            );
        }
        if result.get("ordinaryUsageAllowed") == Some(&Json::Bool(false)) {
            out.insert("blocked", true);
        }
        out
    }
}
