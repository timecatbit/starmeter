//! What the command line reports, built from the homepage's own dashboard and rows: Summary, the trend, a row per tool
//! (or per model or project), and the plans' and APIs' figures. The terminal layout and --json both read it.

use starmeter_core::catalog;
use starmeter_core::dashboard::{summary_parts, summary_totals};
use starmeter_core::json::{Json, Object};
use starmeter_core::object;

use super::english;
use super::figures;

/// The words of one language: the panel's Chinese, or its English.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Words {
    pub english: bool,
    /// 繁體中文: the Chinese converted as the panel converts it.
    pub traditional: bool,
}

impl Words {
    /// The engine's text as the panel shows it in this language.
    pub fn text(&self, zh: &str) -> String {
        if self.english {
            english::translate(zh)
        } else if self.traditional {
            starmeter_core::hant::convert(zh)
        } else {
            zh.to_string()
        }
    }

    /// The panel's own words, with {n} and the like filled in.
    pub fn say(&self, zh: &str, values: &[(&str, &str)]) -> String {
        let n = values.iter().find(|(name, _)| *name == "n").map(|(_, value)| *value);
        let mut text = if self.english { english::phrase(zh, n) } else { zh.to_string() };
        for (name, value) in values {
            text = text.replace(&format!("{{{name}}}"), value);
        }
        if self.traditional { starmeter_core::hant::convert(&text) } else { text }
    }

    /// A page of text as the reader's language writes it: in 繁體中文 all of its Chinese converted, the names in it
    /// too, as the panel shows them.
    pub fn page(&self, text: String) -> String {
        if self.traditional { starmeter_core::hant::convert(&text) } else { text }
    }

    /// The command line's own words, which the panel doesn't have.
    pub fn own(&self, zh: &'static str, en: &'static str) -> &'static str {
        if self.english { en } else { zh }
    }
}

/// How bad a figure is: a plan's level, or a card's note that the numbers can't be read as usual.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Plain,
    Muted,
    Warning,
    Critical,
}

impl Tone {
    fn of_level(level: Option<&str>) -> Tone {
        match level {
            Some("warning") => Tone::Warning,
            Some("critical") => Tone::Critical,
            _ => Tone::Plain,
        }
    }

