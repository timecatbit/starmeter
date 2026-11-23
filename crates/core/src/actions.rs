//! What the menu's and the panel's rows do, and the panel's queued requests: each runs only if the rendered panel
//! offered it, and leaves a receipt.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use regex::Regex;

use crate::api_rows::KEY_CHECK_PROVIDERS;
use crate::background::{EXTERNAL_TTLS, KEY_TTLS, RESOURCE_CACHES};
use crate::dashboard::{Selected, summary_usage};
use crate::files::paths;
use crate::host::{HostFailure, connections, terminal_jobs, usage_export};
use crate::json::{Json, Object, parse_python_float};
use crate::opencodex::{ENDPOINTS, monitor_url};
use crate::rows::{g, shorten};
use crate::runtime::{self, RANGES, Runtime, VERSION, now};
use crate::tally::archive;
use crate::update_products::{self, Kind};
use crate::{accounts, catalog, claude_app, claude_bridge, dashboard, keys, object, platform, preferences};
use crate::{py, shell};

#[derive(Clone, Debug)]
pub struct ActionError(pub String);

impl std::fmt::Display for ActionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<preferences::Error> for ActionError {
    fn from(error: preferences::Error) -> Self {
        ActionError(error.to_string())
    }
}

impl From<HostFailure> for ActionError {
    fn from(error: HostFailure) -> Self {
        ActionError(error.0)
    }
}

impl From<std::io::Error> for ActionError {
    fn from(error: std::io::Error) -> Self {
        ActionError(error.to_string())
    }
}

type Outcome = Result<i32, ActionError>;

fn invalid<T>(message: &str) -> Result<T, ActionError> {
    Err(ActionError(message.to_string()))
}

const CONNECTION_ORDER: [&str; 13] = [
    "app_updates",
    "claude_subscription",
    "qoder_subscription",
    "cursor_subscription",
    "gemini_subscription",
    "chatgpt",
    "komari",
    "provider_quotas",
    "grok",
    "minimax",
    "kimi-api",
    "qoder-api",
    "claude-api",
];

const TOGGLES: [&str; 6] = [
    "set-collapse",
    "set-reset-reminder",
    "set-cursor-usage",
    "set-keep-usage",
    "set-session-alerts",
    "set-service-status",
];
const RECEIPT_ACTIONS: [&str; 9] = [
    "set-module",
    "set-appearance",
    "set-theme",
    "reset-modules",
    "set-collapse",
    "set-expanded",
    "detect-modules",
    "move-module",
    "reset-module-order",
];

fn always_allowed() -> Vec<Vec<String>> {
    let mut allowed: Vec<Vec<String>> =
        [["refresh-view"], ["refresh"], ["refresh-live"], ["detect-modules"], ["prefetch-ranges"]]
            .iter()
            .map(|a| vec![a[0].to_string()])
            .collect();
    allowed.extend(preferences::GROUPINGS.iter().map(|key| vec!["set-group".to_string(), key.to_string()]));
    allowed
}

fn connection_names(key: &str) -> Option<Vec<String>> {
    let names: &[&str] = match key {
        "komari" => &["KOMARI_BASE_URL"],
        "deepseek" => &["DEEPSEEK_API_KEY", "DEEPSEEK_API_KEY_FILE"],
        "grok" => &["XAI_MANAGEMENT_API_KEY", "XAI_MANAGEMENT_API_KEY_FILE", "XAI_TEAM_ID"],
        "minimax" => &["MINIMAX_API_KEY", "MINIMAX_API_KEY_FILE", "MINIMAX_REGION"],
        "kimi-api" => &["MOONSHOT_API_KEY", "MOONSHOT_API_KEY_FILE", "MOONSHOT_REGION"],
        "qoder-api" => &["QODER_CN_API_KEY", "QODER_CN_API_KEY_FILE", "QODER_CN_ORG_ID"],
        "claude-api" => &["ANTHROPIC_API_KEY", "ANTHROPIC_API_KEY_FILE"],
        _ if catalog::portal_apis().contains(&key) => {
            return Some(
                catalog::credential_names(key)
                    .iter()
                    .flat_map(|name| [name.to_string(), format!("{name}_FILE")])
                    .collect(),
            );
        }
        _ => return None,
    };
    Some(names.iter().map(|name| name.to_string()).collect())
}

/// Python's '%g' of a number setting, or "" for anything else.
fn python_g(value: Option<&Json>) -> String {
    value.and_then(Json::number).map(g).unwrap_or_default()
}

fn plain(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Why a command gave no result.
#[derive(Debug, PartialEq)]
enum Ran {
    NotStarted,
    TimedOut,
}

/// Runs a command to its end within `timeout` seconds: its exit and everything it wrote, the standard output then
/// the standard error. Each stream is drained on its own thread, so a command that writes much to one while the other
/// is still open finishes instead of waiting on a full pipe until the timeout (OCX-1). Past the timeout it is killed.
fn run_collecting(binary: &str, arguments: &[&str], timeout: f64) -> Result<(std::process::ExitStatus, Vec<u8>), Ran> {
    fn drain<R: Read + Send + 'static>(stream: Option<R>) -> mpsc::Receiver<Vec<u8>> {
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let mut data = Vec::new();
            if let Some(mut stream) = stream {
                let _ = stream.read_to_end(&mut data);
            }
            let _ = sender.send(data);
        });
        receiver
    }
    let mut child = Command::new(binary)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| Ran::NotStarted)?;
    let (out, err) = (drain(child.stdout.take()), drain(child.stderr.take()));
    let deadline = std::time::Instant::now() + Duration::from_secs_f64(timeout);
    let left = || deadline.saturating_duration_since(std::time::Instant::now());
    let (Ok(mut data), Ok(errors)) = (out.recv_timeout(left()), err.recv_timeout(left())) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(Ran::TimedOut);
    };
    data.extend(errors);
    let status = child.wait().map_err(|_| Ran::NotStarted)?;
    Ok((status, data))
}

impl Runtime {
    // Background work

    /// Runs the entry again, detached, for a background mode; the standalone app and desktop hosts queue their own.
    pub fn launch_background(&self, mode: &str) {
        if self.mac_app {
            return;
        }
        if let Some(desktop) = self.desktop() {
            desktop.request_refresh(Some(mode));
            return;
        }
        shell::detach(&["/bin/bash", "-p", &self.plugin, mode], None);
    }

    pub fn schedule_background_refresh(&self) {
        if let Some(desktop) = self.desktop() {
            desktop.request_refresh(Some("--background-refresh"));
            return;
        }
        let due = self.external_loaders().iter().any(|(name, _)| self.loader_due(name));
        if due || !self.usage_ranges_due().is_empty() {
            self.launch_background("--background-refresh");
        }
    }

    fn loader_names(&self) -> Vec<String> {
        self.external_loaders().into_iter().map(|(name, _)| name).collect()
    }

    fn invalidate(&self, names: &[String]) {
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        self.invalidate_caches(&names);
    }

    fn refresh_resource(&self, source: &str) -> Result<(), ActionError> {
        if !RESOURCE_CACHES.iter().any(|pair| pair.0 == source) || !self.module_enabled(source) {
            return invalid("invalid resource");
        }
        let names = self.resource_cache_names(source, &self.loader_names());
        if names.is_empty() {
            return Ok(());
        }
        let mut checks = self.read_cache("resource-checks");
        let moment = now();
        if let Some(last) = checks.get(source).and_then(Json::number)
            && (0.0..5.0).contains(&(moment - last))
        {
            return Ok(());
        }
        checks.insert(source, moment);
        checks.insert("schema", 1);
        self.write_json_cache(&self.cache_path("resource-checks"), &Json::Object(checks));
        self.invalidate(&names);
        self.schedule_background_refresh();
        Ok(())
    }

