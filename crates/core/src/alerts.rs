//! Usage alerts, adapted from magpie's quota_alert (MIT, github.com/yetone/magpie).
//!
//! A subscription window used past the share the user chose, an API balance at or under the line the user chose,
//! and (if turned on) a window of a day or more renewing soon with little used. Each is said once; what was said is
//! kept in alerts.json. A reading that failed or is no longer current neither alerts nor clears.

use std::collections::HashSet;
use std::path::Path;

use crate::api_rows::counted_values;
use crate::claude_subscription::claude_usage_stale;
use crate::dashboard::{self, day_number};
use crate::files::{self, Failure};
use crate::json::{Json, Object};
use crate::module_rows::{chatgpt_window_label, failure, object};
use crate::rows::{claude_span, g, money_text, refresh_failed};
use crate::runtime::{Runtime, now};
use crate::time::IsoTime;
use crate::{catalog, object, py};

const SAME_RUN: f64 = 600.0;
const KEEP: f64 = 40.0 * 86400.0;
const REMINDER_UNDER: f64 = 85.0;
const REMINDER_SPAN: f64 = 86400.0;
const REMINDER_LEAD: f64 = 12.0 * 3600.0;
pub const AT_ONCE: usize = 5;
const FRESH_READING: f64 = 900.0;

fn same_run(mark: &Object, resets_at: &Json, now: f64) -> bool {
    let until = mark.get("until").cloned().unwrap_or(Json::Null);
    match (until.number(), resets_at.number()) {
        (Some(end), Some(resets)) => (resets - end).abs() <= SAME_RUN || now < end + SAME_RUN,
        _ => until.is_null() && resets_at.is_null(),
    }
}

fn marked(marks: &Object, key: &str, resets_at: &Json, now: f64) -> bool {
    marks.get(key).and_then(Json::as_object).is_some_and(|mark| same_run(mark, resets_at, now))
}

fn alert(fields: &Object, kind: &str, key: &str, mark: Object) -> Object {
    let mut alert = fields.clone();
    alert.insert("kind", kind);
    alert.insert("mark", Json::Array(vec![Json::from(key), Json::Object(mark)]));
    alert
}

/// What the readings call for. Marks of what no longer applies are dropped; a returned alert is marked only by told.
pub fn due(
    windows: &[Object],
    balances: &[Object],
    marks: &mut Object,
    percent: f64,
    reminder: bool,
    now: f64,
) -> Vec<Object> {
    let mut out = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for window in windows {
        let id = py::str(window.get("id"));
        let key = format!("w|{id}");
        let renewal = format!("r|{id}");
        seen.insert(key.clone());
        seen.insert(renewal.clone());
        if !window.get("current").is_some_and(Json::is_truthy) {
            continue;
        }
        let resets = window.get("resetsAt").cloned().unwrap_or(Json::Null);
        let used = window.get("used").and_then(Json::number).unwrap_or(0.0);
        if percent <= 0.0 || used < percent {
            marks.remove(&key);
        } else if !marked(marks, &key, &resets, now) {
            out.push(alert(window, "usage", &key, object! { "at" => now, "until" => resets.clone() }));
        }
        let span = py::or(window.get("span"), Json::Int(0)).number().unwrap_or(0.0);
        if reminder
            && resets.is_truthy()
            && let Some(end) = resets.number()
            && span >= REMINDER_SPAN
            && used < REMINDER_UNDER
            && now < end
            && end <= now + REMINDER_LEAD
            && !marked(marks, &renewal, &resets, now)
        {
            out.push(alert(window, "renews", &renewal, object! { "at" => now, "until" => resets.clone() }));
        }
    }
    for balance in balances {
        let key = format!("b|{}", py::str(balance.get("id")));
        seen.insert(key.clone());
        if !balance.get("current").is_some_and(Json::is_truthy) {
            continue;
        }
        let line = balance.get("line");
        if !line.is_some_and(Json::is_truthy)
            || balance.get("amount").and_then(Json::number).unwrap_or(0.0) > line.and_then(Json::number).unwrap_or(0.0)
        {
            marks.remove(&key);
        } else if !marks.contains_key(&key) {
            out.push(alert(balance, "balance", &key, object! { "at" => now }));
        }
    }
    let stale: Vec<String> = marks
        .iter()
        .filter(|(key, mark)| !seen.contains(*key) && now - mark.get("at").and_then(Json::number).unwrap_or(0.0) > KEEP)
        .map(|(key, _)| key.clone())
        .collect();
    for key in stale {
        marks.remove(&key);
    }
    out
}

