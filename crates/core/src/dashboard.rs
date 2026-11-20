//! The homepage's Summary, chart, heatmap and cards from a range bundle, in the JSON the panel reads.

use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;
use jiff::tz::TimeZone;

use crate::catalog;
use crate::json::{Json, Object};
use crate::preferences::{self, MODULES, USAGE};
use crate::range_usage::Wall;
use crate::runtime::{self, HOURLY_RANGES, PENDING_READ, RANGES, Runtime, SHORT_RANGES};
use crate::tally::add;
use crate::time::{days_from_civil, days_in};
use crate::{object, py};

const STREAK_RANGES: [&str; 5] = ["7d", "week", "30d", "month", "all"];
const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const HEATMAP_DAYS: i64 = 360;
const HEATMAP_ROW: i64 = 30;
pub const GROUPINGS: [(&str, &str); 3] = [("tool", "工具"), ("model", "模型"), ("project", "项目")];
const GATEWAYS: [&str; 2] = ["opencodex", "magpie"];
const GROUP_FIELDS: [&str; 8] = ["tokens", "cost", "requests", "unpriced", "output", "credits", "cached", "inputs"];
const MODEL_MAKERS: [(&str, &str); 19] = [
    ("claude", "claude"),
    ("gpt", "codex"),
    ("chatgpt", "codex"),
    ("codex", "codex"),
    ("o1", "codex"),
    ("o3", "codex"),
    ("o4", "codex"),
    ("gemini", "gemini"),
    ("deepseek", "deepseek-harness"),
    ("kimi", "kimi-api"),
    ("moonshot", "kimi-api"),
    ("grok", "grok"),
    ("minimax", "minimax"),
    ("abab", "minimax"),
    ("qwen", "qoder"),
    ("qfmodel", "qoder"),
    ("qoder", "qoder"),
    ("composer", "cursor"),
    ("cursor", "cursor"),
];

fn magpie_client_name(agent: &str) -> Option<&'static str> {
    Some(match agent {
        "codex" => "ChatGPT",
        "claude" | "claude-code" => "Claude Code",
        "claude-desktop" => "Claude",
        "opencode" => "OpenCode",
        "dsh" => "DeepSeek Harness",
        "gemini" | "gemini-cli" => "Gemini CLI",
        "agy" | "antigravity" => "Antigravity",
        "pi" => "Pi",
        "cursor" => "Cursor",
        "" => "未标注",
        _ => return None,
    })
}

pub fn brand_logo(brand: &str) -> Option<(&'static str, bool)> {
    Some(match brand {
        "ChatGPT" => ("openai.svg", true),
        "Claude" => ("claude.svg", false),
        "MiniMax" => ("minimax.svg", false),
        "Gemini" => ("gemini.svg", false),
        "Grok" => ("grok.svg", true),
        "Kimi" => ("kimi.svg", true),
        "Qoder" => ("qoder.svg", true),
        "Cursor" => ("cursor.svg", true),
        _ => return None,
    })
}

fn module_logo(key: &str) -> Option<(&'static str, bool)> {
    Some(match key {
        "magpie" => ("magpie.svg", true),
        "claude" => ("claude.svg", false),
        "codex" => ("openai.svg", true),
        "opencode" => ("opencode.svg", true),
        "workbuddy" => ("workbuddy.svg", true),
        "qoder" => ("qoder.svg", true),
        "gemini" => ("gemini.svg", false),
        "cursor" => ("cursor.svg", true),
        _ => return None,
    })
}

fn module_symbol(key: &str) -> &'static str {
    match key {
        "summary" => "chart.xyaxis.line",
        "heatmap" => "square.grid.3x3.fill",
        "opencodex" => "network",
        "komari" => "server.rack",
        "updates" => "arrow.down.circle",
        "sessions" => "terminal",
        _ => "square.grid.2x2",
    }
}

pub fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let value = u32::from(chunk[0]) << 16
            | u32::from(*chunk.get(1).unwrap_or(&0)) << 8
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for index in 0..4 {
            if index <= chunk.len() {
                output.push(ALPHABET[(value >> (18 - 6 * index) & 63) as usize] as char);
            } else {
                output.push('=');
            }
        }
    }
    output
}

// Small helpers

/// The dashboard's number(): float(value) within 0..1e20, else 0.0.
fn figure(value: Option<&Json>) -> f64 {
    value.and_then(Json::python_float).filter(|number| *number >= 0.0 && *number < 1e20).unwrap_or(0.0)
}

/// min(1, x) as Python returns it: the integer 1 when x is not smaller.
fn capped(value: f64) -> Json {
    if value < 1.0 { Json::Double(value) } else { Json::Int(1) }
}

/// round(x, 6) as the reference rounds: the exact value to six decimals.
fn round6(value: f64) -> f64 {
    format!("{value:.6}").parse().unwrap_or(value)
}

pub fn summary_cost_value(summary: &Object) -> Option<&Json> {
    ["estimatedCostUsd", "costUsd", "totalCostUsd", "totalCost", "cost"]
        .iter()
        .find_map(|key| summary.get(key).filter(|value| !value.is_null() && **value != Json::from("")))
}

fn refresh_failed(error: Option<&str>) -> bool {
    error.is_some_and(|error| error != PENDING_READ)
}

/// Python's sorted(key=...): ascending by key, equal keys keep their order.
fn stable_sorted<T: Clone>(items: &[T], key: impl Fn(&T) -> f64) -> Vec<T> {
    let mut indexed: Vec<(usize, &T)> = items.iter().enumerate().collect();
    indexed.sort_by(|(a_index, a), (b_index, b)| {
        let (x, y) = (key(a), key(b));
        if x != y { x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal) } else { a_index.cmp(b_index) }
    });
    indexed.into_iter().map(|(_, item)| item.clone()).collect()
}

/// A gateway's clients by their share of its tokens, largest first; one name per client.
fn client_rows(clients: Option<&Json>) -> Vec<Json> {
    let mut merged: IndexMap<String, f64> = IndexMap::new();
    for row in clients.and_then(Json::as_array).into_iter().flatten() {
        let agent =
            row.get("agent").map(|agent| py::str(Some(&py::or(Some(agent), Json::from(""))))).unwrap_or_default();
        let name = magpie_client_name(&agent).map(String::from).unwrap_or_else(|| py::str(row.get("agent")));
        *merged.entry(name).or_default() +=
            py::or(row.get("totalTokens"), Json::Int(0)).python_float().unwrap_or(0.0).max(0.0);
    }
    let total: f64 = merged.values().sum();
    if total <= 0.0 {
        return vec![];
    }
    let items: Vec<(String, f64)> = merged.into_iter().collect();
    stable_sorted(&items, |(_, tokens)| -tokens)
        .into_iter()
        .filter(|(_, tokens)| *tokens > 0.0)
        .map(|(name, tokens)| object! { "name" => name, "tokens" => tokens, "share" => tokens / total }.into())
        .collect()
}

/// Wall-clock text of a moment in the zone, by the strftime pieces the dashboard uses.
pub fn format(moment: f64, zone: &TimeZone, pattern: &str) -> String {
    let wall = Wall::at(moment, zone);
    pattern
        .replace("%Y", &format!("{:04}", wall.year))
        .replace("%m", &format!("{:02}", wall.month))
        .replace("%d", &format!("{:02}", wall.day))
        .replace("%H", &format!("{:02}", wall.seconds / 3600))
        .replace("%M", &format!("{:02}", wall.seconds / 60 % 60))
}

