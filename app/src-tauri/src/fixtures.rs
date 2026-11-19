//! Stand-in data until starmeter-core is wired in: the synthetic fixtures in app/ui/fixtures, with the choices made in
//! the panel kept, as app/ui/src/lib/fixtureBackend.ts keeps them for the browser preview. Pick a scenario with
//! STARMETER_FIXTURE=failed, pending or empty.

use serde_json::{Map, Value, json};

const SCENARIOS: [(&str, &str); 4] = [
    ("default", include_str!("../../ui/fixtures/default.json")),
    ("failed", include_str!("../../ui/fixtures/failed.json")),
    ("pending", include_str!("../../ui/fixtures/pending.json")),
    ("empty", include_str!("../../ui/fixtures/empty.json")),
];

/// The logos the fixtures name as logo:<file>; the engine sends the files themselves, base64.
const LOGOS: [(&str, &[u8]); 12] = [
    ("deepseek-logo.svg", include_bytes!("../../../assets/deepseek-logo.svg")),
    ("logos/claude.svg", include_bytes!("../../../assets/logos/claude.svg")),
    ("logos/cursor.svg", include_bytes!("../../../assets/logos/cursor.svg")),
    ("logos/gemini.svg", include_bytes!("../../../assets/logos/gemini.svg")),
    ("logos/grok.svg", include_bytes!("../../../assets/logos/grok.svg")),
    ("logos/kimi.svg", include_bytes!("../../../assets/logos/kimi.svg")),
    ("logos/magpie.svg", include_bytes!("../../../assets/logos/magpie.svg")),
    ("logos/minimax.svg", include_bytes!("../../../assets/logos/minimax.svg")),
    ("logos/openai.svg", include_bytes!("../../../assets/logos/openai.svg")),
    ("logos/opencode.svg", include_bytes!("../../../assets/logos/opencode.svg")),
    ("logos/qoder.svg", include_bytes!("../../../assets/logos/qoder.svg")),
    ("logos/workbuddy.svg", include_bytes!("../../../assets/logos/workbuddy.svg")),
];

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n =
            (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            out.push(if i <= chunk.len() { TABLE[(n >> shift & 63) as usize] as char } else { '=' });
        }
    }
    out
}

fn expand(value: &mut Value) {
    match value {
        Value::Array(items) => items.iter_mut().for_each(expand),
        Value::Object(fields) => {
            for (key, field) in fields.iter_mut() {
                if let (true, Some(file)) = (key == "image", field.as_str().and_then(|s| s.strip_prefix("logo:"))) {
                    let data = LOGOS
                        .iter()
                        .find(|(name, _)| *name == file)
                        .map(|(_, bytes)| base64(bytes))
                        .unwrap_or_default();
                    *field = Value::String(data);
                } else {
                    expand(field);
                }
            }
        }
        _ => {}
    }
}

/// What an action did to the fixtures: whether the panel should reload, and the saved display preferences.
pub struct Outcome {
    pub changed: bool,
    pub preferences: Option<Value>,
}

pub struct Fixtures {
    data: Value,
    range: String,
    grouping: String,
    prefs: Value,
    pin_interval: u64,
    menu_bar: String,
    cny: bool,
    live: u64,
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| items.iter().filter_map(|s| s.as_str().map(str::to_owned)).collect())
        .unwrap_or_default()
}

fn same(action: &Value, wanted: &[String]) -> bool {
    action.as_array().is_some_and(|items| {
        items.len() == wanted.len() && items.iter().zip(wanted).all(|(a, b)| a.as_str() == Some(b))
    })
}

/// A switch or a choice, saved: the radio buttons of the same kind move together, a switch flips.
fn apply_choice(list: &mut Value, action: &[String]) -> bool {
    let Some(items) = list.as_array_mut() else { return false };
    if let Some(index) = items.iter().position(|e| same(&e["action"], action)) {
        if items[index]["radio"].as_bool() == Some(true) {
            for (i, entry) in items.iter_mut().enumerate() {
                if entry["radio"].as_bool() == Some(true) && entry["action"][0].as_str() == Some(action[0].as_str()) {
                    entry["checked"] = json!(i == index);
                }
            }
        } else if items[index].get("checked").is_some()
            && matches!(action.last().map(String::as_str), Some("true" | "false"))
        {
            let on = action.last().map(String::as_str) == Some("true");
            items[index]["checked"] = json!(on);
            let mut next = action.to_vec();
            *next.last_mut().expect("an action has a name") = (!on).to_string();
            items[index]["action"] = json!(next);
        }
        return true;
    }
    items.iter_mut().any(|e| apply_choice(&mut e["children"], action))
}

