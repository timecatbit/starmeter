//! The menu rows SwiftBar shows and the panel parses back into entries: one line each, with "| key=value" options.

use crate::catalog;
use crate::dashboard::{self, day_number};
use crate::json::{Json, Object};
use crate::menu_icons::menu_icon;
use crate::runtime::{PENDING_READ, Runtime, now};
use crate::{object, py};

pub type Options = Object;

pub const RANK_COLOR: &str = "#24272D,#F2F3F5";
pub const MUTED_COLOR: &str = "#6A707A,#ABAFB7";
pub const KOMARI_ONLINE: &str = "#2F7D50,#8AC6A4";
pub const WARN_COLOR: &str = "#B54708,#FEC84B";
pub const BAD_COLOR: &str = "#B42318,#FDA29B";
pub const MENU_ICON_SIZE: i64 = 14;
pub const FALLBACK_SEPARATOR: &str = "\u{1F}";

/// SwiftBar splits options on bars and lines, so dynamic text holds neither; leading hyphens can't fake nesting.
pub fn sanitize(value: &str) -> String {
    let replaced: String = value
        .chars()
        .map(|character| match character {
            '|' => '｜',
            _ if (character as u32) < 32 => ' ',
            _ => character,
        })
        .collect();
    let text = replaced.trim();
    let stripped = text.trim_start_matches('-');
    let hyphens = text.chars().count() - stripped.chars().count();
    format!("{}{stripped}", "‑".repeat(hyphens))
}