pub fn day_number(moment: f64, zone: &TimeZone) -> i64 {
    let wall = Wall::at(moment, zone);
    days_from_civil(wall.year, wall.month, wall.day)
}

pub fn iso(day: i64) -> String {
    let (year, month, day) = py::civil_from_days(day);
    format!("{year:04}-{month:02}-{day:02}")
}

pub fn day_from_iso(text: &str) -> Option<i64> {
    let parts: Vec<&str> = text.split('-').filter(|part| !part.is_empty()).collect();
    if parts.len() != 3 {
        return None;
    }
    Some(days_from_civil(parts[0].parse().ok()?, parts[1].parse().ok()?, parts[2].parse().ok()?))
}

fn weekday_name(day: i64) -> &'static str {
    WEEKDAYS[(((day % 7) + 7 + 3) % 7) as usize]
}

fn date_prefix(value: Option<&Json>) -> String {
    py::str(Some(value.unwrap_or(&Json::from("")))).chars().take(10).collect()
}

// Adding up a source's parts

/// Tallies of one source's records added up: summary fields, models, days and hours.
fn add_usage(parts: &[Object]) -> Object {
    let mut summary = Object::new();
    let mut tables: [(&str, &str, IndexMap<String, Object>); 3] = [
        ("models", "model", IndexMap::new()),
        ("days", "date", IndexMap::new()),
        ("hours", "timestamp", IndexMap::new()),
    ];
    let mut firsts: Vec<Json> = Vec::new();
    let add_into = |target: &mut Object, row: &Object, skip: &[&str]| {
        for (key, value) in row.iter() {
            if !skip.contains(&key.as_str()) && value.number().is_some() {
                let current = target.get(key).cloned().unwrap_or(Json::Int(0));
                target.insert(key.clone(), add(&current, value));
            }
        }
    };
    for part in parts {
        if let Some(first) = part.get("firstActivityAt").filter(|first| !first.is_null()) {
            firsts.push(first.clone());
        }
        add_into(
            &mut summary,
            &part.get("summary").and_then(Json::as_object).cloned().unwrap_or_default(),
            &["coverageRatio", "sessions"],
        );
        for (field, key, table) in tables.iter_mut() {
            for row in part.get(field).and_then(Json::as_array).into_iter().flatten() {
                let Some(fields) = row.as_object() else { continue };
                let id = py::dumps(fields.get(key).unwrap_or(&Json::Null));
                let target = table
                    .entry(id)
                    .or_insert_with(|| object! { *key => fields.get(key).cloned().unwrap_or(Json::Null) });
                add_into(target, fields, &[key]);
            }
        }
    }
    let requests = summary.get("requests").and_then(Json::number).unwrap_or(0.0);
    let unpriced = summary.get("unpricedRequests").and_then(Json::number).unwrap_or(0.0);
    summary.insert("coverageRatio", (requests - unpriced) / requests.max(1.0));
    let mut first = Json::Null;
    for value in firsts {
        if first.is_null() || value.number().unwrap_or(0.0) < first.number().unwrap_or(0.0) {
            first = value;
        }
    }
    let [(_, _, models), (_, _, days), (_, _, hours)] = tables;
    let rows = |table: IndexMap<String, Object>| Json::Array(table.into_values().map(Json::Object).collect());
    object! { "summary" => summary, "models" => rows(models), "days" => rows(days), "hours" => rows(hours), "firstActivityAt" => first }
}

pub type Selected = (String, String, Object);

/// What Summary adds for each selected source: matched request by request, a gateway counts what it carried and
/// the client adds only the rest. "usage" is null when Summary adds nothing for it.
pub fn summary_usage(selected: &[Selected]) -> HashMap<String, Object> {
    let available: HashSet<&str> = selected
        .iter()
        .filter(|(_, _, usage)| {
            usage.get("summary").is_some_and(Json::is_truthy) && !usage.get("error").is_some_and(Json::is_truthy)
        })
        .map(|(source, _, _)| source.as_str())
        .collect();
    let mut result = HashMap::new();
    for (source, _, usage) in selected {
        let mut carried = Object::new();
        for (name, count) in
            usage.get("carried").and_then(Json::as_object).into_iter().flat_map(|carried| carried.iter())
        {
            if count.is_truthy() && available.contains(name.to_lowercase().as_str()) {
                carried.insert(name.clone(), count.clone());
            }
        }
        // A gateway with nothing to show can't count what it carried, so the client counts those again.
        let missing: Vec<Object> = usage
            .get("carriedParts")
            .and_then(Json::as_object)
            .into_iter()
            .flat_map(|parts| parts.iter())
            .filter(|(name, _)| !carried.contains_key(name))
            .filter_map(|(_, part)| part.as_object().cloned())
            .collect();
        let base = usage.get("counted").and_then(Json::as_object).cloned();
        let counted = match &base {
            Some(base) if !base.is_empty() && !missing.is_empty() => {
                let mut all = vec![base.clone()];
                all.extend(missing);
                add_usage(&all)
            }
            _ => base.unwrap_or_default(),
        };
        // Without OpenCodex's own log the proxy stays the summary authority for the local ChatGPT records. The other
        // computers' part (多台电脑合并) is no local record: counted once where it was used, it counts here.
        let counted_requests =
            counted.get("summary").and_then(|summary| summary.get("requests")).is_some_and(Json::is_truthy);
        let others = usage
            .get("others")
            .and_then(Json::as_object)
            .filter(|others| others.get("summary").is_some_and(Json::is_truthy))
            .cloned();
        let mut entry = if source == "codex"
            && available.contains("opencodex")
            && !usage.get("proxyMatched").is_some_and(Json::is_truthy)
        {
            match &others {
                Some(others) => object! { "usage" => others.clone(), "proxy" => true, "others" => others.clone() },
                None => object! { "usage" => Json::Null, "proxy" => true },
            }
        } else if !carried.is_empty() && !counted_requests {
            object! { "usage" => Json::Null }
        } else {
            object! { "usage" => if carried.is_empty() { usage.clone() } else { counted.clone() } }
        };
        let empty = carried.is_empty();
        entry.insert("carried", carried);
        entry.insert("counted", if empty { Json::Null } else { Json::Object(counted) });
        result.insert(source.clone(), entry);
    }
    result
}

// Model and project cards

#[derive(Clone)]
struct Group {
    fields: IndexMap<&'static str, f64>,
    tools: IndexMap<String, f64>,
    tool_names: HashMap<String, String>,
}

impl Default for Group {
    fn default() -> Self {
        Group {
            fields: GROUP_FIELDS.iter().map(|field| (*field, 0.0)).collect(),
            tools: IndexMap::new(),
            tool_names: HashMap::new(),
        }
    }
}

impl Group {
    fn field(&self, name: &str) -> f64 {
        self.fields.get(name).copied().unwrap_or(0.0)
    }
}

