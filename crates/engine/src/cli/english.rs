//! English for the command line, the panel's own (app/ui/src/lib/en.ts): its phrases are read from that file when the
//! program is built, and its rules for text with numbers and names in it are mirrored here pattern for pattern, in the
//! same order (a test holds them to the file). translate() works as the panel's does: the whole text as a phrase or by
//! a rule for the whole, then each part between " · ", then the part before a "：", then a rule for the part.

use std::collections::HashMap;
use std::sync::{LazyLock, OnceLock};

use regex::{Captures, Regex};

/// The panel's dictionary, as the program was built.
pub const SOURCE: &str = include_str!("../../../../app/ui/src/lib/en.ts");

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

const QUOTES: [char; 2] = ['\'', '"'];

/// The phrases of en.ts: `key: 'value'` pairs, keys bare or quoted, values in single or double quotes.
pub fn parse_phrases(source: &str) -> Result<HashMap<String, String>, String> {
    let open = source.find("export const phrases").ok_or("no phrases")?;
    let body_start = open + source[open..].find('{').ok_or("no opening brace")? + 1;
    let body_end = body_start + source[body_start..].find("\n};").ok_or("no closing brace")?;
    let mut chars = source[body_start..body_end].chars().peekable();
    let mut phrases = HashMap::new();
    fn quoted(chars: &mut std::iter::Peekable<std::str::Chars<'_>>, quote: char) -> Result<String, String> {
        let mut text = String::new();
        loop {
            match chars.next().ok_or("an unfinished string")? {
                c if c == quote => return Ok(text),
                '\\' => match chars.next().ok_or("an unfinished escape")? {
                    'n' => text.push('\n'),
                    other => text.push(other),
                },
                other => text.push(other),
            }
        }
    }
    loop {
        match chars.peek().copied() {
            None => break,
            Some(c) if c.is_whitespace() || c == ',' => {
                chars.next();
            }
            Some('/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            Some(first) => {
                let key = if QUOTES.contains(&first) {
                    chars.next();
                    quoted(&mut chars, first)?
                } else {
                    let mut name = String::new();
                    while let Some(&c) = chars.peek() {
                        if c == ':' {
                            break;
                        }
                        name.push(c);
                        chars.next();
                    }
                    name.trim().to_string()
                };
                while chars.peek().is_some_and(|c| c.is_whitespace()) {
                    chars.next();
                }
                if chars.next() != Some(':') {
                    return Err(format!("no value for {key}"));
                }
                while chars.peek().is_some_and(|c| c.is_whitespace()) {
                    chars.next();
                }
                let quote = chars.next().filter(|quote| QUOTES.contains(quote));
                let Some(quote) = quote else { return Err(format!("the value of {key} is not a plain string")) };
                phrases.insert(key, quoted(&mut chars, quote)?);
            }
        }
    }
    Ok(phrases)
}

static PHRASES: LazyLock<HashMap<String, String>> = LazyLock::new(|| parse_phrases(SOURCE).unwrap_or_default());
static CJK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\x{3400}-\x{9fff}\x{ff08}-\x{ff1f}\x{3001}-\x{3002}]").expect("pattern"));
static HAN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\x{3400}-\x{9fff}]").expect("pattern"));
static TOKENS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d[\d.,]*[KMB]?) Token\b").expect("pattern"));
static NAMED_SHARE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(.+) (\d+(?:\.\d+)?%)$").expect("pattern"));
static WEEKDAY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^周([一二三四五六日天])").expect("pattern"));

type Write = fn(&Captures<'_>) -> String;

/// One of en.ts's rules: its pattern exactly as the file writes it, and what it writes.
pub struct Rule {
    pub pattern: &'static str,
    regex: OnceLock<Regex>,
    write: Write,
}

impl Rule {
    fn regex(&self) -> &Regex {
        self.regex.get_or_init(|| Regex::new(self.pattern).expect("pattern"))
    }
}

macro_rules! rule {
    ($pattern:literal, $write:expr) => {
        Rule { pattern: $pattern, regex: OnceLock::new(), write: $write }
    };
}

/// A range after "active": today, yesterday, this week; in the last 7 days.
fn during(range: &str) -> String {
    if range.starts_with("Last ") { format!("in the {}", range.to_lowercase()) } else { range.to_lowercase() }
}

fn group<'a>(m: &'a Captures<'_>, index: usize) -> &'a str {
    m.get(index).map_or("", |found| found.as_str())
}

fn plural<'a>(n: &str, one: &'a str, many: &'a str) -> &'a str {
    if n == "1" { one } else { many }
}

fn month(n: &str) -> Option<&'static str> {
    n.parse::<usize>().ok().and_then(|n| n.checked_sub(1)).and_then(|index| MONTHS.get(index).copied())
}

