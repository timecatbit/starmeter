//! Everything the panel shows, in the file the panel reads: entries parsed back from the menu rows, the module
//! options with each module's connection page, the range options and the dashboard.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use indexmap::IndexMap;
use regex::Regex;

use crate::background::RESOURCE_CACHES;
use crate::claude_subscription::{self, SNAPSHOT_FILE};
use crate::client_accounts::qoder;
use crate::dashboard::{Selected, summary_cost_value, summary_usage};
use crate::files::paths;
use crate::json::{Json, Object};
use crate::more_rows::update_attention_count;
use crate::opencodex::ENDPOINTS;
use crate::rows::FALLBACK_SEPARATOR;
use crate::runtime::{self, PRODUCT, RANGES, Runtime, now};
use crate::{catalog, claude_app, cursor, object, preferences, py};

pub const MENU_BAR_LABELS: [(&str, &str); 3] =
    [("none", "只显示星星"), ("cost", "今天的 API 估算"), ("quota", "最紧张的订阅额度")];

/// The file browser's name where the user will look for it.
pub const REVEAL_LABEL: &str = if cfg!(windows) { "在资源管理器中打开" } else { "在 Finder 中打开" };

fn action(parts: &[&str]) -> Json {
    Json::Array(parts.iter().map(|part| Json::from(*part)).collect())
}

fn leaf(label: &str) -> Json {
    object! { "label" => label, "children" => Json::Array(vec![]) }.into()
}

fn separator() -> Json {
    object! { "label" => "", "separator" => true, "children" => Json::Array(vec![]) }.into()
}

fn location_entry(label: &str, paths: &[String]) -> Json {
    object! { "label" => label, "detail" => paths.join("\n"), "children" => Json::Array(vec![]) }.into()
}

fn with(label: &str, fields: Object) -> Json {
    let mut entry = object! { "label" => label };
    for (key, value) in fields.iter() {
        entry.insert(key.clone(), value.clone());
    }
    if !entry.contains_key("children") {
        entry.insert("children", Json::Array(vec![]));
    }
    entry.into()
}

/// shlex.split in POSIX mode, for the options of one row: spaces separate, quotes group, a backslash escapes.
pub fn shell_split(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_word = false;
    let mut quote: Option<char> = None;
    let mut characters = text.chars().peekable();
    while let Some(c) = characters.next() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            } else if c == '\\' && q == '"' && matches!(characters.peek(), Some('"' | '\\')) {
                current.push(characters.next().unwrap_or_default());
            } else {
                current.push(c);
            }
        } else if c == '"' || c == '\'' {
            quote = Some(c);
            in_word = true;
        } else if c == '\\' {
            if let Some(next) = characters.next() {
                current.push(next);
            }
            in_word = true;
        } else if matches!(c, ' ' | '\t' | '\n' | '\r') {
            if in_word {
                words.push(std::mem::take(&mut current));
                in_word = false;
            }
        } else {
            current.push(c);
            in_word = true;
        }
    }
    if in_word {
        words.push(current);
    }
    words
}

