//! Where the engine runs from and what it reads: the environment (config.local.sh is sourced by the entry script),
//! the state and cache folders, settings for one render, and the JSON caches shared with the panel.

use std::any::Any;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::claude_usage::state_of;
use crate::files::{self, paths};
use crate::harness::{is_absolute, normalize};
use crate::host::{DesktopHost, Host, PanelAskHost};
use crate::json::{Json, Object};
use crate::py::{self, FileError};
use crate::{claude_app, cursor, object, platform, preferences, pricing};

pub const VERSION: &str = crate::VERSION;
pub const PRODUCT: &str = "Starmeter";
pub const RANGES: [(&str, &str); 8] = [
    ("today", "今天"),
    ("yesterday", "昨天"),
    ("24h", "近 24 小时"),
    ("7d", "近 7 天"),
    ("30d", "近 30 天"),
    ("week", "本周"),
    ("month", "本月"),
    ("all", "全部"),
];
pub const HOURLY_RANGES: [&str; 5] = ["today", "yesterday", "24h", "7d", "week"];
pub const SHORT_RANGES: [&str; 3] = ["today", "yesterday", "24h"];
pub const PRICING_TABLES: [&str; 2] = ["codex-pricing.json", "model-pricing.json"];
pub const PENDING_READ: &str = "正在后台读取";

/// Magpie's agent names for each Harness its log may share requests with.
pub fn magpie_agents(source: &str) -> Option<&'static [&'static str]> {
    match source {
        "codex" => Some(&["codex"]),
        "claude" => Some(&["claude", "claude-code", "claude-desktop"]),
        "opencode" => Some(&["opencode"]),
        "deepseek-harness" => Some(&["dsh"]),
        "gemini" => Some(&["gemini", "gemini-cli"]),
        "cursor" => Some(&["cursor"]),
        _ => None,
    }
}

/// What a loader threw, as a failed read reports it: the Python exception's name.
#[derive(Clone, Debug)]
pub enum Thrown {
    File(FileError),
    Line,
    Other(String),
}

impl From<FileError> for Thrown {
    fn from(error: FileError) -> Self {
        Thrown::File(error)
    }
}

pub type Loaded = (Option<Object>, Option<String>);

type Cadence = Box<dyn Fn() -> (bool, bool) + Send + Sync>;

pub struct Runtime {
    pub environment: HashMap<String, String>,
    pub home: String,
    pub ocx_home: String,
    pub token_file: String,
    pub state_dir: String,
    pub cache_dir: String,
    pub plugin: String,
    pub base: String,
    pub mac_app: bool,
    /// Set by an in-process host: whether its panel is open, and on battery.
    pub host_cadence: Option<Cadence>,
    /// Dialogs, notices and files for actions; a desktop host also takes over refreshes and settings.
    pub host: Arc<dyn Host>,
    /// Settings and local metadata read once per render.
    memo: Mutex<Option<HashMap<String, Box<dyn Any + Send>>>>,
}

impl Runtime {
    pub fn from_process() -> Self {
        let argv0 = std::env::args().next().unwrap_or_else(|| "starmeter".into());
        Self::new(std::env::vars().collect(), &argv0)
    }

