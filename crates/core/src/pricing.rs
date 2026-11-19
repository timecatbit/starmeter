//! Model prices. Codex and other vendors' models come from scripts/update_pricing.py's tables (the newest valid
//! copy among the one that ships and the one downloaded later); Claude's are fixed here from Anthropic's page.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex, RwLock};

use jiff::tz::TimeZone;

use crate::json::{Json, Object};
use crate::py;
use crate::time::IsoTime;

/// Folders holding codex-pricing.json and model-pricing.json, in the order the Python reference looks.
static FOLDERS: RwLock<Vec<PathBuf>> = RwLock::new(Vec::new());
static LOADED: LazyLock<Mutex<HashMap<String, Object>>> = LazyLock::new(Default::default);

pub fn set_folders(folders: Vec<PathBuf>) {
    *FOLDERS.write().expect("pricing folders") = folders;
    reset();
}

/// Sets the folders unless some were set already (the probe's, or an earlier runtime's).
pub fn ensure_folders(folders: Vec<PathBuf>) {
    let mut current = FOLDERS.write().expect("pricing folders");
    if current.is_empty() {
        *current = folders;
    }
}

pub fn reset() {
    LOADED.lock().expect("pricing cache").clear();
}

fn rate(value: Option<&Json>) -> bool {
    match value {
        Some(value) if value.as_bool().is_none() => {
            value.number().is_some_and(|number| number.is_finite() && (0.0..=10000.0).contains(&number))
        }
        _ => false,
    }
}

const FIELDS: [&str; 4] = ["input", "output", "cacheRead", "cacheWrite"];

fn valid_rates(price: Option<&Json>) -> bool {
    let Some(price) = price.and_then(Json::as_object) else { return false };
    if !FIELDS.iter().all(|field| rate(price.get(field))) {
        return false;
    }
    match price.get("longContext") {
        None | Some(Json::Null) => true,
        Some(tier) => tier.as_object().is_some_and(|fields| {
            FIELDS.iter().all(|field| rate(fields.get(field)))
                && matches!(fields.get("threshold"), Some(Json::Int(threshold)) if *threshold > 0)
        }),
    }
}

/// Downloaded tables are data only: every field is checked before a single price is used.
pub fn valid(name: &str, data: &Json) -> bool {
    let Some(table) = data.as_object() else { return false };
    let Some(source) = table.get("source").and_then(Json::as_object) else { return false };
    let commit = py::str(Some(source.get("commit").unwrap_or(&Json::from(""))));
    if commit.len() != 40 || !commit.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)) {
        return false;
    }
    let Some(models) = table.get("models").and_then(Json::as_object) else { return false };
    if models.is_empty()
        || models.len() > 5000
        || !models.iter().all(|(key, value)| !key.is_empty() && key.chars().count() <= 200 && valid_rates(Some(value)))
    {
        return false;
    }
    if name == "codex-pricing.json" {
        let aliases = table.get("aliases").and_then(Json::as_object);
        if !aliases.is_some_and(|aliases| aliases.iter().all(|(_, value)| value.as_str().is_some())) {
            return false;
        }
        let fallbacks = table.get("autoReviewFallbacks").and_then(Json::as_array);
        let fallback_ok = |row: &Json| {
            row.as_object().is_some_and(|row| {
                row.get("model").and_then(Json::as_str).is_some()
                    && is_day(&py::str(Some(row.get("releasedOn").unwrap_or(&Json::from("")))))
            })
        };
        if !fallbacks.is_some_and(|rows| rows.iter().all(fallback_ok)) {
            return false;
        }
        let multipliers = table.get("multipliers").and_then(Json::as_object);
        let tier_ok = |tier: &Json| {
            tier.as_object().is_some_and(|rates| {
                rates.iter().all(|(_, value)| {
                    let number = value.number().unwrap_or(0.0);
                    rate(Some(value)) && number > 0.0 && number <= 100.0
                })
            })
        };
        if !multipliers.is_some_and(|tiers| tiers.iter().all(|(_, tier)| tier_ok(tier))) {
            return false;
        }
    }
    true
}

fn is_day(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes.iter().enumerate().all(|(offset, byte)| offset == 4 || offset == 7 || byte.is_ascii_digit())
}

/// A table's stamp: ccusage's commit time (or a second past the last); 0 when unknown.
fn committed(table: &Object) -> f64 {
    let Some(text) = table.get("source").and_then(|source| source.get("committedAt")).filter(|text| text.is_truthy())
    else {
        return 0.0;
    };
    IsoTime::parse(&py::str(Some(text))).map_or(0.0, |time| time.timestamp(&TimeZone::system()))
}