/// The panel's entries from menu rows: labels, nesting, options and actions.
pub fn panel_entries(lines: &[String]) -> Vec<Json> {
    struct Node {
        entry: Object,
        children: Vec<usize>,
    }
    let mut nodes: Vec<Node> = Vec::new();
    let mut roots: Vec<usize> = Vec::new();
    let mut parents: HashMap<usize, usize> = HashMap::new();
    let spaces = Regex::new(r"\s{2,}").expect("pattern");
    for raw in lines.iter().filter(|line| !line.is_empty()) {
        let mut level = 0;
        let entry = if raw.chars().all(|c| c == '-') {
            level = (raw.chars().count().saturating_sub(3)) / 2;
            object! { "label" => "", "separator" => true }
        } else {
            let mut line = raw.as_str();
            while let Some(rest) = line.strip_prefix("--") {
                level += 1;
                line = rest;
            }
            let (label, options) = match line.split_once(" | ") {
                Some((label, options)) => (label, options),
                None => (line, ""),
            };
            let mut attrs: HashMap<String, String> = HashMap::new();
            for part in shell_split(options) {
                if let Some((key, value)) = part.split_once('=') {
                    attrs.insert(key.to_string(), value.to_string());
                }
            }
            let attr = |key: &str| attrs.get(key).filter(|value| !value.is_empty()).cloned();
            let mut entry = object! { "label" => label.trim() };
            if let Some(image) = attr("templateimage").or_else(|| attr("image")) {
                entry.insert("image", image);
                entry.insert("template", attr("templateimage").is_some());
            } else if let Some(symbol) = attr("sfimage") {
                entry.insert("symbol", symbol);
            }
            if attrs.get("panelsection").map(String::as_str) == Some("true") {
                entry.insert("section", true);
            }
            if attrs.get("panelattention").map(String::as_str) == Some("true") {
                entry.insert("attention", true);
            }
            if let Some(level) =
                attrs.get("panelusagelevel").filter(|l| ["normal", "warning", "critical"].contains(&l.as_str()))
            {
                entry.insert("usageLevel", level.as_str());
            }
            if let Some(light) =
                attrs.get("panelstatus").filter(|l| ["new", "ok", "issue", "unknown"].contains(&l.as_str()))
            {
                entry.insert("statusLight", light.as_str());
            }
            if let Some(radio) = attrs.get("panelradio").filter(|r| *r == "on" || *r == "off") {
                entry.insert("radio", true);
                entry.insert("checked", radio == "on");
            }
            if let Some(badge) = attr("badge") {
                entry.insert("value", badge);
            }
            if let Some(fallback) = attr("panelfallback") {
                entry.insert("fallbacks", Json::Array(fallback.split(FALLBACK_SEPARATOR).map(Json::from).collect()));
            }
            if let Some(detail) = attr("paneldetail") {
                entry.insert("detail", detail);
            }
            if let Some(href) = attr("href") {
                entry.insert("href", href);
            }
            if attrs.get("param2").map(String::as_str) == Some("--action") && attr("param3").is_some() {
                let parts: Vec<Json> = (3..20)
                    .filter_map(|index| attrs.get(&format!("param{index}")))
                    .map(|v| Json::from(v.as_str()))
                    .collect();
                entry.insert("action", Json::Array(parts));
            }
            if level > 0
                && label.chars().next().is_some_and(|first| "●○?".contains(first))
                && let Some(tooltip) = attr("tooltip")
            {
                // The panel draws the status light; the text mark is only for SwiftBar's own menu.
                let rest: String = label.chars().skip(1).collect();
                let text = spaces.replace_all(rest.trim(), " · ").into_owned();
                entry.insert("label", text.split(" · ").next().unwrap_or(&text).to_string());
                entry.insert("detail", tooltip);
            }
            entry
        };
        let index = nodes.len();
        nodes.push(Node { entry, children: Vec::new() });
        match parents.get(&(level.wrapping_sub(1))).copied().filter(|_| level > 0) {
            Some(parent) => nodes[parent].children.push(index),
            None => roots.push(index),
        }
        parents.insert(level, index);
        parents.retain(|key, _| *key <= level);
    }
    fn build(nodes: &[Node], index: usize) -> Json {
        let node = &nodes[index];
        let children: Vec<Json> = node.children.iter().map(|child| build(nodes, *child)).collect();
        // "children" sits where Python's dict put it: after label (and separator).
        let mut ordered = Object::new();
        ordered.insert("label", node.entry.get("label").cloned().unwrap_or(Json::Null));
        if let Some(separator) = node.entry.get("separator") {
            ordered.insert("separator", separator.clone());
        }
        ordered.insert("children", Json::Array(children));
        for (key, value) in node.entry.iter() {
            if !ordered.contains_key(key) {
                ordered.insert(key.clone(), value.clone());
            }
        }
        ordered.into()
    }
    roots.iter().map(|root| build(&nodes, *root)).collect()
}

/// format(n, ','): thousands separated by commas.
pub fn grouped(value: i64) -> String {
    let digits = value.unsigned_abs().to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    if value < 0 { format!("-{out}") } else { out }
}

fn without_new_bridge(entries: &[Json]) -> Vec<Json> {
    entries
        .iter()
        .filter(|entry| entry.get("action") != Some(&action(&["set-claude-local", "true"])))
        .map(|entry| {
            let mut fields = entry.as_object().cloned().unwrap_or_default();
            let children = fields.get("children").and_then(Json::as_array).cloned().unwrap_or_default();
            fields.insert("children", Json::Array(without_new_bridge(&children)));
            fields.into()
        })
        .collect()
}

impl Runtime {
    pub fn resource_panel_entries(&self, data: &Object) -> Vec<Json> {
        let entries = panel_entries(&self.management_lines(data));
        let services: HashMap<String, &catalog::Service> =
            catalog::SERVICES.iter().map(|service| (service.label(), service)).collect();
        let loaders: Vec<String> = self.external_loaders().into_iter().map(|(name, _)| name).collect();
        let mut groups: IndexMap<&str, Vec<Json>> = IndexMap::new();
        for name in ["订阅", "API", "其他"] {
            groups.insert(name, Vec::new());
        }
        for entry in entries {
            let Some(mut fields) = entry.as_object().cloned() else { continue };
            if fields.contains_key("separator") {
                continue;
            }
            let label = py::str(fields.get("label"));
            let service = services.get(&label).copied();
            let source = service.map(|service| service.key.to_string()).or_else(|| {
                if label.starts_with("Komari 监控") {
                    Some("komari".into())
                } else if label == "工具更新" {
                    Some("updates".into())
                } else if label == "会话" {
                    Some("sessions".into())
                } else if label.starts_with("OpenCodex 管理") {
                    Some("opencodex".into())
                } else {
                    None
                }
            });
            if fields.get("children").and_then(Json::as_array).is_some_and(|children| !children.is_empty())
                && let Some(source) = &source
                && !self.resource_cache_names(source, &loaders).is_empty()
            {
                fields.insert("onOpenAction", action(&["refresh-source", source]));
            }
            // Refresh sits beside Summary; SwiftBar's own submenu covers its housekeeping.
            if label == "刷新数据" {
                continue;
            }
            let group = service.map_or("其他", |service| service.category);
            groups.entry(group).or_default().push(fields.into());
        }
        // Groups are separated by a line; their names are obvious from the rows themselves.
        let mut result: Vec<Json> = Vec::new();
        for (_, items) in groups {
            if items.is_empty() {
                continue;
            }
            if !result.is_empty() {
                result.push(separator());
            }
            result.extend(items);
        }
        result
    }