    pub fn new(environment: HashMap<String, String>, argv0: &str) -> Self {
        let home = paths::home().to_string_lossy().into_owned();
        let given = |key: &str| environment.get(key).filter(|value| !value.is_empty()).cloned();
        let expand = |value: &str| paths::expand_user(value).to_string_lossy().into_owned();
        let ocx_home = expand(&given("OCX_HOME").unwrap_or_else(|| format!("{home}/.opencodex")));
        let token_file = expand(&given("OCX_TOKEN_FILE").unwrap_or_else(|| format!("{ocx_home}/admin-api-token")));
        let plugin = given("OCX_PLUGIN_PATH").unwrap_or_else(|| argv0.to_string());
        let mac_app = environment.get("OCX_MAC_APP").map(String::as_str) == Some("1");
        let state_dir = match (given("OCX_STATE_DIR"), given("OCX_CACHE_DIR")) {
            (Some(state), _) => expand(&state),
            (None, Some(cache)) => format!("{cache}/settings"),
            (None, None) => format!("{home}/Library/Application Support/SwiftBarToken"),
        };
        let scope = &py::sha256_hex(absolute(&state_dir).as_bytes())[..16];
        let parent = given("OCX_CACHE_DIR").unwrap_or_else(|| std::env::temp_dir().to_string_lossy().into_owned());
        let cache_dir = Path::new(&parent)
            .join(format!("opencodex-swiftbar-v5-{}-{scope}", platform::user_id()))
            .to_string_lossy()
            .into_owned();
        let base = detect_base(&environment, &home);
        pricing::ensure_folders(vec![lib_directory(&plugin).into(), format!("{state_dir}/pricing").into()]);
        let host: Arc<dyn Host> = Arc::new(PanelAskHost::new(cache_dir.clone()));
        Runtime {
            environment,
            home,
            ocx_home,
            token_file,
            state_dir,
            cache_dir,
            plugin,
            base,
            mac_app,
            host_cadence: None,
            host,
            memo: Mutex::new(None),
        }
    }

    /// The same runtime after connection settings changed: config.local.sh is not re-read, the new values are applied.
    pub fn updated(&self, values: &[(String, String)]) -> Runtime {
        let mut environment = self.environment.clone();
        for (key, value) in values {
            environment.insert(key.clone(), value.clone());
        }
        let mut next = Runtime::new(environment, &self.plugin);
        next.host = self.host.clone();
        next
    }

    // One render's view of the settings

    /// Runs body with settings and local metadata read at most once inside it.
    pub fn render<T>(&self, body: impl FnOnce() -> T) -> T {
        *self.memo.lock().expect("memo") = Some(HashMap::new());
        let value = body();
        *self.memo.lock().expect("memo") = None;
        value
    }