/// 周一 18:20 → Mon 18:20.
fn time(zh: &str) -> String {
    WEEKDAY
        .replace(zh, |m: &Captures<'_>| {
            match group(m, 1) {
                "一" => "Mon",
                "二" => "Tue",
                "三" => "Wed",
                "四" => "Thu",
                "五" => "Fri",
                "六" => "Sat",
                _ => "Sun",
            }
            .to_string()
        })
        .into_owned()
}

/// A plan's window and its share inside “Used: …”: a common window name is written in lower case there; one that
/// starts as a model's or an account's own (Fable, Claude 和 GPT) keeps its capitals.
fn window_share(part: &str) -> String {
    let Some(named) = NAMED_SHARE.captures(part).filter(|named| {
        HAN.is_match(group(named, 1)) && !group(named, 1).starts_with(|c: char| c.is_ascii_alphabetic())
    }) else {
        return translate(part);
    };
    let name = translate(group(&named, 1));
    let mut letters = name.chars();
    let first = letters.next().map(|c| c.to_lowercase().collect::<String>()).unwrap_or_default();
    format!("{first}{} {}", letters.as_str(), group(&named, 2))
}

/// Rules for the whole text, before it is split into parts.
pub static WHOLE: [Rule; 2] = [
    rule!(r"^已用 (.+ · .+)$", |m| format!(
        "Used: {}",
        group(m, 1).split(" · ").map(window_share).collect::<Vec<_>>().join(" · ")
    )),
    rule!(r"^更新时间：(.+)$", |m| format!("Updated {}", group(m, 1))),
];

