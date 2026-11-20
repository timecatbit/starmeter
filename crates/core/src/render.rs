//! The menu SwiftBar draws, and the entry's commands: the menu itself, the background modes, actions, the Claude
//! status line, the standalone app's worker and a desktop shell's requests.

use std::path::Path;
use std::sync::Arc;

use crate::host::StreamHost;
use crate::json::{Format, Json, Object};
use crate::panel_snapshot::shell_split;
use crate::rows::{MENU_ICON_SIZE, MUTED_COLOR, Options, as_option, fmt_tokens, item, merged};
use crate::runtime::{Runtime, VERSION, now};
use crate::self_update::{AUTHOR, REPOSITORY};
use crate::{claude_bridge, discovery, files, object, preferences, pricing, py};

const NATIVE_ONLY: [&str; 11] = [
    "color",
    "sfcolor",
    "sfcolors",
    "panelattention",
    "paneldetail",
    "panelfallback",
    "panelsection",
    "panelstatus",
    "panelradio",
    "panelusagelevel",
    "panelkey",
];

/// A row for SwiftBar's own menu: the panel-only options dropped, system colors kept for contrast.
pub fn native_menu_line(line: &str) -> String {
    let Some(at) = line.find(" | ") else { return line.to_string() };
    let mut options = Options::new();
    for part in shell_split(&line[at + 3..]) {
        if let Some((key, value)) = part.split_once('=') {
            options.insert(key, value);
        }
    }
    for key in NATIVE_ONLY {
        options.remove(key);
    }
    let rest: Vec<String> = options.iter().map(|(key, value)| as_option(key, value)).collect();
    format!("{}{}", &line[..at + 3], rest.join(" "))
}

fn reply(value: Json) {
    StreamHost::emit(&object! { "result" => value });
}

impl Runtime {
    /// The star, with the figure the user chose beside it; kept steady while a source reconnects.
    pub fn title(&self, data: Option<&Object>) -> String {
        // 18 points: the mark's dots fall on whole pixels there, a little under the system icons beside it.
        let value = data.map(|data| self.menu_bar_value(Some(data))).unwrap_or_default();
        item(
            &value,
            0,
            merged(&[
                &object! { "tooltip" => "AI 用量面板", "emojize" => "false", "size" => Json::Null },
                &self.logo_mark(18),
            ]),
        )
    }

    fn panel_title(&self, line: &str) -> String {
        if !Path::new(&self.panel_app()).is_dir() {
            return line.to_string();
        }
        let mut options = self.act("panel", &[]);
        options.remove("refresh");
        let rest: Vec<String> = options.iter().map(|(key, value)| as_option(key, value)).collect();
        format!("{line} {}", rest.join(" "))
    }

    fn about_menu_lines(&self) -> Vec<String> {
        vec![
            item("关于 Starmeter", 0, self.logo_mark(MENU_ICON_SIZE)),
            item(
                &format!("版本 {VERSION} · 更新记录"),
                1,
                object! { "sfimage" => "tag", "href" => format!("{REPOSITORY}/blob/main/CHANGELOG.md") },
            ),
            item(
                &format!("作者 {AUTHOR} · GitHub 仓库"),
                1,
                object! { "sfimage" => "person.crop.circle", "href" => REPOSITORY },
            ),
        ]
    }

    fn detection_menu_lines(&self) -> Vec<String> {
        // One clickable row: its confirmation dialog explains what detection reads and what it replaces.
        vec![item(
            "自动检测…",
            0,
            merged(&[&object! { "sfimage" => "sparkle.magnifyingglass" }, &self.act("detect-modules", &[])]),
        )]
    }