/// A parent row's value follows its chosen child (金额显示 · 人民币, 钉住时刷新 · 每 5 秒).
fn sync_values(list: &mut Value) {
    let Some(items) = list.as_array_mut() else { return };
    for entry in items {
        let chosen = entry["children"]
            .as_array()
            .and_then(|children| children.iter().find(|c| c["radio"] == json!(true) && c["checked"] == json!(true)))
            .and_then(|c| c["label"].as_str())
            .map(|label| label.split('（').next().unwrap_or(label).to_owned());
        if let (Some(label), true) = (chosen, entry.get("value").is_some()) {
            entry["value"] = json!(label);
        }
        sync_values(&mut entry["children"]);
    }
}

impl Fixtures {
    pub fn load() -> Self {
        let wanted = std::env::var("STARMETER_FIXTURE").unwrap_or_default();
        let text = SCENARIOS.iter().find(|(name, _)| *name == wanted).unwrap_or(&SCENARIOS[0]).1;
        let mut data: Value = serde_json::from_str(text).expect("the fixtures are valid JSON");
        expand(&mut data);
        let prefs = data["snapshot"]["preferences"].clone();
        Fixtures {
            data,
            range: "today".into(),
            grouping: "tool".into(),
            prefs,
            pin_interval: 2,
            menu_bar: "none".into(),
            cny: false,
            live: 0,
        }
    }

    fn visible(&self) -> Vec<String> {
        strings(&self.prefs["visible"])
    }

    /// Today's figures creep up on each live tick, so the pinned chart has something to draw.
    fn bump(&self, d: &mut Value) {
        if self.live == 0 || d["rangeKey"] != "today" {
            return;
        }
        let live = self.live as f64;
        for p in d["providers"].as_array_mut().into_iter().flatten() {
            let step = match p["id"].as_str() {
                Some("claude") => 41_000.0,
                Some("codex") => 23_000.0,
                Some("magpie") => 9_000.0,
                _ => continue,
            };
            let added = step * live * (0.6 + ((self.live * 7919) % 13) as f64 / 13.0);
            p["tokens"] = json!(p["tokens"].as_f64().unwrap_or(0.0) + added);
            // Most of what a request adds is context read from the cache; a sliver is what the model wrote.
            p["output"] = json!(p["output"].as_f64().unwrap_or(0.0) + added * 0.004);
            if let Some(cost) = p["cost"].as_f64() {
                p["cost"] = json!(cost + added * 0.38e-6);
            }
            p["requests"] = json!(p["requests"].as_i64().unwrap_or(0) + (live / 3.0).round() as i64);
            if let Some(last) = p["history"].as_array_mut().and_then(|h| h.last_mut()) {
                *last = json!(last.as_f64().unwrap_or(0.0) + added);
            }
            if p["counted"].is_object() {
                p["counted"]["tokens"] = json!(p["counted"]["tokens"].as_f64().unwrap_or(0.0) + added);
                if let Some(last) = p["counted"]["history"].as_array_mut().and_then(|h| h.last_mut()) {
                    *last = json!(last.as_f64().unwrap_or(0.0) + added);
                }
            }
        }
    }

    pub fn dashboard(&self, range: &str) -> Value {
        let dashboards = &self.data["dashboards"];
        let mut d = dashboards.get(range).unwrap_or(&dashboards["today"]).clone();
        self.bump(&mut d);
        let visible = self.visible();
        if let Some(providers) = d["providers"].as_array_mut() {
            providers.retain(|p| p["id"].as_str().is_some_and(|id| visible.iter().any(|v| v == id)));
        }
        d["showSummary"] = json!(visible.iter().any(|v| v == "summary"));
        if !visible.iter().any(|v| v == "heatmap") {
            d["heatmap"] = Value::Null;
        }
        d["allowCollapse"] = self.prefs["allowCollapse"].clone();
        d["expandedProviders"] = self.prefs["expandedProviders"].clone();
        d["grouping"] = json!(self.grouping);
        let has_cards = d["providers"].as_array().is_some_and(|p| !p.is_empty());
        if self.grouping != "tool" && has_cards {
            d["cards"] = self.data["cards"][range][&self.grouping].clone();
            d["groupNote"] =
                if self.grouping == "project" { json!("按客户端自己的记录") } else { Value::Null };
        }
        if self.cny {
            d["currency"] =
                json!({"code": "CNY", "symbol": "¥", "rate": 7.1, "note": "欧洲央行 2026-11-18 参考汇率 7.1000"});
        }
        d
    }