/// One part of the engine's text.
pub static RULES: [Rule; 92] = [
    rule!(r"^已用 (\d+(?:\.\d+)?%)$", |m| format!("{} used", group(m, 1))),
    rule!(r"^已用 (\d+(?:\.\d+)?%) 截至 (.+)$", |m| format!("{} used as of {}", group(m, 1), group(m, 2))),
    rule!(r"^已用 (.+)$", |m| format!("Used: {}", translate(group(m, 1)))),
    rule!(r"^本周已用 (\d+(?:\.\d+)?%)$", |m| format!("{} used this week", group(m, 1))),
    rule!(r"^(本周|本周期|5 小时) (\d+(?:\.\d+)?%)$", |m| format!("{} {}", translate(group(m, 1)), group(m, 2))),
    rule!(r"^截至 (.+)$", |m| format!("as of {}", group(m, 1))),
    rule!(r"^(\d+) 个账户$", |m| format!("{} accounts", group(m, 1))),
    rule!(r"^(\d+) 台电脑$", |m| format!("{} {}", group(m, 1), plural(group(m, 1), "computer", "computers"))),
    rule!(r"^还剩 (\d+) 分钟$", |m| format!("{} min left", group(m, 1))),
    rule!(r"^电脑暂时没有返回数据（HTTP (\d+)），稍后会自动重试$", |m| format!(
        "The computer sent nothing this time (HTTP {}); trying again shortly",
        group(m, 1)
    )),
    rule!(r"^(\d+) 分钟前同步$", |m| format!("Synced {} min ago", group(m, 1))),
    rule!(r"^(\d+) 小时前同步$", |m| format!("Synced {} h ago", group(m, 1))),
    rule!(r"^(\d+) 天前同步$", |m| format!("Synced {} {} ago", group(m, 1), plural(group(m, 1), "day", "days"))),
    rule!(r"^(\d+) 小时$", |m| format!("{} {}", group(m, 1), plural(group(m, 1), "hour", "hours"))),
    rule!(r"^重置于 (.+)$", |m| format!("Resets {}", time(group(m, 1)))),
    rule!(r"^比匀速多用 (.+)$", |m| format!("{} ahead of even pace", group(m, 1))),
    rule!(r"^比匀速少用 (.+)$", |m| format!("{} behind even pace", group(m, 1))),
    rule!(r"^本月回本 (.+) 倍$", |m| format!("Paid back {}× this month", group(m, 1))),
    rule!(r"^本月 API 等值 (.+)$", |m| format!("API equivalent this month {}", group(m, 1))),
    rule!(r"^月费 (.+)$", |m| format!("Monthly {}", group(m, 1))),
    rule!(r"^剩余 (.+)$", |m| format!("{} left", group(m, 1))),
    rule!(r"^余 (.+) Credits$", |m| format!("{} credits left", group(m, 1))),
    rule!(r"^可免费重置 (\d+) 次$", |m| format!(
        "{} free {}",
        group(m, 1),
        plural(group(m, 1), "reset", "resets")
    )),
    rule!(r"^可用 (\d+) 次$", |m| format!("{} available", group(m, 1))),
    rule!(r"^最早 (.+) 过期 · 在 Codex 里使用$", |m| format!(
        "First expires {} · use it in Codex",
        group(m, 1)
    )),
    rule!(r"^额外用量剩 (.+)$", |m| format!("{} of extra usage left", group(m, 1))),
    rule!(r"^按需剩 (.+)$", |m| format!("{} of on-demand left", group(m, 1))),
    rule!(r"^到期 (.+)$", |m| format!("Expires {}", group(m, 1))),
    rule!(r"^(.+) 订阅$", |m| format!("{} plan", translate(group(m, 1)))),
    rule!(r"^需勾选 (.+)$", |m| format!("Needs {} on", translate(group(m, 1)))),
    rule!(r"^加到主页：(.+)$", |m| format!(
        "Add to the homepage: {}",
        group(m, 1).split('、').map(translate).collect::<Vec<_>>().join(", ")
    )),
    rule!(r"^Antigravity 没有给出额度（HTTP (\d+)）$", |m| format!(
        "Antigravity gave no quota (HTTP {})",
        group(m, 1)
    )),
    rule!(r"^(\d+) 分钟前更新$", |m| format!("{} min ago", group(m, 1))),
    rule!(r"^(\d+) 小时前更新$", |m| format!("{} h ago", group(m, 1))),
    rule!(r"^(\d+) 天前更新$", |m| format!("{} {} ago", group(m, 1), plural(group(m, 1), "day", "days"))),
    rule!(r"^(\d+) 分钟前$", |m| format!("{} min ago", group(m, 1))),
    rule!(r"^更新时间：(.+)$", |m| format!("Updated {}", group(m, 1))),
    rule!(r"^实时 (.+)$", |m| format!("Live {}", group(m, 1))),
    rule!(r"^余额 (.+)$", |m| format!("Balance {}", group(m, 1))),
    rule!(r"^本月已用 (.+)$", |m| format!("{} this month", group(m, 1))),
    rule!(r"^今天已用 (.+)$", |m| format!("{} today", group(m, 1))),
    rule!(r"^已知 (.+)$", |m| format!("Known {}", group(m, 1))),
    rule!(r"^(\d+) 次已由 (.+) 计入$", |m| format!("{} counted by {}", group(m, 1), group(m, 2))),
    rule!(r"^(\d+) 次未定价$", |m| format!("{} unpriced", group(m, 1))),
    rule!(r"^(\d+) 次请求$", |m| format!("{} {}", group(m, 1), plural(group(m, 1), "request", "requests"))),
    rule!(r"^其他 (\d+) 个(模型|项目|工具)$", |m| format!(
        "{} more {}",
        group(m, 1),
        match group(m, 2) {
            "模型" => "models",
            "项目" => "projects",
            _ => "tools",
        }
    )),
    rule!(r"^已定价 (.+)$", |m| format!("{} priced", group(m, 1))),
    rule!(r"^每 (\d+) 天$", |m| format!("Every {} days", group(m, 1))),
    rule!(r"^每 (\d+) 秒$", |m| if group(m, 1) == "1" {
        "Every second".into()
    } else {
        format!("Every {} seconds", group(m, 1))
    }),
    rule!(r"^图表展示最近 (\d+) 天$", |m| format!("The chart shows the last {} days", group(m, 1))),
    rule!(r"^(.+) 不可读$", |m| format!("{} can't be read", group(m, 1))),
    rule!(r"^每格 (\d+) 小时$", |m| format!("{} {} per dot", group(m, 1), plural(group(m, 1), "hour", "hours"))),
    rule!(r"^每格 (\d+) 天$", |m| format!("{} {} per dot", group(m, 1), plural(group(m, 1), "day", "days"))),
    rule!(r"^(\d+)小时$", |m| format!("{}h", group(m, 1))),
    rule!(r"^(\d+)天$", |m| format!("{}d", group(m, 1))),
    rule!(r"^(\d+)月$", |m| month(group(m, 1)).map_or_else(|| group(m, 0).to_string(), String::from)),
    rule!(r"^(\d+)月(\d+)日(.*)$", |m| format!(
        "{} {}{}",
        month(group(m, 1)).unwrap_or("undefined"),
        group(m, 2),
        group(m, 3)
    )),
    rule!(r"^连续 (\d+) 天$", |m| format!("{}-day streak", group(m, 1))),
    rule!(r"^最长 (\d+) 天$", |m| format!("longest {}", group(m, 1))),
    rule!(r"^最长连续 (\d+) 天$", |m| format!("Longest streak {} days", group(m, 1))),
    rule!(r"^(\d+) 运行中$", |m| format!("{} running", group(m, 1))),
    rule!(r"^(\d+) 空闲$", |m| format!("{} idle", group(m, 1))),
    rule!(r"^(\d+) 等待输入$", |m| format!("{} waiting", group(m, 1))),
    rule!(r"^最近 (\d+) 轮$", |m| if group(m, 1) == "1" {
        "Last turn".into()
    } else {
        format!("Last {} turns", group(m, 1))
    }),
    rule!(r"^(\d+) 轮$", |m| if group(m, 1) == "1" { "1 turn".into() } else { format!("{} turns", group(m, 1)) }),
    rule!(r"^累计 (.+)$", |m| {
        let total = translate(group(m, 1));
        match total.strip_prefix("Known ") {
            Some(rest) => format!("Total known {rest}"),
            None => format!("Total {total}"),
        }
    }),
    rule!(r"^金额只算(.+)$", |m| format!("amounts cover {}", translate(group(m, 1)).to_lowercase())),
    rule!(r"^(.+)活动过的会话$", |m| format!("Sessions active {}", during(&translate(group(m, 1))))),
    rule!(r"^(.+)没有会话$", |m| format!("No sessions {}", during(&translate(group(m, 1))))),
    rule!(r"^运行中 (.+)$", |m| format!("Running {}", translate(group(m, 1)))),
    rule!(r"^空闲 (.+)$", |m| format!("Idle {}", translate(group(m, 1)))),
    rule!(r"^(\d+) 分钟$", |m| format!("{} min", group(m, 1))),
    rule!(r"^上下文 (.+)$", |m| format!("Context {}", group(m, 1))),
    rule!(r"^(\d+\/\d+) 在线$", |m| format!("{} online", group(m, 1))),
    rule!(r"^内存 (.+)$", |m| format!("Memory {}", group(m, 1))),
    rule!(r"^最后在线 (.+)$", |m| format!("Last online {}", group(m, 1))),
    rule!(r"^(\d+) 项待更新$", |m| format!("{} to update", group(m, 1))),
    rule!(r"^(\d+) 项$", |m| group(m, 1).to_string()),
    rule!(r"^点击运行 (.+)$", |m| format!("Click to run {}", group(m, 1))),
    rule!(r"^更新到 (.+)$", |m| format!("Update to {}", group(m, 1))),
    rule!(r"^重试更新到 (.+)$", |m| format!("Retry the update to {}", group(m, 1))),
    rule!(r"^可更新 (.+)$", |m| format!("{} available", group(m, 1))),
    rule!(r"^当前 (.+) → 最新 (.+)$", |m| format!("Current {} → latest {}", group(m, 1), group(m, 2))),
    rule!(r"^已自动更新至 (.+)$", |m| format!("Updated automatically to {}", group(m, 1))),
    rule!(r"^(.+) 返回 HTTP (\d+)$", |m| format!("{}: HTTP {}", group(m, 1), group(m, 2))),
    rule!(r"^找不到版本 (.+) 的发布$", |m| format!("No release for version {}", group(m, 1))),
    rule!(r"^安装程序退出码 (-?\d+)$", |m| format!("The installer exited with code {}", group(m, 1))),
    rule!(r"^安装包里的版本不是 (.+)$", |m| format!("The package isn't version {}", group(m, 1))),
    rule!(r"^不能写入 (.+)，请从发布页下载安装$", |m| format!(
        "Can't write to {}; download it from the release page",
        group(m, 1)
    )),
    rule!(
        r"^当前版本 (.+)。将从 GitHub 下载这一版的安装包，校验 SHA-256 后替换本机的 Starmeter，并自动重新打开；设置、Key 和统计记录都会保留。$",
        |m| format!(
            "Now {}. The new version's package comes from GitHub and, once its SHA-256 checks out, takes the place of this Starmeter, which opens again; settings, keys and usage records stay.",
            group(m, 1)
        )
    ),
    rule!(r"^打开 (.+)$", |m| format!("Open {}", translate(group(m, 1)))),
    rule!(r"^(.+) (\d+(?:\.\d+)?%)$", |m| format!("{} {}", translate(group(m, 1)), group(m, 2))),
];

