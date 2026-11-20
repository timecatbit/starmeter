//! What an action needs from the place it runs: questions to the user, notices, files and the clipboard. The SwiftBar
//! plugin's runs use AppleScript dialogs; a host with its own windows (a desktop shell) answers through its own UI.
//! Also the connection overrides in config.local.sh, the usage export and the terminal tabs for CLI updates.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, Write};
use std::path::Path;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use jiff::tz::TimeZone;

use crate::dashboard::{self, Selected};
use crate::files::{self, paths};
use crate::json::{Format, Json, Object};
use crate::preferences::Error;
use crate::runtime::{self, PRODUCT};
use crate::time::utc_string;
use crate::{catalog, object, platform, py, shell};

#[derive(Clone, Debug)]
pub struct HostFailure(pub String);

impl std::fmt::Display for HostFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

pub trait Host: Send + Sync {
    fn confirm(&self, message: &str, detail: &str, accept: &str) -> bool;
    /// The text typed, or None when the user cancelled.
    fn text_dialog(&self, message: &str, value: &str, hidden: bool, title: &str) -> Option<String>;
    /// The value of the option chosen, or None.
    fn choose(&self, title: &str, prompt: &str, first: (&str, &str), second: (&str, &str)) -> Option<String>;
    fn choose_path(&self, message: &str, folder: bool) -> Option<String>;
    fn save_path(&self, message: &str, name: &str) -> Option<String>;
    fn error(&self, title: &str, detail: &str);
    fn notify(&self, message: &str, subtitle: &str);
    fn open_local(&self, path: &str, text: bool) -> Result<(), HostFailure>;
    fn copy(&self, text: &str);
    /// A host with its own windows and refresh queue.
    fn desktop(&self) -> Option<&dyn DesktopHost> {
        None
    }
}

/// An in-process host with its own windows and refresh queue: it takes over refreshes and connection settings.
pub trait DesktopHost: Host {
    fn save_overrides(&self, values: &[(String, String)]) -> Result<(), HostFailure>;
    fn config_file(&self) -> String;
    fn open_claude(&self);
    fn update_cli(&self, binary: &str, key: &str);
    fn submit_test(&self, provider: &str, id: &str);
    fn request_refresh(&self, mode: Option<&str>);
    fn save_range(&self, key: &str);
    fn local_proxy(&self, base: &str) -> bool;
}

/// Whether an address points at this machine.
pub fn local_address(base: &str) -> bool {
    reqwest::Url::parse(base)
        .ok()
        .and_then(|url| url.host_str().map(|host| host.trim_start_matches('[').trim_end_matches(']').to_string()))
        .is_some_and(|host| ["127.0.0.1", "localhost", "::1"].contains(&host.as_str()))
}

// SwiftBar's panel answers in place

/// The plugin's questions while SwiftBar's panel is open and says it can answer them (its `panel-state.json`, written
/// by a running panel): the question goes to the panel in `panel-ask.json` and the answer comes back in
/// `panel-answer.json`, so the user answers under the row that asked. Otherwise, and for files and notices the panel
/// can't show, AppleScript's dialogs.
pub struct PanelAskHost {
    folder: String,
    dialogs: AppleScriptHost,
}

impl PanelAskHost {
    pub fn new(folder: String) -> Self {
        PanelAskHost { folder, dialogs: AppleScriptHost }
    }

    fn file(&self, name: &str) -> std::path::PathBuf {
        Path::new(&self.folder).join(name)
    }

    fn panel_asks(&self) -> bool {
        let state = std::fs::read(self.file("panel-state.json"))
            .ok()
            .and_then(|data| Json::parse(&data).ok())
            .and_then(|value| value.as_object().cloned())
            .unwrap_or_default();
        let pid = state.get("pid").and_then(Json::number).map(|pid| pid as u32);
        state.get("visible") == Some(&Json::Bool(true))
            && state.get("asks") == Some(&Json::Bool(true))
            && pid.is_some_and(platform::alive)
    }

    /// The answer (null when cancelled or the panel closed), or None when the panel can't ask.
    fn ask(&self, mut question: Object) -> Option<Json> {
        if !self.panel_asks() {
            return None;
        }
        let id = format!("{}-{}", platform::process_id(), files::time_nanoseconds());
        question.insert("id", id.as_str());
        let (ask, answer) = (self.file("panel-ask.json"), self.file("panel-answer.json"));
        platform::remove(&answer);
        let data = Json::Object(question).serialized();
        files::replace_atomically(&ask, data.as_bytes(), ".panel-ask", || Ok(())).ok()?;
        files::owner_only(&ask);
        let started = Instant::now();
        let reply = loop {
            std::thread::sleep(Duration::from_millis(150));
            let reply = std::fs::read(&answer).ok().and_then(|data| Json::parse(&data).ok());
            if let Some(reply) = reply.filter(|reply| reply.get("id") == Some(&Json::from(id.as_str()))) {
                break reply.get("answer").cloned().unwrap_or(Json::Null);
            }
            if !self.panel_asks() || started.elapsed() > Duration::from_secs(900) {
                break Json::Null;
            }
        };
        platform::remove(&ask);
        platform::remove(&answer);
        Some(reply)
    }
}

