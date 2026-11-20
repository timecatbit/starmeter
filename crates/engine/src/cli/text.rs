//! The report laid out for a terminal: the title and date, Summary's four figures, a trend line, a table of tools (or
//! models or projects), and the plans' and APIs' figures. Columns drop out as the terminal narrows: the share bar,
//! then cache and requests; below 50 columns every tool is one line.

use super::figures;
use super::report::{Quota, Report, Row, Tone};
use super::terminal::{fit, pad, width};

/// What to show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    /// Summary, the tools and the plans, on one screen.
    Overview,
    /// Summary, the trend, the cards with their models when asked.
    Usage { detail: bool },
    /// The plans and APIs with their windows and keys.
    Quota,
}

pub struct Paint {
    pub colour: bool,
}

impl Paint {
    fn wrap(&self, code: &str, text: &str) -> String {
        if self.colour && !text.is_empty() { format!("\x1b[{code}m{text}\x1b[0m") } else { text.to_string() }
    }
    fn bold(&self, text: &str) -> String {
        self.wrap("1", text)
    }
    fn dim(&self, text: &str) -> String {
        self.wrap("2", text)
    }
    fn accent(&self, text: &str) -> String {
        self.wrap("36", text)
    }
    fn tone(&self, text: &str, tone: Tone) -> String {
        match tone {
            Tone::Plain => text.to_string(),
            Tone::Muted => self.dim(text),
            Tone::Warning => self.wrap("33", text),
            Tone::Critical => self.wrap("31", text),
        }
    }
}

const SPARKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
const EIGHTHS: [char; 8] = ['▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'];

/// A series in one line of block characters at most `room` long: neighbouring points are added when it is longer.
pub fn sparkline(values: &[f64], room: usize) -> String {
    if values.is_empty() || room == 0 {
        return String::new();
    }
    let size = values.len().div_ceil(room);
    let buckets: Vec<f64> = values.chunks(size).map(|chunk| chunk.iter().map(|value| value.max(0.0)).sum()).collect();
    let top = buckets.iter().copied().fold(0.0, f64::max);
    buckets
        .iter()
        .map(|value| {
            if top <= 0.0 || *value <= 0.0 {
                SPARKS[0]
            } else {
                SPARKS[(1.0 + (value / top) * 6.0).round().clamp(1.0, 7.0) as usize]
            }
        })
        .collect()
}

/// A share as a bar `room` cells wide, in eighths.
pub fn bar(share: f64, room: usize) -> String {
    let eighths = (share.clamp(0.0, 1.0) * room as f64 * 8.0).round() as usize;
    let mut out = "█".repeat(eighths / 8);
    if !eighths.is_multiple_of(8) {
        out.push(EIGHTHS[eighths % 8 - 1]);
    }
    if out.is_empty() && share > 0.0 {
        out.push(EIGHTHS[0]);
    }
    out
}

struct Writer<'a> {
    report: &'a Report,
    paint: &'a Paint,
    width: usize,
    lines: Vec<String>,
}