    /// The module list as SwiftBar rows, for when the native panel isn't installed.
    fn module_menu_lines(&self) -> Vec<String> {
        let mut rows = vec![item("主页模块", 0, object! { "sfimage" => "square.grid.2x2" })];
        fn append(runtime: &Runtime, rows: &mut Vec<String>, entries: &[Json], level: usize) {
            for entry in entries.iter().filter_map(Json::as_object) {
                let label = py::str(entry.get("label"));
                let action: Vec<String> = entry
                    .get("action")
                    .and_then(Json::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|part| part.as_str().map(String::from))
                    .collect();
                let checked = entry.get("checked").cloned().unwrap_or(Json::Bool(false));
                let act = |action: &[String]| {
                    let rest: Vec<&str> = action[1..].iter().map(String::as_str).collect();
                    runtime.act(&action[0], &rest)
                };
                let children = entry.get("children").and_then(Json::as_array).cloned().unwrap_or_default();
                if entry.get("separator").is_some_and(Json::is_truthy) {
                    rows.push("-".repeat(2 * level + 3));
                } else if entry.get("section").is_some_and(Json::is_truthy) {
                    rows.push(item(&label, level, object! { "color" => MUTED_COLOR, "size" => 10 }));
                } else if !children.is_empty() {
                    let value = entry
                        .get("value")
                        .filter(|value| value.is_truthy())
                        .map(|value| format!(" · {}", py::str(Some(value))))
                        .unwrap_or_default();
                    rows.push(item(&format!("{label}{value}"), level, Options::new()));
                    if action.first().map(String::as_str) == Some("set-module") {
                        rows.push(item(
                            "显示在主页",
                            level + 1,
                            merged(&[&object! { "checked" => checked.clone() }, &act(&action)]),
                        ));
                    }
                    append(runtime, rows, &children, level + 1);
                } else if !action.is_empty() {
                    rows.push(item(&label, level, merged(&[&object! { "checked" => checked }, &act(&action)])));
                } else {
                    rows.push(item(&label, level, object! { "size" => 11 }));
                }
            }
        }
        append(self, &mut rows, &self.module_options(&Object::new()), 1);
        rows
    }

    /// Rendering only reads caches; network and log reads run in the background worker.
    pub fn gather(&self) -> Object {
        self.schedule_background_refresh();
        self.cached_data()
    }

    /// SwiftBar's text for one run. With the native panel installed, its snapshot is written and the menu keeps only
    /// the right-click rows.
    pub fn menu_text(&self, data: &Object) -> String {
        let moment = now();
        let dashboard = self.dashboard_payload(data, data.get("range_usage_error").and_then(Json::as_str), moment);
        let options = self.module_options(data);
        let mut native = vec![item(
            "主页模块…",
            0,
            merged(&[&object! { "sfimage" => "square.grid.2x2" }, &self.act("panel-modules", &[])]),
        )];
        native.extend(self.detection_menu_lines());
        native.extend(self.about_menu_lines());
        let panel = Path::new(&self.panel_app()).is_dir();
        if self.environment.get("OCX_NATIVE_LAYOUT").map(String::as_str) == Some("1") || !panel {
            let mut overview: Vec<String> = Vec::new();
            let providers: Vec<&Object> = dashboard
                .get("providers")
                .and_then(Json::as_array)
                .into_iter()
                .flatten()
                .filter_map(Json::as_object)
                .collect();
            if self.module_enabled("summary") {
                let valid: Vec<&&Object> = providers
                    .iter()
                    .filter(|provider| {
                        provider.get("available").is_some_and(Json::is_truthy)
                            && provider.get("summaryIncluded").is_none_or(Json::is_truthy)
                    })
                    .collect();
                let total: f64 = valid
                    .iter()
                    .map(|provider| {
                        let counted =
                            provider.get("counted").and_then(Json::as_object).filter(|counted| !counted.is_empty());
                        counted.unwrap_or(provider).get("tokens").and_then(Json::python_float).unwrap_or(0.0)
                    })
                    .sum();
                let text = if valid.is_empty() {
                    "Summary · 数据待同步".to_string()
                } else {
                    format!(
                        "Summary · {} · {} Token",
                        py::str(dashboard.get("rangeLabel")),
                        fmt_tokens(Some(&Json::Double(total)))
                    )
                };
                overview.push(item(&text, 0, Options::new()));
            }
            for provider in &providers {
                let name = py::str(provider.get("name"));
                let figure = if provider.get("available").is_some_and(Json::is_truthy) {
                    format!("{} Token", fmt_tokens(provider.get("tokens")))
                } else {
                    py::str(provider.get("status"))
                };
                overview.push(item(&format!("{name} · {figure}"), 0, Options::new()));
            }
            let mut lines = overview.clone();
            if !overview.is_empty() {
                lines.push("---".into());
            }
            lines.extend(self.management_lines(data));
            lines.push("---".into());
            lines.extend(self.module_menu_lines());
            let mut text = vec![self.title(Some(data)), "---".into()];
            text.extend(lines.iter().map(|line| native_menu_line(line)));
            return text.join("\n");
        }
        let snapshot = self.panel_snapshot(data, Some(dashboard), Some(options), moment);
        self.write_json_cache(&self.cache_path("panel-view"), &Json::Object(snapshot));
        let mut text = vec![self.panel_title(&self.title(Some(data))), "---".into()];
        text.extend(native.iter().map(|line| native_menu_line(line)));
        text.join("\n")
    }