/// The newest valid copy of a table, cached until `reset`; empty when no folder has a valid one.
pub fn table(name: &str) -> Object {
    let mut loaded = LOADED.lock().expect("pricing cache");
    if let Some(table) = loaded.get(name) {
        return table.clone();
    }
    let mut best: Option<Object> = None;
    for folder in FOLDERS.read().expect("pricing folders").iter() {
        let Some(table) = std::fs::read(folder.join(name))
            .ok()
            .and_then(|data| Json::parse(&data).ok())
            .filter(|value| valid(name, value))
            .and_then(|value| value.as_object().cloned())
        else {
            continue;
        };
        // max() keeps the first of equal stamps.
        if best.as_ref().is_none_or(|best| committed(&table) > committed(best)) {
            best = Some(table);
        }
    }
    let table = best.unwrap_or_default();
    loaded.insert(name.to_string(), table.clone());
    table
}

fn priced(price: &Object, event: &Object, multiplier: f64) -> f64 {
    let inputs = py::num(event.get("input"));
    let cached = py::num(event.get("cached"));
    let output = py::num(event.get("output"));
    let writes = py::num(event.get("writes"));
    let mut price = price;
    // Each event is one request, so its own context size selects the long-context tier.
    if let Some(tier) = price.get("longContext").and_then(Json::as_object).filter(|tier| !tier.is_empty())
        && inputs + cached > tier.get("threshold").and_then(Json::number).unwrap_or(f64::INFINITY)
    {
        price = tier;
    }
    let p = |key: &str| price.get(key).and_then(Json::number).unwrap_or(0.0);
    (inputs * p("input") + cached * p("cacheRead") + output * p("output") + writes * p("cacheWrite")) * multiplier / 1e6
}

/// The model name without a trailing date: -2026-01-31 or -20260131.
pub fn undated(model: &str) -> &str {
    let bytes = model.as_bytes();
    let n = bytes.len();
    let digits = |range: std::ops::Range<usize>| bytes[range].iter().all(u8::is_ascii_digit);
    if n >= 11
        && bytes[n - 11] == b'-'
        && digits(n - 10..n - 6)
        && bytes[n - 6] == b'-'
        && digits(n - 5..n - 3)
        && bytes[n - 3] == b'-'
        && digits(n - 2..n)
    {
        return &model[..n - 11];
    }
    if n >= 9 && bytes[n - 9] == b'-' && digits(n - 8..n) {
        return &model[..n - 9];
    }
    model
}

fn nonempty_object(value: Option<&Json>) -> Option<&Object> {
    value.and_then(Json::as_object).filter(|object| !object.is_empty())
}

/// API-equivalent estimate with ccusage's Codex rules; unknown models stay unpriced.
pub fn codex_cost(event: &Object) -> Option<f64> {
    let table = table("codex-pricing.json");
    let mut model = py::str(Some(&py::or(event.get("model"), Json::from(""))));
    if model == "codex-auto-review" {
        // The reviewer bills as the newest model released by the request's (UTC) day; a request read live is today's.
        let when = event.get("timestamp").and_then(Json::number).unwrap_or_else(now);
        let day = utc_day(when);
        model = table
            .get("autoReviewFallbacks")
            .and_then(Json::as_array)
            .and_then(|rows| {
                rows.iter().find(|row| day.as_str() >= row.get("releasedOn").and_then(Json::as_str).unwrap_or("~"))
            })
            .and_then(|row| row.get("model").and_then(Json::as_str))
            .unwrap_or("gpt-5")
            .to_string();
    }
    let base = undated(&model);
    let resolved = table.get("aliases").and_then(|aliases| aliases.get(base)).and_then(Json::as_str).unwrap_or(base);
    let models = table.get("models").and_then(Json::as_object).cloned().unwrap_or_default();
    let price = nonempty_object(models.get(resolved)).or_else(|| nonempty_object(models.get(base)))?;
    let tier = py::or(event.get("tier"), Json::from("standard"));
    let rates = tier
        .as_str()
        .and_then(|tier| table.get("multipliers").and_then(|tiers| tiers.get(tier)))
        .and_then(Json::as_object)
        .cloned()
        .unwrap_or_default();
    let multiplier =
        rates.get(resolved).and_then(Json::number).or_else(|| rates.get(base).and_then(Json::number)).unwrap_or(1.0);
    Some(priced(price, event, multiplier))
}