/// One model or project row of a tool, added to its group.
fn group_add(groups: &mut IndexMap<String, Group>, key: String, tool: (&str, &str), row: &Object) {
    let num = |name: &str| py::or(row.get(name), Json::Int(0)).python_float().unwrap_or(0.0).max(0.0);
    let group = groups.entry(key).or_default();
    let tokens = num("totalTokens");
    let cached =
        if num("cacheReadInputTokens") != 0.0 { num("cacheReadInputTokens") } else { num("cachedInputTokens") };
    let inputs =
        if row.contains_key("cacheEligibleInputTokens") { num("cacheEligibleInputTokens") } else { num("inputTokens") };
    for (field, value) in [
        ("tokens", tokens),
        ("cost", num("estimatedCostUsd")),
        ("requests", num("requests")),
        ("unpriced", num("unpricedRequests")),
        ("output", num("outputTokens")),
        ("credits", num("credits")),
        ("cached", cached),
        ("inputs", inputs),
    ] {
        *group.fields.get_mut(field).expect("group field") += value;
    }
    let id = format!("{}\u{1F}{}", tool.0, tool.1);
    *group.tools.entry(id.clone()).or_default() += if tokens != 0.0 { tokens } else { num("credits") };
    group.tool_names.insert(id, tool.1.to_string());
}

/// A model or project card in the shape of a tool's, with the tools that make it up as its rows.
fn group_card(id: &str, name: &str, mark: Object, accent: Option<&str>, group: &Group) -> Json {
    let weight: f64 = group.tools.values().sum();
    let divisor = if weight != 0.0 { weight } else { 1.0 };
    let requests = group.field("requests") as i64;
    let unpriced = group.field("unpriced") as i64;
    let tokens = group.field("tokens");
    let credits = group.field("credits");
    let items: Vec<(String, f64)> = group.tools.iter().map(|(tool, amount)| (tool.clone(), *amount)).collect();
    let tools: Vec<Json> = stable_sorted(&items, |(_, amount)| -amount)
        .into_iter()
        .take(3)
        .map(|(tool, amount)| {
            let mut row = object! {
                "name" => group.tool_names.get(&tool).cloned().unwrap_or_else(|| tool.clone()),
                "tokens" => if tokens != 0.0 { Json::Double(amount) } else { Json::Int(0) },
                "share" => capped(amount / divisor),
            };
            if tokens == 0.0 {
                row.insert("credits", amount);
            }
            row.into()
        })
        .collect();
    let priced = requests > unpriced;
    let mut card = object! { "id" => id, "name" => name };
    for (key, value) in mark {
        card.insert(key, value);
    }
    card.insert("accent", accent.map(Json::from).unwrap_or(Json::Null));
    card.insert("subtitle", "");
    card.insert("status", if priced || credits != 0.0 { "已同步" } else { "费用待定价" });
    card.insert("available", true);
    card.insert("credits", if credits != 0.0 { Json::Double(credits) } else { Json::Null });
    card.insert("clients", Json::Null);
    card.insert("cost", if priced { Json::Double(group.field("cost")) } else { Json::Null });
    card.insert("tokens", tokens);
    card.insert("requests", requests);
    card.insert("output", group.field("output"));
    card.insert(
        "cache",
        if group.field("inputs") != 0.0 { capped(group.field("cached") / group.field("inputs")) } else { Json::Null },
    );
    card.insert("coverage", Json::Null);
    card.insert("sessions", 0);
    card.insert("unpriced", unpriced);
    card.insert("models", Json::Array(tools));
    card.insert("history", Json::Array(vec![]));
    card.insert("summaryIncluded", false);
    let note = if 0 < unpriced && unpriced < requests && credits == 0.0 {
        Json::from(format!("{unpriced} 次未定价"))
    } else {
        Json::Null
    };
    card.insert("priceNote", note);
    card.into()
}

/// Groups with requests, the costliest first: every one of them, the panel lists five and the rest a tap away.
fn ranked_groups(groups: &IndexMap<String, Group>) -> Vec<(String, Group)> {
    let mut order: Vec<(usize, (String, Group))> = groups
        .iter()
        .filter(|(_, group)| group.field("requests") != 0.0)
        .map(|(key, group)| (key.clone(), group.clone()))
        .enumerate()
        .collect();
    order.sort_by(|(a_index, (_, a)), (b_index, (_, b))| {
        for field in ["cost", "tokens", "credits"] {
            if a.field(field) != b.field(field) {
                return b.field(field).partial_cmp(&a.field(field)).unwrap_or(std::cmp::Ordering::Equal);
            }
        }
        a_index.cmp(b_index)
    });
    order.into_iter().map(|(_, item)| item).collect()
}

/// One name per model across tools: lower case, without a provider's path or a date stamp.
pub fn model_key(name: Option<&Json>) -> String {
    let lowered = py::str(Some(&py::or(name, Json::from("")))).trim().to_lowercase();
    let mut text = lowered.rsplit('/').next().unwrap_or(&lowered).to_string();
    let bytes = text.as_bytes();
    let n = bytes.len();
    if n >= 9
        && b"-@".contains(&bytes[n - 9])
        && bytes[n - 8] == b'2'
        && bytes[n - 7] == b'0'
        && bytes[n - 6..].iter().all(u8::is_ascii_digit)
    {
        text.truncate(n - 9);
    }
    if text.is_empty() { "未知模型".into() } else { text }
}

/// A model card's name: Claude's family and version (claude-opus-5-5 → Claude Opus 5.5), other names as they are.
pub fn model_title(key: &str) -> String {
    for family in ["opus", "sonnet", "haiku", "fable"] {
        let prefix = format!("claude-{family}-");
        if let Some(version) = key.strip_prefix(&prefix).filter(|version| !version.is_empty()) {
            let mut capital = family.to_string();
            capital[..1].make_ascii_uppercase();
            return format!("Claude {capital} {}", version.replace('-', "."));
        }
    }
    key.to_string()
}

/// The card's note for a module whose service has trouble right now, from its status page.
pub fn service_note(statuses: Option<&Json>, module: &str) -> Option<String> {
    let page = match module {
        "claude" | "claude-subscription" | "claude-api" => "claude",
        "codex" | "opencodex" | "chatgpt" | "chatgpt-api" => "openai",
        "cursor" | "cursor-subscription" => "cursor",
        _ => "",
    };
    let state = statuses?.get(page)?.as_object()?;
    if state.get("error").is_some_and(Json::is_truthy) {
        return None;
    }
    let note = match state.get("indicator")?.as_str()? {
        "minor" => "服务部分降级",
        "major" => "服务大范围故障",
        "critical" => "服务严重故障",
        "maintenance" => "服务维护中",
        _ => return None,
    };
    Some(note.to_string())
}

/// A heatmap square: what it adds up, its label, and whether it is still to come.
struct Cell {
    slot: String,
    label: String,
    later: bool,
}