    /// A stable cache folder: data from an old connection is dropped when its settings change.
    pub fn sync_connection_cache(&self) {
        let previous = self.read_cache("connection-fingerprints");
        let mut current = object! { "schema" => 1 };
        for name in CONNECTION_ORDER {
            let fingerprint = Json::from(self.connection_fingerprint(name));
            if previous.get(name) != Some(&fingerprint) {
                platform::remove(Path::new(&self.cache_path(name)));
            }
            current.insert(name, fingerprint);
        }
        current.insert("proxy", self.connection_fingerprint("proxy"));
        if previous.get("proxy") != current.get("proxy") {
            for name in ENDPOINTS.iter().map(|(name, _, _)| *name).chain(["badge"]) {
                platform::remove(Path::new(&self.cache_path(name)));
            }
            if let Ok(entries) = std::fs::read_dir(&self.cache_dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if name.starts_with("range_usage_") && name.ends_with(".json") {
                        platform::remove(&entry.path());
                    }
                }
            }
        }
        self.write_json_cache(&self.cache_path("connection-fingerprints"), &Json::Object(current));
    }

    // Small helpers

    /// The native panel installed beside the entry.
    pub fn panel_app(&self) -> String {
        let entry = runtime::absolute(&self.plugin);
        let folder = Path::new(&entry).parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        format!("{folder}/.opencodex-panel/OpenCodexPanel.app")
    }

    fn named_key_loader_names(&self) -> Vec<String> {
        KEY_TTLS
            .iter()
            .filter(|(provider, _)| self.module_enabled(provider))
            .flat_map(|(provider, _)| {
                self.named_keys(provider)
                    .into_iter()
                    .map(move |entry| format!("{provider}@{}", py::str(entry.get("id"))))
            })
            .collect()
    }

    fn confirm(&self, message: &str, detail: &str, accept: &str) -> bool {
        self.host.confirm(message, detail, accept)
    }

    fn dialog_error(&self, title: &str, detail: &str) {
        self.host.error(title, detail);
    }

    fn text_dialog(&self, message: &str, value: &str, title: &str) -> Option<String> {
        self.host.text_dialog(message, value, false, title)
    }

    fn open_local(&self, path: &str, text: bool) -> Result<(), ActionError> {
        Ok(self.host.open_local(path, text)?)
    }

    fn save_overrides(&self, values: &[(String, String)]) -> Result<(), ActionError> {
        match self.desktop() {
            Some(desktop) => Ok(desktop.save_overrides(values)?),
            None => Ok(connections::write_overrides(&self.state_dir, values)?),
        }
    }

    fn update(&self, kind: &str, key: Option<&str>, value: Option<&str>) -> Result<(), preferences::Error> {
        preferences::update(Path::new(&self.state_dir), kind, key, value).map(|_| ())
    }

    fn terminal(&self, command: &str) -> Result<(), ActionError> {
        let preferred = shell::run(&["/usr/bin/defaults", "read", "com.ameba.SwiftBar", "Terminal"], 5.0, None, None)
            .map(|text| text.trim().to_string())
            .unwrap_or_default();
        terminal_jobs::launch(command, &preferred, &self.state_dir)?;
        Ok(())
    }

    // OpenCodex's own commands

    /// None when the command succeeded, else what went wrong.
    fn ocx(&self, arguments: &[&str], timeout: f64) -> Option<String> {
        let binary = shell::which("ocx", &self.environment).unwrap_or_else(|| "/opt/homebrew/bin/ocx".into());
        if !Path::new(&binary).exists() {
            return Some("找不到 ocx 命令".into());
        }
        let (status, data) = match run_collecting(&binary, arguments, timeout) {
            Ok(done) => done,
            Err(Ran::NotStarted) => return Some("找不到 ocx 命令".into()),
            Err(Ran::TimedOut) => return Some(format!("ocx {} 执行超时", arguments.join(" "))),
        };
        if status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&data);
        let last = text.lines().rfind(|line| !line.trim().is_empty());
        Some(last.map_or_else(|| "命令执行失败".into(), |line| shorten(line, 160)))
    }

    fn cli_action(&self, arguments: &[&str], message: &str) {
        let failure = self.ocx(arguments, 240.0);
        let label = format!("ocx {}", arguments.join(" "));
        self.notify(&failure.map_or_else(|| message.to_string(), |failure| format!("失败：{failure}")), &label);
    }

    fn api_action(&self, path: &str, method: &str, body: Json, message: &str, prefix: &str) {
        let (_, failure) = self.opencodex_request(path, method, Some(&body), 30.0, true);
        match failure {
            Some(failure) => {
                let reason = if failure.message.is_empty() { failure.code.to_string() } else { failure.message };
                self.notify(&format!("{prefix}：{reason}"), path);
            }
            None => {
                let names: Vec<&str> = ENDPOINTS.iter().map(|(name, _, _)| *name).collect();
                self.invalidate_caches(&names);
                self.notify(message, path);
            }
        }
    }

    // Connections

    /// Only the plugin's overrides change; client files and the global environment stay as they are.
    fn apply_client_connection(&self, values: &[(String, String)], module: Option<&str>) -> Result<(), ActionError> {
        self.save_overrides(values)?;
        let next = self.updated(values);
        if let Some(module) = module {
            self.update("module", Some(module), Some("true"))?;
        }
        next.sync_connection_cache();
        next.schedule_background_refresh();
        Ok(())
    }

    fn configure_client_action(&self, name: &str, extra: &[String]) -> Result<(), ActionError> {
        let key = if extra.len() == 1 { extra[0].as_str() } else { "" };
        if ["choose-client-home", "reset-client-connection", "reveal-client-home"].contains(&name)
            && !["chatgpt", "opencodex"].contains(&key)
        {
            return invalid("invalid client");
        }
        let pair = |key: &str, value: &str| (key.to_string(), value.to_string());
        let values: Vec<(String, String)> = match name {
            "reveal-client-home" => {
                let root = if key == "opencodex" { self.ocx_home.clone() } else { self.codex_home_absolute() };
                if Path::new(&root).is_dir() {
                    self.open_local(&root, false)?;
                }
                return Ok(());
            }
            "reset-client-connection" => {
                if key == "chatgpt" {
                    vec![pair("OCX_CHATGPT_HOME", "")]
                } else {
                    vec![pair("OCX_HOME", ""), pair("OCX_BASE_URL", ""), pair("OCX_TOKEN_FILE", "")]
                }
            }
            "choose-client-home" => {
                let message = if key == "chatgpt" {
                    "选择包含 auth.json 的 ChatGPT 编程客户端配置目录"
                } else {
                    "选择 OpenCodex 配置目录；自动读取其中的运行端口和管理令牌"
                };
                let Some(chosen) = self.host.choose_path(message, true) else { return Ok(()) };
                let root = runtime::absolute(&paths::expand_user(&chosen).to_string_lossy());
                if !Path::new(&root).is_dir() {
                    return Ok(());
                }
                if key == "chatgpt" {
                    let auth = claude_app::read_object(&format!("{root}/auth.json"));
                    let mode = auth.get("auth_mode").and_then(Json::as_str).unwrap_or("");
                    let token =
                        auth.get("tokens").and_then(|tokens| tokens.get("access_token")).is_some_and(Json::is_truthy);
                    if !["chatgpt", "chatgptAuthTokens"].contains(&mode) && !token {
                        self.dialog_error(
                            "未找到 ChatGPT 登录",
                            "请选择已通过 ChatGPT 账户登录的 ChatGPT 编程客户端配置目录。仅 API Key 登录不能读取订阅额度。",
                        );
                        return Ok(());
                    }
                    vec![pair("OCX_CHATGPT_HOME", &root)]
                } else {
                    let is_file = |name: &str| {
                        let path = format!("{root}/{name}");
                        Path::new(&path).exists() && !Path::new(&path).is_dir()
                    };
                    if !["runtime-port.json", "admin-api-token", "config.json"].iter().any(|name| is_file(name)) {
                        self.dialog_error(
                            "未找到 OpenCodex 配置",
                            "请选择包含运行端口、管理令牌或 config.json 的目录。",
                        );
                        return Ok(());
                    }
                    vec![pair("OCX_HOME", &root), pair("OCX_BASE_URL", ""), pair("OCX_TOKEN_FILE", "")]
                }
            }
            "set-proxy-address" => {
                if !extra.is_empty() {
                    return invalid("invalid proxy address action");
                }
                let Some(chosen) = self.text_dialog("OpenCodex 管理地址", &self.base, "连接设置") else {
                    return Ok(());
                };
                let url = monitor_url(&chosen);
                if url.is_empty() {
                    self.dialog_error("地址无效", "请填写实际的 http:// 或 https:// 管理地址。");
                    return Ok(());
                }
                vec![pair("OCX_BASE_URL", &url)]
            }
            "choose-admin-token" => {
                if !extra.is_empty() {
                    return invalid("invalid token action");
                }
                let Some(chosen) = self.host.choose_path("选择 OpenCodex 管理令牌文件；仅保存文件路径", false)
                else {
                    return Ok(());
                };
                let file = runtime::absolute(&paths::expand_user(&chosen).to_string_lossy());
                if !Path::new(&file).exists() || Path::new(&file).is_dir() {
                    return Ok(());
                }
                vec![pair("OCX_TOKEN_FILE", &file)]
            }
            _ => return invalid("invalid connection action"),
        };
        let module = if name == "reset-client-connection" {
            None
        } else if key == "chatgpt" {
            Some("chatgpt")
        } else {
            Some("opencodex")
        };
        self.apply_client_connection(&values, module)
    }

    /// Grok needs its Team ID, Kimi and MiniMax a region, Qoder CN an organization; None means the user cancelled.
    fn ask_option(&self, key: &str, current: &str) -> Option<String> {
        match key {
            "grok" => {
                let team =
                    self.text_dialog("xAI Team ID（需要管理密钥，普通推理 API Key 不适用）", current, "连接设置");
                if let Some(team) = &team
                    && !plain(team)
                {
                    self.dialog_error("Team ID 无效", "请填写 xAI 控制台中的 Team ID。");
                    return None;
                }
                team
            }
            "minimax" => {
                self.host.choose("MiniMax 套餐", "选择密钥所属地区", ("国际站", "international"), ("中国站", "china"))
            }
            "qoder-api" => {
                let org = self.text_dialog(
                    "Qoder 组织 ID（组织设置网址 qoder.cn/organizations/<组织 ID>/settings 中的那一段）",
                    current,
                    "连接设置",
                );
                if let Some(org) = &org
                    && !plain(org)
                {
                    self.dialog_error("组织 ID 无效", "请填写 Qoder 组织设置网址中的组织 ID。");
                    return None;
                }
                org
            }
            "kimi-api" => self.host.choose(
                "Kimi API",
                "选择 Key 所属平台：中国站 platform.moonshot.cn，国际站 platform.moonshot.ai",
                ("中国站", "china"),
                ("国际站", "international"),
            ),
            _ => Some(String::new()),
        }
    }

    fn configure_connection(&self, key: &str, clear: bool) -> Result<(), ActionError> {
        let Some(names) = connection_names(key) else { return invalid("invalid connection") };
        let mut values: Vec<(String, String)> = names.iter().map(|name| (name.clone(), String::new())).collect();
        let set = |values: &mut Vec<(String, String)>, name: &str, value: &str| {
            if let Some(entry) = values.iter_mut().find(|(key, _)| key == name) {
                entry.1 = value.to_string();
            }
        };
        if clear {
            if !self.confirm(
                "清除连接设置？",
                "清除插件保存的地址或密钥引用；插件保存的 Key 一并删除，你自己选择的密钥文件不会删除。",
                "清除设置",
            ) {
                return Ok(());
            }
        } else if key == "komari" {
            let Some(value) =
                self.text_dialog("Komari 面板地址（http:// 或 https://）", &self.komari_base_url(), "连接设置")
            else {
                return Ok(());
            };
            let url = monitor_url(&value);
            if url.is_empty() {
                self.dialog_error("地址无效", "请填写实际面板地址，不使用示例地址、账号密码或查询参数。");
                return Ok(());
            }
            set(&mut values, "KOMARI_BASE_URL", &url);
        } else {
            let Some(file) =
                self.host.choose_path(&format!("选择 {key} 的密钥文本文件；只保存路径，不复制或展示密钥。"), false)
            else {
                return Ok(());
            };
            if !Path::new(&file).exists() || Path::new(&file).is_dir() {
                return Ok(());
            }
            set(&mut values, &names[1], &file);
            let variable = match key {
                "grok" => "XAI_TEAM_ID",
                "qoder-api" => "QODER_CN_ORG_ID",
                _ => "",
            };
            let current = self.environment.get(variable).cloned().unwrap_or_default();
            let Some(option) = self.ask_option(key, &current) else { return Ok(()) };
            let chosen = std::fs::read_to_string(&file).map(|text| text.trim().to_string()).unwrap_or_default();
            let options = match key {
                "grok" => object! { "team" => option.as_str() },
                "qoder-api" => object! { "org" => option.as_str() },
                _ => Object::new(),
            };
            if self.refuse_repeated(key, &chosen, &options, Some(self.named_keys(key))) {
                return Ok(());
            }
            if !option.is_empty() {
                set(&mut values, &names[2], &option);
            }
        }
        let previous: Vec<String> = names
            .iter()
            .filter(|name| name.ends_with("_FILE"))
            .map(|name| self.environment.get(name).cloned().unwrap_or_default())
            .collect();
        self.save_overrides(&values)?;
        for file in previous {
            if !file.is_empty()
                && !values.iter().any(|(_, value)| *value == file)
                && connections::managed_key(&self.state_dir, &file)
            {
                platform::remove(&paths::expand_user(&file));
            }
        }
        let next = self.updated(&values);
        next.sync_connection_cache();
        if !clear {
            self.update("module", Some(key), Some("true"))?;
        } else if key != "deepseek" && keys::entries(Path::new(&self.state_dir), key).is_empty() {
            self.update("module", Some(key), Some("false"))?;
        }
        // A newly saved key gets one check right away, so the row shows a result instead of nothing.
        if !clear && KEY_CHECK_PROVIDERS.contains(&key) {
            next.dispatch_key_check(key, "default");
        }
        next.schedule_background_refresh();
        Ok(())
    }

    /// A key already here isn't added again: the same secret and, for Grok and Qoder CN, the same team or
    /// organization.
    fn refuse_repeated(&self, provider: &str, secret: &str, options: &Object, given: Option<Vec<Object>>) -> bool {
        let account = self.key_account(provider, &object! { "id" => "new", "options" => options.clone() });
        let keys = given.unwrap_or_else(|| self.api_keys(provider));
        let same = keys.iter().find(|entry| {
            !secret.is_empty()
                && self.entry_secret(provider, &py::str(entry.get("id"))) == secret
                && (account.first().map(String::as_str) == Some("key") || self.key_account(provider, entry) == account)
        });
        if let Some(same) = same {
            self.dialog_error(
                "这把 Key 已经添加过",
                &format!("与「{}」是同一把 Key，不再重复添加。", py::str(same.get("label"))),
            );
            return true;
        }
        false
    }

    fn add_api_key(&self, provider: &str) -> Result<(), ActionError> {
        let Some(service) = catalog::by_key(provider).filter(|service| service.category == "API") else {
            return invalid("invalid API");
        };
        let Some(name) =
            self.text_dialog("给这把 Key 起个名字", &format!("Key {}", self.api_keys(provider).len() + 1), "添加 Key")
        else {
            return Ok(());
        };
        if keys::label(Some(&name)).is_err() {
            self.dialog_error("名字无效", "请填写 1–24 个字符的名字。");
            return Ok(());
        }
        let note = if provider == "claude-api" {
            "\n管理员 Key 权限很大，建议在控制台设置过期时间；插件只用它读取花费。"
        } else {
            ""
        };
        let prompt = match provider {
            "grok" => "xAI 管理密钥（Management API Key）".to_string(),
            "qoder-api" => {
                "Qoder 组织 API Key（组织管理员在 设置 → 高级 → API 密钥 创建，仅团队版/企业版）".to_string()
            }
            _ => format!("{} Key", service.label()),
        };
        let Some(secret) = self.host.text_dialog(
            &format!("{prompt}\n只保存在本机仅你可读的文件中，不会显示或上传。{note}"),
            "",
            true,
            "添加 Key",
        ) else {
            return Ok(());
        };
        if !keys::valid_key(&secret) {
            self.dialog_error("Key 格式无效", "请粘贴完整的 API Key，不含空格或换行。");
            return Ok(());
        }
        let Some(option) = self.ask_option(provider, "") else { return Ok(()) };
        let options = match provider {
            "grok" => object! { "team" => option.as_str() },
            "kimi-api" => object! { "region" => option.as_str() },
            "qoder-api" => object! { "org" => option.as_str() },
            _ => Object::new(),
        };
        if self.refuse_repeated(provider, &secret, &options, None) {
            return Ok(());
        }
        let id = keys::add(Path::new(&self.state_dir), provider, &name, &secret, options)?;
        self.update("module", Some(provider), Some("true"))?;
        if KEY_CHECK_PROVIDERS.contains(&provider) {
            self.dispatch_key_check(provider, &id);
        }
        self.schedule_background_refresh();
        Ok(())
    }

    fn rename_api_key(&self, provider: &str, id: &str) -> Result<(), ActionError> {
        let Some(entry) = self.api_keys(provider).into_iter().find(|entry| entry.get("id") == Some(&Json::from(id)))
        else {
            return invalid("unknown key");
        };
        let Some(name) = self.text_dialog("重命名这把 Key", &py::str(entry.get("label")), "重命名 Key") else {
            return Ok(());
        };
        match keys::rename(Path::new(&self.state_dir), provider, id, &name) {
            Err(preferences::Error::Invalid(_)) => self.dialog_error("名字无效", "请填写 1–24 个字符的名字。"),
            other => other?,
        }
        Ok(())
    }

    fn remove_api_key(&self, provider: &str, id: &str) -> Result<(), ActionError> {
        let Some(entry) = self.api_keys(provider).into_iter().find(|entry| entry.get("id") == Some(&Json::from(id)))
        else {
            return invalid("unknown key");
        };
        if id == "default" {
            return self.configure_connection(provider, true);
        }
        if !self.confirm(
            &format!("删除「{}」？", py::str(entry.get("label"))),
            "只删除插件保存的这把 Key，不影响服务商账户。",
            "删除",
        ) {
            return Ok(());
        }
        keys::remove(Path::new(&self.state_dir), provider, id)?;
        platform::remove(Path::new(&self.cache_path(&py::str(entry.get("cache")))));
        let mut checks = self.read_cache("key-checks");
        let check = if id == "default" { provider.to_string() } else { format!("{provider}@{id}") };
        if checks.remove(&check).is_some() {
            self.write_json_cache(&self.cache_path("key-checks"), &Json::Object(checks));
        }
        if !self.connection_ready(provider) {
            self.update("module", Some(provider), Some("false"))?;
        }
        self.schedule_background_refresh();
        Ok(())
    }

    fn dispatch_key_check(&self, provider: &str, id: &str) {
        match self.desktop() {
            Some(desktop) => desktop.submit_test(provider, id),
            None => {
                let _ = self.test_api_key(provider, id);
            }
        }
    }

    pub fn test_api_key(&self, provider: &str, id: &str) -> Result<(), ActionError> {
        let secret =
            if KEY_CHECK_PROVIDERS.contains(&provider) { self.entry_secret(provider, id) } else { String::new() };
        if secret.is_empty() {
            return invalid("invalid key check");
        }
        let mut checks = self.read_cache("key-checks");
        checks.insert("schema", 1);
        let mut result = accounts::check_key(provider, &secret);
        result.insert("checkedAt", now());
        result.insert("key", self.key_fingerprint(provider, id));
        let check = if id == "default" { provider.to_string() } else { format!("{provider}@{id}") };
        checks.insert(check, result);
        self.write_json_cache(&self.cache_path("key-checks"), &Json::Object(checks));
        Ok(())
    }

    fn set_balance_alert(&self, provider: &str, currency: &str) -> Result<(), ActionError> {
        let key = format!("{provider}|{currency}");
        if !preferences::balance_key(&key) {
            return invalid("invalid balance alert");
        }
        let settings = self.settings();
        let line = settings.get("balanceAlerts").and_then(|alerts| alerts.get(&key)).cloned();
        let unit = match currency {
            "CNY" => "元".to_string(),
            "USD" => "美元".to_string(),
            other => format!(" {other} "),
        };
        let label = catalog::by_key(provider).map(|service| service.label()).unwrap_or_default();
        let current = if line.as_ref().is_some_and(Json::is_truthy) { python_g(line.as_ref()) } else { String::new() };
        let Some(answer) =
            self.text_dialog(&format!("{label} 余额降到多少{unit}以下时提醒？留空关闭。"), &current, "余额提醒")
        else {
            return Ok(());
        };
        let value = answer.trim().trim_start_matches(['¥', '$']).replace(',', "");
        match self.update("balance-alert", Some(&key), Some(&value)) {
            Err(preferences::Error::Invalid(_)) => {
                self.dialog_error("金额无效", "请填写大于 0 的数字，或留空关闭提醒。")
            }
            other => other?,
        }
        Ok(())
    }

    fn set_subscription_price(&self, key: &str) -> Result<(), ActionError> {
        if !catalog::SUBSCRIPTION_USAGE.iter().any(|pair| pair.0 == key) {
            return invalid("invalid subscription");
        }
        let settings = self.settings();
        let price = settings.get("subscriptionPrices").and_then(|prices| prices.get(key)).cloned();
        let label = catalog::by_key(key).map(|service| service.label()).unwrap_or_default();
        let current =
            if price.as_ref().is_some_and(Json::is_truthy) { python_g(price.as_ref()) } else { String::new() };
        let Some(answer) = self.text_dialog(
            &format!("{label} 每月多少美元？用来算本月用量是订阅价的几倍。留空清除。"),
            &current,
            "订阅月费",
        ) else {
            return Ok(());
        };
        let mut value = answer.trim().trim_start_matches('$').replace(',', "");
        if let Some(stripped) = value.strip_suffix("/月") {
            value = stripped.to_string();
        }
        match self.update("subscription-price", Some(key), Some(value.trim())) {
            Err(preferences::Error::Invalid(_)) => {
                self.dialog_error("金额无效", "请填写大于 0 的数字（美元），或留空清除。")
            }
            other => other?,
        }
        self.schedule_background_refresh();
        Ok(())
    }

    // Export

    fn export_usage(&self, kind: &str) -> Result<(), ActionError> {
        if !["csv", "json"].contains(&kind) {
            return invalid("invalid export");
        }
        let key = self.selected_range();
        let label = RANGES.iter().find(|(range, _)| *range == key).map_or("", |(_, label)| *label);
        let bundle = self
            .with_devices(&key, self.read_cache(&self.range_cache_name(Some(&key))))
            .get("payload")
            .and_then(Json::as_object)
            .cloned()
            .unwrap_or_default();
        if bundle.get("range") != Some(&Json::from(key.as_str())) {
            self.dialog_error("还没有可导出的用量", &format!("“{label}”还在读取，读完后再导出。"));
            return Ok(());
        }
        let day = dashboard::format(now(), &self.local_zone(), "%Y%m%d");
        let Some(path) = self.host.save_path(&format!("导出“{label}”的用量"), &format!("starmeter-{key}-{day}.{kind}"))
        else {
            return Ok(());
        };
        let selected: Vec<Selected> = preferences::MODULES
            .iter()
            .filter(|module| preferences::USAGE.contains(&module.key) && self.module_enabled(module.key))
            .map(|module| {
                (
                    module.key.to_string(),
                    module.label.clone(),
                    bundle.get(module.key).and_then(Json::as_object).cloned().unwrap_or_default(),
                )
            })
            .collect();
        let counting = summary_usage(&selected);
        let text = if kind == "csv" {
            usage_export::csv_text(&selected, &counting)
        } else {
            usage_export::json_text(
                &selected,
                &counting,
                &key,
                label,
                bundle.get("start"),
                bundle.get("end"),
                VERSION,
                now(),
            )
        };
        usage_export::write(&path, kind, &text)?;
        let name = Path::new(&path).file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        self.notify(&format!("已导出到 {name}"), "导出用量");
        Ok(())
    }

    pub(crate) fn write_self_update_state(&self, values: Object) {
        let mut state = values;
        state.insert("at", now());
        self.write_json_cache(&format!("{}/self-update.json", self.state_dir), &Json::Object(state));
    }

    // The actions

    pub fn run_action(&self, name: &str, extra: &[String]) -> Outcome {
        let one = || if extra.len() == 1 { extra[0].clone() } else { String::new() };
        let arg = |index: usize| extra.get(index).map(String::as_str);
        let flag = |value: &str| -> Result<(), ActionError> {
            if ["true", "false"].contains(&value) { Ok(()) } else { invalid("invalid setting") }
        };
        match name {
            "panel" | "panel-modules" => {
                let mode = if name == "panel-modules" { "modules" } else { "home" };
                self.write_json_cache(
                    &self.cache_path("panel-open"),
                    &object! { "schema" => 1, "mode" => mode, "createdAt" => now() }.into(),
                );
                let app = self.panel_app();
                if Path::new(&app).is_dir() {
                    // Not "-g": the panel opens active, so its glass and controls don't change look on the first click.
                    let plugin = runtime::absolute(&self.plugin);
                    let snapshot = self.cache_path("panel-view");
                    let _ = shell::run(
                        &["/usr/bin/open", "-a", &app, "--args", "--plugin", &plugin, "--snapshot", &snapshot],
                        5.0,
                        None,
                        None,
                    );
                }
            }
            "set-module" | "set-appearance" | "set-theme" | "reset-modules" => {
                match name {
                    "set-module" => self.update("module", Some(arg(0).unwrap_or("")), Some(arg(1).unwrap_or("")))?,
                    "set-appearance" => self.update("appearance", Some(arg(0).unwrap_or("")), None)?,
                    "set-theme" => self.update("theme", Some(arg(0).unwrap_or("")), None)?,
                    _ => self.update("reset", None, None)?,
                }
                if name == "set-module"
                    && extra.len() == 2
                    && extra[1] == "true"
                    && RESOURCE_CACHES.iter().any(|pair| pair.0 == extra[0])
                {
                    self.invalidate(&self.resource_cache_names(&extra[0], &self.loader_names()));
                }
                self.schedule_background_refresh();
            }
            "move-module" => {
                let place = match extra.get(1).map(String::as_str) {
                    Some("before") => "move-before",
                    Some("after") => "move-after",
                    _ => return invalid("invalid homepage order"),
                };
                if extra.len() != 3 {
                    return invalid("invalid homepage order");
                }
                self.update(place, arg(0), arg(2))?;
            }
            "reset-module-order" => self.update("order-reset", None, None)?,
            "set-subscription" => {
                if extra.len() != 2 {
                    return invalid("invalid subscription");
                }
                self.update("subscription", arg(0), arg(1))?;
            }
            "set-collapse" => {
                if extra.len() != 1 {
                    return invalid("invalid display action");
                }
                self.update("collapse", arg(0), None)?;
            }
            "set-usage-alert" | "set-reset-reminder" => {
                if extra.len() != 1 {
                    return invalid("invalid alert setting");
                }
                self.update(if name == "set-usage-alert" { "usage-alert" } else { "reset-reminder" }, arg(0), None)?;
            }
            "set-balance-alert" => {
                if extra.len() != 2 {
                    return invalid("invalid balance alert");
                }
                self.set_balance_alert(&extra[0], &extra[1])?;
            }
            "set-subscription-price" => {
                if extra.len() != 1 {
                    return invalid("invalid subscription price");
                }
                self.set_subscription_price(&extra[0])?;
            }
            "set-expanded" => {
                if extra.len() != 2 {
                    return invalid("invalid expansion action");
                }
                self.update("expanded", arg(0), arg(1))?;
            }
            "detect-modules" => {
                let detected = self.detected_modules();
                let current: Vec<String> = self
                    .settings()
                    .get("visible")
                    .and_then(Json::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|key| key.as_str().map(String::from))
                    .collect();
                let labels: HashMap<&str, String> =
                    preferences::MODULES.iter().map(|module| (module.key, module.label.clone())).collect();
                let added: Vec<(String, String)> = detected
                    .iter()
                    .filter(|key| !current.contains(key))
                    .filter_map(|key| labels.get(key.as_str()).map(|label| (key.clone(), label.clone())))
                    .collect();
                // Only what is new is added: what is ticked stays ticked, what was taken off stays off unless found.
                self.invalidate(&[self.detection_cache_name()]);
                if added.is_empty() {
                    self.dialog_error("没有新找到的模块", "这台电脑上能找到的都已经在主页上了。");
                } else if self.confirm(
                    &format!(
                        "加到主页：{}",
                        added.iter().map(|(_, label)| label.as_str()).collect::<Vec<_>>().join("、")
                    ),
                    "只加不减：已勾选的模块保持不变。只检查本机目录与已有配置，不读取对话正文，不显示 Key。",
                    "加到主页",
                ) {
                    for (key, _) in &added {
                        self.update("module", Some(key), Some("true"))?;
                    }
                    self.schedule_background_refresh();
                }
            }
            "detect-source" => {
                // One tool looked for again at once, not from the last five minutes' answer: found, it goes on the
                // homepage; either way the answer shows on the row itself (look_again), where it was asked.
                let key = one();
                if !preferences::MODULES.iter().any(|module| module.key == key) {
                    return invalid("invalid source");
                }
                self.invalidate(&[self.detection_cache_name()]);
                let found = self.detected_modules().contains(&key);
                let shown = self
                    .settings()
                    .get("visible")
                    .and_then(Json::as_array)
                    .is_some_and(|visible| visible.iter().any(|item| item.as_str() == Some(key.as_str())));
                if found && !shown {
                    self.update("module", Some(&key), Some("true"))?;
                }
                let mut checks = self.read_cache("source-checks");
                checks.insert("schema", 1);
                checks.insert(
                    key.as_str(),
                    object! { "found" => found, "added" => found && !shown, "checkedAt" => now() },
                );
                self.write_json_cache(&self.cache_path("source-checks"), &Json::Object(checks));
                self.schedule_background_refresh();
            }
            "refresh-view" => self.schedule_background_refresh(),
            "choose-sync-folder" => {
                let message = "选择每台电脑都能同步到的文件夹（如 iCloud 云盘、Dropbox、OneDrive 里的一个文件夹）";
                let Some(chosen) = self.host.choose_path(message, true) else { return Ok(0) };
                let root = runtime::absolute(&paths::expand_user(&chosen).to_string_lossy());
                if !Path::new(&root).is_dir() {
                    return Ok(0);
                }
                // Moving to another folder takes this computer's file along.
                self.leave_devices();
                self.update("sync-folder", Some(&root), None)?;
                self.publish_device(true);
                self.schedule_background_refresh();
            }
            "set-device-name" => {
                let current = self.device_name();
                let Some(name) = self.host.text_dialog("其他电脑上显示的名字", &current, false, "本机名称")
                else {
                    return Ok(0);
                };
                self.update("device-name", Some(name.trim()), None)?;
                self.publish_device(true);
            }
            "clear-sync-folder" => {
                if self.confirm(
                    "停止合并多台电脑？",
                    "这台电脑的用量文件会从同步文件夹里删掉，其他电脑不再计入它；这台电脑也不再计入其他电脑。",
                    "停止合并",
                ) {
                    self.leave_devices();
                    self.update("sync-folder", Some(""), None)?;
                }
            }
            "refresh-source" => self.refresh_resource(&one())?,
            "choose-client-home"
            | "reset-client-connection"
            | "reveal-client-home"
            | "set-proxy-address"
            | "choose-admin-token" => self.configure_client_action(name, extra)?,
            "configure-connection" | "clear-connection" => {
                self.configure_connection(&one(), name == "clear-connection")?
            }
            "add-api-key" => self.add_api_key(&one())?,
            "rename-api-key" | "remove-api-key" => {
                if extra.len() != 2 {
                    return invalid("invalid key action");
                }
                if name == "rename-api-key" {
                    self.rename_api_key(&extra[0], &extra[1])?;
                } else {
                    self.remove_api_key(&extra[0], &extra[1])?;
                }
            }
            "test-api-key" => {
                if !(1..=2).contains(&extra.len()) {
                    return invalid("invalid key check");
                }
                self.test_api_key(&extra[0], arg(1).unwrap_or("default"))?;
            }
            "auto-source" => {
                self.update("auto-path", Some(&one()), None)?;
                self.schedule_background_refresh();
            }
            "open-settings" => {
                let file = match self.desktop() {
                    Some(desktop) => desktop.config_file(),
                    None => connections::ensure_file(&self.state_dir)?,
                };
                self.open_local(&file, true)?;
            }
            "open-claude" => {
                if !extra.is_empty() {
                    return invalid("invalid app action");
                }
                if let Some(desktop) = self.desktop() {
                    desktop.open_claude();
                    return Ok(0);
                }
                if shell::run(&["/usr/bin/open", "-a", "Claude"], 5.0, None, None).is_none() {
                    return invalid("open failed");
                }
            }
            "set-claude-oauth" | "reauthorize-claude-oauth" => {
                let reauthorize = name == "reauthorize-claude-oauth";
                if reauthorize && !extra.is_empty() {
                    return invalid("invalid Claude authorization action");
                }
                let value = if reauthorize { "true".to_string() } else { one() };
                if !["true", "false"].contains(&value.as_str()) {
                    return invalid("invalid Claude OAuth preference");
                }
                if value == "true" {
                    if reauthorize || !self.settings().get("claudeOAuthUsage").is_some_and(Json::is_truthy) {
                        let title = if reauthorize {
                            "重新读取 Claude Code 登录？"
                        } else {
                            "使用 Claude Code 登录查询用量？"
                        };
                        let detail = if self.is_desktop() {
                            "读取 Claude Code 本机登录文件，仅向 api.anthropic.com 查询用量。不会读取 Cookie，也不会调用模型。凭据不写入插件配置或日志；登录过期时在 Claude Code 重新登录。"
                        } else {
                            "读取 Claude Code 现有登录，仅向 api.anthropic.com 查询订阅用量。无需 Cookie 或模型调用。登录过期时可启动 Code 自检续期，不发送消息。\n凭据只在内存；不写入插件配置或日志。macOS 可能询问钥匙串权限。"
                        };
                        if !self.confirm(title, detail, if reauthorize { "重新读取" } else { "启用" }) {
                            return Ok(0);
                        }
                    }
                    self.update("claude-oauth", Some("true"), None)?;
                    // Renewal can take half a minute, so the background worker does it, as a retry.
                    self.write_json_cache(
                        &self.cache_path("claude-retry"),
                        &object! { "schema" => 1, "requestedAt" => now() }.into(),
                    );
                } else {
                    self.update("claude-oauth", Some("false"), None)?;
                    platform::remove(Path::new(&format!("{}/claude-oauth-usage.json", self.state_dir)));
                }
                self.invalidate_caches(&["claude_subscription"]);
                if value == "true" {
                    self.launch_background("--background-refresh-wait");
                } else {
                    self.schedule_background_refresh();
                }
            }
            "set-cursor-usage" => {
                let value = one();
                if !["true", "false"].contains(&value.as_str()) {
                    return invalid("invalid Cursor usage preference");
                }
                if value == "true"
                    && !self.settings().get("cursorUsage").is_some_and(Json::is_truthy)
                    && !self.confirm(
                        "读取 Cursor 用量记录？",
                        "Cursor 不在本机记录 Token。开启后，插件读取 Cursor 桌面版或 Cursor CLI 已保存的登录，只用它向 cursor.com 查询本账户的用量记录（模型、Token 与费用），桌面版和 CLI 的用量都包括在内。\n令牌只在内存中使用，不保存、不刷新、不改写，也不发往其他地址。",
                        "开启",
                    )
                {
                    return Ok(0);
                }
                self.update("cursor-usage", Some(&value), None)?;
                if value == "true" {
                    self.update("module", Some("cursor"), Some("true"))?;
                } else if let Ok(entries) = std::fs::read_dir(&self.cache_dir) {
                    for entry in entries.flatten() {
                        let file = entry.file_name().to_string_lossy().into_owned();
                        if file.starts_with("cursor_usage_") || file.starts_with("cursor_subscription") {
                            platform::remove(&entry.path());
                        }
                    }
                }
                self.invalidate(&[self.range_cache_name(None), "cursor_subscription".into()]);
                self.schedule_background_refresh();
            }
            "set-keep-usage" => {
                let value = one();
                if !["true", "false"].contains(&value.as_str()) {
                    return invalid("invalid keep usage setting");
                }
                if value == "false" && self.settings().get("keepCleanedUsage").is_some_and(Json::is_truthy) {
                    if !self.confirm(
                        "不再保留已清理记录的用量？",
                        "Claude Code 等工具会删掉旧记录（Claude Code 默认保留 30 天）。关闭后删除已保留的用量，“全部”和更早的日子只按还在的记录统计，删除后无法恢复。",
                        "关闭并删除",
                    ) {
                        return Ok(0);
                    }
                    archive::clear(Path::new(&self.state_dir));
                }
                self.update("keep-usage", Some(&value), None)?;
                self.schedule_background_refresh();
            }
            "retry-claude-usage" => {
                if !extra.is_empty() || !self.settings().get("claudeOAuthUsage").is_some_and(Json::is_truthy) {
                    return invalid("invalid Claude refresh action");
                }
                let moment = now();
                let pending = self.claude_retry_pending();
                if (0.0..5.0).contains(&(moment - pending)) {
                    return Ok(0);
                }
                self.write_json_cache(
                    &self.cache_path("claude-retry"),
                    &object! { "schema" => 1, "requestedAt" => moment }.into(),
                );
                self.invalidate_caches(&["claude_subscription"]);
                self.launch_background("--background-refresh-wait");
            }
            "set-claude-live" => {
                let value = one();
                flag(&value).map_err(|_| ActionError("invalid Claude usage preference".into()))?;
                if value == "true" && !self.settings().get("claudeLiveUsage").is_some_and(Json::is_truthy) {
                    let detail = "仅在点击“在线查询一次”时读取 Claude App 的钥匙串和 Cookie，向 https://claude.ai 查询订阅用量。平时只读取本机快照，不自动请求。\n凭据只在内存，不写入配置、日志或 GitHub；不查询余额，不调用模型。";
                    if !self.confirm("允许手动查询 Claude 用量？", detail, "允许读取") {
                        return Ok(0);
                    }
                }
                self.update("claude-live", Some(&value), None)?;
                if value == "false" {
                    self.invalidate_caches(&["claude_subscription_manual"]);
                }
                self.invalidate_caches(&["claude_subscription"]);
                self.schedule_background_refresh();
            }
            "set-claude-local" => {
                let value = one();
                if self.mac_app && value == "true" {
                    return invalid("standalone status-line setup unavailable");
                }
                flag(&value).map_err(|_| ActionError("invalid local Claude setting".into()))?;
                if value == "true" {
                    let detail = format!(
                        "在 {} 中连接官方状态栏回调。只保存订阅百分比、重置时间和更新时间，不读取 Cookie 或对话内容。已有状态栏会备份并继续执行，关闭时恢复。\n需等待 Claude Code 产生响应后才会有新数据。",
                        claude_bridge::settings_path(&self.environment)
                    );
                    if !self.confirm("启用本机用量回传？", &detail, "启用") {
                        return Ok(0);
                    }
                    claude_bridge::install(&self.state_dir, None, &self.environment)?;
                } else {
                    claude_bridge::uninstall(&self.state_dir);
                }
                self.invalidate_caches(&["claude_subscription"]);
                self.schedule_background_refresh();
            }
            "refresh-claude-online" => {
                if !extra.is_empty() {
                    return invalid("invalid Claude query");
                }
                // claude.ai answers only browsers it recognizes; this engine has no such client.
                self.dialog_error("Claude 在线查询未完成", "在线查询暂不可用，请改用 Claude Code 登录查询。");
            }
            "update-cli" => {
                let key = one();
                let Some(product) = update_products::product(&key).filter(|product| product.kind == Kind::Cli) else {
                    return invalid("invalid update target");
                };
                if !self.selected_update_ids().contains(&key) || !self.module_enabled("updates") {
                    return invalid("invalid update target");
                }
                let Some(binary) = self.product_binary(product) else {
                    return invalid("CLI not installed");
                };
                // The command its channel calls for: brew upgrade, npm install -g, or the CLI's own update.
                let channel = update_products::cli_channel(&key, &binary);
                let Some(run) = update_products::update_plan(&key, &binary, &channel, &self.environment) else {
                    self.dialog_error("这个安装方式没有更新命令", "请到官网下载新版本，或用安装它的工具更新。");
                    return Ok(0);
                };
                if let Some(desktop) = self.desktop() {
                    desktop.update_cli(&binary, &key, &run);
                    return Ok(0);
                }
                if run.len() == 2 && run[0] == binary && run[1] == "update" {
                    let Some(help) = shell::run(&[&binary, "--help"], 5.0, None, None) else {
                        return invalid("help failed");
                    };
                    let pattern = Regex::new(r"(?m)^\s+update(?:\s|\|)").expect("pattern");
                    if !pattern.is_match(&help) {
                        self.dialog_error("当前版本不支持 update 命令", "请通过官方安装方式更新。");
                        return Ok(0);
                    }
                }
                let refresh = (!self.mac_app)
                    .then(|| format!("swiftbar://refreshplugin?name={}", accounts::quote(&self.swiftbar_name())));
                let command = terminal_jobs::update_command(
                    &binary,
                    &key,
                    &run,
                    &self.state_dir,
                    refresh.as_deref(),
                    &self.local_zone(),
                )?;
                self.terminal(&command)?;
            }
            "open-cli-update-log" => {
                if !extra.is_empty() || !self.module_enabled("updates") {
                    return invalid("invalid update log action");
                }
                if let Some(path) = terminal_jobs::latest_log(&self.state_dir, &self.selected_update_ids()) {
                    self.open_local(&path, true)?;
                }
            }
            "reveal-source" => {
                let key = one();
                if !preferences::USAGE.contains(&key.as_str()) {
                    return invalid("invalid source");
                }
                if let Some(first) = self.source_locations(&key).into_iter().find(|path| Path::new(path).exists()) {
                    let target = if Path::new(&first).is_dir() {
                        first
                    } else {
                        Path::new(&first).parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or(first)
                    };
                    self.open_local(&target, false)?;
                }
            }
            "choose-source" => {
                let key = arg(0).unwrap_or("").to_string();
                if !preferences::USAGE.contains(&key.as_str()) {
                    return invalid("invalid source");
                }
                let prompt = if key == "claude" {
                    "选择 Claude App 数据目录（包含 claude-code-sessions 或 plan-usage-history.json）".to_string()
                } else {
                    format!("选择 {key} 的用量记录目录")
                };
                if let Some(chosen) = self.host.choose_path(&prompt, true).filter(|chosen| !chosen.is_empty()) {
                    self.update("path", Some(&key), Some(&chosen))?;
                    self.schedule_background_refresh();
                }
            }
            "export-usage" => {
                if extra.len() != 1 {
                    return invalid("invalid export");
                }
                self.export_usage(&extra[0])?;
            }
            "set-currency" => {
                if extra.len() != 1 {
                    return invalid("invalid currency");
                }
                self.update("currency", arg(0), None)?;
                if extra[0] == "CNY" {
                    self.schedule_background_refresh();
                }
            }
            "set-cny-rate" => {
                if !extra.is_empty() {
                    return invalid("invalid exchange rate");
                }
                let rate = self.settings().get("cnyRate").cloned();
                let current =
                    if rate.as_ref().is_some_and(Json::is_truthy) { python_g(rate.as_ref()) } else { String::new() };
                let Some(answer) =
                    self.text_dialog("1 美元合多少人民币？留空则每天按欧洲央行参考汇率自动更新。", &current, "汇率")
                else {
                    return Ok(0);
                };
                let value = answer.trim().trim_start_matches('¥').to_string();
                match self.update("cny-rate", Some(&value), None) {
                    Err(preferences::Error::Invalid(_)) => {
                        self.dialog_error("汇率无效", "请填写 1 到 20 之间的数字，或留空自动更新。");
                        return Ok(0);
                    }
                    other => other?,
                }
                self.schedule_background_refresh();
            }
            "set-service-status" => {
                if extra.len() != 1 {
                    return invalid("invalid service status setting");
                }
                self.update("service-status", arg(0), None)?;
                if extra[0] == "false" {
                    self.invalidate_caches(&["service_status"]);
                } else {
                    self.schedule_background_refresh();
                }
            }
            "set-session-alerts" => {
                if extra.len() != 1 {
                    return invalid("invalid session alert setting");
                }
                self.update("session-alerts", arg(0), None)?;
            }
            "refresh-live" => {
                if !extra.is_empty() {
                    return invalid("invalid live refresh");
                }
                self.refresh_live();
            }
            // The range menu opened: its ranges read ahead in the background (prefetch_ranges).
            "prefetch-ranges" => {
                if !extra.is_empty() {
                    return invalid("invalid prefetch");
                }
                self.launch_background("--prefetch-ranges");
            }
            "set-pin-interval" => {
                if extra.len() != 1 {
                    return invalid("invalid pin interval");
                }
                self.update("pin-interval", arg(0), None)?;
            }
            "set-menu-bar" => {
                let key = one();
                if !preferences::MENU_BAR.contains(&key.as_str()) {
                    return invalid("invalid menu bar value");
                }
                self.update("menu-bar", Some(&key), None)?;
                // Today is read every minute for the estimate: the first reading starts now.
                if key == "cost" {
                    self.schedule_background_refresh();
                }
            }
            "set-group" => {
                let key = one();
                if !preferences::GROUPINGS.contains(&key.as_str()) {
                    return invalid("invalid grouping");
                }
                self.update("group", Some(&key), None)?;
            }
            "set-range" => {
                let key = arg(0).unwrap_or("today").to_string();
                if !RANGES.iter().any(|(range, _)| *range == key) {
                    return invalid("invalid range");
                }
                match self.desktop() {
                    Some(desktop) => desktop.save_range(&key),
                    None => self.write_json_cache(
                        &self.cache_path("view-range"),
                        &object! { "schema" => 1, "range" => key.as_str() }.into(),
                    ),
                }
                if self.is_desktop()
                    || self.cache_due(&self.read_cache(&self.range_cache_name(Some(&key))), self.refresh_ttl(60.0))
                {
                    self.launch_background("--refresh-range");
                }
            }
            "refresh" => {
                let mut names = vec![self.range_cache_name(None)];
                let mut ttls: Vec<&str> = EXTERNAL_TTLS.iter().map(|(name, _)| *name).collect();
                ttls.sort();
                names.extend(ttls.into_iter().map(String::from));
                names.extend(self.named_key_loader_names());
                self.invalidate(&names);
                self.schedule_background_refresh();
            }
            "update-self" => {
                let cached = self
                    .read_cache("self_update")
                    .get("payload")
                    .and_then(Json::as_object)
                    .cloned()
                    .unwrap_or_default();
                let Some(release) = self.plugin_update(&object! { "self_update" => cached }) else {
                    self.notify(&format!("Starmeter 已是最新版本 {VERSION}"), "");
                    return Ok(0);
                };
                if self.self_update_state().get("status") == Some(&Json::from("running")) {
                    return Ok(0);
                }
                let latest = py::str(release.get("latest"));
                // The standalone app (and the Windows and Linux ones): its own package, put in place of itself.
                if self.mac_app || self.is_desktop() {
                    let assets = release.get("assets").and_then(Json::as_array).cloned().unwrap_or_default();
                    if self.app_package(&latest, &assets).is_none() {
                        return invalid("this copy can't be updated in place");
                    }
                    if self.confirm(
                        &format!("更新 Starmeter 到 {latest}？"),
                        &format!(
                            "当前版本 {VERSION}。将从 GitHub 下载这一版的安装包，校验 SHA-256 后替换本机的 Starmeter，并自动重新打开；设置、Key 和统计记录都会保留。"
                        ),
                        "更新",
                    ) {
                        self.write_self_update_state(object! { "status" => "running", "version" => latest.as_str() });
                        if !self.desktop().is_some_and(|desktop| desktop.update_app(&latest)) {
                            self.write_self_update_state(
                                object! { "status" => "failed", "version" => latest.as_str(), "reason" => "这个版本不能在应用内更新" },
                            );
                        }
                    }
                    return Ok(0);
                }
                if self.confirm(
                    &format!("更新 Starmeter 到 {latest}？"),
                    &format!("当前版本 {VERSION}。将从 GitHub 下载该版本的安装包，校验 SHA-256 后安装；原文件先备份，设置、Key 和统计记录都会保留。"),
                    "更新",
                ) {
                    self.write_self_update_state(object! { "status" => "running", "version" => latest.as_str() });
                    shell::detach(&["/bin/bash", "-p", &self.plugin, "--self-update", &latest], None);
                }
            }
            "check-app-updates" => {
                self.invalidate_caches(&["app_updates", "badge", "self_update"]);
                self.schedule_background_refresh();
            }
            "refresh-monitor" => {
                self.invalidate_caches(&["komari"]);
                self.schedule_background_refresh();
            }
            "restart" => {
                if self.confirm(
                    "重新启动 opencodex 代理？",
                    "正在进行的请求会被中断，代理会自动恢复到就绪状态。",
                    "确定",
                ) {
                    self.cli_action(&["restart"], "代理已重新启动");
                }
            }
            "stop" => {
                if self.confirm(
                    "停止 opencodex 代理？",
                    "代理会停止，并同时把 ChatGPT 编程客户端还原为原生配置。\n需要时点「启动代理」可以重新开启。",
                    "确定",
                ) {
                    self.cli_action(&["stop"], "代理已停止");
                }
            }
            "start" => {
                self.notify("正在启动 opencodex…", "");
                let failure = self.ocx(&["ensure"], 240.0).or_else(|| self.ocx(&["start"], 240.0));
                self.notify(
                    &failure.map_or_else(|| "代理已启动".to_string(), |failure| format!("启动失败：{failure}")),
                    "",
                );
            }
            "sync" => {
                self.notify("正在同步模型到 ChatGPT…", "");
                self.api_action("/api/sync", "POST", Json::Object(Object::new()), "模型已同步到 ChatGPT", "操作失败");
            }
            "repair" => {
                if self.confirm("修复自启动服务？", "会重新注册后台服务，可能需要输入管理员密码。", "确定")
                {
                    self.cli_action(&["service", "repair"], "自启动服务已修复");
                }
            }
            "status" => self.terminal("ocx status")?,
            "update" => {
                let tag = arg(0).unwrap_or("latest").to_string();
                if self.confirm(
                    &format!("更新 opencodex（通道 {tag}）？"),
                    "更新需要几分钟，完成后会自动重启代理。",
                    "确定",
                ) {
                    let body = Json::from(object! { "tag" => tag.as_str(), "restart" => true });
                    let (_, failure) = self.opencodex_request("/api/update/run", "POST", Some(&body), 30.0, true);
                    platform::remove(Path::new(&self.cache_path("badge")));
                    match failure {
                        Some(failure) if failure.message.contains("already_latest") => {
                            self.notify("已是最新版本", "OpenCodex 无需更新");
                        }
                        Some(failure) => {
                            let reason =
                                if failure.message.is_empty() { failure.code.to_string() } else { failure.message };
                            self.notify(&format!("操作失败：{reason}"), "/api/update/run");
                        }
                        None => self.notify("更新任务已启动", "/api/update/run"),
                    }
                }
            }
            "check-update" => {
                let (payload, failure) = self.opencodex_request("/api/update/check", "GET", None, 30.0, true);
                match failure {
                    Some(failure) => {
                        let reason =
                            if failure.message.is_empty() { failure.code.to_string() } else { failure.message };
                        self.notify(&format!("检查更新失败：{reason}"), "");
                    }
                    None => {
                        self.write_json_cache(
                            &self.cache_path("badge"),
                            &object! { "schema" => 1, "payload" => payload.clone(), "fetchedAt" => now() }.into(),
                        );
                        let payload = payload.unwrap_or_default();
                        if payload.get("updateAvailable").is_some_and(Json::is_truthy) {
                            self.notify(
                                &format!("发现新版本 {}", py::str(payload.get("latestVersion"))),
                                &format!("当前 {}", py::str(payload.get("currentVersion"))),
                            );
                        } else {
                            let current = py::str(Some(&py::or(payload.get("currentVersion"), Json::from(""))));
                            self.notify(&format!("已是最新版本 {current}"), "");
                        }
                    }
                }
            }
            "account-use" => {
                let id = arg(0).unwrap_or("").to_string();
                if self.confirm(&format!("切换到账号 {id}？"), "之后的请求会使用这个账号。", "确定")
                {
                    self.api_action(
                        "/api/codex-auth/active",
                        "PUT",
                        object! { "accountId" => id.as_str() }.into(),
                        "已切换激活账号",
                        "切换失败",
                    );
                }
            }
            "account-pause" => {
                let id = arg(0).unwrap_or("").to_string();
                let paused = arg(1).unwrap_or("true") == "true";
                self.api_action(
                    "/api/codex-auth/accounts/pause",
                    "PUT",
                    object! { "id" => id.as_str(), "paused" => paused }.into(),
                    if paused { "账号已暂停" } else { "账号已启用" },
                    "操作失败",
                );
            }
            "pool-strategy" => {
                let strategy = arg(0).unwrap_or("").to_string();
                let label = match strategy.as_str() {
                    "quota" => "按额度",
                    "round-robin" => "轮询",
                    "fill-first" => "用完再换",
                    other => other,
                };
                self.api_action(
                    "/api/codex-auth/pool-strategy",
                    "PUT",
                    object! { "strategy" => strategy.as_str() }.into(),
                    &format!("调度策略已改为「{label}」"),
                    "切换失败",
                );
            }
            "auto-switch" => {
                let threshold = arg(0).unwrap_or("").to_string();
                let value: i64 = if threshold.is_empty() {
                    80
                } else {
                    threshold.parse().map_err(|_| ActionError("invalid threshold".into()))?
                };
                self.api_action(
                    "/api/codex-auth/auto-switch",
                    "PUT",
                    object! { "threshold" => value }.into(),
                    &format!("额度超过 {threshold}% 时自动切换账号"),
                    "设置失败",
                );
            }
            "failover" => {
                let threshold = arg(0).unwrap_or("").to_string();
                let value: i64 = if threshold.is_empty() {
                    1
                } else {
                    threshold.parse().map_err(|_| ActionError("invalid threshold".into()))?
                };
                let label = if threshold != "0" { threshold.as_str() } else { "不自动换" };
                self.api_action(
                    "/api/codex-auth/failover",
                    "PUT",
                    object! { "threshold" => value }.into(),
                    &format!("失败换账号阈值已设为 {label}"),
                    "设置失败",
                );
            }
            "copy-url" => {
                self.host.copy(&self.base);
                self.notify("已复制管理地址", "");
            }
            "copy-token" => match self.admin_token() {
                Some(token) => {
                    self.host.copy(&token);
                    self.notify("已复制管理令牌", "");
                }
                None => self.notify("没有找到管理令牌", &self.token_file),
            },
            "open-home" => self.open_local(&self.ocx_home, false)?,
            "logs" => {
                let log = format!("{}/service.log", self.ocx_home);
                if self.is_desktop() {
                    self.open_local(&log, true)?;
                    return Ok(0);
                }
                self.terminal(&format!("tail -n 100 -f {}", connections::shell_quote(&log)))?;
            }
            _ => {}
        }
        Ok(0)
    }

    /// The panel queues an action in a file; it runs here, through the plugin's own permissions, only if the
    /// rendered panel offered it. A receipt tells the panel how it went.
    pub fn consume_panel_action(&self) {
        let pending = self.cache_path("panel-action");
        let claimed = format!("{pending}.processing-{}", platform::process_id());
        if std::fs::rename(&pending, &claimed).is_err() {
            return;
        }
        let mut request_id: Option<String> = None;
        let mut action: Option<Vec<String>> = None;
        let mut status = "rejected";
        let mut reason = Json::Null;
        (|| {
            let Some(record) = std::fs::read(&claimed)
                .ok()
                .and_then(|data| Json::parse(&data).ok())
                .and_then(|v| v.as_object().cloned())
            else {
                status = "failed";
                reason = Json::from("action-failed");
                return;
            };
            request_id = record.get("id").and_then(Json::as_str).map(String::from);
            let (Some(id), Some(values)) = (request_id.as_ref(), record.get("action").and_then(Json::as_array)) else {
                return;
            };
            if id.chars().count() > 64 {
                return;
            }
            let created = match record.get("createdAt") {
                None => Some(0.0),
                Some(value) => value.number().or_else(|| value.as_str().and_then(parse_python_float)),
            };
            let Some(created) = created else {
                status = "failed";
                reason = Json::from("action-failed");
                return;
            };
            let age = now() - created;
            let strings: Vec<String> = values.iter().filter_map(|value| value.as_str().map(String::from)).collect();
            if values.is_empty() || strings.len() != values.len() || !(0.0..=60.0).contains(&age) {
                return;
            }
            action = Some(strings.clone());
            // Only an action already present in the rendered panel can be requested.
            let snapshot = self.read_cache("panel-view");
            let list = |key: &str| snapshot.get(key).and_then(Json::as_array).cloned().unwrap_or_default();
            let mut nodes: Vec<Json> = [list("entries"), list("rangeOptions"), list("moduleOptions")].concat();
            let mut allowed = always_allowed();
            let dashboard = snapshot.get("dashboard").and_then(Json::as_object).cloned().unwrap_or_default();
            if dashboard.get("allowCollapse").is_some_and(Json::is_truthy) {
                for provider in dashboard.get("providers").and_then(Json::as_array).into_iter().flatten() {
                    let Some(id) = provider.get("id").and_then(Json::as_str) else { continue };
                    if !preferences::USAGE.contains(&id) {
                        continue;
                    }
                    for value in ["true", "false"] {
                        allowed.push(vec!["set-expanded".into(), id.to_string(), value.into()]);
                    }
                }
            }
            while let Some(node) = nodes.pop() {
                let fields = node.as_object().cloned().unwrap_or_default();
                if let Some(open) = fields.get("onOpenAction").and_then(Json::as_array).filter(|open| !open.is_empty())
                {
                    allowed.push(open.iter().map(|part| py::str(Some(part))).collect());
                }
                if let Some(candidate) = fields.get("action").and_then(Json::as_array) {
                    let candidate: Vec<String> =
                        candidate.iter().filter_map(|part| part.as_str().map(String::from)).collect();
                    if !candidate.is_empty() {
                        // A persistent picker may queue another click before the previous refresh.
                        if candidate.len() == 2 && TOGGLES.contains(&candidate[0].as_str()) {
                            for value in ["true", "false"] {
                                allowed.push(vec![candidate[0].clone(), value.into()]);
                            }
                        }
                        if candidate.len() == 3 && candidate[0] == "set-module" && preferences::is_known(&candidate[1])
                        {
                            for value in ["true", "false"] {
                                allowed.push(vec!["set-module".into(), candidate[1].clone(), value.into()]);
                            }
                        }
                        allowed.push(candidate);
                    }
                }
                nodes.extend(fields.get("children").and_then(Json::as_array).cloned().unwrap_or_default());
            }
            // A homepage row dragged before or after another: any two of the rows the panel shows.
            let rows: Vec<String> = list("entries")
                .iter()
                .filter_map(|entry| entry.get("module").and_then(Json::as_str).map(String::from))
                .collect();
            let moved = strings.len() == 4
                && strings[0] == "move-module"
                && ["before", "after"].contains(&strings[2].as_str())
                && rows.contains(&strings[1])
                && rows.contains(&strings[3]);
            if !moved && !allowed.contains(&strings) {
                return;
            }
            self.write_json_cache(
                &self.cache_path("last-action"),
                &object! { "action" => strings[0].as_str(), "receivedAt" => now() }.into(),
            );
            // Outside a render: an action reads the settings it has just changed.
            match self.run_action(&strings[0], &strings[1..]) {
                Ok(_) => status = "done",
                Err(_) => {
                    status = "failed";
                    reason = Json::from("action-failed");
                }
            }
        })();
        platform::remove(Path::new(&claimed));
        if let Some(id) = request_id {
            let receipt =
                status == "done" && action.as_ref().is_some_and(|action| RECEIPT_ACTIONS.contains(&action[0].as_str()));
            let preferences = if receipt { self.presentation_settings() } else { Json::Null };
            self.write_json_cache(
                &self.cache_path("panel-action-receipt"),
                &object! {
                    "schema" => 1, "id" => id, "status" => status, "reason" => reason, "finishedAt" => now(),
                    "preferences" => preferences,
                }
                .into(),
            );
        }
    }
}