    /// Today's estimate or the tightest subscription window beside the star, or nothing.
    pub fn menu_bar_text(&self) -> String {
        match self.menu_bar.as_str() {
            "cost" => {
                let today = self.dashboard("today");
                let total: f64 = today["providers"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|p| p["counted"]["cost"].as_f64().or(p["cost"].as_f64()).unwrap_or(0.0))
                    .sum();
                if self.cny { format!("¥{:.2}", total * 7.1) } else { format!("${total:.2}") }
            }
            "quota" => "91%".into(),
            _ => String::new(),
        }
    }

    pub fn snapshot(&self) -> Value {
        let visible = self.visible();
        let mut s = self.data["snapshot"].clone();
        let modules = self.data["entryModules"].as_array().cloned().unwrap_or_default();
        let mut entries: Vec<Value> = Vec::new();
        for (entry, module) in s["entries"].as_array().cloned().unwrap_or_default().into_iter().zip(modules) {
            if module.as_str().is_some_and(|m| !visible.iter().any(|v| v == m)) {
                continue;
            }
            let separator = entry["separator"] == json!(true);
            if separator && entries.last().is_none_or(|last| last["separator"] == json!(true)) {
                continue;
            }
            entries.push(entry);
        }
        while entries.last().is_some_and(|last| last["separator"] == json!(true)) {
            entries.pop();
        }
        s["entries"] = Value::Array(entries);
        let dashboard = self.dashboard(&self.range);
        let label = dashboard["rangeLabel"].clone();
        s["dashboard"] = dashboard;
        for option in s["rangeOptions"].as_array_mut().into_iter().flatten() {
            let chosen = option["action"][1].as_str() == Some(self.range.as_str());
            option["checked"] = json!(chosen);
        }
        s["preferences"] = self.prefs.clone();
        s["appearance"] = self.prefs["appearance"].clone();
        s["theme"] = self.prefs["theme"].clone();
        s["pinInterval"] = json!(self.pin_interval);
        s["menuBarText"] = json!(self.menu_bar_text());
        let collapse = self.prefs["allowCollapse"].as_bool().unwrap_or(false);
        for option in s["moduleOptions"].as_array_mut().into_iter().flatten() {
            match option["action"][0].as_str() {
                Some("set-module") => {
                    let id = option["action"][1].as_str().unwrap_or_default().to_owned();
                    let on = visible.contains(&id);
                    option["checked"] = json!(on);
                    option["action"] = json!(["set-module", id, (!on).to_string()]);
                }
                Some("set-collapse") => {
                    option["checked"] = json!(collapse);
                    option["action"] = json!(["set-collapse", (!collapse).to_string()]);
                }
                _ => {}
            }
            if option["label"] == "外观与设置" {
                for row in option["children"].as_array_mut().into_iter().flatten() {
                    match row["action"][0].as_str() {
                        Some("set-theme") => row["checked"] = json!(row["action"][1] == self.prefs["theme"]),
                        Some("set-appearance") => row["checked"] = json!(row["action"][1] == self.prefs["appearance"]),
                        _ => {}
                    }
                    if row["label"] == "导出用量" {
                        row["value"] = label.clone();
                    }
                }
            }
        }
        s
    }

    fn saved(&mut self) -> Outcome {
        let revision = self.prefs["revision"].as_i64().unwrap_or(0) + 1;
        self.prefs["revision"] = json!(revision);
        Outcome { changed: true, preferences: Some(self.prefs.clone()) }
    }