/// Mark the alert as said, so it isn't said again this run.
pub fn told(marks: &mut Object, alert: &Object) {
    let Some(mark) = alert.get("mark").and_then(Json::as_array).filter(|mark| mark.len() == 2) else { return };
    let Some(key) = mark[0].as_str() else { return };
    marks.insert(key, mark[1].clone());
}

pub fn load(state: &str) -> Object {
    let Some(value) = std::fs::read(format!("{state}/alerts.json"))
        .ok()
        .and_then(|data| Json::parse(&data).ok())
        .and_then(|value| value.as_object().cloned())
        .filter(|value| value.get("schema") == Some(&Json::Int(1)))
    else {
        return Object::new();
    };
    let Some(marks) = value.get("marks").and_then(Json::as_object) else { return Object::new() };
    let mut kept = Object::new();
    for (key, mark) in marks.iter() {
        if let Some(at) = mark.as_object().and_then(|fields| fields.get("at"))
            && matches!(at, Json::Int(_) | Json::Double(_) | Json::Bool(_))
        {
            kept.insert(key.clone(), mark.clone());
        }
    }
    kept
}

pub fn save(state: &str, marks: &Object) -> Result<(), Failure> {
    files::make_directory(Path::new(state), 0o700)?;
    let document = Json::Object(object! { "schema" => 1, "marks" => marks.clone() });
    files::replace_atomically(
        Path::new(&format!("{state}/alerts.json")),
        document.serialized().as_bytes(),
        ".alerts-",
        || Ok(()),
    )
}

/// end − start, an integer when both are.
fn difference(end: Option<&Json>, start: Option<&Json>) -> Json {
    match (end, start) {
        (Some(Json::Int(a)), Some(Json::Int(b))) => Json::Int(a - b),
        (Some(end), Some(start)) => Json::Double(end.number().unwrap_or(0.0) - start.number().unwrap_or(0.0)),
        _ => Json::Null,
    }
}

impl Runtime {
    fn reading_current(&self, payload: Option<&Object>, error: Option<&str>, now: f64) -> bool {
        let Some(payload) = payload.filter(|payload| !payload.is_empty()) else { return false };
        !refresh_failed(error)
            && now - py::or(payload.get("_fetchedAt"), Json::Int(0)).number().unwrap_or(0.0) <= FRESH_READING
    }

