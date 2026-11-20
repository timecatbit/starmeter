//! The starmeter command: the homepage's figures in a terminal. It runs the core as the desktop app does (the same
//! settings, connections and caches as the app on this system), reads the range asked for as the homepage would
//! (with the other computers' figures), and refreshes the plans' and APIs' figures that are due, as an opened panel
//! would. Nothing here counts: the dashboard, Summary and the rows all come from starmeter-core.

mod english;
mod figures;
mod report;
mod terminal;
mod text;

use std::collections::HashMap;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use starmeter_core::desktop::{self, Shell};
use starmeter_core::host::{self, DesktopHost, Host, HostFailure};
use starmeter_core::json::Json;
use starmeter_core::runtime::{RANGES, Runtime, now};
use starmeter_core::{VERSION, discovery};

use report::Words;
use text::{Paint, View};

const GROUPS: [&str; 3] = ["tool", "model", "project"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Overview,
    Usage,
    Quota,
    Help,
    Version,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    pub command: Command,
    pub range: String,
    pub group: String,
    pub detail: bool,
    pub json: bool,
    pub refresh: bool,
    pub colour: String,
    /// zh or en when asked for; the system's language otherwise.
    pub language: Option<String>,
}

/// What the program was asked, or what was wrong with it.
pub fn parse(arguments: &[String]) -> Result<Options, String> {
    let mut options = Options {
        command: Command::Overview,
        range: "today".into(),
        group: "tool".into(),
        detail: false,
        json: false,
        refresh: true,
        colour: "auto".into(),
        language: None,
    };
    let mut command: Option<Command> = None;
    let mut rest = arguments.iter().peekable();
    while let Some(argument) = rest.next() {
        let (name, inline) = match argument.split_once('=') {
            Some((name, value)) if name.starts_with("--") => (name, Some(value.to_string())),
            _ => (argument.as_str(), None),
        };
        let mut value = |what: &str| -> Result<String, String> {
            inline.clone().or_else(|| rest.next().cloned()).ok_or_else(|| format!("{what} needs a value"))
        };
        match name {
            "-h" | "--help" | "help" => command = Some(Command::Help),
            "-V" | "--version" | "version" => command = Some(Command::Version),
            "usage" if command.is_none() => command = Some(Command::Usage),
            "quota" | "quotas" if command.is_none() => command = Some(Command::Quota),
            "-r" | "--range" => options.range = value("--range")?,
            "-g" | "--group" => options.group = value("--group")?,
            "-d" | "--detail" => options.detail = true,
            "--json" => options.json = true,
            "--no-refresh" => options.refresh = false,
            "--color" | "--colour" => options.colour = value("--color")?,
            "--lang" => options.language = Some(value("--lang")?),
            _ if RANGES.iter().any(|(key, _)| *key == name) => options.range = name.to_string(),
            _ if GROUPS.contains(&name) => options.group = name.to_string(),
            _ => return Err(format!("unknown argument: {argument}")),
        }
    }
    if !RANGES.iter().any(|(key, _)| *key == options.range) {
        let known: Vec<&str> = RANGES.iter().map(|(key, _)| *key).collect();
        return Err(format!("--range is one of {}", known.join(", ")));
    }
    if !GROUPS.contains(&options.group.as_str()) {
        return Err(format!("--group is one of {}", GROUPS.join(", ")));
    }
    if !["auto", "always", "never"].contains(&options.colour.as_str()) {
        return Err("--color is auto, always or never".into());
    }
    if let Some(language) = &options.language
        && !["zh", "en"].contains(&language.as_str())
    {
        return Err("--lang is zh or en".into());
    }
    options.command = command.unwrap_or(Command::Overview);
    Ok(options)
}