impl Host for PanelAskHost {
    fn confirm(&self, message: &str, detail: &str, accept: &str) -> bool {
        let question = object! { "kind" => "confirm", "title" => message, "detail" => detail, "accept" => accept };
        match self.ask(question) {
            Some(answer) => answer == Json::Bool(true),
            None => self.dialogs.confirm(message, detail, accept),
        }
    }

    fn text_dialog(&self, message: &str, value: &str, hidden: bool, title: &str) -> Option<String> {
        let question =
            object! { "kind" => "text", "title" => title, "detail" => message, "value" => value, "hidden" => hidden };
        match self.ask(question) {
            Some(answer) => answer.as_str().map(String::from),
            None => self.dialogs.text_dialog(message, value, hidden, title),
        }
    }

    fn choose(&self, title: &str, prompt: &str, first: (&str, &str), second: (&str, &str)) -> Option<String> {
        let options = Json::Array(vec![
            Json::Array(vec![Json::from(first.0), Json::from(first.1)]),
            Json::Array(vec![Json::from(second.0), Json::from(second.1)]),
        ]);
        let question = object! { "kind" => "choice", "title" => title, "detail" => prompt, "options" => options };
        match self.ask(question) {
            Some(answer) => answer.as_str().filter(|value| [first.1, second.1].contains(value)).map(String::from),
            None => self.dialogs.choose(title, prompt, first, second),
        }
    }

    fn choose_path(&self, message: &str, folder: bool) -> Option<String> {
        self.dialogs.choose_path(message, folder)
    }

    fn save_path(&self, message: &str, name: &str) -> Option<String> {
        self.dialogs.save_path(message, name)
    }

    fn error(&self, title: &str, detail: &str) {
        // A notice is shown and nothing waits on it: the panel takes it as a question it answers at once.
        if self.ask(object! { "kind" => "notice", "title" => title, "detail" => detail }).is_none() {
            self.dialogs.error(title, detail);
        }
    }

    fn notify(&self, message: &str, subtitle: &str) {
        self.dialogs.notify(message, subtitle);
    }

    fn open_local(&self, path: &str, text: bool) -> Result<(), HostFailure> {
        self.dialogs.open_local(path, text)
    }

    fn copy(&self, text: &str) {
        self.dialogs.copy(text);
    }
}

// Dialogs through osascript

pub struct AppleScriptHost;

