//! Local presentation preferences in preferences.json. No credentials or usage data live here. The file is
//! shared with 5.x, so values are kept as written and read back the same way it reads them.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use crate::catalog::{self, SERVICES};
use crate::files::{self, paths};
use crate::json::{Json, Object, parse_python_float};
use crate::object;

pub struct Module {
    pub key: &'static str,
    pub label: String,
    pub category: &'static str,
}

/// Every homepage module in the order the module list shows them.
pub static MODULES: LazyLock<Vec<Module>> = LazyLock::new(|| {
    let module = |key, label: &str, category| Module { key, label: label.to_string(), category };
    let mut modules = vec![
        module("summary", "Summary", "概览"),
        module("heatmap", "用量热力图", "概览"),
        module("opencodex", "OpenCodex", "Harness"),
        module("magpie", "Magpie", "Harness"),
        module("codex", "ChatGPT", "Harness"),
        module("claude", "Claude", "Harness"),
        module("cursor", "Cursor", "Harness"),
        module("deepseek-harness", "DeepSeek Harness", "Harness"),
        module("gemini", "Gemini", "Harness"),
        module("opencode", "OpenCode", "Harness"),
        module("qoder", "Qoder", "Harness"),
        module("workbuddy", "WorkBuddy", "Harness"),
    ];
    modules.extend(SERVICES.iter().map(|s| Module { key: s.key, label: s.label(), category: s.category }));
    modules.extend([
        module("sessions", "会话", "其他"),
        module("komari", "Komari 监控", "其他"),
        module("updates", "工具更新", "其他"),
    ]);
    modules
});

/// Usage alert levels in percent; 0 is off.
pub const ALERT_LEVELS: [i64; 4] = [0, 70, 80, 90];
/// The panel follows macOS/Windows, or stays light or dark.
pub const THEMES: [&str; 3] = ["system", "light", "dark"];
/// The panel's background: a solid colour (the default), the classic frosted blur, or the system's glass.
pub const APPEARANCES: [&str; 3] = ["solid", "frosted", "glass"];
/// The Harness cards on the homepage, one per tool (the default), per model or per project.
pub const GROUPINGS: [&str; 3] = ["tool", "model", "project"];
/// Beside the menu bar star: nothing (the default), today's API-equivalent estimate, the tightest subscription, or
/// the output tokens a second.
pub const MENU_BAR: [&str; 3] = ["none", "cost", "quota"];
/// API-equivalent estimates read in US dollars, or in yuan at the day's reference rate (or one the user fixed).
pub const CURRENCIES: [&str; 2] = ["USD", "CNY"];
/// How often a pinned panel re-reads the range on screen, in seconds.
pub const PIN_INTERVALS: [i64; 3] = [1, 2, 5];
pub const DEFAULT_VISIBLE: [&str; 9] =
    ["summary", "heatmap", "opencodex", "claude", "deepseek", "chatgpt", "sessions", "komari", "updates"];
/// Modules added after release show once for people who keep Summary on; a saved file lists the modules it
/// already knew, so hiding one of these afterwards sticks.
pub const INTRODUCED: [&str; 2] = ["heatmap", "sessions"];
/// A new module that only means something beside these is introduced only to those who show one of them.
const INTRODUCED_WITH: [(&str, &[&str]); 1] = [("sessions", &["claude", "codex"])];
pub const USAGE: [&str; 10] = [
    "opencodex",
    "magpie",
    "codex",
    "claude",
    "cursor",
    "deepseek-harness",
    "gemini",
    "opencode",
    "qoder",
    "workbuddy",
];

pub fn is_known(key: &str) -> bool {
    MODULES.iter().any(|module| module.key == key)
}

/// The order the homepage's rows follow, every module in it: the user's (homeOrder, set by dragging a row on the
/// homepage), then the modules it doesn't list yet (one added in a later version) in the module list's order. Empty
/// homeOrder is the module list's order.
pub fn home_order(settings: &Object) -> Vec<&'static str> {
    let chosen: Vec<&str> =
        settings.get("homeOrder").and_then(Json::as_array).into_iter().flatten().filter_map(Json::as_str).collect();
    let mut order: Vec<&'static str> = chosen
        .iter()
        .filter_map(|key| MODULES.iter().find(|module| module.key == *key).map(|module| module.key))
        .collect();
    order.extend(MODULES.iter().map(|module| module.key).filter(|key| !chosen.contains(key)));
    order
}