    fn name(self) -> Option<&'static str> {
        match self {
            Tone::Warning => Some("warning"),
            Tone::Critical => Some("critical"),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Model {
    pub name: String,
    pub amount: String,
    pub tokens: f64,
    pub credits: Option<f64>,
    pub share: f64,
    pub unpriced: bool,
}

/// A card: a tool, or a model or project when the cards are grouped so.
#[derive(Clone, Debug)]
pub struct Row {
    pub id: String,
    pub name: String,
    pub available: bool,
    pub status: String,
    pub notes: Vec<(String, Tone)>,
    pub in_summary: bool,
    /// Its share of Summary's tokens, when Summary counts it.
    pub share: Option<f64>,
    pub cost: Option<f64>,
    pub credits: Option<f64>,
    pub tokens: f64,
    pub requests: f64,
    pub output: f64,
    pub cache: Option<f64>,
    pub sessions: f64,
    pub unpriced: f64,
    pub models: Vec<Model>,
}

impl Row {
    pub fn used(&self) -> bool {
        self.requests > 0.0 || self.tokens > 0.0 || self.credits.unwrap_or(0.0) > 0.0
    }
}

#[derive(Clone, Debug)]
pub struct Item {
    pub name: String,
    pub value: String,
    pub tone: Tone,
    pub detail: Option<String>,
}

/// A plan or an API on the homepage, with the windows, balances and keys under it.
#[derive(Clone, Debug)]
pub struct Quota {
    pub id: String,
    pub name: String,
    pub value: String,
    pub short: Option<String>,
    pub tone: Tone,
    pub items: Vec<Item>,
}

#[derive(Clone, Debug)]
pub struct Report {
    pub words: Words,
    pub range: String,
    pub range_label: String,
    pub title: String,
    pub date: String,
    pub period: Json,
    pub zone: String,
    pub grouping: String,
    pub symbol: String,
    pub code: String,
    pub rate: f64,
    pub updated: Option<String>,
    pub devices: i64,
    pub chart_status: String,
    pub cost: Option<f64>,
    pub cost_partial: bool,
    pub tokens: Option<f64>,
    pub requests: Option<f64>,
    pub cache: Option<f64>,
    pub trend: Vec<f64>,
    pub trend_unit: String,
    pub trend_span: String,
    pub tools: Vec<Row>,
    pub groups: Option<Vec<Row>>,
    pub quotas: Vec<Quota>,
}

fn number(value: Option<&Json>) -> Option<f64> {
    value.and_then(Json::number).filter(|number| number.is_finite())
}

fn figure(value: Option<&Json>) -> f64 {
    number(value).unwrap_or(0.0)
}

fn text(value: Option<&Json>) -> String {
    value.and_then(Json::as_str).unwrap_or("").to_string()
}

/// The share picture's title words for a range.
fn title_range(key: &str) -> &'static str {
    match key {
        "today" => "今日",
        "yesterday" => "昨日",
        "24h" => "近 24 小时",
        "7d" => "近 7 天",
        "30d" => "近 30 天",
        "week" => "本周",
        "month" => "本月",
        _ => "累计",
    }
}

/// The share picture's date in the terminal style's writing: 2026-11-20, 11-13 → 11-19, with times for 24 hours.
pub fn poster_date(dashboard: &Object) -> String {
    let numbers = |value: &str| -> Vec<i64> {
        value.split(['-', 'T', ':']).filter_map(|part| part.parse::<i64>().ok()).collect()
    };
    let day = numbers(&text(dashboard.get("date")));
    let p2 = |n: i64| format!("{n:02}");
    let single =
        if day.len() == 3 { format!("{}-{}-{}", day[0], p2(day[1]), p2(day[2])) } else { text(dashboard.get("date")) };
    let period: Vec<String> = dashboard
        .get("period")
        .and_then(Json::as_array)
        .map(|list| list.iter().map(|value| text(Some(value))).collect())
        .unwrap_or_default();
    if text(dashboard.get("rangeKey")) == "today" || period.len() != 2 {
        return single;
    }
    let (a, b) = (numbers(&period[0]), numbers(&period[1]));
    if a.len() < 3 || b.len() != a.len() {
        return single;
    }
    if a.len() >= 5 {
        return format!(
            "{}-{} {}:{} → {}-{} {}:{}",
            p2(a[1]),
            p2(a[2]),
            p2(a[3]),
            p2(a[4]),
            p2(b[1]),
            p2(b[2]),
            p2(b[3]),
            p2(b[4])
        );
    }
    if a[0] != b[0] {
        return format!("{}-{}-{} → {}-{}-{}", a[0], p2(a[1]), p2(a[2]), b[0], p2(b[1]), p2(b[2]));
    }
    if a[1] == b[1] && a[2] == b[2] {
        return format!("{}-{}-{}", a[0], p2(a[1]), p2(a[2]));
    }
    format!("{}-{} → {}-{}", p2(a[1]), p2(a[2]), p2(b[1]), p2(b[2]))
}

/// The homepage's order: used before unused; the costliest, then the most tokens, then the most Credits.
fn by_usage(rows: &mut [Row]) {
    let key = |row: &Row| [row.cost.unwrap_or(0.0), row.tokens, row.credits.unwrap_or(0.0)];
    rows.sort_by(|a, b| {
        b.used().cmp(&a.used()).then_with(|| {
            if a.used() {
                let (ka, kb) = (key(a), key(b));
                for (x, y) in ka.iter().zip(kb.iter()) {
                    if x != y {
                        return y.partial_cmp(x).unwrap_or(std::cmp::Ordering::Equal);
                    }
                }
            }
            std::cmp::Ordering::Equal
        })
    });
}

/// A card's notes as the panel puts them after its name: a problem, else what changes how to read it.
fn notes(card: &Object, words: Words) -> Vec<(String, Tone)> {
    let status = text(card.get("status"));
    let normal = status == "运行正常" || status == "已同步";
    if !card.get("available").is_some_and(Json::is_truthy) {
        return vec![(words.text(if normal { "数据暂不可用" } else { &status }), Tone::Warning)];
    }
    if !normal {
        return vec![(words.text(&status), Tone::Warning)];
    }
    let mut out = Vec::new();
    let subtitle = text(card.get("subtitle"));
    if !subtitle.is_empty() {
        out.push((words.text(&subtitle), Tone::Muted));
    }
    for (field, tone) in [("serviceNote", Tone::Warning), ("priceNote", Tone::Warning)] {
        let note = text(card.get(field));
        if !note.is_empty() {
            out.push((words.text(&note), tone));
        }
    }
    out
}

fn row(card: &Object, part: Option<&Object>, summary_tokens: Option<f64>, words: Words, grouped: bool) -> Row {
    let models = card
        .get("models")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .filter_map(Json::as_object)
        .map(|model| {
            let tokens = figure(model.get("tokens"));
            let credits = number(model.get("credits"));
            let name = text(model.get("name"));
            // A model card lists the tools that used it; a tool card its models.
            let name = if grouped { words.text(&name) } else { words.text(&figures::model_name(&name)) };
            Model {
                name,
                amount: match credits {
                    Some(credits) if tokens == 0.0 => figures::credits(credits),
                    _ => figures::tokens(tokens),
                },
                tokens,
                credits,
                share: figure(model.get("share")),
                unpriced: model.get("priceNote").is_some_and(Json::is_truthy),
            }
        })
        .collect();
    let counted = part.map(|part| figure(part.get("tokens")));
    Row {
        id: text(card.get("id")),
        name: words.text(&text(card.get("name"))),
        available: card.get("available").is_some_and(Json::is_truthy),
        status: words.text(&text(card.get("status"))),
        notes: notes(card, words),
        in_summary: part.is_some(),
        share: match (counted, summary_tokens) {
            (Some(counted), Some(total)) if total > 0.0 && counted > 0.0 => Some(counted / total),
            _ => None,
        },
        cost: number(card.get("cost")),
        credits: number(card.get("credits")),
        tokens: figure(card.get("tokens")),
        requests: figure(card.get("requests")),
        output: figure(card.get("output")),
        cache: number(card.get("cache")),
        sessions: figure(card.get("sessions")),
        unpriced: figure(card.get("unpriced")),
        models,
    }
}

/// The homepage's plans and APIs, from its rows: the row's figure and the windows, balances and keys under it.
fn quotas(entries: &[Json], words: Words) -> Vec<Quota> {
    let services: Vec<(String, &catalog::Service)> =
        catalog::SERVICES.iter().map(|service| (service.label(), service)).collect();
    entries
        .iter()
        .filter_map(Json::as_object)
        .filter_map(|entry| {
            let label = text(entry.get("label"));
            let service = services.iter().find(|(name, _)| *name == label)?.1;
            let items = entry
                .get("children")
                .and_then(Json::as_array)
                .into_iter()
                .flatten()
                .filter_map(Json::as_object)
                // Figures only: not the rows that do something (add a key, open a page, set a reminder).
                .filter(|child| {
                    child.get("value").is_some()
                        && !["symbol", "href", "action", "radio"].iter().any(|key| child.contains_key(key))
                })
                .map(|child| Item {
                    name: words.text(&text(child.get("label"))),
                    value: words.text(&text(child.get("value"))),
                    tone: Tone::of_level(child.get("usageLevel").and_then(Json::as_str)),
                    detail: child.get("detail").and_then(Json::as_str).map(|detail| words.text(detail)),
                })
                .collect();
            Some(Quota {
                id: service.key.to_string(),
                name: words.text(&label),
                value: words.text(&text(entry.get("value"))),
                short: entry
                    .get("fallbacks")
                    .and_then(Json::as_array)
                    .and_then(|list| list.last())
                    .and_then(Json::as_str)
                    .map(|short| words.text(short)),
                tone: Tone::of_level(entry.get("usageLevel").and_then(Json::as_str)),
                items,
            })
        })
        .collect()
}

pub fn build(dashboard: &Object, entries: &[Json], zone: &str, words: Words) -> Report {
    let totals = summary_totals(dashboard);
    let parts = summary_parts(dashboard);
    let summary_tokens = number(totals.get("tokens"));
    let key = text(dashboard.get("rangeKey"));
    let currency = dashboard.get("currency").and_then(Json::as_object);
    let rate = currency.and_then(|currency| number(currency.get("rate"))).unwrap_or(1.0);
    let cards: Vec<&Object> =
        dashboard.get("providers").and_then(Json::as_array).into_iter().flatten().filter_map(Json::as_object).collect();
    let mut tools: Vec<Row> = cards
        .iter()
        .map(|card| {
            let id = card.get("id");
            let part = parts.iter().find(|part| part.get("id") == id);
            row(card, part, summary_tokens, words, false)
        })
        .collect();
    by_usage(&mut tools);
    let grouping = text(dashboard.get("grouping"));
    // A model's or project's share is of all the cards' tokens: projects go by the clients' own records.
    let groups = dashboard.get("cards").and_then(Json::as_array).map(|cards| {
        let cards: Vec<&Object> = cards.iter().filter_map(Json::as_object).collect();
        let total: f64 = cards.iter().map(|card| figure(card.get("tokens"))).sum();
        cards.iter().map(|card| row(card, Some(card), Some(total), words, true)).collect()
    });
    let updated = text(dashboard.get("updated"));
    Report {
        words,
        range: key.clone(),
        range_label: words.text(&text(dashboard.get("rangeLabel"))),
        title: words.say("{range} AI 用量", &[("range", &words.say(title_range(&key), &[]))]),
        date: poster_date(dashboard),
        period: dashboard.get("period").cloned().unwrap_or(Json::Null),
        zone: zone.to_string(),
        grouping,
        symbol: currency.map_or("$".into(), |currency| text(currency.get("symbol"))),
        code: currency.map_or("USD".into(), |currency| text(currency.get("code"))),
        rate,
        updated: updated.split_once('：').map(|(_, time)| time.to_string()),
        devices: dashboard.get("devices").and_then(Json::integer).unwrap_or(0),
        chart_status: words.text(&text(dashboard.get("chartStatus"))),
        cost: number(totals.get("cost")),
        cost_partial: totals.get("costPartial").is_some_and(Json::is_truthy),
        tokens: summary_tokens,
        requests: number(totals.get("requests")),
        cache: number(totals.get("cache")),
        trend: totals.get("history").and_then(Json::as_array).into_iter().flatten().map(|v| figure(Some(v))).collect(),
        trend_unit: words.text(&text(dashboard.get("chartUnit"))),
        trend_span: words.text(&text(dashboard.get("chartSpan"))),
        tools,
        groups,
        quotas: quotas(entries, words),
    }
}

/// Six decimals, so a sum of costs doesn't carry binary noise into the JSON.
fn round6(value: f64) -> Json {
    Json::Double((value * 1e6).round() / 1e6)
}

fn optional(value: Option<f64>, write: impl Fn(f64) -> Json) -> Json {
    value.map_or(Json::Null, write)
}

fn whole(value: f64) -> Json {
    Json::Int(value.round() as i64)
}

impl Report {
    /// A cost in the panel's currency.
    pub fn converted(&self, usd: f64) -> f64 {
        usd * self.rate
    }

