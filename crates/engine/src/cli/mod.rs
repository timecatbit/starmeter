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
use starmeter_core::{VERSION, discovery, preferences};

use report::Words;
use text::{Paint, View};

const GROUPS: [&str; 3] = ["tool", "model", "project"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Overview,
    Usage,
    Quota,
    Detect,
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
    /// detect: add what it finds without asking.
    pub yes: bool,
    pub colour: String,
    /// zh, zh-Hant or en when asked for (zh-TW and zh-HK are zh-Hant); the system's language otherwise.
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
        yes: false,
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
            "detect" if command.is_none() => command = Some(Command::Detect),
            "-y" | "--yes" => options.yes = true,
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
    if let Some(language) = options.language.as_mut() {
        *language = match language.to_ascii_lowercase().as_str() {
            "zh" | "zh-cn" | "zh-hans" => "zh".into(),
            "zh-hant" | "zh-tw" | "zh-hk" => "zh-Hant".into(),
            "en" => "en".into(),
            _ => return Err("--lang is zh, zh-Hant or en".into()),
        };
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
  starmeter detect [--yes]             find the tools on this computer and add new ones to the homepage

Options:
  -r, --range {ranges}
                       the homepage's ranges (default today)
  -g, --group tool|model|project       cards per tool (default), per model or per project
  -d, --detail         each card's top models (usage)
      --json           the figures for scripts
      --no-refresh     show the plans' and APIs' last figures instead of refreshing those that are due
      --color auto|always|never
      --lang zh|zh-Hant|en
                       the language (default: the system's; STARMETER_LANG works too)
  -h, --help           this help
  -y, --yes            detect: add without asking (your other choices stay)
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
  starmeter detect [--yes]         检测本机的工具，把新找到的加到主页

选项：
  -r, --range {ranges}
                       统计范围，与主页相同（默认 today）
  -g, --group tool|model|project   按工具（默认）、模型或项目分组
  -d, --detail         每张卡片列出用得最多的模型（usage）
      --json           输出 JSON，给脚本用
      --no-refresh     订阅额度、API 余额直接用上次的结果，不刷新到期的数据
      --color auto|always|never
      --lang zh|zh-Hant|en
                       语言：简体中文、繁體中文或英文（默认跟随系统；也可设 STARMETER_LANG）
  -h, --help           显示帮助
  -y, --yes            detect：不再询问直接加入（已有的勾选不变）
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
    fn update_cli(&self, _: &str, _: &str, _: &[String]) {}
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

/// First run on this computer: the modules found here, as the app's first launch chooses them.
fn first_run(runtime: &Runtime) {
    if !Path::new(&format!("{}/preferences.json", runtime.state_dir)).exists() {
        discovery::initialize(&runtime.state_dir, &runtime.detected_modules());
    }
}

/// Asks on the terminal; anything but y or yes is no.
fn confirm(question: &str) -> bool {
    use std::io::{BufRead, IsTerminal};
    if !std::io::stdin().is_terminal() {
        return false;
    }
    print!("{question} [y/N] ");
    let _ = std::io::stdout().flush();
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer).is_ok()
        && ["y", "yes"].contains(&answer.trim().to_lowercase().as_str())
}

/// Puts each module on the homepage as the panel's switch does, into the selection as it is when the user says yes,
/// not as it was when the question was asked: a module turned on or off in the panel meanwhile stays so.
fn put_on_homepage(state: &Path, keys: &[String]) -> Result<(), preferences::Error> {
    for key in keys {
        preferences::update(state, "module", Some(key), Some("true"))?;
    }
    Ok(())
}

/// starmeter detect: the modules found on this computer, those not on the homepage yet added when the user says so
/// (or with --yes). Unlike the panel's 自动检测, which replaces the selection, nothing chosen is taken away.
fn detect(runtime: &Runtime, options: &Options, words: Words, out: &mut impl Write) -> i32 {
    first_run(runtime);
    let state = Path::new(&runtime.state_dir);
    let shown = |settings: &starmeter_core::json::Object| -> Vec<String> {
        settings
            .get("visible")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .filter_map(|key| key.as_str().map(String::from))
            .collect()
    };
    let homepage = shown(&preferences::read(state));
    let found = runtime.detected_modules();
    let new: Vec<String> = found.iter().filter(|key| !homepage.contains(key)).cloned().collect();
    let label = |key: &str| {
        preferences::MODULES.iter().find(|module| module.key == key).map_or(key.to_string(), |m| words.text(&m.label))
    };
    let list = |keys: &[String]| {
        keys.iter().map(|key| label(key)).collect::<Vec<_>>().join(if words.english { ", " } else { "、" })
    };
    let add = !new.is_empty()
        && (options.yes
            || (!options.json
                && confirm(&if words.english {
                    format!("Add {} to the homepage?", list(&new))
                } else {
                    format!("把 {} 加到主页？", list(&new))
                })));
    let added = if add {
        if let Err(error) = put_on_homepage(state, &new) {
            eprintln!("starmeter: {error}");
            return 1;
        }
        new.clone()
    } else {
        Vec::new()
    };
    // As the homepage is now: the user may have changed it in the panel while this asked.
    let homepage = shown(&preferences::read(state));
    if options.json {
        let keys = |keys: &[String]| Json::Array(keys.iter().map(|key| Json::from(key.as_str())).collect());
        let value = starmeter_core::object! {
            "found" => keys(&found), "homepage" => keys(&homepage), "new" => keys(&new),
            "added" => keys(&added),
        };
        return emit(out, &(pretty(&Json::Object(value), 0) + "\n"), 0);
    }
    let mut lines = vec![if words.english {
        "Starmeter · Tools on this computer".to_string()
    } else {
        "Starmeter · 检测本机工具".into()
    }];
    lines.push(String::new());
    let listed: Vec<&preferences::Module> =
        preferences::MODULES.iter().filter(|module| found.iter().any(|key| key == module.key)).collect();
    let room = listed.iter().map(|module| terminal::width(&words.text(&module.label))).max().unwrap_or(0);
    for module in listed {
        let (mark, note) = if added.iter().any(|key| key == module.key) {
            ("+", words.own("已加到主页", "added to the homepage"))
        } else if homepage.iter().any(|key| key == module.key) {
            ("✓", words.own("已在主页", "on the homepage"))
        } else {
            ("+", words.own("新找到，未在主页", "found, not on the homepage"))
        };
        lines.push(format!("  {mark} {}  {note}", terminal::pad(&words.text(&module.label), room, false)));
    }
    lines.push(String::new());
    lines.push(
        if new.is_empty() {
            words.own(
                "这台电脑上找到的工具都已在主页。",
                "Everything found on this computer is on the homepage already.",
            )
        } else if added.is_empty() {
            words.own(
                "加 --yes 把新找到的加到主页；已有的勾选不会改动。",
                "Add --yes to put the new ones on the homepage; nothing you chose is changed.",
            )
        } else {
            words.own("已有的勾选没有改动。", "Nothing you chose before was changed.")
        }
        .to_string(),
    );
    let text =
        lines.iter().map(|line| text::wrap_line(line, terminal::columns())).collect::<Vec<_>>().join("\n") + "\n";
    emit(out, &words.page(text), 0)
}

/// Everything one run shows, read the way the homepage reads it.
fn read(runtime: &Runtime, options: &Options, words: Words) -> (report::Report, bool) {
    first_run(runtime);
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
    let language = options.language.as_deref().unwrap_or_else(|| terminal::system_language());
    let words = Words { english: language == "en", traditional: language == "zh-Hant" };
    let mut out = std::io::stdout().lock();
    match options.command {
        Command::Help => return emit(&mut out, &words.page(help(words)), 0),
        Command::Detect => return detect(&runtime(std::env::vars().collect()), &options, words, &mut out),
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
        words.page(text::render(
            &report,
            view,
            terminal::columns(),
            &Paint { colour: terminal::colour(&options.colour) },
        ))
    };
    emit(&mut out, &text, if failed { 1 } else { 0 })
}

#[cfg(test)]
mod tests;