fn apply(rules: &[Rule], text: &str) -> Option<String> {
    rules.iter().find_map(|rule| rule.regex().captures(text).map(|m| (rule.write)(&m)))
}

/// Text without Chinese: a figure in tokens as the engine writes it (2.41M Token) counts them in the plural.
fn plural_tokens(text: &str) -> String {
    let mut out = String::new();
    let mut last = 0;
    for m in TOKENS.captures_iter(text) {
        let whole = m.get(0).expect("match");
        if text[whole.end()..].starts_with('/') {
            continue;
        }
        out.push_str(&text[last..whole.start()]);
        out.push_str(&format!("{} tokens", group(&m, 1)));
        last = whole.end();
    }
    out.push_str(&text[last..]);
    out
}

/// A piece of the engine's text in English; what the dictionary doesn't know stays as it is.
pub fn translate(text: &str) -> String {
    if !CJK.is_match(text) {
        return plural_tokens(text);
    }
    // A detail of several lines: each line on its own.
    if text.contains('\n') {
        return text.split('\n').map(translate).collect::<Vec<_>>().join("\n");
    }
    if let Some(known) = PHRASES.get(text) {
        return known.clone();
    }
    if let Some(out) = apply(&WHOLE, text) {
        return out;
    }
    if text.contains(" · ") {
        return text.split(" · ").map(translate).collect::<Vec<_>>().join(" · ");
    }
    if let Some((head, rest)) = text.split_once('：') {
        let english = translate(head);
        if english != head {
            return format!("{english}: {}", translate(rest));
        }
    }
    apply(&RULES, text).unwrap_or_else(|| text.to_string())
}