    /// Every subscription window the homepage shows, as the usage alerts read it.
    pub fn alert_windows(&self, data: &Object, now: f64) -> Vec<Object> {
        let mut out: Vec<Object> = Vec::new();
        let mut add =
            |source: &str, name: &str, window: &str, used: f64, resets_at: Json, span: Json, current: bool| {
                out.push(object! {
                    "id" => format!("{source}|{window}"), "name" => name, "window" => window, "used" => used,
                    "resetsAt" => resets_at, "span" => span, "current" => current,
                });
            };
        let rows_of = |payload: &Object| -> Vec<Object> {
            payload
                .get("windows")
                .and_then(Json::as_array)
                .into_iter()
                .flatten()
                .map(|row| row.as_object().cloned().unwrap_or_default())
                .collect()
        };
        if self.module_enabled("claude-subscription") {
            let payload = object(data, "claude_subscription");
            let current = !payload.get("fetchError").is_some_and(Json::is_truthy)
                && !refresh_failed(failure(data, "claude_subscription").as_deref())
                && !claude_usage_stale(&payload);
            for window in rows_of(&payload) {
                let resets = window
                    .get("resetsAt")
                    .filter(|raw| raw.is_truthy())
                    .and_then(Json::as_str)
                    .and_then(|text| IsoTime::parse(text).ok())
                    .map_or(Json::Null, |time| Json::Double(time.timestamp(&self.local_zone())));
                let span = match window.get("span").filter(|value| value.is_truthy()) {
                    Some(value) => value.clone(),
                    None => claude_span(&window).map_or(Json::Null, |seconds| Json::Int(seconds as i64)),
                };
                let used = window.get("used").and_then(Json::python_float).unwrap_or(0.0);
                add("claude-subscription", "Claude 订阅", &py::str(window.get("label")), used, resets, span, current);
            }
        }
        if self.module_enabled("chatgpt") && self.chatgpt_source() == "proxy" {
            let quota = data.get("quota").and_then(Json::as_object);
            let current = self.reading_current(quota, self.proxy_quota_error(data).as_deref(), now);
            for (name, used, _) in self.account_usage(data) {
                if let Some(used) = used {
                    add("chatgpt-account", "ChatGPT 订阅", &name, used, Json::Null, Json::Null, current);
                }
            }
        } else if self.module_enabled("chatgpt") {
            let payload = object(data, "chatgpt");
            let current = self.reading_current(Some(&payload), failure(data, "chatgpt").as_deref(), now);
            for window in rows_of(&payload) {
                let label = chatgpt_window_label(&window);
                let span = match py::or(window.get("minutes"), Json::Int(0)) {
                    Json::Int(minutes) if minutes != 0 => Json::Int(minutes * 60),
                    Json::Double(minutes) if minutes != 0.0 => Json::Double(minutes * 60.0),
                    _ => Json::Null,
                };
                let name = label.strip_prefix("ChatGPT · ").map(String::from).unwrap_or(label);
                let used = 100.0 - window.get("remaining").and_then(Json::number).unwrap_or(0.0);
                let resets = window.get("resetsAt").cloned().unwrap_or(Json::Null);
                add("chatgpt", "ChatGPT 订阅", &name, used, resets, span, current);
            }
        }
        if self.module_enabled("minimax") {
            let payload = object(data, "minimax");
            let current = self.reading_current(Some(&payload), failure(data, "minimax").as_deref(), now);
            for window in rows_of(&payload) {
                let span = match (
                    window.get("startsAt").filter(|v| v.is_truthy()),
                    window.get("resetsAt").filter(|v| v.is_truthy()),
                ) {
                    (Some(starts), Some(ends)) => difference(Some(ends), Some(starts)),
                    _ if py::str(Some(&window.get("name").cloned().unwrap_or_else(|| Json::from(""))))
                        .ends_with("本周额度") =>
                    {
                        Json::Int(7 * 86400)
                    }
                    _ => Json::Null,
                };
                let used = 100.0 - window.get("remaining").and_then(Json::number).unwrap_or(0.0);
                let resets = window.get("resetsAt").cloned().unwrap_or(Json::Null);
                add("minimax", "MiniMax 订阅", &py::str(window.get("name")), used, resets, span, current);
            }
        }
        if self.module_enabled("gemini-subscription") {
            let payload = object(data, "gemini_subscription");
            let current = self.reading_current(Some(&payload), failure(data, "gemini_subscription").as_deref(), now);
            for window in crate::module_rows::gemini_windows(&payload, now) {
                let used = 100.0 - window.get("remaining").and_then(Json::number).unwrap_or(0.0);
                let resets = window.get("resetsAt").cloned().unwrap_or(Json::Null);
                let span = crate::module_rows::gemini_span(&window).map_or(Json::Null, Json::Double);
                add(
                    "gemini-subscription",
                    "Gemini 订阅",
                    &crate::module_rows::gemini_window_name(&window),
                    used,
                    resets,
                    span,
                    current,
                );
            }
        }
        if self.module_enabled("qoder-subscription") {
            // The App writes its summary only when the user opens Credits usage; past 10 minutes it isn't current.
            let payload = object(data, "qoder_subscription");
            let current = now - py::or(payload.get("observedAt"), Json::Int(0)).number().unwrap_or(0.0) <= 600.0;
            for (field, window) in [("plan", "套餐 Credits"), ("addOn", "加购 Credits")] {
                let Some(part) = payload.get(field).and_then(Json::as_object) else { continue };
                let Some(total) = part.get("total").filter(|total| total.is_truthy()) else { continue };
                let used =
                    100.0 * part.get("used").and_then(Json::number).unwrap_or(0.0) / total.number().unwrap_or(1.0);
                let resets =
                    if field == "plan" { payload.get("resetsAt").cloned().unwrap_or(Json::Null) } else { Json::Null };
                add("qoder-subscription", "Qoder 订阅", window, used, resets, Json::Null, current);
            }
        }
        if self.module_enabled("cursor-subscription") && self.settings().get("cursorUsage").is_some_and(Json::is_truthy)
        {
            let payload = object(data, "cursor_subscription");
            if let Some(percent) = payload.get("percent").filter(|percent| !percent.is_null()) {
                let end = payload.get("cycleEnd");
                let start = payload.get("cycleStart");
                let span = if end.is_some_and(Json::is_truthy) && start.is_some_and(Json::is_truthy) {
                    difference(end, start)
                } else {
                    Json::Null
                };
                let current =
                    self.reading_current(Some(&payload), failure(data, "cursor_subscription").as_deref(), now);
                add(
                    "cursor-subscription",
                    "Cursor 订阅",
                    "本周期",
                    percent.python_float().unwrap_or(0.0),
                    end.cloned().unwrap_or(Json::Null),
                    span,
                    current,
                );
            }
        }
        out
    }