fn literal(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// A newline inside an AppleScript string is written as `return` joined in.
fn expression(text: &str) -> String {
    text.split('\n').map(literal).collect::<Vec<_>>().join(" & return & ")
}

impl AppleScriptHost {
    /// The script's output, or None when it failed or the user cancelled. Dialogs wait as long as the user takes.
    fn run(&self, source: &str) -> Option<String> {
        shell::run(&["/usr/bin/osascript", "-e", source], 24.0 * 3600.0, None, None).map(|out| out.trim().to_string())
    }
}

impl Host for AppleScriptHost {
    fn confirm(&self, message: &str, detail: &str, accept: &str) -> bool {
        let body = if detail.is_empty() { message.to_string() } else { format!("{message}\n\n{detail}") };
        let script = format!(
            "display dialog {} with title {} buttons {{\"取消\", {}}} default button \"取消\" with icon caution",
            expression(&body),
            literal(PRODUCT),
            literal(accept)
        );
        self.run(&script).is_some_and(|answer| answer.contains(accept))
    }

    fn text_dialog(&self, message: &str, value: &str, hidden: bool, title: &str) -> Option<String> {
        self.run(&format!(
            "text returned of (display dialog {} default answer {}{} buttons {{\"取消\", \"保存\"}} default button \"保存\" cancel button \"取消\" with title {})",
            expression(message),
            literal(value),
            if hidden { " with hidden answer" } else { "" },
            literal(title)
        ))
    }

    fn choose(&self, title: &str, prompt: &str, first: (&str, &str), second: (&str, &str)) -> Option<String> {
        let answer = self.run(&format!(
            "choose from list {{{}, {}}} with title {} with prompt {} default items {{{}}}",
            literal(first.0),
            literal(second.0),
            literal(title),
            literal(prompt),
            literal(first.0)
        ))?;
        if answer == first.0 {
            Some(first.1.to_string())
        } else if answer == second.0 {
            Some(second.1.to_string())
        } else {
            None
        }
    }

    fn choose_path(&self, message: &str, folder: bool) -> Option<String> {
        self.run(&format!(
            "POSIX path of (choose {} with prompt {})",
            if folder { "folder" } else { "file" },
            literal(message)
        ))
    }

    fn save_path(&self, message: &str, name: &str) -> Option<String> {
        self.run(&format!(
            "POSIX path of (choose file name with prompt {} default name {} default location (path to desktop folder))",
            literal(message),
            literal(name)
        ))
    }

    fn error(&self, title: &str, detail: &str) {
        let _ = self.run(&format!("display alert {} message {}", literal(title), literal(detail)));
    }

    fn notify(&self, message: &str, subtitle: &str) {
        let mut script = format!("display notification {} with title {}", literal(message), literal(PRODUCT));
        if !subtitle.is_empty() {
            script += &format!(" subtitle {}", literal(subtitle));
        }
        let _ = shell::run(&["/usr/bin/osascript", "-e", &script], 5.0, None, None);
    }

    fn open_local(&self, path: &str, text: bool) -> Result<(), HostFailure> {
        let mut arguments = vec!["/usr/bin/open"];
        if text {
            arguments.push("-t");
        }
        arguments.push(path);
        shell::run(&arguments, 5.0, None, None).map(|_| ()).ok_or_else(|| HostFailure("open failed".into()))
    }

    fn copy(&self, text: &str) {
        let _ = shell::run(&["/usr/bin/pbcopy"], 5.0, None, Some(text.as_bytes()));
    }
}

// The stream host: a desktop shell running the engine as a helper

/// A shell with its own windows running the engine as a helper: each question goes out as one JSON line on
/// standard output and the host writes back one line, {"value": …}, on standard input.
pub struct StreamHost {
    state: String,
    lock: Mutex<()>,
}

impl StreamHost {
    pub fn new(state: &str) -> Self {
        StreamHost { state: state.to_string(), lock: Mutex::new(()) }
    }

    pub fn emit(message: &Object) {
        let text = Json::Object(message.clone()).serialized_with(Format { ensure_ascii: true, ..Format::default() });
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{text}");
        let _ = out.flush();
    }

    /// Sends a request and waits for its answer; questions from parallel reads are asked one at a time.
    fn ask(&self, request: Object) -> Json {
        let _guard = self.lock.lock().expect("stream host");
        Self::emit(&request);
        let mut line = String::new();
        if std::io::stdin().lock().read_line(&mut line).unwrap_or(0) == 0 {
            return Json::Null;
        }
        Json::parse_str(line.trim_end_matches(['\n', '\r']))
            .ok()
            .and_then(|answer| answer.as_object().and_then(|fields| fields.get("value").cloned()))
            .unwrap_or(Json::Null)
    }
}

impl Host for StreamHost {
    fn confirm(&self, message: &str, detail: &str, accept: &str) -> bool {
        self.ask(object! { "host" => "confirm", "message" => message, "detail" => detail, "accept" => accept })
            == Json::Bool(true)
    }
    fn text_dialog(&self, message: &str, value: &str, hidden: bool, title: &str) -> Option<String> {
        self.ask(
            object! { "host" => "text", "message" => message, "value" => value, "hidden" => hidden, "title" => title },
        )
        .as_str()
        .map(String::from)
    }
    fn choose(&self, title: &str, prompt: &str, first: (&str, &str), second: (&str, &str)) -> Option<String> {
        let pair = |option: (&str, &str)| Json::Array(vec![Json::from(option.0), Json::from(option.1)]);
        self.ask(object! {
            "host" => "choose", "title" => title, "prompt" => prompt, "options" => Json::Array(vec![pair(first), pair(second)]),
        })
        .as_str()
        .map(String::from)
    }
    fn choose_path(&self, message: &str, folder: bool) -> Option<String> {
        self.ask(object! { "host" => "choosePath", "message" => message, "folder" => folder })
            .as_str()
            .map(String::from)
    }
    fn save_path(&self, message: &str, name: &str) -> Option<String> {
        self.ask(object! { "host" => "savePath", "message" => message, "name" => name }).as_str().map(String::from)
    }
    fn error(&self, title: &str, detail: &str) {
        self.ask(object! { "host" => "error", "title" => title, "detail" => detail });
    }
    fn notify(&self, message: &str, subtitle: &str) {
        self.ask(object! { "host" => "notify", "message" => message, "subtitle" => subtitle });
    }
    fn open_local(&self, path: &str, text: bool) -> Result<(), HostFailure> {
        if self.ask(object! { "host" => "openLocal", "path" => path, "text" => text }) == Json::Bool(true) {
            Ok(())
        } else {
            Err(HostFailure("open failed".into()))
        }
    }
    fn copy(&self, text: &str) {
        self.ask(object! { "host" => "copy", "text" => text });
    }
    fn desktop(&self) -> Option<&dyn DesktopHost> {
        Some(self)
    }
}

impl DesktopHost for StreamHost {
    fn save_overrides(&self, values: &[(String, String)]) -> Result<(), HostFailure> {
        let fields: Object = values.iter().map(|(key, value)| (key.as_str(), Json::from(value.as_str()))).collect();
        if self.ask(object! { "host" => "saveOverrides", "values" => fields }) == Json::Bool(true) {
            Ok(())
        } else {
            Err(HostFailure("settings could not be saved".into()))
        }
    }
    /// The shell's own settings file, JSON rather than a shell script.
    fn config_file(&self) -> String {
        let path = format!("{}/config.local.json", self.state);
        if !Path::new(&path).exists() {
            let _ = files::replace_atomically(Path::new(&path), b"{}", ".config-", || Ok(()));
        }
        path
    }
    fn open_claude(&self) {
        self.ask(object! { "host" => "openClaude" });
    }
    fn update_cli(&self, binary: &str, key: &str) {
        self.ask(object! { "host" => "updateCLI", "binary" => binary, "key" => key });
    }
    fn submit_test(&self, provider: &str, id: &str) {
        self.ask(object! { "host" => "submitTest", "provider" => provider, "id" => id });
    }
    fn request_refresh(&self, mode: Option<&str>) {
        self.ask(object! { "host" => "refresh", "mode" => mode.map(Json::from).unwrap_or(Json::Null) });
    }
    fn save_range(&self, key: &str) {
        let _ = files::make_directory(Path::new(&self.state), 0o700);
        let text = Json::Object(object! { "range" => key }).serialized();
        let _ = files::replace_atomically(
            Path::new(&format!("{}/view-range.json", self.state)),
            text.as_bytes(),
            ".range-",
            || Ok(()),
        );
    }
    fn local_proxy(&self, base: &str) -> bool {
        local_address(base)
    }
}

/// Explicit connection overrides in config.local.sh: no copied credentials or inferred monitor addresses.
pub mod connections {
    use super::*;

    pub static KEYS: LazyLock<HashSet<String>> = LazyLock::new(|| {
        let mut keys: HashSet<String> = [
            "OCX_HOME",
            "OCX_BASE_URL",
            "OCX_TOKEN_FILE",
            "OCX_CHATGPT_HOME",
            "KOMARI_BASE_URL",
            "DEEPSEEK_API_KEY",
            "DEEPSEEK_API_KEY_FILE",
            "XAI_MANAGEMENT_API_KEY",
            "XAI_MANAGEMENT_API_KEY_FILE",
            "XAI_TEAM_ID",
            "MINIMAX_API_KEY",
            "MINIMAX_API_KEY_FILE",
            "MINIMAX_REGION",
            "MOONSHOT_REGION",
            "QODER_CN_ORG_ID",
        ]
        .iter()
        .map(|key| key.to_string())
        .collect();
        for service in catalog::SERVICES.iter().filter(|service| !service.credential.is_empty()) {
            for name in catalog::credential_names(service.key) {
                keys.insert(name.to_string());
                keys.insert(format!("{name}_FILE"));
            }
        }
        keys
    });

    pub const HEADER: &str = "# 本机高级设置：仅本用户可读，不随插件上传或覆盖。\n# 常用设置请在「主页模块」悬停对应来源；本机客户端默认自动读取。\n# 注释是说明，不代表已配置。无需填写即可使用已发现的本机客户端。\n# API 连接只保存密钥文件路径；在插件中输入的 Key 存于同目录 keys/，仅本用户可读。\n";

    /// shlex.quote: plain words stay, anything else goes in single quotes.
    pub fn shell_quote(value: &str) -> String {
        if value.is_empty() {
            return "''".into();
        }
        let safe = value.chars().all(|c| c.is_ascii_alphanumeric() || "@%+=:,./-_".contains(c));
        if safe { value.to_string() } else { format!("'{}'", value.replace('\'', "'\"'\"'")) }
    }

    /// Overrides only the keys given, keeping the user's other settings. The last export wins, empty ones included.
    pub fn write_overrides(directory: &str, values: &[(String, String)]) -> Result<(), Error> {
        if values.is_empty()
            || !values.iter().all(|(key, value)| KEYS.contains(key) && !value.contains('\n') && !value.contains('\0'))
        {
            return Err(Error::Invalid("invalid connection settings"));
        }
        files::make_directory(Path::new(directory), 0o700).map_err(Error::Write)?;
        let path = format!("{directory}/config.local.sh");
        let mut text = if Path::new(&path).exists() {
            std::fs::read_to_string(&path).map_err(|error| Error::Write(files::Failure(error.to_string())))?
        } else {
            format!("{HEADER}\n")
        };
        let marker = " # managed by SwiftBarToken";
        for (key, value) in values {
            let prefix = format!("export {key}=");
            text = text
                .split('\n')
                .filter(|line| !(line.starts_with(&prefix) && line.ends_with(marker)))
                .collect::<Vec<_>>()
                .join("\n");
            text = text.trim_end().to_string();
            text += &format!("\nexport {key}={}{marker}\n", shell_quote(value));
        }
        files::replace_atomically(Path::new(&path), text.as_bytes(), ".config-", || Ok(())).map_err(Error::Write)
    }

    /// Only keys typed into the plugin are ours to delete; files the user chose are never removed.
    pub fn managed_key(directory: &str, file: &str) -> bool {
        let path = paths::expand_user(file);
        let resolved = paths::resolve(&path)
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|| runtime::absolute(&path.to_string_lossy()));
        let parent = Path::new(&resolved).parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        let keys = format!("{directory}/keys");
        let root = paths::resolve(Path::new(&keys))
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|| runtime::absolute(&keys));
        parent == root
    }

    pub fn ensure_file(directory: &str) -> Result<String, HostFailure> {
        files::make_directory(Path::new(directory), 0o700).map_err(|error| HostFailure(error.to_string()))?;
        let path = format!("{directory}/config.local.sh");
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        match options.open(&path) {
            Ok(mut file) => file.write_all(format!("{HEADER}\n").as_bytes()).map_err(|e| HostFailure(e.to_string()))?,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(HostFailure(error.to_string())),
        }
        Ok(path)
    }
}