/// A preference change the panel asked for that is not one this version accepts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Invalid(&'static str),
    Write(files::Failure),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Invalid(message) => f.write_str(message),
            Error::Write(failure) => failure.fmt(f),
        }
    }
}

impl std::error::Error for Error {}

/// A balance alert's amount: a positive finite number, given as a number or its text.
fn balance_line(value: Option<&Json>) -> bool {
    in_range(value, 0.0, 1e9)
}

/// Yuan to the dollar, within reason.
pub fn exchange_rate(value: Option<&Json>) -> bool {
    in_range(value, 1.0, 20.0)
}

/// A subscription's monthly price in US dollars, as the user wrote it down.
fn monthly_price(value: Option<&Json>) -> bool {
    in_range(value, 0.0, 1e5)
}

fn in_range(value: Option<&Json>, low: f64, high: f64) -> bool {
    match value {
        Some(value) if value.as_bool().is_none() => {
            value.python_float().is_some_and(|number| low < number && number < high)
        }
        _ => false,
    }
}

/// A balance alert is kept per API and currency, as "kimi-api|USD".
pub fn balance_key(key: &str) -> bool {
    let (api, currency) = key.split_once('|').unwrap_or((key, ""));
    catalog::balance_apis().contains(&api) && catalog::CURRENCIES.contains(&currency)
}

fn strings(items: impl IntoIterator<Item = impl Into<String>>) -> Json {
    Json::Array(items.into_iter().map(|item| Json::String(item.into())).collect())
}

pub fn defaults() -> Object {
    object! {
        "schema" => 1, "visible" => strings(DEFAULT_VISIBLE), "appearance" => "solid", "theme" => "system",
        "paths" => Object::new(), "allowCollapse" => false, "expandedProviders" => Json::Array(vec![]), "revision" => 0,
        "configuredSubscriptions" => Json::Array(vec![]), "claudeLiveUsage" => false, "claudeOAuthUsage" => false,
        "cursorUsage" => false, "usageAlert" => 90, "resetReminder" => false, "balanceAlerts" => Object::new(),
        "keepCleanedUsage" => true, "subscriptionPrices" => Object::new(), "groupBy" => "tool", "menuBar" => "none",
        "sessionAlerts" => false, "serviceStatus" => false, "currency" => "USD", "cnyRate" => Json::Null, "pinInterval" => 2,
        "syncFolder" => "", "deviceName" => "", "homeOrder" => Json::Array(vec![]),
    }
}

fn file(directory: &Path) -> PathBuf {
    directory.join("preferences.json")
}