    pub fn memoized<T: Clone + Send + 'static>(&self, key: &str, compute: impl FnOnce() -> T) -> T {
        if let Some(memo) = self.memo.lock().expect("memo").as_ref()
            && let Some(stored) = memo.get(key).and_then(|value| value.downcast_ref::<T>())
        {
            return stored.clone();
        }
        let value = compute();
        if let Some(memo) = self.memo.lock().expect("memo").as_mut() {
            memo.insert(key.to_string(), Box::new(value.clone()));
        }
        value
    }

    pub fn settings(&self) -> Object {
        self.memoized("settings", || preferences::read(Path::new(&self.state_dir)))
    }

    pub fn module_enabled(&self, key: &str) -> bool {
        self.settings()
            .get("visible")
            .and_then(Json::as_array)
            .is_some_and(|visible| visible.contains(&Json::from(key)))
    }

    pub fn env(&self, key: &str) -> Option<&str> {
        self.environment.get(key).map(String::as_str).filter(|value| !value.is_empty())
    }

    fn codex_usage_home(&self) -> String {
        paths::expand_user(&self.env("CODEX_HOME").map_or_else(|| format!("{}/.codex", self.home), String::from))
            .to_string_lossy()
            .into_owned()
    }

    pub fn source_path(&self, key: &str) -> String {
        let settings = self.settings();
        let custom = settings
            .get("paths")
            .and_then(|paths| paths.get(if key == "claude" { "claude-app" } else { key }))
            .and_then(Json::as_str)
            .filter(|path| !path.is_empty())
            .map(String::from);
        let path = custom.unwrap_or_else(|| {
            let home = &self.home;
            let given = |name: &str| self.env(name).map(String::from);
            match key {
                "opencodex" => self.ocx_home.clone(),
                "claude" => {
                    given("CLAUDE_APP_DATA_DIR").unwrap_or_else(|| claude_app::default_root(home, &self.environment))
                }
                "codex" => given("CODEX_USAGE_DIR").unwrap_or_else(|| self.codex_usage_home()),
                "opencode" => given("OPENCODE_DATA_DIR").unwrap_or_else(|| {
                    let data = given("XDG_DATA_HOME").unwrap_or_else(|| format!("{home}/.local/share"));
                    format!("{}/opencode", paths::expand_user(&data).to_string_lossy())
                }),
                "deepseek-harness" => self.dsh_sessions_root(),
                "workbuddy" => given("WORKBUDDY_USAGE_DIR").unwrap_or_else(|| home.clone()),
                "qoder" => given("QODERCN_CONFIG_DIR").unwrap_or_else(|| format!("{home}/.qoder-cn")),
                "gemini" => given("GEMINI_USAGE_DIR").unwrap_or_else(|| format!("{home}/.gemini")),
                "magpie" => self.magpie_root(),
                "cursor" => cursor::app_root(home, &self.environment),
                _ => String::new(),
            }
        });
        paths::expand_user(&path).to_string_lossy().into_owned()
    }

    fn dsh_sessions_root(&self) -> String {
        dsh_root(&self.home, &self.environment)
    }

    fn magpie_root(&self) -> String {
        if let Some(configured) = self.env("MAGPIE_USAGE_DIR") {
            return normalize(&paths::expand_user(configured).to_string_lossy());
        }
        let xdg = self.environment.get("XDG_CONFIG_HOME").map(String::as_str).unwrap_or("");
        let config = if is_absolute(xdg) { normalize(xdg) } else { format!("{}/.config", self.home) };
        format!("{config}/magpie")
    }

    // Caches

    pub fn cache_path(&self, name: &str) -> String {
        Path::new(&self.cache_dir).join(format!("{name}.json")).to_string_lossy().into_owned()
    }

    pub fn read_cache(&self, name: &str) -> Object {
        let Some(record) = std::fs::read(self.cache_path(name))
            .ok()
            .and_then(|data| Json::parse(&data).ok())
            .and_then(|value| value.as_object().cloned())
            .filter(|record| record.get("schema") == Some(&Json::Int(1)))
        else {
            return Object::new();
        };
        if let Some(connection) = record.get("connection").filter(|connection| connection.is_truthy())
            && connection != &Json::from(self.connection_fingerprint(name))
        {
            return Object::new();
        }
        for key in ["fetchedAt", "retryAt"] {
            if let Some(value) = record.get(key)
                && value.number().is_none()
                && value.as_bool().is_none()
            {
                return Object::new();
            }
        }
        if let Some(payload) = record.get("payload")
            && !payload.is_null()
            && payload.as_object().is_none()
        {
            return Object::new();
        }
        if (name.starts_with("usage_") || name.starts_with("range_usage_") || name == "claude_usage")
            && record.get("day") != Some(&Json::from(today()))
        {
            return Object::new();
        }
        record
    }

    /// Replaced atomically, so a concurrent refresh never reads half a file; skipped when nothing changed.
    pub fn write_json_cache(&self, path: &str, payload: &Json) {
        if let Ok(data) = std::fs::read(path)
            && Json::parse(&data).is_ok_and(|current| &current == payload)
        {
            return;
        }
        let path = Path::new(path);
        if let Some(parent) = path.parent() {
            let _ = files::make_directory(parent, 0o700);
        }
        let _ = files::replace_atomically(path, payload.serialized().as_bytes(), ".cache-", || Ok(()));
    }

    pub fn cache_due(&self, record: &Object, ttl: f64) -> bool {
        let now = now();
        let age = now - record.get("fetchedAt").and_then(Json::python_float).unwrap_or(0.0);
        now >= record.get("retryAt").and_then(Json::python_float).unwrap_or(0.0)
            && (record.get("force").is_some_and(Json::is_truthy)
                || !record.get("payload").is_some_and(Json::is_truthy)
                || age < 0.0
                || age >= ttl)
    }

    /// The cached payload with its read time, marked stale when it failed, was forced or is old; and the error.
    pub fn cache_result(&self, record: &Object, ttl: f64) -> Loaded {
        let error = record.get("error").filter(|error| error.is_truthy()).map(|error| py::str(Some(error)));
        let Some(mut result) = record.get("payload").and_then(Json::as_object).cloned() else {
            return (None, Some(error.unwrap_or_else(|| PENDING_READ.into())));
        };
        result.insert("_fetchedAt", record.get("fetchedAt").cloned().unwrap_or(Json::Int(0)));
        let age = now() - result.get("_fetchedAt").and_then(Json::python_float).unwrap_or(0.0);
        if record.get("error").is_some_and(Json::is_truthy)
            || record.get("force").is_some_and(Json::is_truthy)
            || age < 0.0
            || age >= ttl
        {
            result.insert("_stale", true);
        } else {
            result.remove("_stale");
        }
        (Some(result), error)
    }

    /// The payload a loader gives, cached for ttl seconds; a failed read keeps the last good one and retries.
    pub fn cached_fetch(
        &self,
        name: &str,
        ttl: f64,
        retry: f64,
        loader: impl FnOnce() -> Result<Loaded, Thrown>,
    ) -> Loaded {
        let fingerprint = self.connection_fingerprint(name);
        let mut record = self.read_cache(name);
        if !self.cache_due(&record, ttl) {
            return self.cache_result(&record, ttl);
        }
        let (payload, mut failure) = match loader() {
            Ok(loaded) => loaded,
            Err(Thrown::File(error)) => (None, Some(error.kind.to_string())),
            Err(Thrown::Line) => (None, Some("TypeError".into())),
            Err(Thrown::Other(kind)) => (None, Some(kind)),
        };
        if failure.is_none()
            && let Some(message) = payload.as_ref().and_then(|payload| payload.get("error")).filter(|m| m.is_truthy())
        {
            failure = Some(py::str(Some(message)));
        }
        match (failure, payload) {
            (None, Some(payload)) => {
                record = object! { "schema" => 1, "payload" => payload, "fetchedAt" => now(), "day" => today() };
            }
            (failure, _) => {
                // Failed requests retry after a minute, retaining the last successful data.
                record.insert("schema", 1);
                record.insert("error", failure.unwrap_or_else(|| "数据格式异常".into()));
                record.insert("retryAt", now() + retry);
                record.insert("day", today());
            }
        }
        record.insert("connection", fingerprint);
        self.write_json_cache(&self.cache_path(name), &Json::Object(record.clone()));
        self.cache_result(&record, ttl)
    }

    /// What a cached source depends on: change it and the cache is read afresh. Sources without settings of their
    /// own depend on the OpenCodex address and token file.
    pub fn connection_fingerprint(&self, name: &str) -> String {
        self.memoized(&format!("connection:{name}"), || {
            py::sha256_hex(py::dumps(&Json::Array(self.connection_parts(name))).as_bytes())
        })
    }

    pub fn connection_parts(&self, name: &str) -> Vec<Json> {
        let mut parts: Vec<Json> = match name {
            "claude_subscription" => self.claude_subscription_parts(),
            "app_updates" => self.app_update_parts(),
            _ => Vec::new(),
        };
        let keys = connection_keys(name);
        for key in keys.unwrap_or(&[]) {
            let value = self.environment.get(*key).cloned().unwrap_or_default();
            parts.push(Json::from(value.as_str()));
            if key.ends_with("_FILE") && !value.is_empty() {
                match state_of(&paths::expand_user(&value)) {
                    Some(state) => parts.extend([Json::Int(state.mtime), Json::Int(state.size)]),
                    None => parts.push("unavailable".into()),
                }
            }
        }
        if name == "chatgpt" {
            let codex = self.codex_home_absolute();
            parts.push(Json::from(codex.as_str()));
            match state_of(Path::new(&format!("{codex}/auth.json"))) {
                Some(state) => parts.extend([Json::Int(state.mtime), Json::Int(state.size)]),
                None => parts.push("unavailable".into()),
            }
        }
        // The update checks also follow the OpenCodex address, as the 5.x caches were keyed.
        if keys.is_none() || name == "provider_quotas" || name == "app_updates" {
            parts.extend([Json::from(self.base.as_str()), Json::from(self.token_file.as_str())]);
        }
        parts
    }
}

