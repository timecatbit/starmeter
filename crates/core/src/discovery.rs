//! Which modules this machine can feed, for a first install and for 自动检测: by installed apps, CLIs on PATH, log
//! folders that hold records and configured keys. Never parses a conversation or makes a request.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::Path;

use crate::claude_usage::state_of;
use crate::client_accounts::qoder;
use crate::files::{self, paths};
use crate::harness::walk;
use crate::json::{Json, Object};
use crate::opencodex::monitor_url;
use crate::preferences::{self, Error};
use crate::runtime::{self, Runtime, dsh_root, now};
use crate::{accounts, catalog, claude_app, claude_subscription, platform, py, shell};

fn is_dir(path: &str) -> bool {
    Path::new(path).is_dir()
}

fn has_records(root: Option<&str>, matches: impl Fn(&str, &str) -> bool) -> bool {
    let Some(root) = root.filter(|root| !root.is_empty()) else { return false };
    let base = paths::expand_user(root).to_string_lossy().into_owned();
    if !is_dir(&base) {
        return false;
    }
    for path in walk(&base) {
        let relative = path.get(base.len() + 1..).unwrap_or("");
        let name = path.rsplit('/').next().unwrap_or("");
        if matches(name, relative) && state_of(Path::new(&path)).is_some_and(|state| state.size > 0) {
            return true;
        }
    }
    false
}

fn jsonl(name: &str, _: &str) -> bool {
    name.ends_with(".jsonl")
}

pub struct Inputs<'a> {
    pub paths: HashMap<String, String>,
    pub home: &'a str,
    pub configured_proxy: bool,
    pub komari_url: &'a str,
    pub app_roots: Vec<String>,
    pub chatgpt_home: Option<String>,
    pub configured_subscriptions: Vec<String>,
    pub environment: &'a HashMap<String, String>,
}