/// The preferences in `directory`, each value checked; anything missing or not understood is its default.
pub fn read(directory: &Path) -> Object {
    let mut settings = defaults();
    let Some(value) = std::fs::read(file(directory))
        .ok()
        .and_then(|data| Json::parse(&data).ok())
        .and_then(|value| value.as_object().cloned())
        .filter(|value| value.get("schema") == Some(&Json::Int(1)))
    else {
        return settings;
    };
    let get = |key: &str| value.get(key);

    if let Some(visible) =
        get("visible").and_then(Json::as_array).filter(|items| items.iter().all(|item| item.as_str().is_some()))
    {
        // The ChatGPT subscription source now follows the OpenCodex selection; no separate choice.
        let mut shown: Vec<String> = Vec::new();
        for name in visible.iter().filter_map(Json::as_str) {
            if is_known(name) || name == "codex-quota" {
                let key = if name == "codex-quota" { "chatgpt" } else { name };
                if !shown.iter().any(|shown| shown == key) {
                    shown.push(key.to_string());
                }
            }
        }
        let known_before = get("modules").and_then(Json::as_array).cloned().unwrap_or_default();
        let showing = |key: &str| shown.iter().any(|shown| shown == key);
        let new: Vec<&str> = INTRODUCED
            .into_iter()
            .filter(|key| {
                let alongside = INTRODUCED_WITH.iter().find(|(name, _)| name == key).map(|(_, with)| *with);
                !known_before.contains(&Json::from(*key))
                    && !showing(key)
                    && showing("summary")
                    && alongside.is_none_or(|with| with.iter().any(|other| showing(other)))
            })
            .collect();
        if !new.is_empty()
            && let Some(at) = shown.iter().position(|key| key == "summary")
        {
            for (offset, key) in new.into_iter().enumerate() {
                shown.insert(at + 1 + offset, key.to_string());
            }
        }
        settings.insert("visible", strings(shown));
    }
    if let Some(chosen) = get("configuredSubscriptions").and_then(Json::as_array) {
        let kept = catalog::manual_subscriptions().into_iter().filter(|key| chosen.contains(&Json::from(*key)));
        settings.insert("configuredSubscriptions", strings(kept));
    }
    if let Some(appearance) = get("appearance").and_then(Json::as_str).filter(|v| APPEARANCES.contains(v)) {
        settings.insert("appearance", appearance);
    }
    if let Some(theme) = get("theme").and_then(Json::as_str).filter(|v| THEMES.contains(v)) {
        settings.insert("theme", theme);
    }
    for flag in ["allowCollapse", "claudeLiveUsage", "claudeOAuthUsage", "cursorUsage"] {
        if let Some(on) = get(flag).and_then(Json::as_bool) {
            settings.insert(flag, on);
        }
    }
    if let Some(level) = get("usageAlert")
        && level.number().is_some_and(|number| ALERT_LEVELS.iter().any(|allowed| *allowed as f64 == number))
    {
        settings.insert("usageAlert", level.clone());
    }
    for flag in ["resetReminder", "keepCleanedUsage"] {
        if let Some(on) = get(flag).and_then(Json::as_bool) {
            settings.insert(flag, on);
        }
    }
    if let Some(grouping) = get("groupBy").and_then(Json::as_str).filter(|v| GROUPINGS.contains(v)) {
        settings.insert("groupBy", grouping);
    }
    if let Some(shown) = get("menuBar").and_then(Json::as_str).filter(|v| MENU_BAR.contains(v)) {
        settings.insert("menuBar", shown);
    }
    for flag in ["sessionAlerts", "serviceStatus"] {
        if let Some(on) = get(flag).and_then(Json::as_bool) {
            settings.insert(flag, on);
        }
    }
    if let Some(currency) = get("currency").and_then(Json::as_str).filter(|v| CURRENCIES.contains(v)) {
        settings.insert("currency", currency);
    }
    if let Some(interval) = get("pinInterval")
        && interval.number().is_some_and(|number| PIN_INTERVALS.iter().any(|allowed| *allowed as f64 == number))
    {
        settings.insert("pinInterval", interval.clone());
    }
    // 多台电脑合并: the folder picked, and this computer's name there.
    for (name, size) in [("syncFolder", 512), ("deviceName", 64)] {
        if let Some(text) = get(name).and_then(Json::as_str) {
            settings.insert(name, text.chars().take(size).collect::<String>());
        }
    }
    if exchange_rate(get("cnyRate"))
        && let Some(rate) = get("cnyRate").and_then(Json::python_float)
    {
        settings.insert("cnyRate", rate);
    }
    if let Some(prices) = get("subscriptionPrices").and_then(Json::as_object) {
        let kept: Object = prices
            .iter()
            .filter(|(key, price)| {
                catalog::SUBSCRIPTION_USAGE.iter().any(|(name, _)| name == key) && monthly_price(Some(price))
            })
            .map(|(key, price)| (key.clone(), price.clone()))
            .collect();
        settings.insert("subscriptionPrices", kept);
    }
    if let Some(lines) = get("balanceAlerts").and_then(Json::as_object) {
        // 5.14.0 kept one line per API, asked in the API's usual currency.
        let mut migrated = Object::new();
        for (key, line) in lines.iter() {
            let key = if key.contains('|') {
                key.clone()
            } else {
                let currency = catalog::BALANCE_CURRENCIES.iter().find(|(api, _)| api == key).map_or("", |(_, c)| *c);
                format!("{key}|{currency}")
            };
            migrated.insert(key, line.clone());
        }
        let kept: Object =
            migrated.into_iter().filter(|(key, line)| balance_key(key) && balance_line(Some(line))).collect();
        settings.insert("balanceAlerts", kept);
    }
    if let Some(expanded) = get("expandedProviders").and_then(Json::as_array) {
        settings
            .insert("expandedProviders", strings(USAGE.into_iter().filter(|key| expanded.contains(&Json::from(*key)))));
    } else if let Some(open) = get("expandedProvider").and_then(Json::as_str).filter(|v| USAGE.contains(v)) {
        // Migrate 5.1.0 without losing the open section.
        settings.insert("expandedProviders", strings([open]));
    }
    if let Some(order) = get("homeOrder").and_then(Json::as_array) {
        let mut kept: Vec<&str> = Vec::new();
        for key in order.iter().filter_map(Json::as_str).filter(|key| is_known(key)) {
            if !kept.contains(&key) {
                kept.push(key);
            }
        }
        settings.insert("homeOrder", strings(kept));
    }
    if let Some(revision) = get("revision")
        && revision.integer().is_some_and(|number| number >= 0)
    {
        settings.insert("revision", revision.clone());
    }
    if let Some(paths) = get("paths").and_then(Json::as_object) {
        let kept: Object = paths
            .iter()
            .filter(|(key, path)| {
                (USAGE.contains(&key.as_str()) || *key == "claude-app") && !path.as_str().unwrap_or("").is_empty()
            })
            .map(|(key, path)| (key.clone(), path.clone()))
            .collect();
        settings.insert("paths", kept);
    }
    settings
}

