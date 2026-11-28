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
pub fn committed(table: &Object) -> f64 {
    let Some(text) = table.get("source").and_then(|source| source.get("committedAt")).filter(|text| text.is_truthy())
    else {
        return 0.0;
    };
    IsoTime::parse(&py::str(Some(text))).map_or(0.0, |time| time.timestamp(&TimeZone::system()))
}

/// The prices in effect, as a short digest: both tables as read and this build (Claude's prices are built in), so a
/// cost kept from other prices is counted again.
pub fn fingerprint() -> String {
    let tables = ["model-pricing.json", "codex-pricing.json"].map(|name| Json::Object(table(name)).serialized());
    let text = format!("{}\n{}\n{}", tables[0], tables[1], env!("CARGO_PKG_VERSION"));
    py::sha256_hex(text.as_bytes())[..16].to_string()
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

/// Trailing colon tags added by routers/gateways (e.g. `:free`, `:thinking`, `:nitro`).
const COLON_TAGS: [&str; 17] = [
    "free", "thinking", "nitro", "online", "exacto", "extended", "floor", "beta", "preview", "fast", "high", "medium",
    "low", "xhigh", "standard", "flex", "priority",
];

/// Explicit allowlist of trailing hyphenated region/channel/effort/release modifiers that may be stripped when an exact
/// price entry is absent. Arbitrary segments (such as parameter sizes `1.5b`/`8b`, architectures `qwen`/`llama`, or
/// `distill`/`pro`/`flash`/`mini`) are NEVER stripped, preventing unlisted distilled or sibling models from inheriting
/// a differently priced base model's rate (`PRICE-DISTILL-1`).
const TRAILING_MODIFIERS: [&str; 42] = [
    // Region / deployment suffixes
    "sg",
    "cn",
    "us",
    "eu",
    "hk",
    "jp",
    "kr",
    "uk",
    "au",
    "in",
    "global",
    "intl",
    "east",
    "west",
    // Effort / release / routing tags
    "low",
    "medium",
    "high",
    "xhigh",
    "latest",
    "preview",
    "exp",
    "beta",
    "alpha",
    "ga",
    "original",
    "thinking",
    "nothinking",
    "free",
    "online",
    "standard",
    "flex",
    "priority",
    "interactive",
    "abliterated",
    "uncensored",
    "cyber",
    "speed",
    "fast",
    "economy",
    "ultrafast",
    "turbo",
    "nitro",
];

fn is_date_tag(segment: &str) -> bool {
    matches!(segment.len(), 4 | 6 | 8) && segment.bytes().all(|b| b.is_ascii_digit())
}

fn strip_leading_dot_prefix(s: &str) -> Option<&str> {
    let (prefix, rest) = s.split_once('.')?;
    if prefix.len() >= 2
        && prefix.bytes().all(|b| b.is_ascii_alphabetic() || b == b'_')
        && rest.len() >= 3
        && rest.bytes().next().is_some_and(|b| b.is_ascii_alphabetic())
    {
        Some(rest)
    } else {
        None
    }
}

/// Yields candidate names by peeling off one explicit wrapper layer at a time (`/`, `@`, `:`, leading alphabetic `.`)
/// so a namespaced table entry like `qwen.qwen3-coder-next` or `moonshot.kimi-k2-thinking` is checked right after
/// stripping `gateway/` instead of being skipped (`PRICE-WRAP-1`).
fn wrapper_candidates(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut push = |value: String| {
        if !value.is_empty() && !out.contains(&value) {
            out.push(value);
        }
    };
    let mut s = raw.trim().to_lowercase();
    push(s.clone());
    if let Some((_, tail)) = s.rsplit_once('/') {
        s = tail.to_string();
        push(s.clone());
    }
    if let Some((head, _)) = s.split_once('@') {
        s = head.to_string();
        push(s.clone());
    }
    while let Some((head, tail)) = s.rsplit_once(':') {
        if COLON_TAGS.contains(&tail) {
            s = head.to_string();
            push(s.clone());
        } else {
            break;
        }
    }
    if let Some((_, tail)) = s.rsplit_once(':')
        && !tail.is_empty()
    {
        s = tail.to_string();
        push(s.clone());
    }
    while let Some(rest) = strip_leading_dot_prefix(&s) {
        s = rest.to_string();
        push(s.clone());
    }
    out
}

/// Fully unwrapped model name (used for family prefix routing such as `claude-` / `gpt-`).
pub fn normalize_model(raw: &str) -> String {
    wrapper_candidates(raw).into_iter().last().unwrap_or_default()
}

/// Swaps version separators between digits (`v4.1` <-> `v4-1`).
fn swap_version_sep(model: &str, from: u8, to: u8) -> String {
    let bytes = model.as_bytes();
    let mut out = bytes.to_vec();
    for i in 1..bytes.len().saturating_sub(1) {
        if bytes[i] == from && bytes[i - 1].is_ascii_digit() && bytes[i + 1].is_ascii_digit() {
            out[i] = to;
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| model.to_string())
}

fn lookup_exact_or_variant(models: &Object, candidate: &str) -> Option<String> {
    let base = undated(candidate);
    for text in [candidate, base] {
        if nonempty_object(models.get(text)).is_some() {
            return Some(text.to_string());
        }
        let dashed = swap_version_sep(text, b'.', b'-');
        if dashed != text && nonempty_object(models.get(&dashed)).is_some() {
            return Some(dashed);
        }
        let dotted = swap_version_sep(text, b'-', b'.');
        if dotted != text && nonempty_object(models.get(&dotted)).is_some() {
            return Some(dotted);
        }
    }
    None
}

/// Resolves a model key in `models` layer-by-layer: after peeling each explicit wrapper (`/`, `@`, `:`, leading `.`),
/// checks exact/undated/version-separator matches before stripping further (`PRICE-WRAP-1`). Only if no wrapped or
/// unwrapped exact match exists does it strip explicit trailing region/effort/date modifiers (`PRICE-DISTILL-1`).
pub fn lookup_key(models: &Object, raw: &str) -> Option<String> {
    let candidates = wrapper_candidates(raw);
    for candidate in &candidates {
        if let Some(hit) = lookup_exact_or_variant(models, candidate) {
            return Some(hit);
        }
    }
    for candidate in &candidates {
        let mut cur = undated(candidate).to_string();
        while let Some((head, tail)) = cur.rsplit_once('-') {
            if (!TRAILING_MODIFIERS.contains(&tail) && !is_date_tag(tail)) || !head.contains('-') {
                break;
            }
            if let Some(hit) = lookup_exact_or_variant(models, head) {
                return Some(hit);
            }
            cur = head.to_string();
        }
    }
    None
}

/// API-equivalent estimate with ccusage's Codex rules; unknown models stay unpriced.
pub fn codex_cost(event: &Object) -> Option<f64> {
    let table = table("codex-pricing.json");
    let raw = py::str(Some(&py::or(event.get("model"), Json::from(""))));
    let mut model = normalize_model(&raw);
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
    let matched = lookup_key(&models, resolved).or_else(|| lookup_key(&models, base))?;
    let price = nonempty_object(models.get(&matched))?;
    let tier = py::or(event.get("tier"), Json::from("standard"));
    let rates = tier
        .as_str()
        .and_then(|tier| table.get("multipliers").and_then(|tiers| tiers.get(tier)))
        .and_then(Json::as_object)
        .cloned()
        .unwrap_or_default();
    let multiplier = rates
        .get(&matched)
        .and_then(Json::number)
        .or_else(|| rates.get(resolved).and_then(Json::number))
        .or_else(|| rates.get(base).and_then(Json::number))
        .unwrap_or(1.0);
    Some(priced(price, event, multiplier))
}

/// Other vendors' models (DeepSeek, GLM, Kimi, Gemini…) from the same snapshot, when the client logged no cost.
/// Also handles gateway-wrapped OpenAI/Codex models logged through non-Codex harnesses.
pub fn model_cost(event: &Object) -> Option<f64> {
    let raw = py::str(Some(&py::or(event.get("model"), Json::from(""))));
    let models = table("model-pricing.json").get("models").and_then(Json::as_object).cloned().unwrap_or_default();
    if let Some(matched) = lookup_key(&models, &raw)
        && let Some(price) = nonempty_object(models.get(&matched))
    {
        return Some(priced(price, event, 1.0));
    }
    let norm = normalize_model(&raw);
    if ["gpt-", "o1", "o3", "o4", "codex-"].iter().any(|prefix| norm.starts_with(prefix)) {
        return codex_cost(event);
    }
    None
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object;

    #[test]
    fn normalizes_wrapped_model_names_without_crossing_pricing_tiers() {
        assert_eq!(normalize_model("cn:deepseek-v4.1-flash-1017"), "deepseek-v4.1-flash-1017");
        assert_eq!(normalize_model("workbuddy/deepseek-v4.1-flash"), "deepseek-v4.1-flash");
        assert_eq!(normalize_model("ai.deepseek-v4-pro"), "deepseek-v4-pro");
        assert_eq!(normalize_model("myapi.gpt-5.5-pro"), "gpt-5.5-pro");
        assert_eq!(normalize_model("oii/gpt-5.6-sol"), "gpt-5.6-sol");
        assert_eq!(normalize_model("grok-4.7@500k"), "grok-4.7");
        assert_eq!(normalize_model("cn:deepseek-v4.1-flash:thinking"), "deepseek-v4.1-flash");
        assert_eq!(normalize_model("us.anthropic.claude-sonnet-4-5"), "claude-sonnet-4-5");
        // Version dots must stay intact.
        assert_eq!(normalize_model("gpt-5.5"), "gpt-5.5");
        assert_eq!(normalize_model("deepseek-v4.1-flash"), "deepseek-v4.1-flash");

        let table = object! {
            "deepseek-v4.1-flash" => object! { "input" => 0.1, "output" => 0.2, "cacheRead" => 0.01, "cacheWrite" => 0.0 },
            "deepseek-v4-pro" => object! { "input" => 1.0, "output" => 2.0, "cacheRead" => 0.1, "cacheWrite" => 0.0 },
            "deepseek-r1" => object! { "input" => 0.574, "output" => 2.19, "cacheRead" => 0.14, "cacheWrite" => 0.0 },
            "deepseek-r1-distill-qwen-7b" => object! { "input" => 0.072, "output" => 0.14, "cacheRead" => 0.0, "cacheWrite" => 0.0 },
            "qwen.qwen3-coder-next" => object! { "input" => 0.50, "output" => 1.50, "cacheRead" => 0.05, "cacheWrite" => 0.0 },
            "qwen3-coder-next" => object! { "input" => 0.2054, "output" => 0.80, "cacheRead" => 0.02, "cacheWrite" => 0.0 },
            "moonshot.kimi-k2-thinking" => object! { "input" => 0.60, "output" => 2.50, "cacheRead" => 0.15, "cacheWrite" => 0.0 },
            "kimi-k2-thinking" => object! { "input" => 0.40, "output" => 1.50, "cacheRead" => 0.10, "cacheWrite" => 0.0 },
            "grok-4.7" => object! { "input" => 3.0, "output" => 15.0, "cacheRead" => 0.3, "cacheWrite" => 0.0 },
            "gpt-5" => object! { "input" => 1.25, "output" => 10.0, "cacheRead" => 0.125, "cacheWrite" => 0.0 },
        };
        assert_eq!(lookup_key(&table, "cn:deepseek-v4.1-flash-sg").as_deref(), Some("deepseek-v4.1-flash"));
        assert_eq!(lookup_key(&table, "deepseek-v4-1-flash-cn").as_deref(), Some("deepseek-v4.1-flash"));
        assert_eq!(lookup_key(&table, "ai.deepseek-v4-pro").as_deref(), Some("deepseek-v4-pro"));
        assert_eq!(lookup_key(&table, "grok-4.7@500k").as_deref(), Some("grok-4.7"));
        // PRICE-WRAP-1: gateway path preserves exact namespaced pricing entries instead of skipping to unnamespaced keys.
        assert_eq!(lookup_key(&table, "qwen.qwen3-coder-next").as_deref(), Some("qwen.qwen3-coder-next"));
        assert_eq!(lookup_key(&table, "gateway/qwen.qwen3-coder-next").as_deref(), Some("qwen.qwen3-coder-next"));
        assert_eq!(lookup_key(&table, "moonshot.kimi-k2-thinking").as_deref(), Some("moonshot.kimi-k2-thinking"));
        assert_eq!(
            lookup_key(&table, "gateway/moonshot.kimi-k2-thinking").as_deref(),
            Some("moonshot.kimi-k2-thinking")
        );
        // PRICE-DISTILL-1: listed distilled models match, unlisted distilled sizes stay unpriced (never fall back to full R1).
        assert_eq!(lookup_key(&table, "deepseek-r1-distill-qwen-7b").as_deref(), Some("deepseek-r1-distill-qwen-7b"));
        assert_eq!(lookup_key(&table, "deepseek-r1-distill-qwen-1.5b"), None);
        assert_eq!(lookup_key(&table, "deepseek-r1-distill-llama-8b"), None);
        // Guardrails: never drop a pricing-tier keyword or match bare/ambiguous families.
        assert_eq!(lookup_key(&table, "deepseek-v4"), None);
        assert_eq!(lookup_key(&table, "gpt-5-super-pro"), None);
        assert_eq!(lookup_key(&table, "gpt-5-unknown-mini"), None);
        assert_eq!(lookup_key(&table, "default-model"), None);
    }
}