/// Which sources have connection settings of their own.
fn connection_keys(name: &str) -> Option<&'static [&'static str]> {
    Some(match name {
        "app_updates"
        | "claude_subscription"
        | "qoder_subscription"
        | "cursor_subscription"
        | "gemini_subscription" => &[],
        "chatgpt" => &["OCX_CHATGPT_HOME"],
        "komari" => &["KOMARI_BASE_URL"],
        "provider_quotas" => &["DEEPSEEK_API_KEY", "DEEPSEEK_API_KEY_FILE"],
        "grok" => &["XAI_MANAGEMENT_API_KEY", "XAI_MANAGEMENT_API_KEY_FILE", "XAI_TEAM_ID"],
        "minimax" => &["MINIMAX_API_KEY", "MINIMAX_API_KEY_FILE", "MINIMAX_REGION"],
        "kimi-api" => &["MOONSHOT_API_KEY", "MOONSHOT_API_KEY_FILE", "MOONSHOT_REGION"],
        "qoder-api" => &["QODER_CN_API_KEY", "QODER_CN_API_KEY_FILE", "QODER_CN_ORG_ID"],
        "claude-api" => &["ANTHROPIC_API_KEY", "ANTHROPIC_API_KEY_FILE"],
        _ => return None,
    })
}

pub fn now() -> f64 {
    files::time_nanoseconds() as f64 / 1e9
}