/// Other vendors' models (DeepSeek, GLM, Kimi, Gemini…) from the same snapshot, when the client logged no cost.
pub fn model_cost(event: &Object) -> Option<f64> {
    let models = table("model-pricing.json").get("models").and_then(Json::as_object).cloned().unwrap_or_default();
    let name = py::str(Some(&py::or(event.get("model"), Json::from("")))).to_lowercase();
    let model = name.rsplit('/').next().unwrap_or(&name);
    let price = nonempty_object(models.get(model)).or_else(|| nonempty_object(models.get(undated(model))))?;
    Some(priced(price, event, 1.0))
}

fn now() -> f64 {
    crate::files::time_nanoseconds() as f64 / 1e9
}

pub fn utc_day(seconds: f64) -> String {
    let (year, month, day) = py::civil_from_days((seconds / 86400.0).floor() as i64);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Claude's API prices per million tokens: input, output, 5-minute cache write, 1-hour cache write, cache read.
/// Source verified 2026-11-10: https://platform.claude.com/docs/en/about-claude/pricing
pub mod claude {
    pub const PRICING_DATE: &str = "2026-11-10";

    /// In the Python reference's order, which breaks ties between equally long names.
    const PRICES: [(&str, [f64; 5]); 15] = [
        ("claude-fable-5-2", [8.0, 40.0, 10.0, 16.0, 0.20]),
        ("claude-fable-5-1", [10.0, 50.0, 12.5, 20.0, 0.25]),
        ("claude-fable-5", [10.0, 50.0, 12.5, 20.0, 1.0]),
        ("claude-haiku-5-5", [0.1, 0.5, 0.125, 0.2, 0.01]),
        ("claude-opus-5-5", [4.0, 20.0, 5.0, 8.0, 0.20]),
        ("claude-sonnet-5-5", [2.0, 10.0, 2.5, 4.0, 0.10]),
        ("claude-opus-5", [5.0, 25.0, 6.25, 10.0, 0.5]),
        ("claude-opus-4-8", [5.0, 25.0, 6.25, 10.0, 0.5]),
        ("claude-opus-4-7", [5.0, 25.0, 6.25, 10.0, 0.5]),
        ("claude-opus-4-6", [5.0, 25.0, 6.25, 10.0, 0.5]),
        ("claude-opus-4-5", [5.0, 25.0, 6.25, 10.0, 0.5]),
        ("claude-sonnet-5", [2.0, 10.0, 2.5, 4.0, 0.2]),
        ("claude-sonnet-4-6", [3.0, 15.0, 3.75, 6.0, 0.3]),
        ("claude-sonnet-4-5", [3.0, 15.0, 3.75, 6.0, 0.3]),
        ("claude-haiku-4-5", [1.0, 5.0, 1.25, 2.0, 0.1]),
    ];
    const FAST: [&str; 4] = ["claude-fable-5-2", "claude-opus-5-5", "claude-opus-5", "claude-opus-4-8"];

    /// The API-equivalent cost of one response, or None for a model without a price. `metrics` are input,
    /// output, 5-minute cache writes, 1-hour cache writes and cache reads; Magpie's may be fractional.
    pub fn cost(model: &str, metrics: &[f64], speed: &str, geo: &str, tier: &str) -> Option<f64> {
        // Longest name first, so claude-fable-5-2 is tried before claude-fable-5; ties keep the table's order.
        let mut by_length: Vec<&(&str, [f64; 5])> = PRICES.iter().collect();
        by_length.sort_by_key(|(name, _)| std::cmp::Reverse(name.len()));
        let (name, rates) =
            by_length.into_iter().find(|(name, _)| model == *name || model.starts_with(&format!("{name}-20")))?;
        let metric = |index: usize| metrics.get(index).copied().unwrap_or(0.0);
        let mut factor = if geo == "us" || geo == "us-only" { 1.1 } else { 1.0 };
        if *name == "claude-haiku-5-5" && [0, 2, 3, 4].iter().map(|&index| metric(index)).sum::<f64>() > 100_000.0 {
            factor *= 5.0;
        }
        if tier == "batch" {
            factor *= 0.5;
        }
        if speed == "fast" {
            if !FAST.contains(name) {
                return None;
            }
            factor *= 2.0;
        }
        let total: f64 = metrics.iter().zip(rates).map(|(count, rate)| count * rate).sum();
        Some(total * factor / 1e6)
    }
}