impl Writer<'_> {
    fn say(&self, zh: &str) -> String {
        self.report.words.say(zh, &[])
    }

    fn own(&self, zh: &'static str, en: &'static str) -> &'static str {
        self.report.words.own(zh, en)
    }

    fn money(&self, usd: Option<f64>) -> String {
        usd.map_or("—".into(), |usd| format!("{}{}", self.report.symbol, figures::money(self.report.converted(usd))))
    }

    fn blank(&mut self) {
        if self.lines.last().is_some_and(|line| !line.is_empty()) {
            self.lines.push(String::new());
        }
    }

    /// The title and the range's dates on the left, when the figures were read on the right.
    fn header(&mut self, title: &str) {
        let report = self.report;
        let mut left = vec![format!("Starmeter · {title}"), report.date.clone()];
        let mut right = String::new();
        if let Some(time) = &report.updated {
            right = report.words.text(&format!("更新时间：{time}"));
        }
        if report.devices > 0 {
            let devices = report.words.say("含 {n} 台电脑", &[("n", &report.devices.to_string())]);
            right = if right.is_empty() { devices } else { format!("{right} · {devices}") };
        }
        let joined = |parts: &[String]| parts.join("  ");
        if width(&joined(&left)) + 2 + width(&right) > self.width {
            left[0] = title.to_string();
        }
        if width(&joined(&left)) + 2 + width(&right) > self.width {
            right.clear();
        }
        let gap = self.width.saturating_sub(width(&joined(&left)) + width(&right)).max(2);
        let mut line = format!("{}  {}", self.paint.bold(&left[0]), self.paint.dim(&left[1]));
        if width(&joined(&left)) > self.width {
            line = fit(&joined(&left), self.width);
        } else if !right.is_empty() {
            line.push_str(&" ".repeat(gap));
            line.push_str(&self.paint.dim(&right));
        }
        self.lines.push(line);
    }

    /// Summary's four figures, as the share picture has them: the estimate, tokens, requests and cache hits.
    fn summary(&mut self) {
        let report = self.report;
        let cost_label = self.say(if report.cost_partial { "已知 API 估算" } else { "API 估算" });
        let figures = [
            (cost_label, self.money(report.cost)),
            (self.say("Token"), report.tokens.map_or("—".into(), figures::tokens)),
            (self.say("请求"), report.requests.map_or("—".into(), figures::count)),
            (self.say("缓存命中"), report.cache.map_or("—".into(), figures::percent)),
        ];
        let columns = if self.width >= 64 { 4 } else { 2 };
        let room = ((self.width.saturating_sub(2)) / columns).clamp(10, 20);
        for row in figures.chunks(columns) {
            let labels: Vec<String> = row.iter().map(|(label, _)| pad(&fit(label, room - 1), room, false)).collect();
            let values: Vec<String> = row.iter().map(|(_, value)| pad(&fit(value, room - 1), room, false)).collect();
            self.lines.push(format!("  {}", self.paint.dim(labels.join("").trim_end())));
            self.lines.push(format!("  {}", self.paint.bold(values.join("").trim_end())));
        }
    }

    fn trend(&mut self) {
        let report = self.report;
        if report.trend.len() < 2 || !report.trend.iter().any(|value| *value > 0.0) {
            return;
        }
        let unit = report.words.say("{unit} Token", &[("unit", &report.trend_unit)]);
        let mut label =
            if report.trend_span.is_empty() { unit.clone() } else { format!("{unit} · {}", report.trend_span) };
        if width(&label) + 14 > self.width {
            label = unit;
        }
        let room = self.width.saturating_sub(4 + width(&label)).min(64);
        if room < 8 {
            return;
        }
        let line = sparkline(&report.trend, room);
        let mut text = format!("  {}  {}", self.paint.accent(&line), self.paint.dim(&label));
        if !report.chart_status.is_empty() {
            let status = format!(" · {}", report.chart_status);
            if width(&line) + width(&label) + width(&status) + 4 <= self.width {
                text.push_str(&self.paint.tone(&status, Tone::Warning));
            }
        }
        self.lines.push(text);
    }

    fn cost_cell(&self, row: &Row) -> String {
        match (row.cost, row.credits) {
            (None, Some(credits)) => figures::credits(credits),
            (cost, _) => self.money(cost),
        }
    }

    fn token_cell(row: &Row) -> String {
        if row.tokens == 0.0 && row.credits.is_some() { "—".into() } else { figures::tokens(row.tokens) }
    }

    fn notes(&self, row: &Row) -> String {
        row.notes.iter().map(|(note, tone)| self.paint.tone(note, *tone)).collect::<Vec<_>>().join(" · ")
    }

    fn notes_width(row: &Row) -> usize {
        let parts: Vec<&str> = row.notes.iter().map(|(note, _)| note.as_str()).collect();
        width(&parts.join(" · "))
    }

    /// The cards as a table, the costliest first; tools with nothing in the range fold into one line.
    fn table(&mut self, rows: &[Row], heading: &str, grouped: bool, detail: bool) {
        let words = self.report.words;
        let (shown, idle): (Vec<&Row>, Vec<&Row>) = rows
            .iter()
            .partition(|row| row.used() || !row.available || row.notes.iter().any(|(_, tone)| *tone == Tone::Warning));
        if shown.is_empty() {
            if rows.is_empty() {
                self.lines.push(format!(
                    "  {}",
                    self.paint.dim(self.own(
                        "主页还没有勾选工具：在 Starmeter 面板的“主页模块”里勾选",
                        "No tools are on the homepage yet: choose them under Home modules in the Starmeter panel"
                    ))
                ));
            } else {
                self.lines.push(format!("  {}", self.paint.dim(&self.say("此时段暂无调用"))));
            }
            return;
        }
        let name_room = shown.iter().map(|row| width(&row.name)).max().unwrap_or(4).clamp(6, 20);
        let model_room = shown
            .iter()
            .flat_map(|row| &row.models)
            .map(|model| width(&model.name))
            .max()
            .unwrap_or(4)
            .clamp(4, 28)
            .min(self.width.saturating_sub(4 + 10 + 2 + 6));
        let compact = self.width < 50;
        let columns: Vec<(&str, usize)> = if self.width >= 80 {
            vec![("cost", 11), ("tokens", 9), ("requests", 9), ("cache", 8), ("share", 7), ("bar", 9)]
        } else if self.width >= 64 {
            vec![("cost", 11), ("tokens", 9), ("requests", 9), ("cache", 8), ("share", 7)]
        } else {
            vec![("cost", 11), ("tokens", 9), ("share", 7)]
        };
        if !compact {
            let mut header = pad(heading, name_room, false);
            for (column, room) in &columns {
                let title = match *column {
                    "cost" => self.say("费用"),
                    "tokens" => self.say("Token"),
                    "requests" => self.say("请求"),
                    "cache" => self.say("缓存"),
                    "share" => self.own("占比", "Share").to_string(),
                    _ => String::new(),
                };
                header.push_str(&if *column == "bar" {
                    " ".repeat(*room)
                } else {
                    pad(&fit(&title, room - 1), *room, true)
                });
            }
            self.lines.push(format!("  {}", self.paint.dim(header.trim_end())));
        }
        for row in &shown {
            let name = fit(&row.name, name_room);
            if compact {
                let mut parts = vec![self.cost_cell(row), Self::token_cell(row)];
                if let Some(share) = row.share {
                    parts.push(figures::share(share));
                }
                let figures_text = if row.available && row.used() { parts.join(" · ") } else { String::new() };
                let mut line = format!("  {}  {}", self.paint.bold(&name), figures_text);
                let used = 4 + width(&name) + width(&figures_text);
                if !row.notes.is_empty() {
                    if used + 3 + Self::notes_width(row) <= self.width {
                        line.push_str(&format!("  {}", self.notes(row)));
                    } else {
                        self.lines.push(line);
                        line = format!("    {}", self.notes(row));
                    }
                }
                self.lines.push(line);
            } else {
                let mut line = self.paint.bold(&pad(&name, name_room, false));
                let mut used = name_room;
                let has_figures = row.available && row.used();
                for (column, room) in &columns {
                    let cell = if !has_figures {
                        String::new()
                    } else {
                        match *column {
                            "cost" => self.cost_cell(row),
                            "tokens" => Self::token_cell(row),
                            "requests" => figures::count(row.requests),
                            "cache" => row.cache.map_or("—".into(), figures::percent),
                            "share" => row.share.map_or(String::new(), figures::share),
                            _ => String::new(),
                        }
                    };
                    if *column == "bar" {
                        let drawn =
                            row.share.filter(|_| has_figures).map(|share| bar(share, room - 2)).unwrap_or_default();
                        line.push_str(&format!(" {}", self.paint.accent(&pad(&drawn, room - 1, false))));
                    } else {
                        line.push_str(&pad(&fit(&cell, room - 1), *room, true));
                    }
                    used += room;
                }
                if !row.notes.is_empty() {
                    if used + 4 + Self::notes_width(row) <= self.width || !has_figures {
                        line.push_str(&format!("  {}", self.notes(row)));
                        self.lines.push(format!("  {}", line.trim_end()));
                    } else {
                        self.lines.push(format!("  {}", line.trim_end()));
                        self.lines.push(format!("  {}  {}", " ".repeat(name_room.min(4)), self.notes(row)));
                    }
                } else {
                    self.lines.push(format!("  {}", line.trim_end()));
                }
            }
            // A model's or project's card that is all one tool says so, as the panel's card does.
            let single = grouped && row.models.len() == 1 && row.models[0].share >= 0.999 && !row.models[0].unpriced;
            if detail && single {
                let from = words.say("全部来自 {name}", &[("name", &row.models[0].name)]);
                self.lines.push(format!("    {}", self.paint.dim(&fit(&from, self.width.saturating_sub(4)))));
            } else if detail {
                for model in &row.models {
                    let name = fit(&model.name, model_room);
                    let text = format!(
                        "    {}{}  {}",
                        pad(&name, model_room, false),
                        pad(&model.amount, 10, true),
                        figures::share(model.share)
                    );
                    self.lines.push(self.paint.dim(&text));
                }
            }
        }
        if !idle.is_empty() {
            let names: Vec<&str> = idle.iter().map(|row| row.name.as_str()).collect();
            let list = names.join(if words.english { ", " } else { "、" });
            let text = format!("{}{}{list}", self.say("无用量"), if words.english { ": " } else { "：" });
            self.lines.push(format!("  {}", self.paint.dim(&fit(&text, self.width.saturating_sub(2)))));
        }
    }

    /// A plan or an API in one line: its name and its figure, coloured by how much is used.
    fn quota_line(&self, quota: &Quota, name_room: usize) -> String {
        let room = self.width.saturating_sub(4 + name_room);
        let value = if width(&quota.value) > room {
            quota.short.clone().unwrap_or_else(|| quota.value.clone())
        } else {
            quota.value.clone()
        };
        format!(
            "  {}  {}",
            pad(&fit(&quota.name, name_room), name_room, false),
            self.paint.tone(&fit(&value, room), quota.tone)
        )
    }

    fn quotas(&mut self, detail: bool) {
        let report = self.report;
        if report.quotas.is_empty() {
            if detail {
                self.lines.push(format!(
                    "  {}",
                    self.paint.dim(self.own(
                        "主页还没有订阅或 API：在 Starmeter 面板的“主页模块”里勾选",
                        "No plans or APIs are on the homepage yet: choose them under Home modules in the Starmeter panel"
                    ))
                ));
            }
            return;
        }
        let name_room = report.quotas.iter().map(|quota| width(&quota.name)).max().unwrap_or(8).clamp(8, 20);
        let item_room = report
            .quotas
            .iter()
            .flat_map(|quota| &quota.items)
            .map(|item| width(&item.name))
            .max()
            .unwrap_or(4)
            .clamp(4, 16);
        for quota in &report.quotas {
            let line = self.quota_line(quota, name_room);
            self.lines.push(if detail { self.paint.bold(&line) } else { line });
            if !detail {
                continue;
            }
            for item in &quota.items {
                let head = format!("    {}  ", pad(&fit(&item.name, item_room), item_room, false));
                let value_room = self.width.saturating_sub(width(&head));
                let mut line = format!("{head}{}", self.paint.tone(&fit(&item.value, value_room), item.tone));
                if let Some(detail) = &item.detail {
                    let used = width(&head) + width(&item.value) + 2;
                    if used + width(detail) <= self.width {
                        line.push_str(&format!("  {}", self.paint.dim(detail)));
                    } else {
                        self.lines.push(line);
                        let indent = " ".repeat(width(&head));
                        line = format!(
                            "{indent}{}",
                            self.paint.dim(&fit(detail, self.width.saturating_sub(width(&head))))
                        );
                    }
                }
                self.lines.push(line);
            }
        }
    }
}