/// Usage export: the range on screen as CSV (a row per day and tool) or JSON (totals, models and days per tool).
/// Only counters leave: no paths, keys, prompts or session IDs.
pub mod usage_export {
    use super::*;

    const COLUMNS: [&str; 13] = [
        "date",
        "tool",
        "requests",
        "input_tokens",
        "output_tokens",
        "cache_read_tokens",
        "cache_write_tokens",
        "total_tokens",
        "cost_usd",
        "unpriced_requests",
        "credits",
        "summary_tokens",
        "summary_cost_usd",
    ];

    fn number(value: Option<&Json>) -> f64 {
        let Some(value) = value else { return 0.0 };
        let number = value
            .python_float()
            .or_else(|| value.as_str().and_then(|text| crate::json::parse_python_float(text.trim())));
        number.filter(|number| *number >= 0.0).unwrap_or(0.0)
    }

    fn round6(value: f64) -> f64 {
        format!("{value:.6}").parse().unwrap_or(value)
    }

    fn counters(row: &Object) -> Object {
        let writes = row.get("cacheCreationInputTokens");
        let reads =
            py::or(row.get("cacheReadInputTokens"), row.get("cachedInputTokens").cloned().unwrap_or(Json::Null));
        let int = |value: Option<&Json>| Json::Int(number(value) as i64);
        object! {
            "requests" => int(row.get("requests")), "input_tokens" => int(row.get("inputTokens")),
            "output_tokens" => int(row.get("outputTokens")), "cache_read_tokens" => int(Some(&reads)),
            "cache_write_tokens" => if writes.is_none_or(Json::is_null) { Json::Null } else { int(writes) },
            "total_tokens" => int(row.get("totalTokens")), "cost_usd" => round6(number(row.get("estimatedCostUsd"))),
            "unpriced_requests" => int(row.get("unpricedRequests")),
            "credits" => format!("{:.4}", number(row.get("credits"))).parse::<f64>().unwrap_or(0.0),
        }
    }