    /// One run of the entry: a background mode, an action, or the menu.
    pub fn main(&mut self, arguments: &[String]) -> i32 {
        self.sync_connection_cache();
        let first = arguments.first().map(String::as_str).unwrap_or("");
        let rest = arguments.get(1..).unwrap_or(&[]);
        match first {
            "--initialize-modules" => {
                if Path::new(&format!("{}/preferences.json", self.state_dir)).exists() {
                    println!("保留已有模块选择");
                    return 0;
                }
                let detected = self.detected_modules();
                if discovery::initialize(&self.state_dir, &detected) {
                    println!("已自动选择模块：{}", detected.join(", "));
                } else {
                    println!("保留已有模块选择");
                }
                0
            }
            "--background-refresh" | "--background-refresh-wait" => {
                self.background_refresh(first == "--background-refresh-wait");
                0
            }
            "--refresh-range" => {
                self.refresh_usage_range(true);
                self.refresh_swiftbar();
                0
            }
            "--self-update" => self.self_update_worker(rest.first().map(String::as_str).unwrap_or("")),
            "--claude-statusline" => {
                let Some(index) = arguments.iter().position(|argument| argument == "--state-dir") else { return 2 };
                let Some(state) = arguments.get(index + 1) else { return 2 };
                claude_bridge::run(state)
            }
            "--action" => {
                let name = rest.first().cloned().unwrap_or_default();
                self.write_json_cache(
                    &self.cache_path("last-action"),
                    &object! { "action" => name.as_str(), "receivedAt" => now() }.into(),
                );
                self.run_action(&name, rest.get(1..).unwrap_or(&[])).unwrap_or(1)
            }
            "--refresh" => 0,
            "--worker" => self.worker(rest),
            "--desktop" => self.desktop_command(rest),
            "--refresh-claude-bridge" => {
                // After an install: a bridge that is on runs from this engine's new copy.
                if claude_bridge::enabled(&self.state_dir) {
                    let _ = claude_bridge::install(&self.state_dir, None, &self.environment);
                }
                0
            }
            _ => {
                self.consume_panel_action();
                // Actions above may change settings; the render below sees one consistent snapshot.
                let text = self.render(|| self.menu_text(&self.gather()));
                println!("{text}");
                0
            }
        }
    }

    fn write_snapshot(&self) {
        self.render(|| {
            let snapshot = self.panel_snapshot(&self.cached_data(), None, None, now());
            self.write_json_cache(&self.cache_path("panel-view"), &Json::Object(snapshot));
        });
    }