    pub fn resource_cache_names(&self, source: &str, loaders: &[String]) -> Vec<String> {
        let has = |name: &str| loaders.iter().any(|loader| loader == name);
        if source == "opencodex" {
            return if has("opencodex") {
                ENDPOINTS.iter().map(|(name, _, _)| name.to_string()).collect()
            } else {
                vec![]
            };
        }
        if source == "chatgpt" && self.chatgpt_source() == "proxy" {
            return if has("opencodex") {
                ["healthz", "quota", "accounts", "active"].iter().map(|name| name.to_string()).collect()
            } else {
                vec![]
            };
        }
        if source == "updates" {
            return ["app_updates", "badge"].iter().filter(|name| has(name)).map(|name| name.to_string()).collect();
        }
        let Some(name) = RESOURCE_CACHES.iter().find(|pair| pair.0 == source).map(|pair| pair.1) else { return vec![] };
        loaders.iter().filter(|loader| *loader == name || loader.starts_with(&format!("{source}@"))).cloned().collect()
    }

    // Module options and their connection pages

    pub fn source_locations(&self, key: &str) -> Vec<String> {
        let root = self.source_path(key);
        let is_dir = |path: &str| Path::new(path).is_dir();
        if key == "claude" {
            let mut roots = vec![root];
            roots.extend(claude_app::project_roots(&self.home, &self.environment));
            return roots;
        }
        if key == "workbuddy" {
            let roots: Vec<String> = [".workbuddy-ai", ".workbuddy"]
                .iter()
                .map(|folder| format!("{root}/{folder}/projects"))
                .filter(|path| is_dir(path))
                .collect();
            if !roots.is_empty() {
                return roots;
            }
            if root == self.home {
                return vec![format!("{}/.workbuddy-ai/projects", self.home)];
            }
            if is_dir(&format!("{root}/projects")) {
                return vec![format!("{root}/projects")];
            }
        }
        if key == "codex" {
            let roots: Vec<String> = ["sessions", "archived_sessions"]
                .iter()
                .map(|folder| format!("{root}/{folder}"))
                .filter(|p| is_dir(p))
                .collect();
            if !roots.is_empty() {
                return roots;
            }
        }
        if key == "gemini" && is_dir(&format!("{root}/tmp")) {
            return vec![format!("{root}/tmp")];
        }
        if key == "qoder" && is_dir(&format!("{root}/projects")) {
            return vec![format!("{root}/projects")];
        }
        if root.is_empty() { vec![] } else { vec![runtime::absolute(&root)] }
    }

    fn client_connection_entry(&self, key: &str) -> Json {
        let proxy = key == "opencodex";
        let configured = if proxy { self.opencodex_configured() } else { self.connection_ready("chatgpt") };
        let root = if proxy { self.ocx_home.clone() } else { self.codex_home_absolute() };
        let mut children: Vec<Json> = Vec::new();
        if !proxy {
            let source = if self.chatgpt_source() == "proxy" { "OpenCodex 账户" } else { "本机 ChatGPT 登录" };
            children.push(leaf(&format!("当前来源 · {source}")));
            children.push(leaf("勾选 OpenCodex 后改用 OpenCodex 账户"));
            if self.chatgpt_source() == "proxy" {
                children.push(location_entry("账户管理地址", std::slice::from_ref(&self.base)));
            }
            children.push(separator());
        }
        children.push(location_entry(if proxy { "配置目录" } else { "本机登录目录" }, std::slice::from_ref(&root)));
        if proxy {
            children.push(location_entry("管理地址", std::slice::from_ref(&self.base)));
            children.push(location_entry("管理令牌文件", std::slice::from_ref(&self.token_file)));
        }
        children.push(with(
            "选择配置目录…",
            object! { "symbol" => "folder", "action" => action(&["choose-client-home", key]) },
        ));
        if proxy {
            children.push(with("修改管理地址…", object! { "action" => action(&["set-proxy-address"]) }));
            children.push(with("选择管理令牌文件…", object! { "action" => action(&["choose-admin-token"]) }));
        }
        children.push(with("恢复自动读取", object! { "action" => action(&["reset-client-connection", key]) }));
        if Path::new(&root).is_dir() {
            children.push(with(
                REVEAL_LABEL,
                object! { "symbol" => "folder", "action" => action(&["reveal-client-home", key]) },
            ));
        }
        object! {
            "label" => if proxy { "OpenCodex" } else { "ChatGPT 订阅" }, "value" => if configured { "已配置" } else { "未配置" },
            "symbol" => if proxy { "network" } else { "person.crop.circle" }, "children" => Json::Array(children),
        }
        .into()
    }