fn help(words: Words) -> String {
    let ranges: Vec<&str> = RANGES.iter().map(|(key, _)| *key).collect();
    let ranges = ranges.join("|");
    if words.english {
        format!(
            "starmeter {VERSION}: your AI tools' token usage, costs and plan limits, as Starmeter's homepage shows them.

Usage:
  starmeter [range] [options]          today's overview: Summary, each tool, plans and balances
  starmeter usage [range] [options]    usage: Summary, the trend, each tool (or model, or project)
  starmeter quota [options]            plans and APIs, with each limit and when it resets

Options:
  -r, --range {ranges}
                       the homepage's ranges (default today)
  -g, --group tool|model|project       cards per tool (default), per model or per project
  -d, --detail         each card's top models (usage)
      --json           the figures for scripts
      --no-refresh     show the plans' and APIs' last figures instead of refreshing those that are due
      --color auto|always|never
      --lang zh|en     the language (default: the system's; STARMETER_LANG works too)
  -h, --help           this help
  -V, --version        the version

The settings, connections and caches are the Starmeter app's on this computer; the figures are the panel's.
"
        )
    } else {
        format!(
            "starmeter {VERSION}：在终端里看 AI 工具的 Token 用量、费用和额度，数字与 Starmeter 主页一致。

用法：
  starmeter [范围] [选项]          今天概览：Summary、各工具、订阅与额度
  starmeter usage [范围] [选项]    用量：Summary、趋势、各工具（或模型、项目）的明细
  starmeter quota [选项]           订阅与 API：每个额度窗口和重置时间

选项：
  -r, --range {ranges}
                       统计范围，与主页相同（默认 today）
  -g, --group tool|model|project   按工具（默认）、模型或项目分组
  -d, --detail         每张卡片列出用得最多的模型（usage）
      --json           输出 JSON，给脚本用
      --no-refresh     订阅额度、API 余额直接用上次的结果，不刷新到期的数据
      --color auto|always|never
      --lang zh|en     语言（默认跟随系统；也可设 STARMETER_LANG）
  -h, --help           显示帮助
  -V, --version        显示版本

设置、连接和缓存与这台电脑上的 Starmeter App 共用，数字与面板一致。
"
        )
    }
}

/// The folder the program is in, links followed: the price tables are in lib/ beside it or a few folders up (the
/// app's Resources, the command line's own folder, a source checkout).
fn resources() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.canonicalize().ok().or(Some(path)))
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
        .to_string_lossy()
        .into_owned()
}

/// The core's host for the command line: it asks nothing, opens nothing and leaves refreshes and notices to the app.
struct Terminal;

impl Host for Terminal {
    fn confirm(&self, _: &str, _: &str, _: &str) -> bool {
        false
    }
    fn text_dialog(&self, _: &str, _: &str, _: bool, _: &str) -> Option<String> {
        None
    }
    fn choose(&self, _: &str, _: &str, _: (&str, &str), _: (&str, &str)) -> Option<String> {
        None
    }
    fn choose_path(&self, _: &str, _: bool) -> Option<String> {
        None
    }
    fn save_path(&self, _: &str, _: &str) -> Option<String> {
        None
    }
    fn error(&self, _: &str, _: &str) {}
    fn notify(&self, _: &str, _: &str) {}
    fn open_local(&self, _: &str, _: bool) -> Result<(), HostFailure> {
        Err(HostFailure("the command line opens nothing".into()))
    }
    fn copy(&self, _: &str) {}
    fn desktop(&self) -> Option<&dyn DesktopHost> {
        Some(self)
    }
}

impl DesktopHost for Terminal {
    fn save_overrides(&self, _: &[(String, String)]) -> Result<(), HostFailure> {
        Err(HostFailure("the command line changes no settings".into()))
    }
    fn config_file(&self) -> String {
        String::new()
    }
    fn open_claude(&self) {}
    fn update_cli(&self, _: &str, _: &str) {}
    fn submit_test(&self, _: &str, _: &str) {}
    fn request_refresh(&self, _: Option<&str>) {}
    fn save_range(&self, _: &str) {}
    fn local_proxy(&self, base: &str) -> bool {
        host::local_address(base)
    }
}

/// The core as the desktop app on this system runs it, at an open panel's pace.
fn runtime(base: HashMap<String, String>) -> Runtime {
    let environment = desktop::environment(Shell::current(), base, &resources());
    let mut runtime = Runtime::new(environment, "starmeter");
    runtime.host = Arc::new(Terminal);
    runtime.host_cadence = Some(Box::new(|| (true, false)));
    runtime
}