    /// The standalone app's private worker: the panel's snapshot after setup, an action, or the background reads.
    fn worker(&self, arguments: &[String]) -> i32 {
        let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
        match arguments.as_slice() {
            ["paths"] => {
                let paths = Json::Object(object! { "snapshot" => self.cache_path("panel-view") });
                println!("{}", paths.serialized_with(Format { ensure_ascii: true, ..Format::default() }));
            }
            ["smoke-test"] => {
                // The test rewrites the module selection, so it runs only in the build's private home.
                if self.environment.get("TOKEN_MONITOR_SMOKE_TEST").map(String::as_str) != Some("1") {
                    return 2;
                }
                let root = format!("{}/smoke-fixture", self.state_dir);
                let _ = files::make_directory(Path::new(&root), 0o700);
                let record = Json::Object(object! {
                    "t" => now() - 10.0, "agent" => "codex", "provider" => "test", "model" => "gpt-5.4", "in" => 1230, "out" => 4,
                });
                let _ = std::fs::write(format!("{root}/usage.jsonl"), record.serialized() + "\n");
                let mut settings = preferences::read(Path::new(&self.state_dir));
                settings.insert("visible", Json::Array(vec!["summary".into(), "heatmap".into(), "magpie".into()]));
                settings.insert("paths", object! { "magpie" => root.as_str() });
                if preferences::write(Path::new(&self.state_dir), settings).is_err() {
                    return 1;
                }
                self.render(|| {
                    self.range_usage(Some("all"), None);
                });
                self.write_json_cache(
                    &self.cache_path("view-range"),
                    &object! { "schema" => 1, "range" => "all" }.into(),
                );
                self.write_snapshot();
                let view = self.read_cache("panel-view");
                let dashboard = view.get("dashboard").and_then(Json::as_object).cloned().unwrap_or_default();
                let tokens = dashboard
                    .get("providers")
                    .and_then(Json::as_array)
                    .and_then(|providers| providers.first())
                    .and_then(|provider| provider.get("tokens"))
                    .and_then(Json::python_float);
                let heat: f64 = dashboard
                    .get("heatmap")
                    .and_then(|heatmap| heatmap.get("tokens"))
                    .and_then(Json::as_array)
                    .into_iter()
                    .flatten()
                    .map(|value| value.python_float().unwrap_or(0.0))
                    .sum();
                let priced = pricing::table("model-pricing.json")
                    .get("source")
                    .and_then(|source| source.get("commit"))
                    .is_some_and(Json::is_truthy);
                if tokens != Some(1234.0) || heat != 1234.0 || !priced {
                    return 1;
                }
                if claude_bridge::publish(&self.state_dir, &Object::new(), now(), false).is_err() {
                    return 1;
                }
            }
            ["prepare"] | ["action"] | ["refresh"] => {
                if arguments == ["prepare"] && !Path::new(&format!("{}/preferences.json", self.state_dir)).exists() {
                    discovery::initialize(&self.state_dir, &self.detected_modules());
                }
                if arguments == ["action"] {
                    self.consume_panel_action();
                }
                if arguments == ["refresh"] {
                    self.background_refresh(false);
                }
                // Published before any long read; clicks are acknowledged on their own.
                self.write_snapshot();
            }
            _ => return 2,
        }
        0
    }

    /// One request from a desktop shell: its answer is the last line, {"result": …}; questions for the user come
    /// before it, one line each, and wait for their answers.
    fn desktop_command(&mut self, arguments: &[String]) -> i32 {
        if self.desktop().is_none() {
            self.host = Arc::new(StreamHost::new(&self.state_dir));
        }
        // Each snapshot notes its range, so while a new range is read the last one stays on screen.
        let snapshot = |runtime: &Runtime| -> Json {
            let value = runtime.render(|| runtime.panel_snapshot(&runtime.cached_data(), None, None, now()));
            let range =
                value.get("dashboard").and_then(|dashboard| dashboard.get("rangeKey")).cloned().unwrap_or(Json::Null);
            runtime.write_json_cache(
                &runtime.cache_path("panel-view"),
                &object! { "schema" => 1, "dashboard" => object! { "rangeKey" => range } }.into(),
            );
            value.into()
        };
        let rest = arguments.get(1..).unwrap_or(&[]);
        match arguments.first().map(String::as_str).unwrap_or("") {
            "snapshot" => reply(snapshot(self)),
            "prepare" => {
                if !Path::new(&format!("{}/preferences.json", self.state_dir)).exists() {
                    discovery::initialize(&self.state_dir, &self.detected_modules());
                }
                reply(snapshot(self));
            }
            "refresh" => {
                self.background_refresh(rest.first().map(String::as_str) == Some("wait"));
                reply(snapshot(self));
            }
            "range" => {
                self.refresh_usage_range(true);
                reply(snapshot(self));
            }
            "live" => {
                self.refresh_live();
                reply(snapshot(self));
            }
            "action" => {
                let Some(name) = rest.first() else {
                    reply(object! { "error" => "invalid action" }.into());
                    return 2;
                };
                match self.run_action(name, &rest[1..]) {
                    Ok(_) => reply(Json::Bool(true)),
                    Err(error) => {
                        reply(object! { "error" => error.0 }.into());
                        return 1;
                    }
                }
            }
            "detect" => reply(Json::Array(self.detected_modules().into_iter().map(Json::from).collect())),
            "select" => {
                if discovery::replace_selection(&self.state_dir, rest).is_err() {
                    reply(object! { "error" => "not saved" }.into());
                    return 1;
                }
                reply(Json::Bool(true));
            }
            "version" => reply(Json::from(VERSION)),
            _ => {
                reply(object! { "error" => "unknown request" }.into());
                return 2;
            }
        }
        0
    }
}