/// Days in a row with usage: the run up to today (or yesterday, while today is still empty) and the longest.
fn streak_text(days: &HashSet<String>, today: i64) -> Option<String> {
    let mut used: Vec<i64> = days.iter().filter_map(|day| day_from_iso(day)).collect();
    used.sort();
    used.dedup();
    let (mut longest, mut run) = (0, 0);
    let mut previous: Option<i64> = None;
    for &day in &used {
        run = match previous {
            Some(before) if day - before == 1 => run + 1,
            _ => 1,
        };
        longest = longest.max(run);
        previous = Some(day);
    }
    let present: HashSet<i64> = used.into_iter().collect();
    let mut current = 0;
    let mut cursor = if present.contains(&today) { today } else { today - 1 };
    while present.contains(&cursor) {
        current += 1;
        cursor -= 1;
    }
    if longest < 2 {
        return None;
    }
    Some(if current >= 2 {
        format!("连续 {current} 天 · 最长 {longest} 天")
    } else {
        format!("最长连续 {longest} 天")
    })
}

impl Runtime {
    /// The installed assets: the hidden folder beside the entry, else one beside it.
    pub fn asset_dir(&self) -> String {
        let folder = runtime::parent_folder(&runtime::absolute(&self.plugin));
        runtime::installed(&folder, "assets", "logos")
            .unwrap_or_else(|| std::path::Path::new(&folder).join("assets").to_string_lossy().into_owned())
    }

    pub fn logo(&self, file: &str) -> Option<String> {
        self.memoized(&format!("logo:{file}"), || {
            std::fs::read(format!("{}/{file}", self.asset_dir()))
                .ok()
                .filter(|data| !data.is_empty())
                .map(|data| base64_encode(&data))
        })
    }

    /// A module's icon for the panel: an image (template when monochrome) or a system symbol.
    pub fn module_icon(&self, key: &str) -> Object {
        if let Some(service) = catalog::by_key(key) {
            let (raw, template) = if service.brand == "DeepSeek" {
                (self.logo("deepseek-logo.svg"), false)
            } else if let Some((file, mono)) = brand_logo(service.brand) {
                (self.logo(&format!("logos/{file}")), mono)
            } else {
                (None, false)
            };
            if let Some(raw) = raw {
                return object! { "image" => raw, "template" => template };
            }
            return object! { "symbol" => if service.category == "订阅" { "person.crop.circle" } else { "creditcard" } };
        }
        let (raw, template) = if key == "deepseek-harness" {
            (self.logo("deepseek-logo.svg"), false)
        } else if let Some((file, mono)) = module_logo(key) {
            (self.logo(&format!("logos/{file}")), mono)
        } else {
            (None, false)
        };
        if let Some(raw) = raw {
            return object! { "image" => raw, "template" => template };
        }
        object! { "symbol" => module_symbol(key) }
    }

    /// How API-equivalent estimates read: null for US dollars, else the yuan with its rate and where it came from.
    pub fn currency_unit(&self, data: &Object) -> Json {
        let settings = self.settings();
        if settings.get("currency") != Some(&Json::from("CNY")) {
            return Json::Null;
        }
        if let Some(rate) = settings.get("cnyRate").filter(|rate| rate.is_truthy())
            && let Some(value) = rate.number()
        {
            return object! { "code" => "CNY", "symbol" => "¥", "rate" => rate.clone(), "note" => format!("手动汇率 {value:.4}") }.into();
        }
        let known = data.get("exchange_rate").and_then(Json::as_object).cloned().unwrap_or_default();
        let Some(rate) =
            known.get("rate").filter(|rate| preferences::exchange_rate(Some(rate))).and_then(Json::python_float)
        else {
            return Json::Null;
        };
        let date = py::str(Some(known.get("date").unwrap_or(&Json::from(""))));
        object! { "code" => "CNY", "symbol" => "¥", "rate" => rate, "note" => format!("欧洲央行 {date} 参考汇率 {rate:.4}") }.into()
    }

    fn model_cards(&self, selected: &[Selected], counting: &HashMap<String, Object>) -> Vec<Json> {
        let mut groups: IndexMap<String, Group> = IndexMap::new();
        for (source, label, _) in selected {
            let Some(usage) = counting
                .get(source)
                .and_then(|part| part.get("usage"))
                .and_then(Json::as_object)
                .filter(|usage| !usage.is_empty() && !usage.get("error").is_some_and(Json::is_truthy))
            else {
                continue;
            };
            for row in usage.get("models").and_then(Json::as_array).into_iter().flatten() {
                if let Some(fields) = row.as_object() {
                    group_add(&mut groups, model_key(fields.get("model")), (label, label), fields);
                }
            }
        }
        ranked_groups(&groups)
            .iter()
            .map(|(key, group)| {
                let maker = MODEL_MAKERS.iter().find(|(prefix, _)| key.starts_with(prefix)).map(|(_, maker)| *maker);
                let mark = maker.map_or_else(|| object! { "symbol" => "cpu" }, |maker| self.module_icon(maker));
                group_card(&format!("model:{key}"), &model_title(key), mark, maker, group)
            })
            .collect()
    }

    fn project_cards(&self, selected: &[Selected]) -> Vec<Json> {
        let mut groups: IndexMap<String, Group> = IndexMap::new();
        for (source, label, usage) in selected {
            if GATEWAYS.contains(&source.as_str())
                || !usage.get("summary").is_some_and(Json::is_truthy)
                || usage.get("error").is_some_and(Json::is_truthy)
            {
                continue;
            }
            // A tool that keeps no project lands in 未标注项目 whole.
            let mut rows = usage.get("projects").and_then(Json::as_array).cloned().unwrap_or_default();
            if rows.is_empty()
                && let Some(summary) = usage.get("summary").and_then(Json::as_object)
            {
                let mut summary = summary.clone();
                summary.insert("project", "");
                rows = vec![summary.into()];
            }
            for row in &rows {
                if let Some(fields) = row.as_object() {
                    let project = py::str(Some(&py::or(fields.get("project"), Json::from(""))));
                    group_add(&mut groups, project, (source, label), fields);
                }
            }
        }
        let unmarked = groups.shift_remove("");
        let lead = |group: &Group| -> String {
            let mut best: Option<(&String, f64)> = None;
            for (tool, amount) in &group.tools {
                if best.is_none_or(|(_, top)| *amount > top) {
                    best = Some((tool, *amount));
                }
            }
            best.map(|(tool, _)| tool.split('\u{1F}').next().unwrap_or("").to_string()).unwrap_or_default()
        };
        let mut cards: Vec<Json> = ranked_groups(&groups)
            .iter()
            .map(|(key, group)| {
                group_card(&format!("project:{key}"), key, object! { "symbol" => "folder" }, Some(&lead(group)), group)
            })
            .collect();
        if let Some(unmarked) = unmarked.filter(|group| group.field("requests") != 0.0) {
            cards.push(group_card(
                "project:",
                "未标注项目",
                object! { "symbol" => "questionmark.folder" },
                Some(&lead(&unmarked)),
                &unmarked,
            ));
        }
        cards
    }

    // Heatmap