#[cfg(test)]
mod run_tests {
    use super::*;

    // The commands are shell scripts: Windows has no /bin/sh.
    #[cfg(unix)]
    #[test]
    fn a_command_writing_much_to_its_error_stream_finishes_with_everything_it_wrote() {
        // OCX-1: a megabyte of diagnostics on the standard error before the standard output closes used to wait on the
        // full pipe until the timeout.
        let script = "head -c 1048576 /dev/zero | tr '\\0' x >&2; echo >&2; echo done; echo 'last words' >&2; exit 3";
        let started = std::time::Instant::now();
        let (status, data) = run_collecting("/bin/sh", &["-c", script], 20.0).expect("finished");
        assert!(started.elapsed().as_secs_f64() < 10.0, "took {:?}", started.elapsed());
        assert_eq!(status.code(), Some(3));
        let text = String::from_utf8_lossy(&data);
        assert!(text.starts_with("done\n"));
        assert_eq!(text.lines().rfind(|line| !line.trim().is_empty()), Some("last words"));
        assert_eq!(data.len(), "done\n".len() + 1048576 + 1 + "last words\n".len());
    }

    #[test]
    fn a_command_past_its_time_is_stopped_and_one_not_found_says_so() {
        #[cfg(unix)]
        assert_eq!(run_collecting("/bin/sh", &["-c", "sleep 30"], 0.5).unwrap_err(), Ran::TimedOut);
        assert_eq!(run_collecting("/nonexistent/ocx", &[], 1.0).unwrap_err(), Ran::NotStarted);
    }
}