pub fn detect(inputs: &Inputs) -> Vec<String> {
    let environment = inputs.environment;
    let home = inputs.home;
    let which = |name: &str| shell::which(name, environment).is_some();
    let app =
        |names: &[&str]| inputs.app_roots.iter().any(|root| names.iter().any(|name| is_dir(&format!("{root}/{name}"))));
    let credential = |name: &str| !accounts::credential(name, environment).is_empty();
    let path = |key: &str| inputs.paths.get(key).filter(|path| !path.is_empty()).cloned();
    let mut found: HashSet<&str> = ["summary", "heatmap", "updates"].into_iter().collect();
    if inputs.configured_proxy {
        found.extend(["opencodex", "chatgpt"]);
    }
    for (source, binary) in
        [("codex", "codex"), ("opencode", "opencode"), ("gemini", "gemini"), ("deepseek-harness", "dsh")]
    {
        if which(binary) {
            found.insert(source);
        }
    }
    let magpie = format!("{}/usage.jsonl", path("magpie").unwrap_or_else(|| format!("{home}/.config/magpie")));
    if which("magpie") || (Path::new(&magpie).exists() && !is_dir(&magpie)) || app(&["magpie.app", "Magpie.app"]) {
        found.insert("magpie");
    }
    if app(&["WorkBuddy.app"]) {
        found.insert("workbuddy");
    }
    // Cursor keeps no usage on disk; the module only offers to read the account's records once turned on.
    let cursor_state = format!("{}/state.vscdb", path("cursor").unwrap_or_else(|| home.to_string()));
    if which("cursor-agent") || app(&["Cursor.app"]) || Path::new(&cursor_state).exists() {
        found.extend(["cursor", "cursor-subscription"]);
    }
    let qoder_root = path("qoder")
        .or_else(|| environment.get("QODERCN_CONFIG_DIR").filter(|value| !value.is_empty()).cloned())
        .unwrap_or_else(|| format!("{home}/.qoder-cn"));
    if which("qodercn")
        || app(&["Qoder CN.app", "QoderCN.app"])
        || has_records(Some(&format!("{qoder_root}/projects")), jsonl)
    {
        found.insert("qoder");
    }
    if qoder::snapshot(home, environment, now()).is_some() {
        found.insert("qoder-subscription");
    }
    let claude_root = path("claude").unwrap_or_else(|| claude_app::default_root(home, environment));
    if app(&["Claude.app"])
        || !claude_app::registry_files(&claude_root).is_empty()
        || which("claude")
        || claude_app::project_roots(home, environment).iter().any(|root| has_records(Some(root), jsonl))
    {
        found.insert("claude");
    }
    if claude_subscription::desktop(&claude_root, now()).get("windows").is_some_and(Json::is_truthy) {
        found.insert("claude-subscription");
    }
    let codex = path("codex").unwrap_or_else(|| format!("{home}/.codex"));
    let folders = if ["sessions", "archived_sessions"].iter().any(|folder| is_dir(&format!("{codex}/{folder}"))) {
        vec![format!("{codex}/sessions"), format!("{codex}/archived_sessions")]
    } else {
        vec![codex.clone()]
    };
    if folders
        .iter()
        .any(|folder| has_records(Some(folder), |name, _| name.ends_with(".jsonl") || name.ends_with(".jsonl.zst")))
    {
        found.insert("codex");
    }
    // Active sessions are Claude Code's and Codex's.
    if found.contains("claude") || found.contains("codex") {
        found.insert("sessions");
    }
    let opencode = path("opencode").unwrap_or_else(|| format!("{home}/.local/share/opencode"));
    let database = if is_dir(&opencode) { format!("{opencode}/opencode.db") } else { opencode };
    if !is_dir(&database) && state_of(Path::new(&database)).is_some_and(|state| state.size > 0) {
        found.insert("opencode");
    }
    let dsh = path("deepseek-harness").unwrap_or_else(|| dsh_root(home, environment));
    if has_records(Some(&dsh), |name, _| {
        name.starts_with("session") && (name.ends_with(".jsonl") || name.ends_with(".jsonl.zstd"))
    }) {
        found.insert("deepseek-harness");
    }
    let gemini = path("gemini").unwrap_or_else(|| format!("{home}/.gemini"));
    let gemini_root = if is_dir(&format!("{gemini}/tmp")) { format!("{gemini}/tmp") } else { gemini };
    if has_records(Some(&gemini_root), |name, relative| {
        let parts: Vec<&str> = relative.split('/').collect();
        (name.starts_with("session-") && (name.ends_with(".json") || name.ends_with(".jsonl")))
            || (name.ends_with(".jsonl") && parts[..parts.len().saturating_sub(1)].contains(&"chats"))
    }) {
        found.insert("gemini");
    }
    let workbuddy = path("workbuddy").unwrap_or_else(|| home.to_string());
    let mut roots: Vec<String> = [".workbuddy-ai", ".workbuddy"]
        .iter()
        .map(|folder| format!("{workbuddy}/{folder}/projects"))
        .filter(|root| is_dir(root))
        .collect();
    if roots.is_empty() && workbuddy != home {
        let projects = format!("{workbuddy}/projects");
        roots = vec![if is_dir(&projects) { projects } else { workbuddy.clone() }];
    }
    if roots.iter().any(|root| has_records(Some(root), jsonl)) {
        found.insert("workbuddy");
    }
    let auth_root = paths::expand_user(
        &inputs
            .chatgpt_home
            .clone()
            .or_else(|| environment.get("CODEX_HOME").filter(|value| !value.is_empty()).cloned())
            .unwrap_or_else(|| format!("{home}/.codex")),
    )
    .to_string_lossy()
    .into_owned();
    let auth = claude_app::read_object(&format!("{auth_root}/auth.json"));
    let mode = auth.get("auth_mode");
    if which("codex")
        && (mode == Some(&Json::from("chatgpt"))
            || mode == Some(&Json::from("chatgptAuthTokens"))
            || auth.get("tokens").and_then(|tokens| tokens.get("access_token")).is_some_and(Json::is_truthy))
    {
        found.insert("chatgpt");
    }
    let config = claude_app::read_object(&format!(
        "{}/config.json",
        path("opencodex").unwrap_or_else(|| format!("{home}/.opencodex"))
    ));
    let providers = py::or(config.get("providers"), Json::Object(Object::new()));
    let provider = providers.get("deepseek").and_then(Json::as_object).cloned().unwrap_or_default();
    let base = py::str(Some(&py::or(provider.get("baseUrl"), Json::from(""))));
    let official = ["https://api.deepseek.com", "https://api.deepseek.com/v1"].contains(&base.trim_end_matches('/'));
    if credential("DEEPSEEK_API_KEY") || (official && provider.get("apiKey").is_some_and(Json::is_truthy)) {
        found.insert("deepseek");
    }
    if credential("MINIMAX_API_KEY") {
        found.insert("minimax");
    }
    let given = |name: &str| !environment.get(name).map(|value| value.trim()).unwrap_or("").is_empty();
    if credential("XAI_MANAGEMENT_API_KEY") && given("XAI_TEAM_ID") {
        found.insert("grok");
    }
    if credential("QODER_CN_API_KEY") && given("QODER_CN_ORG_ID") {
        found.insert("qoder-api");
    }
    for key in catalog::key_only_apis() {
        if catalog::credential_names(key).iter().any(|name| credential(name)) {
            found.insert(key);
        }
    }
    let manual = catalog::manual_subscriptions();
    let chosen: Vec<&str> =
        inputs.configured_subscriptions.iter().map(String::as_str).filter(|key| manual.contains(key)).collect();
    found.extend(chosen);
    if !monitor_url(inputs.komari_url).is_empty() {
        found.insert("komari");
    }
    preferences::MODULES.iter().map(|module| module.key).filter(|key| found.contains(key)).map(String::from).collect()
}