    /// The heatmap's squares for a range in reading order, the number of columns and a name for each row.
    fn heatmap_cells(&self, key: &str, now: f64, zone: &TimeZone, used: &[String]) -> (Vec<Cell>, i64, Vec<String>) {
        let wall = Wall::at(now, zone);
        let today = days_from_civil(wall.year, wall.month, wall.day);
        let name = |day: i64| {
            let (_, month, date) = py::civil_from_days(day);
            format!("{month}月{date}日 {}", weekday_name(day))
        };
        if HOURLY_RANGES.contains(&key) {
            let mut moments: Vec<(f64, i64, i64)> = Vec::new();
            let names: Vec<String> = if key == "24h" {
                let base = now.floor() as i64 / 3600 * 3600 - 23 * 3600;
                for i in 0..24 {
                    let moment = (base + 3600 * i) as f64;
                    let local = Wall::at(moment, zone);
                    moments.push((moment, days_from_civil(local.year, local.month, local.day), local.seconds / 3600));
                }
                vec!["24小时".into()]
            } else {
                let weekday = ((today % 7) + 7 + 3) % 7;
                let (back, rows) = match key {
                    "today" => (0, 1),
                    "yesterday" => (1, 1),
                    "7d" => (6, 7),
                    _ => (weekday, weekday + 1),
                };
                let first = today - back;
                for d in 0..rows {
                    let (year, month, day) = py::civil_from_days(first + d);
                    for h in 0..24 {
                        let mut at = Wall::date(year, month, day, zone);
                        at.seconds = h * 3600;
                        moments.push((at.timestamp(), first + d, h));
                    }
                }
                match key {
                    "today" => vec!["今天".into()],
                    "yesterday" => vec!["昨天".into()],
                    _ => (0..rows).map(|offset| weekday_name(first + offset).to_string()).collect(),
                }
            };
            let cells = moments
                .into_iter()
                .map(|(moment, day, hour)| Cell {
                    slot: (moment.floor() as i64 / 3600 * 3600).to_string(),
                    label: format!("{} {hour:02}:00", name(day)),
                    later: moment > now,
                })
                .collect();
            return (cells, 24, names);
        }
        let (first, count) = match key {
            "30d" => (today - 29, 30),
            "month" => (days_from_civil(wall.year, wall.month, 1), days_in(wall.month, wall.year)),
            _ => {
                let start = iso(today - (HEATMAP_DAYS - 1));
                let earliest = used.iter().filter(|day| **day >= start).min().and_then(|day| day_from_iso(day));
                let span = earliest.map_or(1, |day| today - day + 1);
                let count = HEATMAP_DAYS.min((span + HEATMAP_ROW - 1) / HEATMAP_ROW * HEATMAP_ROW);
                (today - (count - 1), count)
            }
        };
        let dates: Vec<i64> = (0..count).map(|offset| first + offset).collect();
        let names: Vec<String> = if key == "all" {
            // A row is named for the month it starts in, once per month.
            let starts: Vec<i64> = (0..count)
                .step_by(HEATMAP_ROW as usize)
                .map(|index| py::civil_from_days(dates[index as usize]).1)
                .collect();
            starts
                .iter()
                .enumerate()
                .map(
                    |(index, month)| {
                        if index == 0 || *month != starts[index - 1] { format!("{month}月") } else { String::new() }
                    },
                )
                .collect()
        } else {
            vec![if key == "30d" { "30天".into() } else { format!("{}月", wall.month) }]
        };
        let cells = dates.iter().map(|&day| Cell { slot: iso(day), label: name(day), later: day > today }).collect();
        (cells, if key == "all" { HEATMAP_ROW } else { count }, names)
    }

    /// The squares under Summary for the selected range, counted as Summary counts them.
    fn heatmap_payload(
        &self,
        bundle: &Object,
        key: &str,
        now: f64,
        zone: &TimeZone,
        selected: &[Selected],
        error: Option<&str>,
    ) -> Json {
        let hourly = HOURLY_RANGES.contains(&key);
        let mut totals: IndexMap<String, (f64, i64, f64, i64)> = IndexMap::new();
        let mut partial = false;
        let mut days: HashSet<String> = HashSet::new();
        let float = |value: Option<&Json>| py::or(value, Json::Int(0)).python_float().unwrap_or(0.0);
        let int = |value: Option<&Json>| float(value).trunc() as i64;
        let counting = summary_usage(selected);
        for (source, _, _) in selected {
            let usage = counting
                .get(source)
                .and_then(|part| part.get("usage"))
                .and_then(Json::as_object)
                .cloned()
                .unwrap_or_default();
            for row in usage.get("days").and_then(Json::as_array).into_iter().flatten() {
                if float(row.get("totalTokens")) > 0.0 || int(row.get("requests")) > 0 {
                    days.insert(date_prefix(row.get("date")));
                }
            }
            let rows =
                usage.get(if hourly { "hours" } else { "days" }).and_then(Json::as_array).cloned().unwrap_or_default();
            // A source whose hours couldn't be read (OpenCodex's timeline) leaves the squares short.
            if hourly
                && rows.is_empty()
                && usage.get("summary").and_then(|s| s.get("requests")).is_some_and(Json::is_truthy)
            {
                partial = true;
            }
            for row in &rows {
                let slot = if hourly {
                    (int(row.get("timestamp")) / 3600 * 3600).to_string()
                } else {
                    date_prefix(row.get("date"))
                };
                let total = totals.entry(slot).or_default();
                total.0 += float(row.get("totalTokens")).max(0.0);
                total.1 += int(row.get("requests"));
                total.2 += float(row.get("estimatedCostUsd")).max(0.0);
                total.3 += int(row.get("unpricedRequests"));
            }
        }
        let used: Vec<String> = if hourly {
            vec![]
        } else {
            totals.iter().filter(|(_, t)| t.0 > 0.0 || t.1 > 0).map(|(slot, _)| slot.clone()).collect()
        };
        let (cells, columns, names) = self.heatmap_cells(key, now, zone, &used);
        let values: Vec<(f64, i64, f64, i64)> =
            cells.iter().map(|cell| totals.get(&cell.slot).copied().unwrap_or_default()).collect();
        let status = if !bundle.is_empty() {
            ""
        } else if refresh_failed(error) {
            "数据暂不可用"
        } else {
            "待同步"
        };
        let streak = if STREAK_RANGES.contains(&key) && !bundle.is_empty() {
            streak_text(&days, day_number(now, zone)).map_or(Json::Null, Json::from)
        } else {
            Json::Null
        };
        object! {
            "columns" => columns, "rowLabels" => Json::from(names),
            "labels" => Json::Array(cells.iter().map(|cell| Json::from(cell.label.as_str())).collect()),
            "tokens" => Json::Array(values.iter().map(|v| Json::Double(v.0)).collect()),
            "requests" => Json::Array(values.iter().map(|v| Json::Int(v.1)).collect()),
            "costs" => Json::Array(values.iter().map(|v| Json::Double(round6(v.2))).collect()),
            "unpriced" => Json::Array(values.iter().map(|v| Json::Int(v.3)).collect()),
            "future" => cells.iter().filter(|cell| cell.later).count() as i64, "streak" => streak, "status" => status,
            "notice" => if partial { "部分小时数据不可用" } else { "" }, "hint" => if hourly { "每格 1 小时" } else { "每格 1 天" },
        }
        .into()
    }

    // The dashboard