pub fn as_option(key: &str, value: &Json) -> String {
    if let Some(flag) = value.as_bool() {
        return format!("{key}={}", if flag { "true" } else { "false" });
    }
    // An option can't span lines, or a tooltip would leak out as a grey row.
    let text = py::str(Some(value))
        .replace('\r', "\n")
        .split('\n')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    if !text.is_empty() && !text.contains([' ', '\t', '"']) {
        return format!("{key}={text}");
    }
    format!("{key}=\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// One menu row; its level is how deeply it nests (SwiftBar's -- prefixes).
pub fn item(text: &str, level: usize, given: Options) -> String {
    let mut options = given;
    if !options.contains_key("color") {
        options.insert("color", Json::Null);
    }
    if !options.contains_key("size") {
        options.insert("size", 12);
    }
    if !text.is_empty() && level == 0 {
        options.remove("tooltip");
    }
    if !text.is_empty()
        && let Some(icon) = options.get("sfimage").and_then(Json::as_str).and_then(menu_icon)
    {
        options.remove("sfimage");
        options.insert("templateimage", icon);
        for key in ["sfsize", "width", "height"] {
            options.remove(key);
        }
    }
    if !text.is_empty() && options.get("sfimage").is_some_and(Json::is_truthy) && !options.contains_key("sfsize") {
        options.insert("sfsize", 13);
    }
    // None and False drop out; 0 and "" stay, as `is not None and is not False` keeps them.
    let attributes: Vec<String> = options
        .iter()
        .filter(|(_, value)| !value.is_null() && value.as_bool() != Some(false))
        .map(|(key, value)| as_option(key, value))
        .collect();
    let mut line = format!("{}{}", "--".repeat(level), sanitize(text));
    if !attributes.is_empty() {
        line += " | ";
        line += &attributes.join(" ");
    }
    line
}

pub fn merged(parts: &[&Options]) -> Options {
    let mut result = Options::new();
    for part in parts {
        for (key, value) in part.iter() {
            result.insert(key.clone(), value.clone());
        }
    }
    result
}

pub fn fmt_tokens(value: Option<&Json>) -> String {
    let Some(number) = value.and_then(Json::python_float) else { return "—".into() };
    if number >= 1e9 {
        format!("{:.2}B", number / 1e9)
    } else if number >= 1e6 {
        format!("{:.2}M", number / 1e6)
    } else if number >= 1e3 {
        format!("{:.1}k", number / 1e3)
    } else {
        (number.trunc() as i64).to_string()
    }
}

pub fn shorten(value: &str, limit: usize) -> String {
    let text = sanitize(value);
    if text.chars().count() <= limit {
        text
    } else {
        format!("{}…", text.chars().take(limit - 1).collect::<String>())
    }
}

/// Wide and fullwidth characters take two columns, as unicodedata's W, F and A classes do in a terminal.
fn display_width(character: char) -> usize {
    const WIDE: [(u32, u32); 24] = [
        (0x1100, 0x115F),
        (0x2E80, 0x303E),
        (0x3041, 0x33FF),
        (0x3400, 0x4DBF),
        (0x4E00, 0x9FFF),
        (0xA000, 0xA4CF),
        (0xAC00, 0xD7A3),
        (0xF900, 0xFAFF),
        (0xFE30, 0xFE4F),
        (0xFF00, 0xFF60),
        (0xFFE0, 0xFFE6),
        (0x1F300, 0x1F64F),
        (0x1F900, 0x1F9FF),
        (0x20000, 0x3FFFD),
        (0x00A1, 0x00A1),
        (0x00B0, 0x00B1),
        (0x00D7, 0x00D7),
        (0x00F7, 0x00F7),
        (0x2018, 0x2019),
        (0x201C, 0x201D),
        (0x2026, 0x2026),
        (0x2030, 0x2030),
        (0x25A0, 0x25FF),
        (0x2600, 0x26FF),
    ];
    let value = character as u32;
    if WIDE.iter().any(|(low, high)| (*low..=*high).contains(&value)) { 2 } else { 1 }
}

pub fn pad_display(value: &str, width: usize) -> String {
    let text = sanitize(value);
    let used: usize = text.chars().map(display_width).sum();
    format!("{text}{}", " ".repeat(width.saturating_sub(used)))
}

fn strip_zeros(text: &str) -> String {
    if text.contains('.') { text.trim_end_matches('0').trim_end_matches('.').to_string() } else { text.to_string() }
}

/// Python's '%g'.
pub fn g(value: f64) -> String {
    if value == 0.0 {
        return if value.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    let exponent = value.abs().log10().floor() as i32;
    if !(-4..6).contains(&exponent) {
        let text = format!("{value:.5e}");
        let (mantissa, power) = text.split_once('e').unwrap_or((&text, "0"));
        let power: i32 = power.parse().unwrap_or(0);
        return format!("{}e{}{:02}", strip_zeros(mantissa), if power < 0 { '-' } else { '+' }, power.unsigned_abs());
    }
    strip_zeros(&format!("{value:.*}", (5 - exponent).max(0) as usize))
}

/// Python's round(value, digits): the exact value to that many decimals.
pub fn round(value: f64, digits: usize) -> f64 {
    format!("{value:.digits$}").parse().unwrap_or(value)
}

/// A window whose share left is a number: one missing or null is no reading, not a full window (shown as 已用 100%
/// in red it said the plan was spent).
pub fn known_share(window: &Object) -> bool {
    window.get("remaining").filter(|value| value.as_bool().is_none()).and_then(Json::number).is_some()
}

/// 订阅额度框架 (AGENTS.md): once the windows are used up, what is left to spend (Credits, extra usage, on-demand)
/// is the homepage's value, in orange: still usable, on the reserve. Red is for used up with nothing left.
pub fn reserve_level(stale: bool) -> Json {
    if stale { Json::Null } else { Json::from("warning") }
}

/// On the reserve the value is what is left to spend; the week's share used goes under it, beside it on the row
/// (用户：字有点多……优化下布局): the bars' names say it when there are two, a line under the single bar when there is
/// one window.
pub fn reserve_week(on_reserve: bool, limits: &[(String, f64, bool)], headline: Option<&(f64, String)>) -> Json {
    match headline {
        Some((_, text)) if on_reserve && headline_pair(limits).is_empty() => Json::from(text.as_str()),
        _ => Json::Null,
    }
}

/// On the reserve the value is what is left to spend, not 已用 X%, so the share used goes beside it for the row's bar
/// (with one window there is no pair of bars to show it; 用户：ChatGPT 的订阅用量条怎么没了).
pub fn reserve_share(on_reserve: bool, used: Option<f64>) -> Json {
    match used.filter(|_| on_reserve) {
        Some(used) => Json::from(g(round(used.clamp(0.0, 100.0), 1))),
        None => Json::Null,
    }
}

pub fn usage_level(percent: Option<f64>, stale: bool) -> Json {
    match percent {
        Some(percent) if !stale => Json::from(if percent >= 90.0 {
            "critical"
        } else if percent >= 70.0 {
            "warning"
        } else {
            "normal"
        }),
        _ => Json::Null,
    }
}

/// Every subscription reports the share already used, worded like the API rows' 本月已用.
pub fn used_text(percent: f64) -> String {
    format!("已用 {}%", g(round(percent.clamp(0.0, 100.0), 1)))
}

/// A plan's homepage figure: its week's share as 本周已用 X% when it has a weekly window (用户：已用那里改成本周已用,
/// the week's own window, not the tightest: five hours and a model's own limit are the bars under it), else the
/// tightest as 已用 X%.
pub fn headline_used(week: Option<f64>, tightest: Option<f64>) -> Option<(f64, String)> {
    match (week, tightest) {
        (Some(week), _) => Some((week, format!("本周{}", used_text(week)))),
        (None, Some(most)) => Some((most, used_text(most))),
        _ => None,
    }
}

/// Between a homepage bar's name, share, level, place, group and marks in panelgauges; the bars themselves are split
/// by FALLBACK_SEPARATOR.
pub const GAUGE_FIELD: &str = "\u{1E}";

/// A bar under a homepage row.
#[derive(Debug, PartialEq)]
pub struct Bar {
    /// 0 and 1 the left and right of the first line, 2 and 3 of the second: the left a five-hour limit, the right a
    /// week's.
    pub place: usize,
    /// The model group its line is for (Gemini 订阅's), on the line's first bar.
    pub group: String,
    /// The window, as its page names it.
    pub name: String,
    pub used: f64,
    /// The group's logos (each base64, and whether it takes the text colour), drawn at the right end of its line's
    /// names, so each of Gemini 订阅's lines says whose models it counts.
    pub marks: Vec<(String, bool)>,
}

impl Bar {
    pub fn new(place: usize, group: &str, name: &str, used: f64) -> Bar {
        Bar { place, group: group.into(), name: name.into(), used, marks: Vec::new() }
    }
}

/// The two limits a plan's homepage row draws as bars, on one line: the five-hour one, or else the first, and the
/// tightest of the rest (the week, or a model's own limit when that is tighter), so the row's figure (the tightest of
/// all) is always one of the two. Fewer than two limits keep the single bar.
pub fn headline_pair(limits: &[(String, f64, bool)]) -> Vec<Bar> {
    if limits.len() < 2 {
        return Vec::new();
    }
    let first = limits.iter().position(|limit| limit.2).unwrap_or(0);
    let mut tightest: Option<&(String, f64, bool)> = None;
    for (index, limit) in limits.iter().enumerate() {
        if index != first && tightest.is_none_or(|best| limit.1 > best.1) {
            tightest = Some(limit);
        }
    }
    let tightest = tightest.expect("a second limit");
    vec![Bar::new(0, "", &limits[first].0, limits[first].1), Bar::new(1, "", &tightest.0, tightest.1)]
}

/// The panel's bars under a homepage row, each its name, share, level, place, group and marks (image:1 for a
/// template, :0 otherwise, split by commas); none while the figures are stale or the last read failed, as a single bar
/// isn't drawn then either.
pub fn gauges(bars: &[Bar], stale: bool) -> Json {
    if bars.len() < 2 || stale {
        return Json::Null;
    }
    let bars: Vec<String> = bars
        .iter()
        .map(|bar| {
            let level = py::str(Some(&usage_level(Some(bar.used), false)));
            let share = g(round(bar.used.clamp(0.0, 100.0), 1));
            let field = GAUGE_FIELD;
            let marks: Vec<String> =
                bar.marks.iter().map(|(image, template)| format!("{image}:{}", u8::from(*template))).collect();
            format!(
                "{}{field}{share}{field}{level}{field}{}{field}{}{field}{}",
                bar.name,
                bar.place,
                bar.group,
                marks.join(",")
            )
        })
        .collect();
    Json::from(bars.join(FALLBACK_SEPARATOR))
}

/// Pace through a window: the share used less the share of the window gone by.
pub fn pace_text(used: f64, resets_at: Option<f64>, span: Option<f64>, now: f64) -> Option<String> {
    let span = span.filter(|span| *span != 0.0)?;
    let resets_at = resets_at.filter(|at| *at != 0.0)?;
    let start = resets_at - span;
    if !(start <= now && now < resets_at) {
        return None;
    }
    let ahead = used - 100.0 * (now - start) / span;
    if ahead.abs() < 3.0 {
        return Some("与匀速持平".into());
    }
    Some(format!("比匀速{} {}%", if ahead > 0.0 { "多用" } else { "少用" }, ahead.abs().round_ties_even() as i64))
}

pub fn claude_span(window: &Object) -> Option<f64> {
    // Statusline and older cached readings carry no length; their labels still say it.
    if let Some(span) = window.get("span").filter(|span| span.is_truthy()) {
        return span.number();
    }
    let label = py::str(Some(window.get("label").unwrap_or(&Json::from(""))));
    if label == "5 小时" {
        Some(5.0 * 3600.0)
    } else if label.ends_with("本周") {
        Some(7.0 * 86400.0)
    } else {
        None
    }
}

pub fn minimax_span(window: &Object) -> Option<f64> {
    if let (Some(starts), Some(ends)) =
        (window.get("startsAt").filter(|v| v.is_truthy()), window.get("resetsAt").filter(|v| v.is_truthy()))
    {
        return Some(ends.number().unwrap_or(0.0) - starts.number().unwrap_or(0.0));
    }
    py::str(Some(window.get("name").unwrap_or(&Json::from("")))).ends_with("本周额度").then_some(7.0 * 86400.0)
}

pub fn with_failure(badge: &str, has_data: bool, failed: bool) -> String {
    // Values stay put while refreshing; only a real failure is marked, and plainly.
    if failed {
        if has_data { format!("{badge} · 刷新失败") } else { "刷新失败".into() }
    } else {
        badge.to_string()
    }
}

pub fn refresh_failed(error: Option<&str>) -> bool {
    error.is_some_and(|error| error != PENDING_READ)
}

pub fn credits_amount(value: f64) -> String {
    if value < 1000.0 {
        return g(round(value, 1));
    }
    let whole = value.round_ties_even() as i64;
    let digits = whole.unsigned_abs().to_string();
    let mut grouped = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    if whole < 0 { format!("-{grouped}") } else { grouped }
}

pub fn money_text(amount: f64, currency: &str) -> String {
    if currency == "Credits" {
        return format!("{} Credits", credits_amount(amount));
    }
    let prefix = match currency {
        "CNY" => "¥".to_string(),
        "USD" => "$".to_string(),
        _ => format!("{currency} "),
    };
    format!("{prefix}{amount:.2}")
}

impl Runtime {
    /// The options that make a row run one of this plugin's actions through SwiftBar.
    pub fn act(&self, name: &str, args: &[&str]) -> Options {
        let mut options = object! {
            "bash" => "/bin/bash", "terminal" => "false", "refresh" => "true", "param1" => self.plugin.as_str(),
            "param2" => "--action", "param3" => name,
        };
        for (index, value) in args.iter().enumerate() {
            options.insert(format!("param{}", index + 4), *value);
        }
        options
    }

    /// A window row's second line: when it resets, and its pace while the reading is current.
    pub fn window_detail(
        &self,
        used: f64,
        resets_at: Option<f64>,
        span: Option<f64>,
        read_at: Option<f64>,
        failed: bool,
    ) -> Json {
        let Some(resets_at) = resets_at.filter(|at| *at != 0.0) else { return Json::Null };
        // The service often returns xx:59:59.999 for an xx:00 reset.
        let mut parts =
            vec![format!("重置于 {}", dashboard::format(resets_at + 30.0, &self.local_zone(), "%m/%d %H:%M"))];
        if let Some(read_at) = read_at.filter(|at| *at != 0.0)
            && !failed
            && now() - read_at <= 600.0
            && let Some(pace) = pace_text(used, Some(resets_at), span, read_at)
        {
            parts.push(pace);
        }
        Json::from(parts.join(" · "))
    }

    pub fn freshness(&self, fetched_at: Option<&Json>) -> String {
        let Some(at) = fetched_at.and_then(Json::python_float).filter(|at| *at != 0.0) else {
            return "待同步".into();
        };
        let elapsed = (now() - at).max(0.0) as i64;
        if elapsed < 60 {
            "刚刚更新".into()
        } else if elapsed < 3600 {
            format!("{} 分钟前更新", elapsed / 60)
        } else if elapsed < 86400 {
            format!("{} 小时前更新", elapsed / 3600)
        } else {
            format!("{} 天前更新", elapsed / 86400)
        }
    }

    pub fn refresh_row(&self, source: &str, payload: &Object, error: Option<&str>, level: usize) -> String {
        let failed = refresh_failed(error);
        let detail = if failed {
            let mut text = error.unwrap_or("").to_string();
            if payload.get("_fetchedAt").is_some_and(Json::is_truthy) {
                text += &format!("\n保留最近一次成功的数据 · {}", self.freshness(payload.get("_fetchedAt")));
            }
            Some(text)
        } else {
            None
        };
        let badge = if failed { "刷新失败".to_string() } else { self.freshness(payload.get("_fetchedAt")) };
        self.refresh_now_row(self.act("refresh-now", &[source]), &badge, detail.as_deref(), failed, level)
    }

    /// 立即刷新, where a page says when its figures were read: the source read again on a click, the row turning till
    /// the reading is in (用户：用户看到刷新失败的结果后，没有任何办法). Beside it, when the figures were read or 刷新失败
    /// in orange, and under it why.
    pub fn refresh_now_row(
        &self,
        action: Options,
        badge: &str,
        detail: Option<&str>,
        failed: bool,
        level: usize,
    ) -> String {
        item(
            "立即刷新",
            level,
            merged(&[
                &object! {
                    "sfimage" => "arrow.clockwise", "badge" => badge, "paneldetail" => detail.map_or(Json::Null, Json::from),
                    "panelusagelevel" => if failed { Json::from("warning") } else { Json::Null },
                },
                &action,
            ]),
        )
    }

    pub fn snapshot_time(&self, stamp: f64) -> String {
        let zone = self.local_zone();
        let pattern = if day_number(stamp, &zone) == day_number(now(), &zone) { "%H:%M" } else { "%m/%d %H:%M" };
        dashboard::format(stamp, &zone, pattern)
    }

    pub fn logo_mark(&self, size: i64) -> Options {
        match self.logo("starmeter-mark.svg") {
            Some(raw) => object! { "templateimage" => raw, "width" => size, "height" => size },
            None => object! { "sfimage" => "sparkles", "sfsize" => size.min(18) },
        }
    }

    pub fn brand_icon(&self, key: &str) -> Options {
        let Some(service) = catalog::by_key(key) else { return object! { "sfimage" => "square.grid.2x2" } };
        let (raw, template) = if service.brand == "DeepSeek" {
            (self.logo("deepseek-logo.svg"), false)
        } else if let Some((file, mono)) = dashboard::brand_logo(service.brand) {
            (self.logo(&format!("logos/{file}")), mono)
        } else {
            (None, false)
        };
        match raw {
            Some(raw) => {
                let mut options = Options::new();
                options.insert(if template { "templateimage" } else { "image" }, raw);
                options.insert("width", MENU_ICON_SIZE);
                options.insert("height", MENU_ICON_SIZE);
                options
            }
            None => {
                object! { "sfimage" => if service.category == "订阅" { "person.crop.circle" } else { "creditcard" } }
            }
        }
    }

    /// A session's tool by its own mark: Claude and ChatGPT as their plans show them, the others as their cards do.
    pub fn session_icon(&self, tool: &str, module: Option<&str>) -> Options {
        match (tool, module) {
            ("Claude", _) => self.brand_icon("claude-subscription"),
            ("ChatGPT", _) => self.brand_icon("chatgpt"),
            (_, Some("deepseek-harness")) => self.brand_icon("deepseek"),
            (_, Some(module)) => match dashboard::module_logo(module) {
                Some((file, mono)) => match self.logo(&format!("logos/{file}")) {
                    Some(raw) => {
                        let mut options = Options::new();
                        options.insert(if mono { "templateimage" } else { "image" }, raw);
                        options.insert("width", MENU_ICON_SIZE);
                        options.insert("height", MENU_ICON_SIZE);
                        options
                    }
                    None => object! { "sfimage" => "terminal" },
                },
                None => object! { "sfimage" => "terminal" },
            },
            _ => object! { "sfimage" => "terminal" },
        }
    }

    pub fn file_href(&self, path: &str) -> String {
        let encoded: String = path
            .bytes()
            .map(|byte| {
                if byte.is_ascii_alphanumeric() || b"_.-~/".contains(&byte) {
                    (byte as char).to_string()
                } else {
                    format!("%{byte:02X}")
                }
            })
            .collect();
        format!("file://{encoded}")
    }
}

#[cfg(test)]
mod tests {
    use super::{Bar, FALLBACK_SEPARATOR, GAUGE_FIELD, gauges, headline_pair};
    use crate::json::Json;

    #[test]
    fn a_plan_with_two_limits_draws_five_hours_and_the_tightest_of_the_rest() {
        let limit = |name: &str, used: f64, five: bool| (name.to_string(), used, five);
        let pair = headline_pair(&[
            limit("本周", 60.0, false),
            limit("5 小时", 30.0, true),
            limit("Spark · 本周", 94.5, false),
        ]);
        assert_eq!(pair, vec![Bar::new(0, "", "5 小时", 30.0), Bar::new(1, "", "Spark · 本周", 94.5)]);
        // No five-hour limit: the first, then the tightest of the rest (the earlier on a tie).
        let pair =
            headline_pair(&[limit("本周", 48.0, false), limit("Fable", 59.0, false), limit("Opus", 59.0, false)]);
        assert_eq!(pair, vec![Bar::new(0, "", "本周", 48.0), Bar::new(1, "", "Fable", 59.0)]);
        assert!(headline_pair(&[limit("本周", 48.0, false)]).is_empty(), "one limit keeps the single bar");
        let field = GAUGE_FIELD;
        assert_eq!(
            gauges(&[Bar::new(0, "", "5 小时", 100.0 - 94.4), Bar::new(1, "Claude 和 GPT", "本周", 91.0)], false),
            Json::from(format!(
                "5 小时{field}5.6{field}normal{field}0{field}{field}{FALLBACK_SEPARATOR}本周{field}91{field}critical{field}1{field}Claude 和 GPT{field}"
            ))
        );
        assert_eq!(
            gauges(&[Bar::new(0, "", "5 小时", 5.0), Bar::new(1, "", "本周", 9.0)], true),
            Json::Null,
            "stale figures draw no bars"
        );
    }
}