/// The report as lines for a terminal `width` columns wide.
pub fn render(report: &Report, view: View, width: usize, paint: &Paint) -> String {
    let mut writer = Writer { report, paint, width: width.max(30), lines: Vec::new() };
    match view {
        View::Overview | View::Usage { .. } => {
            writer.header(&report.title);
            writer.blank();
            writer.summary();
            writer.trend();
            writer.blank();
            let detail = matches!(view, View::Usage { detail: true });
            let words = report.words;
            match &report.groups {
                Some(groups) if view != View::Overview => {
                    let heading = words.say(if report.grouping == "model" { "模型" } else { "项目" }, &[]);
                    writer.table(groups, &heading, true, detail);
                    if report.grouping == "project" {
                        writer.lines.push(format!("  {}", paint.dim(&words.say("按客户端自己的记录", &[]))));
                    }
                }
                _ => writer.table(&report.tools, &words.say("工具", &[]), false, detail),
            }
            if view == View::Overview && !report.quotas.is_empty() {
                writer.blank();
                writer.lines.push(format!("  {}", paint.bold(writer.own("订阅与额度", "Plans and balances"))));
                writer.quotas(false);
            }
        }
        View::Quota => {
            let title = writer.own("订阅与额度", "Plans and balances").to_string();
            writer.lines.push(paint.bold(&format!("Starmeter · {title}")));
            writer.blank();
            writer.quotas(true);
        }
    }
    let mut text = writer.lines.join("\n");
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_series_fits_the_room() {
        assert_eq!(sparkline(&[0.0, 1.0, 2.0, 4.0], 10), "▁▄▅█");
        assert_eq!(sparkline(&[1.0; 20], 10).chars().count(), 10);
        assert_eq!(sparkline(&[0.0, 0.0], 5), "▁▁");
        assert_eq!(bar(0.5, 8), "████");
        assert_eq!(bar(0.0625, 8), "▌");
        assert_eq!(bar(0.001, 8), "▏");
        assert_eq!(bar(0.0, 8), "");
    }
}