/// Today's date in the system's zone.
pub fn today() -> String {
    jiff::Zoned::now().strftime("%Y-%m-%d").to_string()
}

pub fn absolute(path: &str) -> String {
    if is_absolute(path) {
        normalize(path)
    } else {
        let cwd = std::env::current_dir().map(|path| path.to_string_lossy().into_owned()).unwrap_or_default();
        normalize(&format!("{cwd}/{path}"))
    }
}

/// The installed lib folder beside the entry, where the price tables ship.
pub fn lib_directory(plugin: &str) -> String {
    let folder = parent_folder(&absolute(plugin));
    installed(&folder, "lib", "model-pricing.json")
        .unwrap_or_else(|| Path::new(&folder).join("lib").to_string_lossy().into_owned())
}

/// The folder a path sits in, with either separator on Windows.
pub fn parent_folder(path: &str) -> String {
    Path::new(path).parent().map(|parent| parent.to_string_lossy().into_owned()).unwrap_or_default()
}

/// The first folder named `name` holding `marker`: beside the entry (or in its hidden .opencodex-panel folder), or
/// up to three levels above, as in a source checkout.
pub fn installed(folder: &str, name: &str, marker: &str) -> Option<String> {
    let mut level = Some(Path::new(folder).to_path_buf());
    for depth in 0..4 {
        let current = level?;
        let mut candidates = Vec::new();
        if depth == 0 {
            candidates.push(current.join(".opencodex-panel").join(name));
        }
        candidates.push(current.join(name));
        if let Some(found) = candidates.into_iter().find(|candidate| candidate.join(marker).exists()) {
            return Some(normalize(&found.to_string_lossy()));
        }
        level = current.parent().map(Path::to_path_buf);
    }
    None
}

fn detect_base(environment: &HashMap<String, String>, home: &str) -> String {
    let explicit = environment.get("OCX_BASE_URL").map(|value| value.trim()).unwrap_or("");
    if !explicit.is_empty() {
        return explicit.trim_end_matches('/').to_string();
    }
    let ocx_home = environment
        .get("OCX_HOME")
        .filter(|value| !value.is_empty())
        .cloned()
        .unwrap_or_else(|| format!("{home}/.opencodex"));
    let ocx_home = paths::expand_user(&ocx_home).to_string_lossy().into_owned();
    if let Some(port) = claude_app::read_object(&format!("{ocx_home}/runtime-port.json")).get("port")
        && let Ok(number) = py::int(Some(port))
        && (1..=65535).contains(&number)
    {
        return format!("http://127.0.0.1:{number}");
    }
    "http://127.0.0.1:10100".into()
}

impl Runtime {
    /// What the Claude subscription cache depends on.
    fn claude_subscription_parts(&self) -> Vec<Json> {
        self.claude_subscription_connection()
    }