    fn tools<'a>(
        selected: &'a [Selected],
        counting: &HashMap<String, Object>,
    ) -> Vec<(&'a str, &'a str, &'a Object, Object)> {
        selected
            .iter()
            .filter(|(_, _, usage)| {
                usage.get("summary").is_some_and(Json::is_truthy) && !usage.get("error").is_some_and(Json::is_truthy)
            })
            .map(|(key, name, usage)| {
                let counted = counting
                    .get(key)
                    .and_then(|part| part.get("usage"))
                    .and_then(Json::as_object)
                    .cloned()
                    .unwrap_or_default();
                (key.as_str(), name.as_str(), usage, counted)
            })
            .collect()
    }

    fn csv_field(value: &Json) -> String {
        let text = match value {
            Json::Null => String::new(),
            Json::Double(_) => py::repr(value),
            _ => py::str(Some(value)),
        };
        if text.contains([',', '"', '\n', '\r']) { format!("\"{}\"", text.replace('"', "\"\"")) } else { text }
    }

    fn date(row: &Object) -> String {
        py::str(Some(&row.get("date").cloned().unwrap_or_else(|| Json::from("")))).chars().take(10).collect()
    }

    pub fn csv_text(selected: &[Selected], counting: &HashMap<String, Object>) -> String {
        let mut lines = vec![COLUMNS.join(",")];
        for (_, name, usage, counted) in tools(selected, counting) {
            let mut counted_days: HashMap<String, Object> = HashMap::new();
            for day in counted.get("days").and_then(Json::as_array).into_iter().flatten() {
                if let Some(fields) = day.as_object() {
                    counted_days.insert(date(fields), fields.clone());
                }
            }
            // sorted(key=str(date)): by code points, equal dates keep their order.
            let mut ordered: Vec<&Object> =
                usage.get("days").and_then(Json::as_array).into_iter().flatten().filter_map(Json::as_object).collect();
            ordered.sort_by_key(|row| py::str(Some(&row.get("date").cloned().unwrap_or_else(|| Json::from("")))));
            for day in ordered {
                let when = date(day);
                let part = counted_days.get(&when).cloned().unwrap_or_default();
                let mut row = counters(day);
                row.insert("date", when.as_str());
                row.insert("tool", name);
                row.insert("summary_tokens", Json::Int(number(part.get("totalTokens")) as i64));
                row.insert("summary_cost_usd", round6(number(part.get("estimatedCostUsd"))));
                let fields: Vec<String> =
                    COLUMNS.iter().map(|column| csv_field(row.get(column).unwrap_or(&Json::Null))).collect();
                lines.push(fields.join(","));
            }
        }
        lines.join("\n") + "\n"
    }

    #[allow(clippy::too_many_arguments)]
    pub fn json_text(
        selected: &[Selected],
        counting: &HashMap<String, Object>,
        key: &str,
        label: &str,
        start: Option<&Json>,
        end: Option<&Json>,
        version: &str,
        now: f64,
    ) -> String {
        let moment = |stamp: Option<&Json>| {
            stamp
                .filter(|stamp| stamp.is_truthy())
                .and_then(Json::number)
                .map_or(Json::Null, |seconds| utc_string(seconds).into())
        };
        let mut list = Vec::new();
        for (tool, name, usage, counted) in tools(selected, counting) {
            let rows = |field: &str, label: &str, value: &dyn Fn(&Object) -> Json| -> Json {
                Json::Array(
                    usage
                        .get(field)
                        .and_then(Json::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(Json::as_object)
                        .map(|row| {
                            let mut fields = counters(row);
                            fields.insert(label, value(row));
                            fields.into()
                        })
                        .collect(),
                )
            };
            let models = rows("models", "model", &|row: &Object| {
                py::str(Some(&py::or(row.get("model"), Json::from("")))).into()
            });
            let days = rows("days", "date", &|row: &Object| date(row).into());
            let empty = Object::new();
            list.push(Json::from(object! {
                "id" => tool, "name" => name,
                "totals" => counters(usage.get("summary").and_then(Json::as_object).unwrap_or(&empty)),
                "summary" => counters(counted.get("summary").and_then(Json::as_object).unwrap_or(&empty)),
                "models" => models, "days" => days,
            }));
        }
        let body = object! {
            "product" => "Starmeter", "version" => version, "range" => key, "rangeLabel" => label, "start" => moment(start),
            "end" => moment(end), "exportedAt" => utc_string(now), "tools" => Json::Array(list),
        };
        Json::Object(body).serialized_indented(false, 2) + "\n"
    }

    /// Writes the export, CSV with a byte-order mark so Excel reads it as UTF-8.
    pub fn write(path: &str, kind: &str, text: &str) -> std::io::Result<()> {
        let mut bytes: Vec<u8> = if kind == "csv" { vec![0xEF, 0xBB, 0xBF] } else { Vec::new() };
        bytes.extend_from_slice(text.as_bytes());
        std::fs::write(path, bytes)
    }
}