/// Everything one run shows, read the way the homepage reads it.
fn read(runtime: &Runtime, options: &Options, words: Words) -> (report::Report, bool) {
    // First run on this computer: the modules found here, as the app's first launch chooses them.
    if !Path::new(&format!("{}/preferences.json", runtime.state_dir)).exists() {
        discovery::initialize(&runtime.state_dir, &runtime.detected_modules());
    }
    if options.refresh {
        runtime.refresh_quotas(true);
    }
    let loaded =
        if options.command == Command::Quota { (None, None) } else { runtime.range_with_devices(&options.range) };
    let failed = loaded.0.is_none() && loaded.1.is_some();
    let zone = runtime.local_zone();
    let (dashboard, entries) = runtime.render(|| {
        let dashboard = runtime.range_dashboard(&options.range, Some(&options.group), loaded, now());
        (dashboard, runtime.resource_panel_entries(&runtime.cached_data()))
    });
    let zone = zone.iana_name().unwrap_or("").to_string();
    (report::build(&dashboard, &entries, &zone, words), failed)
}

/// JSON indented two spaces a level, with lists of plain values kept on one line.
pub fn pretty(value: &Json, level: usize) -> String {
    let inner = "  ".repeat(level + 1);
    let outer = "  ".repeat(level);
    match value {
        Json::Object(fields) if !fields.is_empty() => {
            let lines: Vec<String> = fields
                .iter()
                .map(|(key, value)| {
                    format!("{inner}{}: {}", Json::from(key.as_str()).serialized(), pretty(value, level + 1))
                })
                .collect();
            format!("{{\n{}\n{outer}}}", lines.join(",\n"))
        }
        Json::Array(items) if items.iter().any(|item| matches!(item, Json::Object(_) | Json::Array(_))) => {
            let lines: Vec<String> = items.iter().map(|item| format!("{inner}{}", pretty(item, level + 1))).collect();
            format!("[\n{}\n{outer}]", lines.join(",\n"))
        }
        other => other.serialized(),
    }
}

/// Writes the output; a reader that stopped early (head, a pager closed) ends the run quietly with `status`, any
/// other failure is reported and fails it.
pub fn emit(out: &mut impl Write, text: &str, status: i32) -> i32 {
    match out.write_all(text.as_bytes()).and_then(|()| out.flush()) {
        Ok(()) => status,
        Err(error) if error.kind() == ErrorKind::BrokenPipe => status,
        Err(error) => {
            eprintln!("starmeter: {error}");
            1
        }
    }
}

/// One run of the command line, with the arguments after the program's name; the exit status.
pub fn main(arguments: &[String]) -> i32 {
    let options = match parse(arguments) {
        Ok(options) => options,
        Err(problem) => {
            eprintln!(
                "starmeter: {problem}\n{}",
                if terminal::system_chinese() { "用法见 starmeter --help" } else { "See starmeter --help" }
            );
            return 2;
        }
    };
    let english = match options.language.as_deref() {
        Some(language) => language == "en",
        None => !terminal::system_chinese(),
    };
    let words = Words { english };
    let mut out = std::io::stdout().lock();
    match options.command {
        Command::Help => return emit(&mut out, &help(words), 0),
        Command::Version => return emit(&mut out, &format!("starmeter {VERSION}\n"), 0),
        _ => {}
    }
    let runtime = runtime(std::env::vars().collect());
    let (report, failed) = read(&runtime, &options, words);
    let text = if options.json {
        let sections: &[&str] = match options.command {
            Command::Usage => &["usage"],
            Command::Quota => &["quotas"],
            _ => &["usage", "quotas"],
        };
        pretty(&report.json(sections), 0) + "\n"
    } else {
        let view = match options.command {
            Command::Usage => View::Usage { detail: options.detail },
            Command::Quota => View::Quota,
            _ => View::Overview,
        };
        text::render(&report, view, terminal::columns(), &Paint { colour: terminal::colour(&options.colour) })
    };
    emit(&mut out, &text, if failed { 1 } else { 0 })
}

#[cfg(test)]
mod tests;