    fn cursor_connection_entry(&self) -> Json {
        let enabled = self.settings().get("cursorUsage").is_some_and(Json::is_truthy);
        let mut children = vec![
            with(
                "读取 Cursor 用量记录",
                object! {
                    "detail" => "用本机 Cursor 登录查询 cursor.com，令牌只在内存中使用", "checked" => enabled,
                    "action" => action(&["set-cursor-usage", if enabled { "false" } else { "true" }]),
                },
            ),
            leaf("来源 · Cursor 桌面版与 Cursor CLI"),
            leaf("两者同一账户，用量都在账户记录里"),
            location_entry(
                "登录来源",
                &[
                    format!("{}/state.vscdb", self.source_path("cursor")),
                    cursor::cli_auth(&self.home, &self.environment),
                ],
            ),
        ];
        if let Some(app) = self.update_app_path("cursor") {
            children.push(with("打开 Cursor", object! { "symbol" => "macwindow", "href" => self.file_href(&app) }));
        }
        object! {
            "label" => "Cursor", "value" => if enabled { "已配置" } else { "未配置" }, "symbol" => "key",
            "children" => Json::Array(children),
        }
        .into()
    }

    fn service_connection_entry(&self, key: &str, data: &Object) -> Json {
        let service = catalog::by_key(key);
        let configured = self.connection_ready(key);
        let value = if configured { "已配置" } else { "未配置" };
        let note = catalog::SETUP_NOTES.iter().find(|pair| pair.0 == key).map(|pair| pair.1);
        if let Some(service) = service.filter(|service| service.category == "API") {
            // The same key list as the homepage, plus pointing the classic connection at a key file.
            let mut children = panel_entries(&self.api_lines(key, data))
                .first()
                .and_then(|entry| entry.get("children"))
                .and_then(Json::as_array)
                .cloned()
                .unwrap_or_default();
            let at = children.len().saturating_sub(1);
            children.insert(at, with("选择密钥文件…", object! { "action" => action(&["configure-connection", key]) }));
            if !configured && let Some(note) = note {
                children.insert(0, leaf(note));
            }
            return object! { "label" => service.label(), "value" => value, "symbol" => "key", "children" => Json::Array(children) }
                .into();
        }
        let settings = self.settings();
        if key == "claude-subscription"
            && let Some(service) = service
        {
            let oauth = settings.get("claudeOAuthUsage").is_some_and(Json::is_truthy);
            let local = claude_subscription::local_enabled(&self.state_dir);
            let oauth_row = with(
                "Claude Code 登录查询",
                object! { "checked" => oauth, "action" => action(&["set-claude-oauth", if oauth { "false" } else { "true" }]) },
            );
            let app_snapshot =
                location_entry("App 快照文件", &[format!("{}/plan-usage-history.json", self.source_path("claude"))]);
            let children = if cfg!(windows) {
                vec![
                    oauth_row,
                    location_entry("Code 登录目录", &[self.claude_config_root()]),
                    app_snapshot,
                    with("选择 Claude 数据目录…", object! { "action" => action(&["choose-source", "claude"]) }),
                    with("打开订阅用量", object! { "href" => service.url }),
                ]
            } else {
                let mut children = vec![
                    oauth_row,
                    location_entry("Code 登录来源", &[self.claude_config_root()]),
                    with(
                        if local { "关闭本机回传" } else { "启用本机用量回传…" },
                        object! { "action" => action(&["set-claude-local", if local { "false" } else { "true" }]) },
                    ),
                    if local {
                        location_entry("本机快照", &[format!("{}/{SNAPSHOT_FILE}", self.state_dir)])
                    } else {
                        app_snapshot
                    },
                ];
                if settings.get("claudeLiveUsage").is_some_and(Json::is_truthy) {
                    children
                        .push(with("清除在线查询授权", object! { "action" => action(&["set-claude-live", "false"]) }));
                }
                children
                    .push(with("选择 Claude 数据目录…", object! { "action" => action(&["choose-source", "claude"]) }));
                children.push(with("打开 Claude", object! { "action" => action(&["open-claude"]) }));
                children.push(with("打开订阅用量", object! { "href" => service.url }));
                children
            };
            return object! {
                "label" => service.label(), "value" => value, "symbol" => "person.crop.circle", "children" => Json::Array(children),
            }
            .into();
        }
        let url = service.map_or("", |service| service.url);
        let mut children: Vec<Json>;
        if catalog::manual_subscriptions().contains(&key) {
            children = vec![leaf("手动标记，未验证套餐")];
            if configured {
                children.push(location_entry("本机标记文件", &[format!("{}/preferences.json", self.state_dir)]));
            }
            children.push(with(
                if configured { "清除配置标记" } else { "标记为已配置" },
                object! { "action" => action(&["set-subscription", key, if configured { "false" } else { "true" }]) },
            ));
            children.push(with("打开订阅管理", object! { "symbol" => "arrow.up.right", "href" => url }));
        } else if key == "minimax" {
            let file = self.environment.get("MINIMAX_API_KEY_FILE").cloned().unwrap_or_default();
            children = if configured && !file.is_empty() {
                vec![location_entry("密钥文件", &[runtime::absolute(&paths::expand_user(&file).to_string_lossy())])]
            } else {
                vec![]
            };
            if !configured && let Some(note) = note {
                children.push(leaf(note));
            }
            children.push(with("选择密钥文件…", object! { "action" => action(&["configure-connection", key]) }));
            if configured {
                children.push(with("清除连接设置…", object! { "action" => action(&["clear-connection", key]) }));
            }
            children.push(with("打开用量与账单", object! { "symbol" => "arrow.up.right", "href" => url }));
        } else if key == "qoder-subscription" {
            // Read from the Qoder CN app's own log, nothing to set up: where it reads, and how to bring it up to date.
            let roots = qoder::log_roots(&self.home, &self.environment);
            let found: Vec<String> = roots.iter().filter(|root| Path::new(root).is_dir()).cloned().collect();
            children = vec![
                leaf("来源 · Qoder 桌面 App 的本机日志"),
                leaf("在 Qoder 窗口右下角打开 Credits 用量后更新"),
                location_entry("日志目录", if found.is_empty() { &roots[..1] } else { &found }),
            ];
            if let Some(app) = self.update_app_path("qoder") {
                children.push(with("打开 Qoder", object! { "symbol" => "macwindow", "href" => self.file_href(&app) }));
            }
            children.push(with("打开订阅用量", object! { "symbol" => "arrow.up.right", "href" => url }));
        } else {
            children =
                if configured { vec![location_entry("面板地址", &[self.komari_base_url()])] } else { vec![] };
            children.push(with("设置面板地址…", object! { "action" => action(&["configure-connection", key]) }));
            if configured {
                children.push(with("清除连接设置…", object! { "action" => action(&["clear-connection", key]) }));
            }
        }
        let symbol = if key == "komari" {
            "server.rack"
        } else if service.is_some_and(|service| service.category == "订阅") {
            "person.crop.circle"
        } else {
            "key"
        };
        let label = service.map(|service| service.label()).unwrap_or_else(|| "Komari 监控".into());
        object! { "label" => label, "value" => value, "symbol" => symbol, "children" => Json::Array(children) }.into()
    }