    /// What the update checks depend on.
    fn app_update_parts(&self) -> Vec<Json> {
        self.app_update_connection()
    }

    /// A notification titled Starmeter, through the host.
    pub fn notify(&self, message: &str, subtitle: &str) {
        self.host.notify(message, subtitle);
    }

    /// The host when it has its own windows and refresh queue.
    pub fn desktop(&self) -> Option<&dyn DesktopHost> {
        self.host.desktop()
    }

    pub fn is_desktop(&self) -> bool {
        self.desktop().is_some()
    }
}

impl Runtime {
    /// SwiftBar's name for the plugin: the file name up to its first dot.
    pub fn swiftbar_name(&self) -> String {
        let file = self.plugin.rsplit(['/', '\\']).next().unwrap_or("");
        file.split('.').next().filter(|name| !name.is_empty()).unwrap_or("starmeter").to_string()
    }

    /// Whether a panel chose 繁體中文 on this computer (the `language` file the app keeps in the settings folder both
    /// panels share): the engine's own words then read in Traditional Chinese too.
    pub fn traditional(&self) -> bool {
        std::fs::read_to_string(Path::new(&self.state_dir).join("language")).is_ok_and(|text| text.trim() == "zh-Hant")
    }

    /// The engine's own Chinese as this computer reads it: converted for 繁體中文, else as written.
    pub fn shown(&self, text: &str) -> String {
        if self.traditional() { crate::hant::convert(text) } else { text.to_string() }
    }

    /// A notification from Starmeter: the desktop host's own, the standalone app's, or SwiftBar's.
    pub fn post_notice(&self, subtitle: &str, body: &str) -> bool {
        let (subtitle, body) = (self.shown(subtitle), self.shown(body));
        let (subtitle, body) = (subtitle.as_str(), body.as_str());
        if let Some(desktop) = self.desktop() {
            desktop.notify(body, subtitle);
            return true;
        }
        if self.mac_app {
            if self.read_cache("notifications-enabled").get("allowed") != Some(&Json::Bool(true)) {
                return false;
            }
            let path = self.cache_path(&format!("notice-{}", random_hex(32)));
            self.write_json_cache(&path, &Json::Object(object! { "subtitle" => subtitle, "body" => body }));
            return Path::new(&path).exists();
        }
        let name = self.swiftbar_name();
        let href = format!("swiftbar://refreshplugin?name={}", quote(&name));
        let query: Vec<String> = [
            ("plugin", name.as_str()),
            ("title", PRODUCT),
            ("subtitle", subtitle),
            ("body", body),
            ("href", href.as_str()),
        ]
        .iter()
        .map(|(key, value)| format!("{key}={}", quote(value)))
        .collect();
        let url = format!("swiftbar://notify?{}", query.join("&"));
        crate::shell::run(&["/usr/bin/open", "-g", &url], 5.0, None, None).is_some()
    }
}

/// DeepSeek Harness's sessions folder: DSH_SESSIONS_DIR, else DSH_HOME/sessions, else ~/.dsh/sessions.
pub fn dsh_root(home: &str, environment: &HashMap<String, String>) -> String {
    let explicit = environment.get("DSH_SESSIONS_DIR").map(|value| value.trim()).unwrap_or("");
    if !explicit.is_empty() {
        return normalize(&paths::expand_user(explicit).to_string_lossy());
    }
    let configured = environment.get("DSH_HOME").map(|value| value.trim()).unwrap_or("");
    let root = if configured.is_empty() {
        format!("{home}/.dsh")
    } else {
        normalize(&paths::expand_user(configured).to_string_lossy())
    };
    format!("{root}/sessions")
}

/// Percent-encodes everything but the unreserved characters, as urllib.parse.quote(value, safe='').
pub fn quote(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

/// Random lowercase hex, for one-off file names.
pub fn random_hex(length: usize) -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut text = String::new();
    while text.len() < length {
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_i64(files::time_nanoseconds());
        text.push_str(&format!("{:016x}", hasher.finish()));
    }
    text.truncate(length);
    text
}