    fn row_json(&self, row: &Row) -> Json {
        object! {
            "id" => row.id.as_str(), "name" => row.name.as_str(), "available" => row.available,
            "status" => row.status.as_str(),
            "notes" => Json::Array(row.notes.iter().map(|(note, _)| Json::from(note.as_str())).collect()),
            "inSummary" => row.in_summary,
            "share" => optional(row.share, |share| Json::Double((share * 1e4).round() / 1e4)),
            "cost" => optional(row.cost, |cost| round6(self.converted(cost))),
            "costUsd" => optional(row.cost, round6),
            "credits" => optional(row.credits, round6),
            "tokens" => whole(row.tokens), "requests" => whole(row.requests), "output" => whole(row.output),
            "cacheHitRate" => optional(row.cache, round6), "sessions" => whole(row.sessions),
            "unpriced" => whole(row.unpriced),
            "models" => Json::Array(row.models.iter().map(|model| {
                let mut entry = object! {
                    "name" => model.name.as_str(), "tokens" => whole(model.tokens), "share" => round6(model.share),
                };
                if let Some(credits) = model.credits {
                    entry.insert("credits", round6(credits));
                }
                Json::Object(entry)
            }).collect()),
        }
        .into()
    }

    /// The JSON for scripts: keys in English, figures as numbers, costs in the panel's currency and in dollars.
    pub fn json(&self, sections: &[&str]) -> Json {
        let mut out = object! {
            "schema" => 1, "version" => starmeter_core::VERSION,
            "language" => if self.words.english { "en" } else { "zh" },
            "range" => object! {
                "key" => self.range.as_str(), "label" => self.range_label.as_str(), "period" => self.period.clone(),
                "timeZone" => self.zone.as_str(),
            },
            "grouping" => self.grouping.as_str(),
            "currency" => object! { "code" => self.code.as_str(), "symbol" => self.symbol.as_str(), "rate" => self.rate },
            "updated" => self.updated.as_deref().map_or(Json::Null, Json::from),
            "devices" => self.devices,
        };
        if sections.contains(&"usage") {
            out.insert(
                "summary",
                object! {
                    "cost" => optional(self.cost, |cost| round6(self.converted(cost))),
                    "costUsd" => optional(self.cost, round6), "costPartial" => self.cost_partial,
                    "tokens" => optional(self.tokens, whole), "requests" => optional(self.requests, whole),
                    "cacheHitRate" => optional(self.cache, round6),
                },
            );
            out.insert(
                "trend",
                object! {
                    "unit" => self.trend_unit.as_str(), "span" => self.trend_span.as_str(),
                    "tokens" => Json::Array(self.trend.iter().map(|value| whole(*value)).collect()),
                },
            );
            out.insert("tools", Json::Array(self.tools.iter().map(|row| self.row_json(row)).collect()));
            out.insert(
                "groups",
                self.groups
                    .as_ref()
                    .map_or(Json::Null, |groups| Json::Array(groups.iter().map(|row| self.row_json(row)).collect())),
            );
        }
        if sections.contains(&"quotas") {
            out.insert(
                "quotas",
                Json::Array(
                    self.quotas
                        .iter()
                        .map(|quota| {
                            object! {
                                "id" => quota.id.as_str(), "name" => quota.name.as_str(), "value" => quota.value.as_str(),
                                "level" => quota.tone.name().map_or(Json::Null, Json::from),
                                "items" => Json::Array(quota.items.iter().map(|item| object! {
                                    "name" => item.name.as_str(), "value" => item.value.as_str(),
                                    "level" => item.tone.name().map_or(Json::Null, Json::from),
                                    "detail" => item.detail.as_deref().map_or(Json::Null, Json::from),
                                }.into()).collect()),
                            }
                            .into()
                        })
                        .collect(),
                ),
            );
        }
        out.into()
    }
}