    /// One balance per account of each balance API, with the line set for its currency.
    pub fn alert_balances(&self, data: &Object, now: f64) -> Vec<Object> {
        let mut out = Vec::new();
        let lines = self.settings().get("balanceAlerts").and_then(Json::as_object).cloned().unwrap_or_default();
        for provider in catalog::balance_apis() {
            if !self.module_enabled(provider) {
                continue;
            }
            let read: Vec<Object> = self
                .api_values(provider, data)
                .1
                .into_iter()
                .filter(|value| value.get("kind") == Some(&Json::from("balance")))
                .collect();
            let counted = counted_values(&read);
            let label = catalog::by_key(provider).map(|service| service.label()).unwrap_or_default();
            for value in &counted {
                let account: Vec<String> = value
                    .get("account")
                    .and_then(Json::as_array)
                    .into_iter()
                    .flatten()
                    .map(|part| py::str(Some(part)))
                    .collect();
                let currency = py::str(value.get("currency"));
                let name = if counted.len() > 1 {
                    format!("{label} · {}", py::str(value.get("label")))
                } else {
                    label.clone()
                };
                let current = !value.get("failed").is_some_and(Json::is_truthy)
                    && now - py::or(value.get("at"), Json::Int(0)).number().unwrap_or(0.0) <= FRESH_READING;
                out.push(object! {
                    "id" => format!("{provider}|{}", account.join(":")), "name" => name,
                    "amount" => value.get("amount").cloned().unwrap_or(Json::Null),
                    "currency" => value.get("currency").cloned().unwrap_or(Json::Null),
                    "line" => lines.get(&format!("{provider}|{currency}")).cloned().unwrap_or(Json::Null),
                    "current" => current,
                });
            }
        }
        out
    }

    fn alert_clock(&self, at: f64, now: f64) -> String {
        // The service often returns xx:59:59.999 for an xx:00 reset.
        let zone = self.local_zone();
        let moment = at + 30.0;
        let days = day_number(moment, &zone) - day_number(now, &zone);
        let clock = dashboard::format(moment, &zone, "%H:%M");
        if days <= 0 {
            return clock;
        }
        if days == 1 {
            return format!("明天 {clock}");
        }
        if days < 7 {
            let weekday = ((day_number(moment, &zone) + 3) % 7 + 7) % 7;
            let name = "一二三四五六日".chars().nth(weekday as usize).unwrap_or('一');
            return format!("周{name} {clock}");
        }
        let number = |pattern: &str| dashboard::format(moment, &zone, pattern).parse::<i64>().unwrap_or(0);
        format!("{}月{}日 {clock}", number("%m"), number("%d"))
    }

    /// (subtitle, body) of one alert's notification; the title is always Starmeter.
    pub fn alert_notice(&self, alert: &Object, now: f64) -> (String, String) {
        let name = py::str(alert.get("name"));
        let number = |key: &str| alert.get(key).and_then(Json::number).unwrap_or(0.0);
        if alert.get("kind") == Some(&Json::from("balance")) {
            let currency = py::str(alert.get("currency"));
            return (
                "余额提醒".into(),
                format!(
                    "{name} 余额 {}，已低于提醒线 {}",
                    money_text(number("amount"), &currency),
                    money_text(number("line"), &currency)
                ),
            );
        }
        let reset = if alert.get("resetsAt").is_some_and(Json::is_truthy) {
            format!("，{} 重置", self.alert_clock(number("resetsAt"), now))
        } else {
            String::new()
        };
        let used = format!("{}%", g(number("used").round_ties_even()));
        let window = py::str(alert.get("window"));
        if alert.get("kind") == Some(&Json::from("renews")) {
            return ("重置提醒".into(), format!("{name} · {window} 只用了 {used}{reset}，记得用掉"));
        }
        ("用量提醒".into(), format!("{name} · {window} 已用 {used}{reset}"))
    }

    /// Run by the background worker after its reads; only cached readings are looked at.
    pub fn check_usage_alerts(&self) {
        let settings = self.settings();
        let percent = settings.get("usageAlert").and_then(Json::number).unwrap_or(0.0);
        let reminder = settings.get("resetReminder").is_some_and(Json::is_truthy);
        if percent == 0.0 && !reminder && !settings.get("balanceAlerts").is_some_and(Json::is_truthy) {
            return;
        }
        let data = self.cached_data();
        let mut marks = load(&self.state_dir);
        let moment = now();
        let due = due(
            &self.alert_windows(&data, moment),
            &self.alert_balances(&data, moment),
            &mut marks,
            percent,
            reminder,
            moment,
        );
        // Only a notice that went out is marked; one that failed, or past the few sent at once, is due again.
        for alert in due.iter().take(AT_ONCE) {
            let (subtitle, body) = self.alert_notice(alert, moment);
            if self.post_notice(&subtitle, &body) {
                told(&mut marks, alert);
            }
        }
        let _ = save(&self.state_dir, &marks);
    }
}