    /// Everything the homepage draws for the range on screen. data holds the cached bundle and its error, the
    /// OpenCodex health, the exchange rate and the service statuses.
    pub fn dashboard_payload(&self, data: &Object, range_error: Option<&str>, now: f64) -> Object {
        let zone = self.local_zone();
        let key = data
            .get("displayed_range")
            .and_then(Json::as_str)
            .filter(|key| !key.is_empty())
            .map(String::from)
            .unwrap_or_else(|| self.selected_range());
        let mut bundle = data.get("range_usage").and_then(Json::as_object).cloned().unwrap_or_default();
        if bundle.get("range") != Some(&Json::from(key.as_str())) {
            bundle = Object::new();
        }
        let (mut start, mut end) = self.range_bounds(&key, now, &zone);
        start = bundle.get("start").and_then(Json::number).unwrap_or(start);
        end = bundle.get("end").and_then(Json::number).unwrap_or(end);
        let settings = self.settings();
        let selected: Vec<Selected> = MODULES
            .iter()
            .filter(|module| USAGE.contains(&module.key) && self.module_enabled(module.key))
            .map(|module| {
                (
                    module.key.to_string(),
                    module.label.clone(),
                    bundle.get(module.key).and_then(Json::as_object).cloned().unwrap_or_default(),
                )
            })
            .collect();
        let usage_sources: Vec<&Object> = selected.iter().map(|(_, _, usage)| usage).collect();
        let short = SHORT_RANGES.contains(&key.as_str());
        let mut labels: Vec<String>;
        let mut span: String;
        let mut period: Vec<String>;
        let slots: Vec<Json>;
        if short {
            let first = (start / 3600.0).floor() as i64 * 3600;
            let last = (end / 3600.0).floor() as i64 * 3600;
            let stamps: Vec<i64> = (0..).map(|step| first + step * 3600).take_while(|stamp| *stamp <= last).collect();
            slots = stamps.iter().map(|stamp| Json::Int(*stamp)).collect();
            labels = stamps.iter().map(|stamp| format(*stamp as f64, &zone, "%H:%M")).collect();
            if !labels.is_empty() {
                labels[0] = format(start, &zone, "%H:%M");
                let last_index = labels.len() - 1;
                labels[last_index] = format(end, &zone, "%H:%M");
            }
            span = format!(
                "{}–{}",
                format(start, &zone, "%m/%d %H:%M"),
                format(end, &zone, if key == "today" { "%H:%M" } else { "%m/%d %H:%M" })
            );
            period = [start, end].iter().map(|moment| format(*moment, &zone, "%Y-%m-%dT%H:%M")).collect();
            if key == "yesterday" {
                // A whole day: it ends at 24:00 and is written as its date.
                if let Some(last) = labels.last_mut() {
                    *last = "24:00".into();
                }
                let day = day_number(start, &zone);
                span = iso(day)[5..].replace('-', "/");
                period = vec![iso(day), iso(day)];
            }
        } else {
            let last = day_number(end, &zone);
            let earliest: Vec<String> = usage_sources
                .iter()
                .flat_map(|source| source.get("days").and_then(Json::as_array).cloned().unwrap_or_default())
                .filter(|day| {
                    day.get("date").is_some_and(Json::is_truthy)
                        && (figure(day.get("requests")) > 0.0 || figure(day.get("totalTokens")) > 0.0)
                })
                .map(|day| py::str(day.get("date")).chars().take(10).collect())
                .collect();
            let mut first = if start != 0.0 {
                day_number(start, &zone)
            } else {
                earliest.iter().min().and_then(|day| day_from_iso(day)).unwrap_or(last)
            };
            // The days the figures cover; the chart shows at most the last 366 of them.
            period = vec![iso(first), iso(last)];
            first = first.max(last - 365);
            slots = if first > last { vec![] } else { (first..=last).map(|day| Json::from(iso(day))).collect() };
            labels = slots.iter().map(|slot| slot.as_str().unwrap_or("")[5..].replace('-', "/")).collect();
            span = format!(
                "{}–{}",
                labels.first().cloned().unwrap_or_default(),
                labels.last().cloned().unwrap_or_default()
            );
        }
        let slot_key =
            |slot: &Json| slot.as_str().map(String::from).unwrap_or_else(|| slot.integer().unwrap_or(0).to_string());
        // A model the price tables don't know yet says so; Credits are never priced.
        let price_note = |row: &Object| -> Option<String> {
            let missing = figure(row.get("unpricedRequests")) as i64;
            if missing <= 0 || figure(row.get("credits")) != 0.0 {
                return None;
            }
            Some(if missing >= figure(row.get("requests")) as i64 {
                "未定价".into()
            } else {
                format!("{missing} 次未定价")
            })
        };
        let provider = |id: &str, label: &str, usage: &Object, subtitle: &str, given_status: &str| -> Object {
            let summary = usage.get("summary").and_then(Json::as_object).cloned().unwrap_or_default();
            let available = !summary.is_empty() && !usage.get("error").is_some_and(Json::is_truthy);
            let total = figure(summary.get("totalTokens"));
            let models: Vec<Object> = usage
                .get("models")
                .and_then(Json::as_array)
                .map(|rows| rows.iter().filter_map(|row| row.as_object().cloned()).collect())
                .unwrap_or_default();
            let credits = figure(summary.get("credits"));
            // Qoder CN plan requests log Credits with no tokens; rank and share those models by Credits instead.
            let by_credits = total == 0.0 && credits != 0.0;
            let weight =
                |row: &Object| if by_credits { figure(row.get("credits")) } else { figure(row.get("totalTokens")) };
            let scale = if by_credits { models.iter().map(weight).sum::<f64>().max(1e-9) } else { total.max(1.0) };
            let rows: Vec<Json> = stable_sorted(&models, |row| -weight(row))
                .into_iter()
                .take(3)
                .map(|row| {
                    let mut entry = object! {
                        "name" => py::str(Some(&py::or(row.get("model"), Json::from("未知模型")))),
                        "tokens" => figure(row.get("totalTokens")),
                        "share" => capped(weight(&row) / scale),
                    };
                    if figure(row.get("credits")) != 0.0 {
                        entry.insert("credits", figure(row.get("credits")));
                    }
                    if let Some(note) = price_note(&row) {
                        entry.insert("priceNote", note);
                    }
                    entry.into()
                })
                .collect();
            let mut points: HashMap<String, f64> = HashMap::new();
            for row in usage.get(if short { "hours" } else { "days" }).and_then(Json::as_array).into_iter().flatten() {
                let slot = if short {
                    (py::or(row.get("timestamp"), Json::Int(0)).python_float().unwrap_or(0.0).trunc() as i64)
                        .to_string()
                } else {
                    date_prefix(row.get("date"))
                };
                points.insert(slot, figure(row.get("totalTokens")));
            }
            let history: Vec<Json> = slots
                .iter()
                .map(|slot| points.get(&slot_key(slot)).map_or(Json::Int(0), |v| Json::Double(*v)))
                .collect();
            let inputs = figure(summary.get("cacheEligibleInputTokens").or(summary.get("inputTokens")));
            let cache = figure(Some(&py::or(
                summary.get("cacheReadInputTokens"),
                summary.get("cachedInputTokens").cloned().unwrap_or(Json::Null),
            )));
            let mut cost = summary_cost_value(&summary).map_or(Json::Null, |value| Json::Double(figure(Some(value))));
            let requests = figure(summary.get("requests")) as i64;
            let unpriced = figure(summary.get("unpricedRequests")) as i64;
            if requests != 0 && unpriced >= requests {
                cost = Json::Null;
            }
            let status = if !available {
                usage
                    .get("error")
                    .filter(|error| error.is_truthy())
                    .map(|error| py::str(Some(error)))
                    .unwrap_or_else(|| if bundle.is_empty() { "待同步".into() } else { "暂不可用".into() })
            } else if usage.get("refreshError").is_some_and(Json::is_truthy) {
                "刷新失败 · 保留原数据".into()
            } else if usage.get("localError").is_some_and(Json::is_truthy) {
                "本机记录未读取".into()
            } else if usage.get("partial").is_some_and(Json::is_truthy) {
                "部分记录未读取".into()
            } else if unpriced >= requests && requests != 0 && credits == 0.0 {
                "费用待定价".into()
            } else {
                given_status.to_string()
            };
            let mut card = object! { "id" => id, "name" => label };
            for (field, value) in self.module_icon(id) {
                card.insert(field, value);
            }
            let clients = client_rows(usage.get("clients"));
            card.insert("subtitle", subtitle);
            card.insert("status", status);
            card.insert("available", available);
            card.insert("credits", if credits != 0.0 { Json::Double(credits) } else { Json::Null });
            card.insert("clients", if clients.is_empty() { Json::Null } else { Json::Array(clients) });
            card.insert("cost", cost);
            card.insert("tokens", total);
            card.insert("requests", requests);
            card.insert("output", figure(summary.get("outputTokens")));
            card.insert("cache", if inputs != 0.0 { capped(cache / inputs) } else { Json::Null });
            // What the share is of: the cache-eligible input, so several tools' shares add up by it.
            card.insert("cacheInputs", inputs);
            card.insert(
                "coverage",
                summary
                    .get("coverageRatio")
                    .filter(|v| !v.is_null())
                    .map_or(Json::Null, |v| Json::Double(figure(Some(v)))),
            );
            card.insert("sessions", figure(summary.get("sessions")) as i64);
            card.insert("unpriced", unpriced);
            card.insert("models", Json::Array(rows));
            card.insert("history", Json::Array(history));
            // Some requests unpriced: the cost covers only the rest. All unpriced already reads 费用待定价; OpenCodex
            // shows its 已定价 share instead.
            let note = if 0 < unpriced && unpriced < requests && credits == 0.0 && id != "opencodex" {
                Json::from(format!("{unpriced} 次未定价"))
            } else {
                Json::Null
            };
            card.insert("priceNote", note);
            card
        };
        let mut providers: Vec<Object> = Vec::new();
        for (source, label, usage) in &selected {
            let status = if source == "opencodex" && data.get("healthz").is_some_and(Json::is_truthy) {
                "运行正常"
            } else {
                "已同步"
            };
            let mut card = provider(source, label, usage, "", status);
            // A service in trouble right now, from its official status page (服务状态 on).
            if settings.get("serviceStatus").is_some_and(Json::is_truthy) {
                card.insert(
                    "serviceNote",
                    service_note(data.get("service_status"), source).map_or(Json::Null, Json::from),
                );
            }
            providers.push(card);
        }
        // The client's card still shows all its own records, while Summary adds what summary_usage counts.
        let counting = summary_usage(&selected);
        for (index, (source, label, _)) in selected.iter().enumerate() {
            let part = &counting[source];
            let carried = part.get("carried").and_then(Json::as_object).cloned().unwrap_or_default();
            let names = carried.keys().cloned().collect::<Vec<_>>().join("、");
            if let Some(others) = part.get("others").and_then(Json::as_object) {
                // This computer's ChatGPT records defer to OpenCodex; the other computers' count.
                let counted = provider(source, label, others, "", "已同步");
                let kept: Object = ["cost", "tokens", "output", "requests", "unpriced", "cache", "cacheInputs", "history"]
                    .into_iter()
                    .map(|field| (field, counted.get(field).cloned().unwrap_or(Json::Null)))
                    .collect();
                providers[index].insert("summaryIncluded", true);
                providers[index].insert("counted", kept);
                providers[index].insert("subtitle", "本机记录未计入 Summary");
            } else if part.get("proxy").is_some_and(Json::is_truthy) {
                providers[index].insert("summaryIncluded", false);
                providers[index].insert("subtitle", "未计入 Summary");
            } else if part.get("usage").is_none_or(Json::is_null) {
                providers[index].insert("summaryIncluded", false);
                providers[index].insert("subtitle", format!("已由 {names} 计入"));
            } else {
                providers[index].insert("summaryIncluded", true);
                if !carried.is_empty() {
                    let counted_usage = part.get("counted").and_then(Json::as_object).cloned().unwrap_or_default();
                    let counted = provider(source, label, &counted_usage, "", "已同步");
                    let kept: Object = ["cost", "tokens", "output", "requests", "unpriced", "cache", "cacheInputs", "history"]
                        .into_iter()
                        .map(|field| (field, counted.get(field).cloned().unwrap_or(Json::Null)))
                        .collect();
                    providers[index].insert("counted", kept);
                    let total: i64 = carried.iter().map(|(_, count)| count.integer().unwrap_or(0)).sum();
                    providers[index].insert("subtitle", format!("{total} 次已由 {names} 计入"));
                }
            }
        }
        let history = |card: &Object| -> Vec<Json> {
            let source = card.get("counted").and_then(Json::as_object).unwrap_or(card);
            source.get("history").and_then(Json::as_array).cloned().unwrap_or_default()
        };
        let first_activity = |card: &Object| -> Option<Json> {
            let id = card.get("id").and_then(Json::as_str).unwrap_or("");
            let usage = bundle.get(id).and_then(Json::as_object).cloned().unwrap_or_default();
            let source = if card.contains_key("counted") {
                usage.get("counted").and_then(Json::as_object).cloned().unwrap_or_default()
            } else {
                usage
            };
            source.get("firstActivityAt").cloned()
        };
        // Summary's chart draws what each source counts, so both series are rebucketed alike: the cards' own
        // histories and the counted parts.
        let rebucket = |providers: &mut Vec<Object>, transform: &dyn Fn(&[Json]) -> Vec<Json>| {
            for card in providers.iter_mut() {
                let series = card.get("history").and_then(Json::as_array).cloned().unwrap_or_default();
                card.insert("history", Json::Array(transform(&series)));
                if let Some(mut counted) = card.get("counted").and_then(Json::as_object).cloned() {
                    let series = counted.get("history").and_then(Json::as_array).cloned().unwrap_or_default();
                    counted.insert("history", Json::Array(transform(&series)));
                    card.insert("counted", counted);
                }
            }
        };
        let mut chart_positions = Json::Null;
        // Crop only today's chart, never the statistics window, keeping a short zero lead-in before the first call.
        if key == "today" && !slots.is_empty() {
            let included: Vec<&Object> =
                providers.iter().filter(|card| card.get("summaryIncluded") == Some(&Json::Bool(true))).collect();
            let active: Vec<usize> = (0..slots.len())
                .filter(|&i| {
                    included.iter().any(|card| history(card).get(i).and_then(Json::number).unwrap_or(0.0) > 0.0)
                })
                .collect();
            if let Some(&first_index) = active.first() {
                let activity: Vec<f64> = included
                    .iter()
                    .filter_map(|card| first_activity(card))
                    .filter(|value| !value.is_null())
                    .map(|value| figure(Some(&value)))
                    .filter(|moment| start <= *moment && *moment <= end)
                    .collect();
                let mut first_call = activity
                    .iter()
                    .copied()
                    .reduce(f64::min)
                    .unwrap_or_else(|| start.max(slots[first_index].integer().unwrap_or(0) as f64));
                first_call = first_call.min(end - 0.001);
                let padding = 300f64.min(30f64.max((end - first_call) * 0.06));
                let chart_start = start.max(first_call - padding);
                // A midnight request still receives a tiny visual zero lead-in.
                let mut chart_times = vec![chart_start, first_call];
                let mut groups: Vec<Vec<usize>> = Vec::new();
                for (index, slot) in slots.iter().enumerate().skip(first_index) {
                    let stamp = (slot.integer().unwrap_or(0) as f64 + 3600.0).min(end);
                    if chart_times.len() > 2 && stamp <= *chart_times.last().expect("times") {
                        groups.last_mut().expect("groups").push(index);
                    } else {
                        chart_times.push(stamp);
                        groups.push(vec![index]);
                    }
                }
                let duration = (end - chart_start).max(0.001);
                let mut positions: Vec<Json> =
                    chart_times.iter().map(|time| Json::Double((time - chart_start) / duration)).collect();
                if positions[1].number().unwrap_or(0.0) < 0.01 {
                    let mut adjusted = vec![Json::Int(0), Json::Double(0.04)];
                    adjusted
                        .extend(positions[2..].iter().map(|p| Json::Double(0.04 + 0.96 * p.number().unwrap_or(0.0))));
                    positions = adjusted;
                }
                chart_positions = Json::Array(positions);
                rebucket(&mut providers, &|series: &[Json]| {
                    let mut out = vec![Json::Int(0), Json::Int(0)];
                    out.extend(groups.iter().map(|group| {
                        group.iter().fold(Json::Int(0), |sum, &i| add(&sum, series.get(i).unwrap_or(&Json::Int(0))))
                    }));
                    out
                });
                labels = chart_times.iter().map(|time| format(*time, &zone, "%H:%M")).collect();
                span = format!("{}–{}", format(chart_start, &zone, "%m/%d %H:%M"), format(end, &zone, "%H:%M"));
            }
        }
        // Keep long histories legible without dropping recorded totals from the cards.
        let mut unit = if short { "每小时".to_string() } else { "每天".to_string() };
        if slots.len() > 60 {
            let size = slots.len().div_ceil(60);
            let count = slots.len();
            rebucket(&mut providers, &|series: &[Json]| {
                (0..count)
                    .step_by(size)
                    .map(|begin| {
                        series[begin..(begin + size).min(series.len())]
                            .iter()
                            .fold(Json::Int(0), |sum, value| add(&sum, value))
                    })
                    .collect()
            });
            labels = (0..count).step_by(size).map(|index| labels[index].clone()).collect();
            unit = format!("每 {size} 天");
        }
        let mut chart_status = if bundle.is_empty() {
            if refresh_failed(range_error) { "数据暂不可用".to_string() } else { "待同步".to_string() }
        } else if usage_sources.iter().any(|source| {
            source.get("error").is_some_and(Json::is_truthy)
                || source.get("chartUnavailable").is_some_and(Json::is_truthy)
        }) {
            "部分图表数据不可用".to_string()
        } else {
            String::new()
        };
        if usage_sources.iter().any(|source| {
            source.get("historyTruncated").is_some_and(Json::is_truthy)
                || source.get("entriesTruncated").is_some_and(Json::is_truthy)
        }) {
            chart_status = "仅含现有记录".into();
        }
        if key == "all"
            && usage_sources.iter().any(|source| {
                let requests = figure(source.get("summary").and_then(|summary| summary.get("requests")));
                let counted: f64 = source
                    .get("days")
                    .and_then(Json::as_array)
                    .into_iter()
                    .flatten()
                    .map(|day| figure(day.get("requests")))
                    .sum();
                requests > counted
            })
        {
            chart_status = "图表展示最近 366 天".into();
        }
        // The Summary adds every shown source, so its time is the oldest of their last good reads.
        let reads: Vec<f64> = usage_sources
            .iter()
            .filter(|source| source.get("summary").is_some_and(Json::is_truthy))
            .map(|source| py::or(source.get("readAt"), bundle.get("_fetchedAt").cloned().unwrap_or(Json::Null)))
            .filter(Json::is_truthy)
            .filter_map(|value| value.number())
            .collect();
        let updated = match reads.iter().copied().reduce(f64::min) {
            Some(read) => {
                let pattern = if day_number(read, &zone) == day_number(now, &zone) { "%H:%M" } else { "%m/%d %H:%M" };
                format!("更新时间：{}", format(read, &zone, pattern))
            }
            None => String::new(),
        };
        // A phone's own grouping (在手机上看) for this reading, else the computer's.
        let grouping = data
            .get("displayed_group")
            .and_then(Json::as_str)
            .or_else(|| settings.get("groupBy").and_then(Json::as_str))
            .filter(|value| GROUPINGS.iter().any(|(key, _)| key == value))
            .unwrap_or("tool")
            .to_string();
        let cards = if bundle.is_empty() {
            Json::Null
        } else if grouping == "model" {
            Json::Array(self.model_cards(&selected, &counting))
        } else if grouping == "project" {
            Json::Array(self.project_cards(&selected))
        } else {
            Json::Null
        };
        let heatmap = if self.module_enabled("heatmap") && !selected.is_empty() {
            self.heatmap_payload(&bundle, &key, now, &zone, &selected, range_error)
        } else {
            Json::Null
        };
        let label = RANGES.iter().find(|(range, _)| *range == key).map_or("", |(_, label)| *label);
        let group_note = if grouping == "project" && !cards.is_null() {
            Json::from("按客户端自己的记录")
        } else {
            Json::Null
        };
        let mut payload = object! {
            "date" => iso(day_number(now, &zone)), "dates" => Json::from(labels),
            "providers" => Json::Array(providers.into_iter().map(Json::Object).collect()),
            "rangeKey" => key.as_str(), "rangeLabel" => label, "grouping" => grouping.as_str(),
            "groupings" => Json::Array(GROUPINGS.iter().map(|(key, label)| object! { "key" => *key, "label" => *label }.into()).collect()),
            "cards" => cards, "currency" => self.currency_unit(data), "groupNote" => group_note,
            "chartUnit" => unit, "chartSpan" => span, "period" => Json::from(period), "chartStatus" => chart_status,
            "updated" => updated, "chartPositions" => chart_positions, "heatmap" => heatmap,
            "showSummary" => self.module_enabled("summary"),
            "allowCollapse" => settings.get("allowCollapse").cloned().unwrap_or(Json::Bool(false)),
            "expandedProviders" => settings.get("expandedProviders").cloned().unwrap_or(Json::Array(vec![])),
        };
        // 多台电脑合并: how many other computers' figures are in, said beside the update time.
        if let Some(devices) = bundle.get("devices").and_then(Json::integer).filter(|devices| *devices > 0) {
            payload.insert("devices", devices);
        }
        payload
    }
}