    pub fn act(&mut self, action: &[String]) -> Outcome {
        let arg = |i: usize| action.get(i).cloned().unwrap_or_default();
        let unchanged = Outcome { changed: false, preferences: None };
        match action.first().map(String::as_str).unwrap_or_default() {
            "set-range" => {
                self.range = arg(1);
                return Outcome { changed: true, preferences: None };
            }
            "set-group" => {
                self.grouping = arg(1);
                return Outcome { changed: true, preferences: None };
            }
            "set-module" => {
                let (id, on) = (arg(1), arg(2) == "true");
                let mut visible = self.visible();
                visible.retain(|v| *v != id);
                if on {
                    visible.push(id);
                }
                self.prefs["visible"] = json!(visible);
                return self.saved();
            }
            "set-collapse" => {
                let on = arg(1) == "true";
                self.prefs["allowCollapse"] = json!(on);
                if on {
                    self.prefs["expandedProviders"] = json!([]);
                }
                return self.saved();
            }
            "set-expanded" => {
                let mut open = strings(&self.prefs["expandedProviders"]);
                open.retain(|v| *v != arg(1));
                if arg(2) == "true" {
                    open.push(arg(1));
                }
                open.sort();
                self.prefs["expandedProviders"] = json!(open);
                return self.saved();
            }
            "set-theme" => {
                self.prefs["theme"] = json!(arg(1));
                return self.saved();
            }
            "set-appearance" => {
                self.prefs["appearance"] = json!(arg(1));
                return self.saved();
            }
            "reset-modules" | "detect-modules" => {
                self.prefs["visible"] = self.data["snapshot"]["preferences"]["visible"].clone();
                if action[0] == "reset-modules" {
                    self.prefs["theme"] = json!("system");
                    self.prefs["appearance"] = json!("solid");
                }
                return self.saved();
            }
            "refresh-live" => {
                self.live += 1;
                return Outcome { changed: true, preferences: None };
            }
            "set-pin-interval" => self.pin_interval = arg(1).parse().unwrap_or(2),
            "set-menu-bar" => self.menu_bar = arg(1),
            "set-currency" => self.cny = arg(1) == "CNY",
            _ => {}
        }
        let snapshot = &mut self.data["snapshot"];
        if apply_choice(&mut snapshot["entries"], action) || apply_choice(&mut snapshot["moduleOptions"], action) {
            sync_values(&mut snapshot["moduleOptions"]);
            return Outcome { changed: true, preferences: None };
        }
        unchanged
    }

    /// The range on screen as the export would write it: CSV with a BOM for Excel, or JSON; counts only.
    pub fn export(&self, format: &str) -> String {
        let d = self.dashboard(&self.range);
        let providers = d["providers"].as_array().cloned().unwrap_or_default();
        if format == "json" {
            let tools: Map<String, Value> = providers
                .iter()
                .map(|p| {
                    let fields = json!({"totals": {"requests": p["requests"], "tokens": p["tokens"], "costUsd": p["cost"], "credits": p["credits"]},
                        "models": p["models"]});
                    (p["id"].as_str().unwrap_or_default().to_owned(), fields)
                })
                .collect();
            return serde_json::to_string_pretty(
                &json!({"range": d["rangeKey"], "period": d["period"], "tools": tools}),
            )
            .unwrap_or_default();
        }
        let mut csv = String::from(
            "\u{feff}date,tool,requests,input_tokens,output_tokens,total_tokens,cost_usd,unpriced_requests,credits\n",
        );
        for p in providers {
            csv += &format!(
                "{},{},{},{},{},{},{},{},{}\n",
                d["date"].as_str().unwrap_or_default(),
                p["id"].as_str().unwrap_or_default(),
                p["requests"],
                p["tokens"].as_f64().unwrap_or(0.0) - p["output"].as_f64().unwrap_or(0.0),
                p["output"],
                p["tokens"],
                p["cost"].as_f64().map(|c| format!("{c:.6}")).unwrap_or_default(),
                p["unpriced"],
                p["credits"].as_f64().map(|c| c.to_string()).unwrap_or_default()
            );
        }
        csv
    }

    pub fn range_label(&self) -> String {
        self.dashboard(&self.range)["rangeLabel"].as_str().unwrap_or("今天").to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_standard_alphabet() {
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
    }

    #[test]
    fn logos_are_expanded_and_choices_are_kept() {
        let mut fixtures = Fixtures::load();
        let snapshot = fixtures.snapshot();
        assert!(snapshot["entries"][0]["image"].as_str().is_some_and(|s| s.starts_with("PHN2")));
        let outcome = fixtures.act(&["set-module".into(), "claude".into(), "false".into()]);
        assert!(outcome.preferences.is_some());
        assert!(fixtures.dashboard("today")["providers"].as_array().unwrap().iter().all(|p| p["id"] != "claude"));
        fixtures.act(&["set-currency".into(), "CNY".into()]);
        assert_eq!(fixtures.dashboard("today")["currency"]["symbol"], "¥");
    }
}