fn selection(detected: &[String]) -> Json {
    let chosen: HashSet<&str> = detected.iter().map(String::as_str).collect();
    Json::Array(
        preferences::MODULES
            .iter()
            .filter(|module| chosen.contains(module.key))
            .map(|module| Json::from(module.key))
            .collect(),
    )
}

/// Only first installs are set up; existing choices survive upgrades and a concurrent manual choice wins.
pub fn initialize(directory: &str, detected: &[String]) -> bool {
    let target = Path::new(directory).join("preferences.json");
    if target.exists() {
        return false;
    }
    let mut current = preferences::read(Path::new(directory));
    current.insert("visible", selection(detected));
    let _ = files::make_directory(Path::new(directory), 0o700);
    let temporary = Path::new(directory).join(format!(".discovery-{}", runtime::random_hex(12)));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let Ok(mut file) = options.open(&temporary) else { return false };
    let written = file.write_all(Json::Object(current).serialized().as_bytes()).is_ok();
    drop(file);
    let linked = written && platform::hard_link(&temporary, &target);
    platform::remove(&temporary);
    linked
}

/// Only after the user confirms the detected selection.
pub fn replace_selection(directory: &str, detected: &[String]) -> Result<(), Error> {
    let mut current = preferences::read(Path::new(directory));
    current.insert("visible", selection(detected));
    preferences::write(Path::new(directory), current)
}

impl Runtime {
    pub fn detected_modules(&self) -> Vec<String> {
        let settings = self.settings();
        let subscriptions: Vec<String> = settings
            .get("configuredSubscriptions")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .filter_map(|key| key.as_str().map(String::from))
            .collect();
        let komari = self.komari_base_url();
        let inputs = Inputs {
            paths: preferences::USAGE.iter().map(|key| (key.to_string(), self.source_path(key))).collect(),
            home: &self.home,
            configured_proxy: self.opencodex_configured(),
            komari_url: &komari,
            app_roots: if self.is_desktop() {
                Vec::new()
            } else {
                vec![format!("{}/Applications", self.home), "/Applications".into()]
            },
            chatgpt_home: Some(self.codex_home_absolute()),
            configured_subscriptions: subscriptions,
            environment: &self.environment,
        };
        detect(&inputs)
    }
}