/// Writes the preferences with a new revision, newer than both the clock and the one on disk.
pub fn write(directory: &Path, mut value: Object) -> Result<(), Error> {
    let on_disk = read(directory).get("revision").and_then(Json::integer).unwrap_or(0);
    value.insert("revision", files::time_nanoseconds().max(on_disk + 1));
    value.insert("modules", strings(MODULES.iter().map(|module| module.key)));
    files::make_directory(directory, 0o700).map_err(Error::Write)?;
    let data = Json::Object(value).serialized();
    let path = file(directory);
    // read() fell back to defaults for a file it couldn't use; keep the user's original before replacing it.
    files::replace_atomically(&path, data.as_bytes(), ".preferences-", || {
        files::keep_unreadable(&path, files::schema_one).map(|_| ())
    })
    .map_err(Error::Write)
}

/// Runs a read-modify-write of the preferences holding their lock: the app, the command line and the SwiftBar entry
/// can each be changing them at once, and one's write must not drop what another just saved.
pub fn locked<T>(directory: &Path, change: impl FnOnce() -> T) -> T {
    let _ = files::make_directory(directory, 0o700);
    let _lock = crate::platform::lock(&directory.join("preferences.lock"), true);
    change()
}

/// Applies one change the panel asked for (`kind` with its key and value) and returns the preferences as saved.
pub fn update(directory: &Path, kind: &str, key: Option<&str>, value: Option<&str>) -> Result<Object, Error> {
    locked(directory, || update_holding(directory, kind, key, value))
}