    /// Claude Code's configuration folder: the first of CLAUDE_CONFIG_DIR, else ~/.claude.
    pub fn claude_config_root(&self) -> String {
        let raw = self.environment.get("CLAUDE_CONFIG_DIR").map(|value| value.trim()).unwrap_or("");
        if raw.is_empty() {
            return format!("{}/.claude", paths::home().to_string_lossy());
        }
        let first = paths::expand_user(raw.split(',').next().unwrap_or(""));
        paths::resolve(&first)
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|| runtime::absolute(&first.to_string_lossy()))
    }

    fn module_connections(&self, data: &Object) -> HashMap<String, Json> {
        let pairs: Vec<Json> = preferences::USAGE
            .iter()
            .map(|key| Json::Array(vec![Json::from(*key), Json::from(self.source_path(key))]))
            .collect();
        let scope = &py::sha256_hex(py::dumps(&Json::Array(pairs)).as_bytes())[..12];
        let (detected, _) = self.cached_fetch(&format!("configured-sources-{scope}"), 300.0, 60.0, || {
            let modules: Vec<Json> = self.detected_modules().into_iter().map(Json::from).collect();
            Ok((Some(object! { "modules" => Json::Array(modules) }), None))
        });
        let found: HashSet<String> = detected
            .and_then(|detected| detected.get("modules").and_then(Json::as_array).cloned())
            .into_iter()
            .flatten()
            .filter_map(|key| key.as_str().map(String::from))
            .collect();
        let mut connections: HashMap<String, Json> = HashMap::new();
        let settings = self.settings();
        let custom_paths = settings.get("paths").and_then(Json::as_object).cloned().unwrap_or_default();
        for module in preferences::MODULES.iter() {
            let key = module.key;
            if key == "cursor" {
                connections.insert(key.into(), self.cursor_connection_entry());
                continue;
            }
            if !preferences::USAGE.contains(&key) || key == "opencodex" {
                continue;
            }
            let custom =
                custom_paths.get(if key == "claude" { "claude-app" } else { key }).is_some_and(Json::is_truthy);
            let mut configured = found.contains(key) || (custom && Path::new(&self.source_path(key)).exists());
            if key == "deepseek-harness" {
                configured = Path::new(&self.source_path(key)).is_dir();
            }
            let origin = if configured {
                if custom { "来源 · 自定记录目录" } else { "来源 · 自动读取本机客户端" }
            } else {
                "未发现客户端或用量记录"
            };
            let mut children = vec![
                leaf(origin),
                with("选择记录目录…", object! { "action" => action(&["choose-source", key]) }),
                with("恢复自动查找", object! { "action" => action(&["auto-source", key]) }),
            ];
            if configured {
                let locations = self.source_locations(key);
                let exists = locations.iter().any(|path| Path::new(path).exists());
                if !locations.is_empty() {
                    children
                        .insert(1, location_entry(if exists { "读取目录" } else { "默认目录 · 尚未生成" }, &locations));
                }
                if exists {
                    children.insert(
                        2,
                        with(
                            REVEAL_LABEL,
                            object! { "symbol" => "folder", "action" => action(&["reveal-source", key]) },
                        ),
                    );
                }
            }
            if key == "magpie" {
                children[0] = leaf(if configured {
                    "来源 · Magpie 本机网关记录"
                } else {
                    "未发现 Magpie 或网关记录"
                });
                if configured {
                    children.insert(1, leaf("只读取 usage.jsonl"));
                }
                if let Some(app) = self.update_app_path(key) {
                    children
                        .push(with("打开 Magpie", object! { "symbol" => "macwindow", "href" => self.file_href(&app) }));
                }
            }
            if key == "codex" {
                children[0] = leaf("来源 · 编程客户端的本机记录");
                children.insert(1, leaf("仅编程会话，不含普通聊天"));
            }
            if key == "claude" {
                children[0] = leaf("来源 · Claude 桌面 App 与 Claude Code");
                children.insert(1, leaf("含 VS Code、JetBrains 与终端里的会话"));
                for child in children.iter_mut() {
                    if child.get("action") == Some(&action(&["choose-source", "claude"]))
                        && let Some(fields) = child.as_object_mut()
                    {
                        fields.insert("label", "选择 Claude 数据目录…");
                    }
                }
                children.push(with(
                    "打开 Claude",
                    object! { "symbol" => "macwindow", "action" => action(&["open-claude"]) },
                ));
            }
            connections.insert(
                key.into(),
                object! {
                    "label" => module.label.as_str(), "value" => if configured { "已配置" } else { "未配置" }, "symbol" => "folder",
                    "children" => Json::Array(children),
                }
                .into(),
            );
        }
        for service in catalog::SERVICES.iter() {
            let entry = match service.key {
                "chatgpt" => self.client_connection_entry("chatgpt"),
                "cursor-subscription" => {
                    let mut entry = self.cursor_connection_entry().as_object().cloned().unwrap_or_default();
                    entry.insert("label", service.label());
                    entry.into()
                }
                key => self.service_connection_entry(key, data),
            };
            connections.insert(service.key.into(), entry);
        }
        connections.insert("opencodex".into(), self.client_connection_entry("opencodex"));
        connections.insert("komari".into(), self.service_connection_entry("komari", data));
        connections
    }

    pub fn module_options(&self, data: &Object) -> Vec<Json> {
        let settings = self.settings();
        let connections = self.module_connections(data);
        let option = |key: &str| settings.get(key).cloned().unwrap_or(Json::Null);
        let radio = |label: &str, checked: bool, parts: &[&str]| {
            with(label, object! { "checked" => checked, "radio" => true, "action" => action(parts) })
        };
        let mut rows: Vec<Json> = [("system", "跟随系统"), ("light", "浅色"), ("dark", "深色")]
            .iter()
            .map(|(key, label)| radio(label, option("theme") == Json::from(*key), &["set-theme", key]))
            .collect();
        rows.push(separator());
        for (key, label) in [("solid", "实色"), ("frosted", "磨砂"), ("glass", "玻璃")] {
            rows.push(radio(label, option("appearance") == Json::from(key), &["set-appearance", key]));
        }
        let level = option("usageAlert");
        let mut alert_rows: Vec<Json> = preferences::ALERT_LEVELS[1..]
            .iter()
            .chain(std::iter::once(&0))
            .map(|value| {
                let label = if *value != 0 { format!("{value}%") } else { "关闭".into() };
                radio(&label, level == Json::Int(*value), &["set-usage-alert", &value.to_string()])
            })
            .collect();
        let reminder = option("resetReminder").is_truthy();
        alert_rows.push(separator());
        alert_rows.push(with(
            "重置前提醒",
            object! {
                "detail" => "每周窗口用了不到 85%，重置前 12 小时提醒", "checked" => reminder,
                "action" => action(&["set-reset-reminder", if reminder { "false" } else { "true" }]),
            },
        ));
        let cny = option("currency") == Json::from("CNY");
        let rate = option("cnyRate");
        let pin = option("pinInterval").number().unwrap_or(2.0) as i64;
        let menu_bar = option("menuBar").as_str().unwrap_or("none").to_string();
        let alert = option("usageAlert").number().unwrap_or(0.0);
        let service = option("serviceStatus").is_truthy();
        let keep = option("keepCleanedUsage").is_truthy();
        let section = |label: &str| with(label, object! { "section" => true });
        let rate_text = if rate.is_truthy() {
            format!("{:.4} · 手动", rate.number().unwrap_or(0.0))
        } else {
            "每天自动更新".into()
        };
        let selected = self.selected_range();
        let range_label = RANGES.iter().find(|(key, _)| *key == selected).map_or("", |(_, label)| *label);
        // Grouped by what they are for, each group under a small title: what the panel shows, pinning and alerts,
        // the data kept and exported, and the module setup.
        rows.extend([
            section("显示"),
            with(
                "金额显示",
                object! {
                    "symbol" => if cny { "yensign.circle" } else { "dollarsign.circle" }, "value" => if cny { "人民币" } else { "美元" },
                    "children" => Json::Array(vec![
                        radio("美元（$）", option("currency") == Json::from("USD"), &["set-currency", "USD"]),
                        radio("人民币（¥）", cny, &["set-currency", "CNY"]),
                        separator(),
                        with("汇率…", object! {
                            "value" => rate_text, "detail" => "API 等值估算按这个汇率换成人民币；订阅月费与 API 实际花费仍是美元",
                            "action" => action(&["set-cny-rate"]),
                        }),
                    ]),
                },
            ),
            with(
                "菜单栏显示",
                object! {
                    "symbol" => "menubar.rectangle",
                    "value" => MENU_BAR_LABELS.iter().find(|(key, _)| *key == menu_bar).map_or("", |(_, label)| *label),
                    "children" => Json::Array(MENU_BAR_LABELS.iter().map(|(key, label)| radio(label, menu_bar == *key, &["set-menu-bar", key])).collect()),
                },
            ),
            section("钉住与提醒"),
            with(
                "钉住时刷新",
                object! {
                    "symbol" => "pin", "value" => format!("每 {pin} 秒"),
                    "children" => Json::Array(preferences::PIN_INTERVALS.iter().map(|seconds| {
                        radio(&format!("每 {seconds} 秒"), option("pinInterval") == Json::Int(*seconds), &["set-pin-interval", &seconds.to_string()])
                    }).collect()),
                },
            ),
            with(
                "用量提醒",
                object! {
                    "detail" => "订阅窗口用到所选比例时通知一次", "symbol" => "bell",
                    "value" => if alert != 0.0 { format!("{}%", alert as i64) } else { "关闭".into() },
                    "children" => Json::Array(alert_rows),
                },
            ),
            with(
                "服务状态",
                object! {
                    "detail" => "每 5 分钟查一次 Claude、OpenAI、Cursor 的官方状态页，出问题时标在卡片上", "symbol" => "waveform.path.ecg",
                    "checked" => service, "action" => action(&["set-service-status", if service { "false" } else { "true" }]),
                },
            ),
            section("数据"),
            with(
                "保留已清理记录的用量",
                object! {
                    "detail" => "工具删掉旧记录后仍计入“全部”等统计", "symbol" => "archivebox", "checked" => keep,
                    "action" => action(&["set-keep-usage", if keep { "false" } else { "true" }]),
                },
            ),
            with(
                "导出用量",
                object! {
                    "symbol" => "square.and.arrow.up", "value" => range_label,
                    "children" => Json::Array(vec![
                        with("CSV（每天每个工具一行）…", object! { "action" => action(&["export-usage", "csv"]) }),
                        with("JSON…", object! { "action" => action(&["export-usage", "json"]) }),
                    ]),
                },
            ),
            section("模块与配置"),
            with(
                "自动检测",
                object! {
                    "symbol" => "sparkle.magnifyingglass",
                    "children" => Json::Array(vec![
                        leaf("确认后覆盖当前模块选择；不读取对话正文，不显示 Key"),
                        separator(),
                        with("开始检测…", object! { "symbol" => "sparkle.magnifyingglass", "action" => action(&["detect-modules"]) }),
                    ]),
                },
            ),
            with("高级配置文件…", object! { "symbol" => "doc.text", "action" => action(&["open-settings"]) }),
            with("恢复默认模块", object! { "action" => action(&["reset-modules"]) }),
        ]);
        let collapse = option("allowCollapse").is_truthy();
        let mut options = vec![
            with(
                "允许折叠",
                object! {
                    "detail" => "点卡片就地展开", "symbol" => "rectangle.compress.vertical", "checked" => collapse,
                    "action" => action(&["set-collapse", if collapse { "false" } else { "true" }]),
                },
            ),
            with("外观与设置", object! { "symbol" => "slider.horizontal.3", "children" => Json::Array(rows) }),
            separator(),
        ];
        let mut group: Option<&str> = None;
        let visible = settings.get("visible").and_then(Json::as_array).cloned().unwrap_or_default();
        for module in preferences::MODULES.iter() {
            if Some(module.category) != group {
                let label = catalog::GROUP_LABELS
                    .iter()
                    .find(|pair| pair.0 == module.category)
                    .map_or(module.category, |pair| pair.1);
                options.push(section(label));
                group = Some(module.category);
            }
            let enabled = visible.contains(&Json::from(module.key));
            let connection = connections.get(module.key).and_then(Json::as_object).cloned().unwrap_or_default();
            let mut row = object! {
                "label" => module.label.as_str(), "checked" => enabled,
                "action" => action(&["set-module", module.key, if enabled { "false" } else { "true" }]),
                "children" => connection.get("children").cloned().unwrap_or(Json::Array(vec![])),
            };
            for (field, value) in self.module_icon(module.key).iter() {
                row.insert(field.clone(), value.clone());
            }
            if let Some(value) = connection.get("value").filter(|value| value.is_truthy()) {
                row.insert("value", value.clone());
            }
            options.push(row.into());
        }
        options
    }

    pub fn range_options(&self, key: Option<&str>) -> Vec<Json> {
        let chosen = key.map(String::from).unwrap_or_else(|| self.selected_range());
        RANGES
            .iter()
            .map(|(key, label)| {
                object! {
                    "label" => *label, "action" => action(&["set-range", key]), "checked" => *key == chosen,
                    "children" => Json::Array(vec![]),
                }
                .into()
            })
            .collect()
    }

    pub fn presentation_settings(&self) -> Json {
        let settings = self.settings();
        ["revision", "visible", "appearance", "theme", "allowCollapse", "expandedProviders"]
            .iter()
            .map(|key| (*key, settings.get(key).cloned().unwrap_or(Json::Null)))
            .collect::<Object>()
            .into()
    }

    /// The figure beside the star, or "": today's estimate or the tightest subscription window, never a state.
    pub fn menu_bar_value(&self, data: Option<&Object>) -> String {
        let choice = self.settings().get("menuBar").and_then(Json::as_str).unwrap_or("none").to_string();
        if choice == "cost" {
            let bundle = self
                .read_cache(&self.range_cache_name(Some("today")))
                .get("payload")
                .and_then(Json::as_object)
                .cloned()
                .unwrap_or_default();
            if bundle.get("range") != Some(&Json::from("today")) {
                return String::new();
            }
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
            let parts = summary_usage(&selected);
            let costs: Vec<f64> = selected
                .iter()
                .filter_map(|(key, _, _)| {
                    let usage = parts.get(key)?.get("usage")?.as_object()?;
                    if !usage.get("summary").is_some_and(Json::is_truthy)
                        || usage.get("error").is_some_and(Json::is_truthy)
                    {
                        return None;
                    }
                    summary_cost_value(usage.get("summary")?.as_object()?)?.python_float()
                })
                .collect();
            if costs.is_empty() {
                return String::new();
            }
            let empty = Object::new();
            let unit = self.currency_unit(data.unwrap_or(&empty));
            let total = costs.iter().sum::<f64>() * unit.get("rate").and_then(Json::python_float).unwrap_or(1.0);
            let symbol = unit.get("symbol").and_then(Json::as_str).unwrap_or("$");
            let figure = if total < 100.0 { format!("{total:.2}") } else { grouped(total.round_ties_even() as i64) };
            return format!("{symbol}{figure}");
        }
        if choice == "quota"
            && let Some(data) = data
        {
            let used = self
                .alert_windows(data, now())
                .into_iter()
                .filter(|window| window.get("current").is_some_and(Json::is_truthy))
                .filter_map(|window| window.get("used").and_then(Json::number))
                .fold(None, |best: Option<f64>, used| {
                    Some(best.map_or(used, |best| if used > best { used } else { best }))
                });
            return used.map(|used| format!("{}%", used.round_ties_even() as i64)).unwrap_or_default();
        }
        String::new()
    }

    /// Everything the panel shows, in the file the panel reads.
    pub fn panel_snapshot(
        &self,
        data: &Object,
        dashboard: Option<Object>,
        options: Option<Vec<Json>>,
        now: f64,
    ) -> Object {
        let dashboard = dashboard
            .unwrap_or_else(|| self.dashboard_payload(data, data.get("range_usage_error").and_then(Json::as_str), now));
        let mut options = options.unwrap_or_else(|| self.module_options(data));
        if self.mac_app {
            options = without_new_bridge(&options);
        }
        let settings = self.settings();
        let range_key = dashboard.get("rangeKey").and_then(Json::as_str).map(String::from);
        object! {
            "schema" => 1, "project" => object! { "name" => PRODUCT, "url" => "https://github.com/timecatbit/starmeter" },
            "dashboard" => dashboard.clone(), "menuBarText" => self.menu_bar_value(Some(data)),
            "pinInterval" => settings.get("pinInterval").cloned().unwrap_or(Json::Int(2)),
            "entries" => Json::Array(self.resource_panel_entries(data)),
            "rangeOptions" => Json::Array(self.range_options(range_key.as_deref())), "moduleOptions" => Json::Array(options),
            "appearance" => settings.get("appearance").cloned().unwrap_or_else(|| Json::from("solid")),
            "theme" => settings.get("theme").cloned().unwrap_or_else(|| Json::from("system")),
            "preferences" => self.presentation_settings(),
            "updateCount" => update_attention_count(&self.update_rows(data)) as i64,
        }
    }
}