/// Terminal tabs for CLI updates, with their output kept in a local log.
pub mod terminal_jobs {
    use super::*;

    pub fn cli_name(key: &str) -> Option<&'static str> {
        match key {
            "claude-code" => Some("Claude Code"),
            "codex-cli" => Some("Codex"),
            _ => None,
        }
    }

    fn literal(value: &str) -> String {
        format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\" & linefeed & \""))
    }

    /// The new window's shell prints its own startup lines and echoes the command, so the screen is cleared first;
    /// afterwards the tab waits for Return instead of starting another login shell.
    fn bootstrap(command: &str) -> String {
        format!(
            "export PATH=\"/opt/homebrew/bin:/usr/local/bin:$PATH\"\ncd \"$HOME\" || exit 1\nif [ -t 1 ]; then printf '\\033[H\\033[2J\\033[3J'; fi\n{command}\ntoken_terminal_status=$?\nif [ -t 0 ]; then printf '\\n按回车键关闭…'; read -r _; fi\nexit \"$token_terminal_status\"\n"
        )
    }

    pub fn join(words: &[&str]) -> String {
        words.iter().map(|word| connections::shell_quote(word)).collect::<Vec<_>>().join(" ")
    }

    fn modified(path: &str) -> f64 {
        std::fs::metadata(path)
            .ok()
            .and_then(|meta| meta.modified().ok())
            .map_or(0.0, |time| time.duration_since(std::time::UNIX_EPOCH).map_or(0.0, |elapsed| elapsed.as_secs_f64()))
    }

    fn is_link(path: &str) -> bool {
        std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink())
    }

    fn listing(root: &str, keep: impl Fn(&str) -> bool) -> Vec<String> {
        let mut found: Vec<String> = std::fs::read_dir(root)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .filter(|name| keep(name))
                    .map(|name| format!("{root}/{name}"))
                    .collect()
            })
            .unwrap_or_default();
        found.sort_by(|a, b| modified(b).total_cmp(&modified(a)));
        found
    }

    fn create_exclusively(path: &str, contents: &[u8]) -> bool {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        options.open(path).and_then(|mut file| file.write_all(contents)).is_ok()
    }

    /// Opens the command in a new terminal tab; where Apple Events are refused, as a unique .command file.
    pub fn launch(command: &str, preferred: &str, state: &str) -> Result<&'static str, HostFailure> {
        let app = if ["iTerm", "iTerm2"].contains(&preferred) { "iTerm" } else { "Terminal" };
        let inline = join(&["/bin/bash", "-c", &bootstrap(command)]);
        let script = if app == "iTerm" {
            format!(
                "tell application \"iTerm\"\nactivate\nif (count of windows) = 0 then\nset tokenWindow to (create window with default profile command {0})\nset tokenTab to current tab of tokenWindow\nelse\ntell current window to set tokenTab to (create tab with default profile command {0})\nend if\ntry\nselect tokenTab\nend try\nend tell",
                literal(&inline)
            )
        } else {
            format!("tell application \"Terminal\"\nactivate\ndo script {}\nend tell", literal(&inline))
        };
        let mut child = std::process::Command::new("/usr/bin/osascript")
            .args(["-e", &script])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|error| HostFailure(error.to_string()))?;
        let deadline = Instant::now() + Duration::from_secs(15);
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
                _ => {
                    // Delivery is uncertain: never start the updater twice after a timeout.
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(HostFailure("终端响应超时，请先检查是否已经打开更新标签".into()));
                }
            }
        };
        if status.success() {
            return Ok("direct");
        }
        let name = job_file(state, "command", &format!("#!/bin/bash\n{}", bootstrap(command)))?;
        let mut opened = shell::run(&["/usr/bin/open", "-a", app, &name], 10.0, None, None).is_some();
        if !opened && app != "Terminal" {
            opened = shell::run(&["/usr/bin/open", "-a", "Terminal", &name], 10.0, None, None).is_some();
        }
        if !opened {
            return Err(HostFailure("无法打开终端，请检查 SwiftBar 的终端设置".into()));
        }
        Ok("file")
    }

    /// A script of its own in terminal-jobs for a terminal to run, the newest twenty of its kind kept, for its owner only.
    fn job_file(state: &str, extension: &str, text: &str) -> Result<String, HostFailure> {
        let root = format!("{state}/terminal-jobs");
        files::make_directory(Path::new(&root), 0o700).map_err(|error| HostFailure(error.to_string()))?;
        let suffix = format!(".{extension}");
        for path in listing(&root, |name| name.ends_with(&suffix)).iter().skip(19) {
            if !is_link(path) {
                let _ = std::fs::remove_file(path);
            }
        }
        let name = format!("{root}/starmeter-{}{suffix}", runtime::random_hex(12));
        if !create_exclusively(&name, text.as_bytes()) {
            return Err(HostFailure("无法写入终端命令".into()));
        }
        platform::set_mode(Path::new(&name), 0o700);
        Ok(name)
    }

    /// The program of this name on the PATH, if any.
    pub fn on_path(name: &str) -> Option<std::path::PathBuf> {
        std::env::split_paths(&std::env::var_os("PATH")?).map(|folder| folder.join(name)).find(|path| path.is_file())
    }

    /// The terminals tried on Linux, with what each wants before the command it runs: the desktop's own choice first
    /// (x-terminal-emulator on Debian and Ubuntu), then the common ones.
    pub const LINUX_TERMINALS: [(&str, &[&str]); 10] = [
        ("x-terminal-emulator", &["-e"]),
        ("gnome-terminal", &["--"]),
        ("konsole", &["-e"]),
        ("xfce4-terminal", &["-x"]),
        ("mate-terminal", &["-x"]),
        ("tilix", &["-e"]),
        ("kitty", &[]),
        ("alacritty", &["-e"]),
        ("foot", &[]),
        ("xterm", &["-e"]),
    ];

    /// Linux: the command in a new terminal window, from a script of its own, by the first terminal found. None found,
    /// the reason says what to install.
    pub fn launch_linux(command: &str, state: &str) -> Result<&'static str, HostFailure> {
        let Some((program, flags)) =
            LINUX_TERMINALS.iter().find_map(|(name, flags)| on_path(name).map(|path| (path, *flags)))
        else {
            return Err(HostFailure("没有找到终端程序（如 GNOME 终端、Konsole 或 xterm），装一个后再试".into()));
        };
        let script = job_file(state, "sh", &format!("#!/bin/bash\n{}", bootstrap(command)))?;
        let mut child = std::process::Command::new(&program)
            .args(flags)
            .args(["/bin/bash", &script])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|error| HostFailure(format!("无法打开终端：{error}")))?;
        // The terminal may hand the window to a server of its own and end at once; it is waited for either way.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok("file")
    }

    /// Windows: the CLI's own update in a new console window, left open after it so its output can be read. The
    /// script is ASCII: a console reads a .cmd in its own code page. `call` comes back after an npm .cmd shim.
    pub fn launch_windows(binary: &str, key: &str, state: &str) -> Result<&'static str, HostFailure> {
        let label = cli_name(key).ok_or_else(|| HostFailure("invalid CLI".into()))?;
        let text = format!(
            "@echo off\r\ntitle Starmeter: {label} update\r\necho $ \"{binary}\" update\r\ncall \"{binary}\" update\r\necho.\r\npause\r\n"
        );
        let script = job_file(state, "cmd", &text)?;
        std::process::Command::new("cmd")
            .args(["/c", "start", "", &script])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map(|_| "file")
            .map_err(|error| HostFailure(format!("无法打开命令提示符：{error}")))
    }

    pub fn latest_log(state: &str, keys: &[String]) -> Option<String> {
        let root = format!("{state}/logs/updates");
        let mut files: Vec<String> = Vec::new();
        for key in keys.iter().filter(|key| cli_name(key).is_some()) {
            files.extend(listing(&root, |name| name.starts_with(&format!("{key}-")) && name.ends_with(".log")));
        }
        files
            .into_iter()
            .filter(|path| !is_link(path) && !Path::new(path).is_dir() && Path::new(path).exists())
            .max_by(|a, b| modified(a).total_cmp(&modified(b)))
    }

    /// The update and its log: versions before and after, the exit status kept from the updater, never from tee.
    pub fn update_command(
        binary: &str,
        key: &str,
        state: &str,
        refresh_url: Option<&str>,
        zone: &TimeZone,
    ) -> Result<String, HostFailure> {
        let label = cli_name(key).ok_or_else(|| HostFailure("invalid CLI".into()))?;
        let root = format!("{state}/logs/updates");
        files::make_directory(Path::new(&root), 0o700).map_err(|error| HostFailure(error.to_string()))?;
        for path in
            listing(&root, |name| name.starts_with(&format!("{key}-")) && name.ends_with(".log")).iter().skip(19)
        {
            if !is_link(path) {
                let _ = std::fs::remove_file(path);
            }
        }
        let stamp = dashboard::format(runtime::now(), zone, "%Y%m%d-%H%M%S");
        let path = (0..100)
            .map(|_| format!("{root}/{key}-{stamp}-{}.log", runtime::random_hex(12)))
            .find(|candidate| create_exclusively(candidate, b""))
            .ok_or_else(|| HostFailure("无法创建更新日志".into()))?;
        let quote = connections::shell_quote;
        let version = join(&[binary, "--version"]);
        let update = join(&[binary, "update"]);
        let lines = [
            format!("token_update_log={}", quote(&path)),
            format!("token_before=$({version} 2>/dev/null) || token_before=\"版本读取失败\""),
            format!(
                "printf '\\n%s\\n%s\\n更新前：%s\\n\\n' {} {} \"$token_before\" | /usr/bin/tee -a \"$token_update_log\"",
                quote(&format!("{label} 更新")),
                quote(&format!("$ {update}"))
            ),
            format!("{update} 2>&1 | /usr/bin/tee -a \"$token_update_log\""),
            "token_update_status=${PIPESTATUS[0]}".into(),
            format!("token_after=$({version} 2>/dev/null); token_version_status=$?"),
            "if [ \"$token_update_status\" -ne 0 ]; then".into(),
            "  printf '\\n更新失败（退出码 %s）。请查看上方日志。\\n当前版本：%s\\n' \"$token_update_status\" \"${token_after:-无法读取}\"".into(),
            "elif [ \"$token_version_status\" -ne 0 ] || [ -z \"$token_after\" ]; then".into(),
            "  printf '\\n更新命令已完成，但无法确认当前版本，请查看上方日志。\\n'".into(),
            "elif [ \"$token_before\" = \"$token_after\" ]; then".into(),
            "  printf '\\n检查完成，版本未变化：%s\\n是否已是最新版本，请以上方更新器日志为准。\\n' \"$token_after\"".into(),
            "else".into(),
            "  printf '\\n更新成功：%s → %s\\n' \"$token_before\" \"$token_after\"".into(),
            "fi | /usr/bin/tee -a \"$token_update_log\"".into(),
            "printf '\\n日志已保存：%s\\n\\n' \"$token_update_log\" | /usr/bin/tee -a \"$token_update_log\"".into(),
            refresh_url.map_or_else(|| ":".to_string(), |url| join(&["/usr/bin/open", "-g", url])),
            "(exit \"$token_update_status\")".into(),
        ];
        Ok(lines.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> String {
        let folder = std::env::temp_dir().join(format!("starmeter-ask-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&folder).expect("a test folder");
        folder.to_string_lossy().into_owned()
    }

    fn panel_state(folder: &str, visible: bool) {
        let state = format!(r#"{{"schema": 1, "visible": {visible}, "asks": true, "pid": {}}}"#, std::process::id());
        std::fs::write(Path::new(folder).join("panel-state.json"), state).expect("the panel state");
    }

    /// The panel's side: waits for the question, then answers it (or closes instead).
    fn panel(folder: String, answer: Option<&'static str>) -> std::thread::JoinHandle<Option<Json>> {
        std::thread::spawn(move || {
            let ask = Path::new(&folder).join("panel-ask.json");
            for _ in 0..200 {
                if let Some(question) = std::fs::read(&ask).ok().and_then(|data| Json::parse(&data).ok()) {
                    match answer {
                        Some(text) => {
                            let id = question.get("id").cloned().unwrap_or(Json::Null);
                            let reply = Json::Object(object! { "id" => id, "answer" => text }).serialized();
                            std::fs::write(Path::new(&folder).join("panel-answer.json"), reply).expect("the answer");
                        }
                        None => panel_state(&folder, false),
                    }
                    return Some(question);
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            None
        })
    }

    #[test]
    fn the_panel_answers_a_question_through_its_files() {
        let folder = folder("answer");
        panel_state(&folder, true);
        let answering = panel(folder.clone(), Some("30"));
        let host = PanelAskHost::new(folder.clone());
        assert_eq!(host.text_dialog("每月多少美元？", "20", false, "订阅月费"), Some("30".to_string()));
        let question = answering.join().expect("the panel").expect("a question");
        assert_eq!(question.get("kind"), Some(&Json::from("text")));
        assert_eq!(question.get("value"), Some(&Json::from("20")));
        // Both files are gone once it is answered.
        assert!(!Path::new(&folder).join("panel-ask.json").exists());
        assert!(!Path::new(&folder).join("panel-answer.json").exists());
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_panel_closing_under_its_question_cancels_it() {
        let folder = folder("close");
        panel_state(&folder, true);
        let closing = panel(folder.clone(), None);
        let host = PanelAskHost::new(folder.clone());
        assert!(!host.confirm("删除这把 Key？", "", "删除"));
        assert!(closing.join().expect("the panel").is_some());
        let _ = std::fs::remove_dir_all(&folder);
    }
}