fn update_holding(directory: &Path, kind: &str, key: Option<&str>, value: Option<&str>) -> Result<Object, Error> {
    let mut current = read(directory);
    let listed = |current: &Object, name: &str| -> Vec<String> {
        current
            .get(name)
            .and_then(Json::as_array)
            .map(|items| items.iter().filter_map(Json::as_str).map(String::from).collect())
            .unwrap_or_default()
    };
    let object =
        |current: &Object, name: &str| current.get(name).and_then(Json::as_object).cloned().unwrap_or_default();
    let boolean = |text: Option<&str>| matches!(text, Some("true" | "false"));
    let flag = |current: &mut Object, name: &str, message: &'static str| -> Result<(), Error> {
        if !boolean(key) {
            return Err(Error::Invalid(message));
        }
        current.insert(name, key == Some("true"));
        Ok(())
    };
    let parsed = |text: Option<&str>| text.and_then(parse_python_float);
    let as_json = |text: Option<&str>| text.map(Json::from);

    match kind {
        "module" => {
            // Existing queued callbacks remain valid.
            let name = if key == Some("codex-quota") { Some("chatgpt") } else { key };
            let Some(name) = name.filter(|name| is_known(name)).filter(|_| boolean(value)) else {
                return Err(Error::Invalid("invalid module preference"));
            };
            let mut visible = listed(&current, "visible");
            visible.retain(|shown| shown != name);
            if value == Some("true") {
                visible.push(name.to_string());
            }
            let ordered =
                MODULES.iter().map(|module| module.key).filter(|key| visible.iter().any(|shown| shown == key));
            current.insert("visible", strings(ordered));
        }
        "claude-live" => flag(&mut current, "claudeLiveUsage", "invalid Claude live usage setting")?,
        "claude-oauth" => flag(&mut current, "claudeOAuthUsage", "invalid Claude OAuth setting")?,
        "cursor-usage" => flag(&mut current, "cursorUsage", "invalid Cursor usage setting")?,
        "usage-alert" => {
            let level = ALERT_LEVELS.into_iter().find(|level| Some(level.to_string().as_str()) == key);
            current.insert("usageAlert", level.ok_or(Error::Invalid("invalid usage alert"))?);
        }
        "reset-reminder" => flag(&mut current, "resetReminder", "invalid reset reminder")?,
        "keep-usage" => flag(&mut current, "keepCleanedUsage", "invalid keep usage setting")?,
        "sync-folder" => {
            current.insert("syncFolder", key.unwrap_or("").chars().take(512).collect::<String>());
        }
        "device-name" => {
            current.insert("deviceName", key.unwrap_or("").trim().chars().take(64).collect::<String>());
        }
        "pin-interval" => {
            let interval = PIN_INTERVALS.into_iter().find(|interval| Some(interval.to_string().as_str()) == key);
            current.insert("pinInterval", interval.ok_or(Error::Invalid("invalid pin interval"))?);
        }
        "currency" => {
            let currency = key.filter(|key| CURRENCIES.contains(key)).ok_or(Error::Invalid("invalid currency"))?;
            current.insert("currency", currency);
        }
        "cny-rate" => match key {
            None | Some("") => current.insert("cnyRate", Json::Null),
            _ if exchange_rate(as_json(key).as_ref()) && parsed(key).is_some() => {
                current.insert("cnyRate", parsed(key).expect("checked"));
            }
            _ => return Err(Error::Invalid("invalid exchange rate")),
        },
        "service-status" => flag(&mut current, "serviceStatus", "invalid service status setting")?,
        "session-alerts" => flag(&mut current, "sessionAlerts", "invalid session alert setting")?,
        "menu-bar" => {
            let shown = key.filter(|key| MENU_BAR.contains(key)).ok_or(Error::Invalid("invalid menu bar value"))?;
            current.insert("menuBar", shown);
        }
        "group" => {
            let grouping = key.filter(|key| GROUPINGS.contains(key)).ok_or(Error::Invalid("invalid grouping"))?;
            current.insert("groupBy", grouping);
        }
        "subscription-price" => {
            let key = key
                .filter(|key| catalog::SUBSCRIPTION_USAGE.iter().any(|(name, _)| name == key))
                .ok_or(Error::Invalid("invalid subscription"))?;
            let mut prices = object(&current, "subscriptionPrices");
            match value {
                None | Some("") => {
                    prices.remove(key);
                }
                _ if monthly_price(as_json(value).as_ref()) && parsed(value).is_some() => {
                    prices.insert(key, parsed(value).expect("checked"));
                }
                _ => return Err(Error::Invalid("invalid subscription price")),
            }
            current.insert("subscriptionPrices", prices);
        }
        "balance-alert" => {
            let key = key.filter(|key| balance_key(key)).ok_or(Error::Invalid("invalid balance alert"))?;
            let mut lines = object(&current, "balanceAlerts");
            match value {
                None | Some("" | "0") => {
                    lines.remove(key);
                }
                _ if balance_line(as_json(value).as_ref()) && parsed(value).is_some() => {
                    lines.insert(key, parsed(value).expect("checked"));
                }
                _ => return Err(Error::Invalid("invalid balance alert")),
            }
            current.insert("balanceAlerts", lines);
        }
        "subscription" => {
            let manual = catalog::manual_subscriptions();
            let Some(key) = key.filter(|key| manual.contains(key)).filter(|_| boolean(value)) else {
                return Err(Error::Invalid("invalid subscription"));
            };
            let mut chosen = listed(&current, "configuredSubscriptions");
            chosen.retain(|name| name != key);
            if value == Some("true") {
                chosen.push(key.to_string());
            }
            current.insert(
                "configuredSubscriptions",
                strings(manual.into_iter().filter(|name| chosen.iter().any(|chosen| chosen == name))),
            );
        }
        "appearance" => {
            let appearance = key.filter(|key| APPEARANCES.contains(key)).ok_or(Error::Invalid("invalid appearance"))?;
            current.insert("appearance", appearance);
        }
        "theme" => {
            let theme = key.filter(|key| THEMES.contains(key)).ok_or(Error::Invalid("invalid theme"))?;
            current.insert("theme", theme);
        }
        "collapse" => {
            flag(&mut current, "allowCollapse", "invalid collapse setting")?;
            // Turning folding on starts with every card folded.
            if key == Some("true") {
                current.insert("expandedProviders", Json::Array(vec![]));
            }
        }
        "expanded" => {
            let Some(key) = key.filter(|key| USAGE.contains(key)).filter(|_| boolean(value)) else {
                return Err(Error::Invalid("invalid expansion preference"));
            };
            let mut expanded = listed(&current, "expandedProviders");
            expanded.retain(|name| name != key);
            if value == Some("true") {
                expanded.push(key.to_string());
            }
            current.insert(
                "expandedProviders",
                strings(USAGE.into_iter().filter(|name| expanded.iter().any(|open| open == name))),
            );
        }
        // A homepage row dragged before or after another (or moved with ⌥↑ ⌥↓): the whole order is kept from then on.
        "move-before" | "move-after" => {
            let (Some(moved), Some(target)) = (key.filter(|key| is_known(key)), value.filter(|value| is_known(value)))
            else {
                return Err(Error::Invalid("invalid homepage order"));
            };
            if moved != target {
                let mut order: Vec<&str> = home_order(&current).into_iter().filter(|key| *key != moved).collect();
                let at = order.iter().position(|key| *key == target).expect("a known module is in the order");
                order.insert(if kind == "move-before" { at } else { at + 1 }, moved);
                current.insert("homeOrder", strings(order));
            }
        }
        "order-reset" => {
            current.insert("homeOrder", Json::Array(vec![]));
        }
        "reset" => {
            current.insert("visible", strings(DEFAULT_VISIBLE));
            current.insert("appearance", "solid");
            current.insert("theme", "system");
            current.insert("allowCollapse", false);
            current.insert("expandedProviders", Json::Array(vec![]));
        }
        "auto-path" => {
            let key = key.filter(|key| USAGE.contains(key)).ok_or(Error::Invalid("invalid data source"))?;
            let mut chosen = object(&current, "paths");
            chosen.remove(if key == "claude" { "claude-app" } else { key });
            current.insert("paths", chosen);
        }
        "path" => {
            let resolved = key
                .filter(|key| USAGE.contains(key))
                .and(value)
                .filter(|value| !value.is_empty())
                .map(paths::expand_user)
                .filter(|path| path.exists())
                .and_then(|path| paths::resolve(&path));
            let (Some(key), Some(resolved)) = (key, resolved) else {
                return Err(Error::Invalid("invalid data directory"));
            };
            let mut chosen = object(&current, "paths");
            chosen.insert(if key == "claude" { "claude-app" } else { key }, resolved.to_string_lossy().into_owned());
            current.insert("paths", chosen);
        }
        _ => return Err(Error::Invalid("invalid preference action")),
    }
    write(directory, current)?;
    Ok(read(directory))
}

#[cfg(test)]
mod lock_tests {
    use super::*;

    #[test]
    fn changes_made_at_once_all_stay() {
        // The app, the command line and the SwiftBar entry can each change the preferences at once: each change is a
        // read, a change and a write under the lock, so none is written over by another's older read.
        let directory = std::env::temp_dir().join(format!("starmeter-prefs-{}", crate::runtime::random_hex(8)));
        let keys = ["chatgpt", "claude-subscription", "cursor-subscription", "gemini-subscription"];
        let workers: Vec<_> = keys
            .iter()
            .map(|key| {
                let (directory, key) = (directory.clone(), key.to_string());
                std::thread::spawn(move || {
                    for n in 1..=25 {
                        update(&directory, "subscription-price", Some(&key), Some(&n.to_string())).expect("saved");
                    }
                })
            })
            .collect();
        for worker in workers {
            worker.join().expect("a worker");
        }
        let prices = read(&directory).get("subscriptionPrices").and_then(Json::as_object).cloned().unwrap_or_default();
        for key in keys {
            assert_eq!(prices.get(key).and_then(Json::number), Some(25.0), "{key}");
        }
        let _ = std::fs::remove_dir_all(&directory);
    }
}