/// The panel's own words (its t()): the phrase for zh, the singular phrase ("…|1") for a count of one.
pub fn phrase(zh: &str, n: Option<&str>) -> String {
    if n == Some("1")
        && let Some(one) = PHRASES.get(&format!("{zh}|1"))
    {
        return one.clone();
    }
    PHRASES.get(zh).cloned().unwrap_or_else(|| translate(zh))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_panels_phrases_are_read_from_its_dictionary() {
        let phrases = parse_phrases(SOURCE).expect("en.ts's phrases parse");
        assert!(phrases.len() > 300, "only {} phrases", phrases.len());
        assert_eq!(phrases.get("近 24 小时").map(String::as_str), Some("Last 24 hours"));
        assert_eq!(phrases.get("今天").map(String::as_str), Some("Today"));
        assert_eq!(phrases.get("缓存命中").map(String::as_str), Some("Cache hits"));
        assert_eq!(phrases.get("{n} 请求|1").map(String::as_str), Some("{n} request"));
    }

    #[test]
    fn every_rule_is_the_panels_in_its_order() {
        let section = |name: &str| -> Vec<&str> {
            let start = SOURCE.find(&format!("export const {name}: Rule[] = [")).expect("the rule list");
            let end = start + SOURCE[start..].find("\n];").expect("its end");
            SOURCE[start..end]
                .lines()
                .filter_map(|line| line.trim_start().strip_prefix("[/"))
                .map(|line| line.split("/, (").next().expect("a pattern"))
                .collect()
        };
        let mirrored = |rules: &[Rule]| rules.iter().map(|rule| rule.pattern).collect::<Vec<_>>();
        // A rule added to en.ts needs its twin here, in the same place, for the command line's English.
        assert_eq!(mirrored(&WHOLE), section("whole"));
        assert_eq!(mirrored(&RULES), section("rules"));
    }

    #[test]
    fn every_status_a_card_or_plan_can_show_has_english() {
        // CLI-5: 未找到 Claude 用量记录 stayed Chinese in English output. The sources' statuses, from the files that
        // read usage, accounts and plans, all need a phrase in en.ts.
        let status = Regex::new(
            r#"(?:"error"\s*=>\s*|Failure\(|fail\(|unwrap_or_else\(\|\| )"([^"]*[\x{4e00}-\x{9fff}][^"]*)""#,
        )
        .expect("pattern");
        let mut seen = 0;
        for file in ["accounts.rs", "client_accounts.rs", "cursor.rs", "harness.rs", "opencodex.rs", "range_usage.rs"] {
            let path = format!("{}/../core/src/{file}", env!("CARGO_MANIFEST_DIR"));
            let source = std::fs::read_to_string(&path).expect("the core's source");
            for found in status.captures_iter(&source) {
                let text = &found[1];
                assert_ne!(translate(text), text, "{file}: \"{text}\" has no English in app/ui/src/lib/en.ts");
                seen += 1;
            }
        }
        assert!(seen >= 20, "only {seen} statuses found: the pattern no longer matches the core");
        assert_eq!(translate("未找到 Claude 用量记录"), "No Claude usage records found");
    }

    #[test]
    fn every_update_row_reads_in_english() {
        // 工具更新: why a product has no version check, the channel a CLI came from, and what its row does.
        let literal = Regex::new(r#""([^"\n]*[\x{4e00}-\x{9fff}][^"\n]*)""#).expect("pattern");
        let path = format!("{}/../core/src/update_products.rs", env!("CARGO_MANIFEST_DIR"));
        let whole = std::fs::read_to_string(&path).expect("the core's source");
        let source = whole.split("#[cfg(test)]").next().unwrap_or("");
        let mut seen = 0;
        for found in literal.captures_iter(source) {
            let text = &found[1];
            assert_ne!(translate(text), text, "update_products.rs: \"{text}\" has no English in app/ui/src/lib/en.ts");
            seen += 1;
        }
        assert!(seen >= 8, "only {seen} texts found");
        assert_eq!(
            translate("0.5.17 · Qoder CN 没有公开 App 的版本接口"),
            "0.5.17 · Qoder CN publishes no version for its app"
        );
        assert_eq!(
            translate("2026.11.13-eacade1 → 2026.11.22-e9042b2 · 官方安装"),
            "2026.11.13-eacade1 → 2026.11.22-e9042b2 · Official installer"
        );
        assert_eq!(
            translate("点击运行 npm install -g @google/gemini-cli@latest"),
            "Click to run npm install -g @google/gemini-cli@latest"
        );
        for text in ["点击打开下载页", "点击打开官网", "没有公开的版本接口", "这个安装方式没有更新命令"]
        {
            assert_ne!(translate(text), text);
        }
    }

    #[test]
    fn the_engines_text_reads_as_the_panel_writes_it() {
        assert_eq!(translate("已用 本周 38% · 5 小时 12%"), "Used: this week 38% · 5 hours 12%");
        assert_eq!(translate("已用 本周 76% · Fable 41%"), "Used: this week 76% · Fable 41%");
        assert_eq!(translate("已用 Gemini 24% · Claude 和 GPT 18%"), "Used: Gemini 24% · Claude and GPT 18%");
        assert_eq!(translate("ChatGPT 订阅"), "ChatGPT plan");
        assert_eq!(translate("余额 ¥229.15 (2)"), "Balance ¥229.15 (2)");
        assert_eq!(translate("重置于 周一 18:20 · 比匀速少用 9%"), "Resets Mon 18:20 · 9% behind even pace");
        assert_eq!(translate("3 次已由 Magpie 计入"), "3 counted by Magpie");
        assert_eq!(translate("刷新失败 · 保留原数据"), "Refresh failed · keeping earlier data");
        assert_eq!(translate("2.41M Token"), "2.41M tokens");
        assert_eq!(translate("12K Token/s"), "12K Token/s");
        assert_eq!(translate("更新时间：14:32"), "Updated 14:32");
        assert_eq!(translate("my-project"), "my-project");
        assert_eq!(phrase("{n} 请求", Some("1")), "{n} request");
        assert_eq!(phrase("{n} 请求", Some("2")), "{n} requests");
    }
}
