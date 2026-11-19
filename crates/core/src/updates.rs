//! Version checks for the apps and CLIs behind the selected Harness, and OpenCodex's own installer. Only public
//! version metadata is read; no login, no real device ID.

use std::collections::HashMap;
use std::path::Path;

use sha1::{Digest, Sha1};

use crate::accounts::{form_quote, quote};
use crate::claude_app;
use crate::database::{self, Database, Mode, Value};
use crate::files::{self, paths};
use crate::http::{self, Request};
use crate::json::{Json, Object};
use crate::preferences::USAGE;
use crate::py::{self, FileError};
use crate::runtime::{Loaded, Runtime, now};
use crate::time::IsoTime;
use crate::{object, platform, shell};

/// v?1.2.3 (with an optional -suffix) as five numbers, or None.
pub fn release_version(value: Option<&Json>) -> Option<[i64; 5]> {
    let text = py::str(Some(&py::or(value, Json::from(""))));
    let text = text.trim();
    let text = text.strip_prefix('v').unwrap_or(text);
    let core = text.split(['-', '+']).next().unwrap_or("");
    let rest = &text[core.len()..];
    if !rest.is_empty() {
        let tail = &rest[1..];
        if tail.is_empty() || !tail.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-') {
            return None;
        }
    }
    let parts: Vec<&str> = core.split('.').collect();
    if !(1..=5).contains(&parts.len())
        || !parts.iter().all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    let mut numbers = [0i64; 5];
    for (index, part) in parts.iter().enumerate() {
        numbers[index] = part.parse().unwrap_or(0);
    }
    Some(numbers)
}

pub fn newer(latest: Option<&Json>, current: Option<&Json>) -> bool {
    match (release_version(latest), release_version(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

fn plain_version(text: Option<&str>) -> Option<[i64; 5]> {
    let text = text.filter(|text| !text.is_empty())?;
    let parts: Vec<&str> = text.split('.').collect();
    if parts.len() > 5 || !parts.iter().all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit())) {
        return None;
    }
    let mut numbers = [0i64; 5];
    for (index, part) in parts.iter().enumerate() {
        numbers[index] = part.parse().unwrap_or(0);
    }
    Some(numbers)
}

fn plist_strings(data: &[u8], keys: &[&str]) -> Option<HashMap<String, String>> {
    let value = plist::Value::from_reader(std::io::Cursor::new(data)).ok()?;
    let dictionary = value.as_dictionary()?;
    Some(
        keys.iter()
            .map(|key| {
                let text = match dictionary.get(key) {
                    Some(plist::Value::String(text)) => text.clone(),
                    Some(plist::Value::Integer(number)) => number.to_string(),
                    Some(plist::Value::Real(number)) => number.to_string(),
                    _ => String::new(),
                };
                (key.to_string(), text)
            })
            .collect(),
    )
}

/// An installed app's version, build and bundle ID, from /Applications or ~/Applications.
pub fn app_info(name: &str, home: &str) -> Object {
    for base in ["/Applications".to_string(), format!("{home}/Applications")] {
        let Ok(data) = std::fs::read(format!("{base}/{name}.app/Contents/Info.plist")) else { continue };
        let keys = ["CFBundleShortVersionString", "CFBundleVersion", "CFBundleIdentifier"];
        let Some(values) = plist_strings(&data, &keys) else { continue };
        return keys.iter().map(|key| (*key, values.get(*key).cloned().unwrap_or_default())).collect();
    }
    Object::new()
}

pub fn installed_app_path(name: &str, home: &str) -> Option<String> {
    ["/Applications".to_string(), format!("{home}/Applications")]
        .into_iter()
        .map(|base| format!("{base}/{name}.app"))
        .find(|path| Path::new(path).is_dir())
}

#[derive(Debug)]
enum CheckFailure {
    /// The server could not be reached or answered with an error: shown under the row.
    Check(String),
    File(FileError),
}

impl From<FileError> for CheckFailure {
    fn from(error: FileError) -> Self {
        CheckFailure::File(error)
    }
}

fn connection_problem(message: &str) -> &'static str {
    let text = message.to_lowercase();
    if text.contains("certificate") || text.contains("ssl") {
        "证书验证失败（Python 缺少根证书）"
    } else if text.contains("timed out") {
        "连接超时（检查网络或代理）"
    } else if text.contains("dns") || text.contains("hostname could not be found") || text.contains("cannot find host")
    {
        "域名无法解析（检查网络或代理）"
    } else if text.contains("refused") || text.contains("could not connect") {
        "连接被拒绝（检查代理）"
    } else {
        "无法连接（检查网络或代理）"
    }
}

fn release_body(url: &str) -> Result<Vec<u8>, CheckFailure> {
    let host = reqwest::Url::parse(url).ok().and_then(|url| url.host_str().map(String::from)).unwrap_or_default();
    let request = Request {
        headers: vec![
            ("User-Agent", "Mozilla/5.0 OpenCodex-SwiftBar/4.8".into()),
            ("Accept", "application/json, application/xml, text/plain".into()),
        ],
        timeout: 8.0,
        limit: 1024 * 1024 + 1,
        ..Request::default()
    };
    let response = http::send(url, request)
        .map_err(|failure| CheckFailure::Check(format!("{host} {}", connection_problem(&failure.0))))?;
    if !(200..300).contains(&response.status) {
        return Err(CheckFailure::Check(format!("{host} 返回 HTTP {}", response.status)));
    }
    if response.body.len() > 1024 * 1024 {
        return Err(FileError::new("release metadata too large").into());
    }
    Ok(response.body)
}

/// The machine name Sparkle compares with an item's hardware requirements.
pub fn machine() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        other => other,
    }
}

/// The macOS version, as "26.1" or "26.1.2"; empty elsewhere.
pub fn mac_version() -> String {
    #[cfg(target_os = "macos")]
    {
        let mut buffer = [0u8; 64];
        let mut size = buffer.len();
        // SAFETY: the buffer and its size are passed together; the name is a valid C string.
        let result = unsafe {
            libc::sysctlbyname(
                c"kern.osproductversion".as_ptr(),
                buffer.as_mut_ptr().cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if result == 0 {
            let text = String::from_utf8_lossy(&buffer[..size.saturating_sub(1)]).into_owned();
            let parts: Vec<&str> = text.split('.').collect();
            return if parts.len() > 2 && parts[2] == "0" { parts[..2].join(".") } else { text };
        }
        String::new()
    }
    #[cfg(not(target_os = "macos"))]
    {
        String::new()
    }
}

/// The newest stable release in a Sparkle appcast this Mac can run: its version and build.
pub fn sparkle_release(body: &[u8]) -> (Option<String>, Option<String>) {
    let Ok(text) = std::str::from_utf8(body) else { return (None, None) };
    let Ok(document) = roxmltree::Document::parse(text) else { return (None, None) };
    let root = document.root_element();
    if root.tag_name().name() != "rss" {
        return (None, None);
    }
    let mut candidates: Vec<(String, String)> = Vec::new();
    let mac = mac_version();
    for channel in root.children().filter(|node| node.is_element() && node.tag_name().name() == "channel") {
        for item in channel.children().filter(|node| node.is_element() && node.tag_name().name() == "item") {
            let mut fields: HashMap<String, String> = HashMap::new();
            for child in item.children().filter(|node| node.is_element()) {
                let name = child.tag_name();
                let sparkle = name.namespace().is_some_and(|space| space.contains("sparkle"));
                if child.tag_name().name() == "enclosure" && !sparkle {
                    fields.insert("enclosure".into(), "1".into());
                    for attribute in child.attributes() {
                        if attribute.namespace().is_some_and(|space| space.contains("sparkle")) {
                            fields.insert(format!("enclosure:{}", attribute.name()), attribute.value().to_string());
                        }
                    }
                } else if sparkle {
                    let text: String = child.descendants().filter(|n| n.is_text()).filter_map(|n| n.text()).collect();
                    fields.entry(name.name().to_string()).or_insert_with(|| text.trim().to_string());
                }
            }
            let mut version = fields.get("shortVersionString").cloned();
            let mut build = fields.get("version").cloned();
            if let Some(minimum) = fields.get("minimumSystemVersion").filter(|m| !m.is_empty())
                && let (Some(need), Some(have)) = (
                    release_version(Some(&Json::from(minimum.as_str()))),
                    release_version(Some(&Json::from(mac.as_str()))),
                )
                && have < need
            {
                continue;
            }
            if let Some(arch) = fields.get("hardwareRequirements").filter(|a| !a.is_empty())
                && !arch.split(',').any(|part| part == machine())
            {
                continue;
            }
            if let Some(channel) = fields.get("channel")
                && !["", "stable", "release"].contains(&channel.as_str())
            {
                continue;
            }
            if fields.contains_key("enclosure") {
                if version.as_deref().is_none_or(str::is_empty) {
                    version = fields.get("enclosure:shortVersionString").cloned();
                }
                if build.as_deref().is_none_or(str::is_empty) {
                    build = fields.get("enclosure:version").cloned();
                }
            }
            if let (Some(version), Some(build)) = (version, build)
                && release_version(Some(&Json::from(version.as_str()))).is_some()
                && release_version(Some(&Json::from(build.as_str()))).is_some()
            {
                candidates.push((version, build));
            }
        }
    }
    let mut best: Option<(String, String)> = None;
    for candidate in candidates {
        let better = best.as_ref().is_none_or(|(_, best_build)| {
            release_version(Some(&Json::from(best_build.as_str())))
                < release_version(Some(&Json::from(candidate.1.as_str())))
        });
        if better {
            best = Some(candidate);
        }
    }
    best.map_or((None, None), |(version, build)| (Some(version), Some(build)))
}

/// The package channel Codex came from, from its executable alone.
pub fn codex_channel(binary: &str) -> Object {
    let path =
        paths::resolve(Path::new(binary)).map_or_else(|| binary.to_string(), |p| p.to_string_lossy().into_owned());
    let parent_of = |path: &str| path.rsplit_once('/').map_or(String::new(), |(parent, _)| parent.to_string());
    let last = |path: &str| path.rsplit('/').next().unwrap_or("").to_string();
    let mut parent = parent_of(&path);
    while parent != "/" && !parent.is_empty() {
        let name = last(&parent);
        let above = last(&parent_of(&parent));
        if name == "codex" && above == "Caskroom" {
            let receipt = claude_app::read_object(&format!("{parent}/.metadata/INSTALL_RECEIPT.json"));
            if py::dict(receipt.get("source")).get("tap") != Some(&Json::from("homebrew/cask")) {
                return object! { "channel" => "Homebrew 自定义 Cask" };
            }
            return object! { "channel" => "Homebrew Cask", "channelId" => "brew-cask", "releaseURL" => "https://formulae.brew.sh/api/cask/codex.json" };
        }
        if name == "codex" && above == "Cellar" {
            let receipt = claude_app::read_object(&format!("{}/INSTALL_RECEIPT.json", parent_of(&parent_of(&path))));
            if py::dict(receipt.get("source")).get("tap") != Some(&Json::from("homebrew/core")) {
                return object! { "channel" => "Homebrew 自定义 Formula" };
            }
            return object! {
                "channel" => "Homebrew Formula", "channelId" => "brew-formula", "releaseURL" => "https://formulae.brew.sh/api/formula/codex.json",
            };
        }
        if name == "codex"
            && above == "@openai"
            && claude_app::read_object(&format!("{parent}/package.json")).get("name")
                == Some(&Json::from("@openai/codex"))
        {
            return object! { "channel" => "npm", "channelId" => "npm", "releaseURL" => "https://registry.npmjs.org/@openai/codex/latest" };
        }
        parent = parent_of(&parent);
    }
    object! { "channel" => "安装渠道待确认" }
}

fn latest_version(channel: &Object, body: &[u8]) -> Result<String, FileError> {
    let data = Json::parse(body)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .ok_or_else(|| FileError::new("bad metadata"))?;
    if channel.get("channelId") == Some(&Json::from("brew-formula")) {
        let stable = data
            .get("versions")
            .and_then(|versions| versions.get("stable"))
            .ok_or_else(|| FileError::new("KeyError"))?;
        return Ok(py::str(Some(stable)));
    }
    data.get("version").map(|version| py::str(Some(version))).ok_or_else(|| FileError::new("KeyError"))
}

/// ChatGPT's own Sparkle metadata: a download it already staged, or the official appcast it cached.
pub fn chatgpt_release(home: &str, bundle: Option<&Json>, now: f64) -> Option<Object> {
    if bundle != Some(&Json::from("com.openai.codex")) {
        return None;
    }
    let root = format!("{home}/Library/Caches/com.openai.codex");
    let mut candidates: Vec<Object> = Vec::new();
    let installation = format!("{root}/org.sparkle-project.Sparkle/Installation");
    let sorted = |path: &str| -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(path)
            .map(|entries| {
                entries.filter_map(Result::ok).map(|e| e.file_name().to_string_lossy().into_owned()).collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    };
    let mut seen = 0;
    'outer: for a in sorted(&installation) {
        for b in sorted(&format!("{installation}/{a}")) {
            for app in sorted(&format!("{installation}/{a}/{b}")).into_iter().filter(|app| app.ends_with(".app")) {
                seen += 1;
                if seen > 32 {
                    break 'outer;
                }
                let Ok(data) = std::fs::read(format!("{installation}/{a}/{b}/{app}/Contents/Info.plist")) else {
                    continue;
                };
                if data.len() > 1024 * 1024 {
                    continue;
                }
                let Some(info) =
                    plist_strings(&data, &["CFBundleIdentifier", "CFBundleShortVersionString", "CFBundleVersion"])
                else {
                    continue;
                };
                let version = info.get("CFBundleShortVersionString").cloned().unwrap_or_default();
                let build = info.get("CFBundleVersion").cloned().unwrap_or_default();
                if info.get("CFBundleIdentifier").map(String::as_str) == Some("com.openai.codex")
                    && plain_version(Some(&version)).is_some()
                    && plain_version(Some(&build)).is_some()
                {
                    candidates.push(object! { "latest" => version, "build" => build, "releaseSource" => "client-download", "observedAt" => now });
                }
            }
        }
    }
    if let Some(feed) = cached_feed(&root, now) {
        candidates.push(feed);
    }
    let build_of = |row: &Object| plain_version(row.get("build").and_then(Json::as_str));
    let mut best: Option<Object> = None;
    for row in candidates {
        if best.as_ref().is_none_or(|best| build_of(best) < build_of(&row)) {
            best = Some(row);
        }
    }
    best
}

fn cached_feed(root: &str, now: f64) -> Option<Object> {
    let database_path = format!("{root}/Cache.db");
    if !Path::new(&database_path).exists() {
        return None;
    }
    let db = Database::open(Path::new(&format!("file:{database_path}?mode=ro")), 0.2, Mode::ReadOnlyUri).ok()?;
    let rows = db
        .rows(
            "SELECT r.request_key,r.time_stamp,d.isDataOnFS,d.receiver_data FROM cfurl_cache_response r JOIN cfurl_cache_receiver_data d USING(entry_ID)
            WHERE r.request_key LIKE 'https://persistent.oaistatic.com/codex-app-prod/appcast%' AND length(d.receiver_data)<=? ORDER BY r.time_stamp DESC LIMIT 12",
            &[Value::Integer(1024 * 1024)],
        )
        .ok()?;
    for row in rows {
        let Some(url) = database::text(&row[0]).and_then(|text| reqwest::Url::parse(text).ok()) else { continue };
        if url.host_str() != Some("persistent.oaistatic.com")
            || url.port().is_some_and(|port| port != 443)
            || !url.username().is_empty()
            || url.password().is_some()
        {
            continue;
        }
        let path = url.path();
        if !path.starts_with("/codex-app-prod/appcast") || !path.ends_with(".xml") {
            continue;
        }
        let time = database::text(&row[1]).and_then(|stamp| IsoTime::parse(stamp).ok())?;
        let mut utc = time;
        utc.offset = Some(0);
        let checked = utc.timestamp(&jiff::tz::TimeZone::UTC);
        if !(-60.0..=7.0 * 86400.0).contains(&(now - checked)) {
            return None;
        }
        let body = if database::int(&row[2]).unwrap_or(0) != 0 {
            let name = database::blob(&row[3])
                .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
                .or_else(|| database::text(&row[3]).map(String::from))
                .unwrap_or_default();
            if !(1..=80).contains(&name.chars().count()) || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
            {
                return None;
            }
            let file = format!("{root}/fsCachedData/{name}");
            if std::fs::symlink_metadata(&file).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
                return None;
            }
            let data = std::fs::read(&file).ok()?;
            if data.len() > 1024 * 1024 {
                return None;
            }
            data
        } else {
            database::blob(&row[3])?.to_vec()
        };
        let (latest, build) = sparkle_release(&body);
        if let (Some(latest), Some(build)) = (latest, build)
            && plain_version(Some(&latest)).is_some()
            && plain_version(Some(&build)).is_some()
        {
            return Some(
                object! { "latest" => latest, "build" => build, "releaseSource" => "client-feed", "observedAt" => checked },
            );
        }
        return None;
    }
    None
}

pub fn update_link(key: &str) -> &'static str {
    match key {
        "opencodex" => "https://github.com/lidge-jun/opencodex/releases/latest",
        "claude" => "https://claude.ai/download",
        "chatgpt" => "https://chatgpt.com/download",
        "claude-code" => "https://github.com/anthropics/claude-code/releases",
        "codex-cli" => "https://github.com/openai/codex/releases",
        "magpie" => "https://github.com/yetone/magpie/releases/latest",
        "opencode" => "https://opencode.ai",
        "deepseek-harness" => "https://github.com/deepseek-ai/deepseek-harness/releases",
        "workbuddy" => "https://cloud.tencent.com/product/workbuddy",
        "qoder" => "https://qoder.cn/download",
        "gemini" => "https://github.com/google-gemini/gemini-cli/releases",
        "cursor" => "https://cursor.com/downloads",
        _ => "",
    }
}

pub fn update_name(key: &str) -> &'static str {
    match key {
        "opencodex" => "OpenCodex",
        "claude" => "Claude",
        "chatgpt" => "ChatGPT",
        "claude-code" => "Claude Code",
        "codex-cli" => "Codex",
        "magpie" => "Magpie",
        "opencode" => "OpenCode",
        "deepseek-harness" => "DeepSeek Harness",
        "workbuddy" => "WorkBuddy",
        "qoder" => "Qoder",
        "gemini" => "Gemini CLI",
        "cursor" => "Cursor",
        _ => "",
    }
}

pub const MANUAL_UPDATES: [&str; 7] =
    ["magpie", "opencode", "deepseek-harness", "workbuddy", "qoder", "gemini", "cursor"];

pub fn update_apps(key: &str) -> &'static [&'static str] {
    match key {
        "magpie" => &["magpie", "Magpie"],
        "claude" => &["Claude"],
        "chatgpt" => &["ChatGPT"],
        "workbuddy" => &["WorkBuddy"],
        "qoder" => &["Qoder CN", "QoderCN"],
        "cursor" => &["Cursor"],
        _ => &[],
    }
}

fn harness_update(key: &str) -> &'static str {
    match key {
        "opencodex" => "opencodex",
        "magpie" => "magpie",
        "claude" => "claude",
        "codex" => "chatgpt",
        "opencode" => "opencode",
        "deepseek-harness" => "deepseek-harness",
        "workbuddy" => "workbuddy",
        "qoder" => "qoder",
        "gemini" => "gemini",
        "cursor" => "cursor",
        _ => "",
    }
}

/// The first version number in a CLI's --version output, by the plugin's own pattern.
pub fn first_version(text: &str) -> Option<String> {
    static PATTERN: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"\b(\d+(?:\.\d+){1,4}(?:[-+][a-zA-Z0-9.-]+)?)\b").expect("version pattern")
    });
    PATTERN.captures(text).and_then(|found| found.get(1)).map(|found| found.as_str().to_string())
}

/// uuid.uuid5(uuid.NAMESPACE_URL, name), the constant anonymous checker ID.
pub fn uuid5_url(name: &str) -> String {
    let namespace: [u8; 16] =
        [0x6b, 0xa7, 0xb8, 0x11, 0x9d, 0xad, 0x11, 0xd1, 0x80, 0xb4, 0x00, 0xc0, 0x4f, 0xd4, 0x30, 0xc8];
    let mut hasher = Sha1::new();
    hasher.update(namespace);
    hasher.update(name.as_bytes());
    let mut digest: Vec<u8> = hasher.finalize()[..16].to_vec();
    digest[6] = digest[6] & 0x0F | 0x50;
    digest[8] = digest[8] & 0x3F | 0x80;
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &hex[..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..])
}

impl Runtime {
    pub fn app_info(&self, name: &str) -> Object {
        self.memoized(&format!("app:{name}"), || app_info(name, &self.home))
    }

    /// The update checks the selected Harness call for; a configured OpenCodex adds ChatGPT and Codex.
    pub fn selected_update_ids(&self) -> Vec<String> {
        let settings = self.settings();
        let mut visible: Vec<String> = settings
            .get("visible")
            .and_then(Json::as_array)
            .map(|v| v.iter().filter_map(Json::as_str).map(String::from).collect())
            .unwrap_or_default();
        if self.opencodex_configured() || visible.iter().any(|key| key == "opencodex") {
            visible.push("codex".into());
        }
        let mut result = Vec::new();
        for key in USAGE {
            if visible.iter().any(|shown| shown == key) {
                result.push(harness_update(key).to_string());
                if key == "claude" {
                    result.push("claude-code".into());
                }
                if key == "codex" {
                    result.push("codex-cli".into());
                }
            }
        }
        result
    }

    pub fn live_app_update_ids(&self) -> Vec<String> {
        // A desktop host checks only the CLIs; its desktop apps update themselves.
        if self.desktop {
            return self
                .selected_update_ids()
                .into_iter()
                .filter(|key| key == "claude-code" || key == "codex-cli")
                .collect();
        }
        self.selected_update_ids()
            .into_iter()
            .filter(|key| key != "opencodex" && !MANUAL_UPDATES.contains(&key.as_str()))
            .collect()
    }

    /// What the update checks depend on: which apps and CLIs are checked, their versions and install channels.
    pub fn app_update_connection(&self) -> Vec<Json> {
        let selected = self.selected_update_ids();
        let mut parts: Vec<Json> = selected.iter().map(|key| Json::from(key.as_str())).collect();
        parts.push(Json::Bool(self.module_enabled("updates")));
        parts.push("channels-v3".into());
        for (key, name) in [("claude", "Claude"), ("chatgpt", "ChatGPT")] {
            if !selected.iter().any(|selected| selected == key) {
                continue;
            }
            let info = self.app_info(name);
            parts.push(Json::Object(info.clone()));
            if key == "chatgpt" {
                let local = chatgpt_release(&self.home, info.get("CFBundleIdentifier"), now()).unwrap_or_default();
                let kept: Object = ["latest", "build", "releaseSource"]
                    .iter()
                    .map(|k| (*k, local.get(k).cloned().unwrap_or(Json::Null)))
                    .collect();
                parts.push(Json::Object(kept));
            }
        }
        for (key, command) in [("claude-code", "claude"), ("codex-cli", "codex")] {
            if !selected.iter().any(|selected| selected == key) {
                continue;
            }
            let binary = shell::which(command, &self.environment);
            parts.push(binary.clone().map(Json::from).unwrap_or(Json::Null));
            match binary
                .as_ref()
                .and_then(|binary| crate::claude_usage::state_of(Path::new(binary)).map(|state| (binary, state)))
            {
                Some((binary, state)) => {
                    let resolved = paths::resolve(Path::new(binary))
                        .map_or_else(|| binary.clone(), |p| p.to_string_lossy().into_owned());
                    parts.extend([
                        Json::from(resolved),
                        Json::Int(state.inode),
                        Json::Int(state.mtime),
                        Json::Int(state.size),
                    ]);
                }
                None => parts.push("unavailable".into()),
            }
            if key == "codex-cli"
                && let Some(binary) = &binary
            {
                parts.push(Json::Object(codex_channel(binary)));
            }
        }
        parts
    }

    /// A client-specific rollout or download wins over anonymous metadata.
    fn apply_chatgpt_release(&self, given: Object, info: &Object) -> Object {
        let mut row = given;
        if info.get("CFBundleIdentifier") != Some(&Json::from("com.openai.codex")) {
            return row;
        }
        let installed = info.get("CFBundleVersion").cloned().unwrap_or(Json::Null);
        row.insert("current", info.get("CFBundleShortVersionString").cloned().unwrap_or(Json::Null));
        row.insert("currentBuild", installed.clone());
        let local = chatgpt_release(&self.home, info.get("CFBundleIdentifier"), now());
        if let Some(local) = local.filter(|local| newer(local.get("build"), Some(&installed))) {
            if !row.get("build").is_some_and(Json::is_truthy) || !newer(row.get("build"), local.get("build")) {
                for (key, value) in local.iter() {
                    row.insert(key.clone(), value.clone());
                }
                row.insert("available", true);
                row.insert("status", "有新版本");
                row.insert("checkedAt", now());
                let channel = if local.get("releaseSource") == Some(&Json::from("client-download")) {
                    "已下载"
                } else {
                    "客户端更新源"
                };
                row.insert("channel", channel);
            }
        } else if row
            .get("releaseSource")
            .is_some_and(|source| *source == Json::from("client-feed") || *source == Json::from("client-download"))
        {
            // The app was updated or its staged download disappeared: no old alert.
            row.insert("available", false);
            row.insert("status", "等待检查");
            for key in ["latest", "build", "releaseSource", "channel"] {
                row.remove(key);
            }
        }
        if row.get("releaseSource") == Some(&Json::from("public")) {
            let available = newer(row.get("build"), Some(&installed));
            row.insert("available", available);
            if !py::str(Some(row.get("status").unwrap_or(&Json::from("")))).contains("失败") {
                row.insert("status", if available { "有新版本" } else { "公开渠道暂无更新" });
            }
        }
        row
    }

    fn app_update_probe(&self, key: &str) -> Object {
        let mut row =
            object! { "id" => key, "name" => update_name(key), "href" => update_link(key), "checkedAt" => now() };
        let checked = (|| -> Result<(), CheckFailure> {
            if key == "claude-code" || key == "codex-cli" {
                let Some(binary) =
                    shell::which(if key == "claude-code" { "claude" } else { "codex" }, &self.environment)
                else {
                    row.insert("status", "未安装");
                    return Ok(());
                };
                let environment: Vec<(String, String)> =
                    self.environment.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                let output = shell::run(&[&binary, "--version"], 4.0, Some(&environment), None)
                    .ok_or_else(|| FileError::of_kind("SubprocessError", "SubprocessError"))?;
                row.insert("current", first_version(&output).unwrap_or_default());
                if key == "claude-code" {
                    row.insert("channel", "Claude 官方");
                    let body = release_body("https://downloads.claude.ai/claude-code-releases/latest")?;
                    row.insert("latest", String::from_utf8_lossy(&body).trim().to_string());
                } else {
                    let channel = codex_channel(&binary);
                    row.insert("channel", channel.get("channel").cloned().unwrap_or(Json::Null));
                    let Some(url) = channel.get("releaseURL").and_then(Json::as_str) else {
                        row.insert("status", "更新渠道待确认");
                        row.insert("available", false);
                        return Ok(());
                    };
                    row.insert("latest", latest_version(&channel, &release_body(url)?)?);
                }
                if release_version(row.get("current")).is_none() || release_version(row.get("latest")).is_none() {
                    return Err(FileError::new("invalid version").into());
                }
                let available = newer(row.get("latest"), row.get("current"));
                row.insert("available", available);
                row.insert("status", if available { "有新版本" } else { "已是最新" });
                return Ok(());
            }
            let info = self.app_info(if key == "claude" { "Claude" } else { "ChatGPT" });
            if info.is_empty() {
                row.insert("status", "未安装");
                return Ok(());
            }
            row.insert("current", info.get("CFBundleShortVersionString").cloned().unwrap_or(Json::Null));
            // A constant anonymous checker ID meets the public feed schema without sending a device ID.
            let ident = uuid5_url("opencodex-swiftbar-update-check");
            let current = py::str(row.get("current"));
            if key == "claude" {
                let query = format!(
                    "device_id={}&version={}&os_version={}",
                    form_quote(&ident),
                    form_quote(&current),
                    form_quote(&mac_version())
                );
                let raw = release_body(&format!(
                    "https://api.anthropic.com/api/desktop/darwin/universal/squirrel/update?{query}"
                ))?;
                if raw.is_empty() {
                    row.insert("latest", row.get("current").cloned().unwrap_or(Json::Null));
                } else {
                    let release = Json::parse(&raw)
                        .ok()
                        .and_then(|value| value.get("currentRelease").cloned())
                        .ok_or_else(|| FileError::new("KeyError"))?;
                    row.insert("latest", release);
                }
            } else {
                if info.get("CFBundleIdentifier") != Some(&Json::from("com.openai.codex")) {
                    row.insert("status", "更新渠道待确认");
                    return Ok(());
                }
                let query: Vec<String> = [
                    ("installation_id", ident.clone()),
                    ("arch", machine().to_string()),
                    ("app_version", current.clone()),
                    ("beta", "false".to_string()),
                    ("os-version", mac_version()),
                    ("plan_type", "unknown".to_string()),
                ]
                .iter()
                .map(|(key, value)| format!("{key}={}", form_quote(value)))
                .collect();
                let (latest, build) = sparkle_release(&release_body(&format!(
                    "https://updates.oaistatic.com/codex/app/appcast?{}",
                    query.join("&")
                ))?);
                let (Some(latest), Some(build)) = (latest, build) else {
                    return Err(FileError::new("no compatible release").into());
                };
                row.insert("latest", latest);
                row.insert("build", build);
                row.insert("currentBuild", info.get("CFBundleVersion").cloned().unwrap_or(Json::Null));
                row.insert("releaseSource", "public");
            }
            if release_version(row.get("latest")).is_none() || release_version(row.get("current")).is_none() {
                return Err(FileError::new("invalid version").into());
            }
            let mut available = newer(row.get("latest"), row.get("current"));
            if row.get("build").is_some_and(Json::is_truthy) {
                available = newer(row.get("build"), row.get("currentBuild"));
            }
            row.insert("available", available);
            row.insert("status", if available { "有新版本" } else { "已是最新" });
            Ok(())
        })();
        if let Err(failure) = checked {
            // The reason is shown under the row: it tells a missing certificate from a blocked network.
            let reason = match failure {
                CheckFailure::Check(reason) => reason,
                CheckFailure::File(error) if error.kind == "SubprocessError" => "读取本机版本失败".into(),
                CheckFailure::File(_) => "版本信息无法识别".into(),
            };
            row.insert("status", "检查失败，稍后重试");
            row.insert("available", false);
            row.insert("reason", reason);
        }
        if key == "chatgpt" {
            row = self.apply_chatgpt_release(row, &self.app_info("ChatGPT"));
        }
        row
    }

    pub fn app_updates_cached(&self) -> Loaded {
        if !self.module_enabled("updates") {
            return (Some(object! { "apps" => Json::Array(vec![]) }), None);
        }
        let selected = self.live_app_update_ids();
        if selected.is_empty() {
            return (Some(object! { "apps" => Json::Array(vec![]) }), None);
        }
        let (result, error) = self.cached_fetch("app_updates", 21600.0, 60.0, || {
            let rows: Vec<Option<Object>> = std::thread::scope(|scope| {
                let handles: Vec<_> = selected
                    .iter()
                    .map(|key| {
                        scope.spawn(move || {
                            // Preferences can change while another check is still in flight.
                            if !self.module_enabled("updates") || !self.live_app_update_ids().contains(key) {
                                return None;
                            }
                            Some(self.app_update_probe(key))
                        })
                    })
                    .collect();
                handles.into_iter().map(|handle| handle.join().ok().flatten()).collect()
            });
            Ok((Some(object! { "apps" => Json::Array(rows.into_iter().flatten().map(Json::Object).collect()) }), None))
        });
        let failed =
            result.as_ref().and_then(|result| result.get("apps")).and_then(Json::as_array).is_some_and(|apps| {
                apps.iter().any(|app| py::str(Some(app.get("status").unwrap_or(&Json::from("")))).contains("失败"))
            });
        if failed {
            let mut record = self.read_cache("app_updates");
            record.insert("force", true);
            record.insert("retryAt", now() + 600.0);
            self.write_json_cache(&self.cache_path("app_updates"), &Json::Object(record));
        }
        (result, error)
    }

    // OpenCodex's own installer

    fn auto_update_path(&self) -> String {
        format!("{}/auto-update.json", self.state_dir)
    }

    pub fn auto_update_state(&self) -> Object {
        let state = claude_app::read_object_limited(&self.auto_update_path(), 2 * 1024 * 1024);
        if state.get("schema") == Some(&Json::Int(1)) { state } else { Object::new() }
    }

    /// OpenCodex's stable-channel installer only, never the other apps. The intent is saved before the request; an
    /// unclear answer is settled from the job and health endpoints, never by asking a second time.
    pub fn auto_update_opencodex(&self) {
        let path = self.auto_update_path();
        let _ = files::make_directory(Path::new(&self.state_dir), 0o700);
        let Some(_lock) = platform::lock(Path::new(&format!("{path}.lock")), false) else { return };
        let moment = now();
        let mut state = self.auto_update_state();
        let save = |state: &mut Object, values: Object| -> bool {
            state.insert("schema", 1);
            state.insert("checkedAt", moment);
            for (key, value) in values {
                state.insert(key, value);
            }
            self.write_json_cache(&path, &Json::Object(state.clone()));
            self.auto_update_state() == *state
        };
        let finish = |state: &mut Object, ok: bool, message: &str| {
            let retry = if ok { Json::Int(0) } else { Json::Double(moment + 3600.0) };
            if !save(
                state,
                object! { "status" => if ok { "succeeded" } else { "failed" }, "message" => message, "retryAt" => retry },
            ) {
                return;
            }
            let notice_key = format!(
                "{}{}",
                py::str(Some(state.get("target").unwrap_or(&Json::Null))),
                if ok { "::success" } else { "::failure" }
            );
            // Every notice is titled Starmeter, so the body names the program it updated.
            if self.module_enabled("opencodex") && state.get("noticeKey") != Some(&Json::from(notice_key.as_str())) {
                self.notify(&format!("OpenCodex {message}"), "OpenCodex 自动更新");
                save(state, object! { "noticeKey" => notice_key });
            }
            self.invalidate_caches(&["badge"]);
        };
        let active = state.get("status") == Some(&Json::from("running"));
        let record = self.read_cache("badge");
        let badge = record.get("payload").and_then(Json::as_object).cloned().unwrap_or_default();
        let number = |value: Option<&Json>| value.and_then(Json::python_float).unwrap_or(0.0);
        if !active
            && (!self.module_enabled("opencodex")
                || moment - number(state.get("checkedAt")) < 30.0
                || moment - number(record.get("fetchedAt")) > 3600.0
                || !badge.get("updateAvailable").is_some_and(Json::is_truthy)
                || !newer(badge.get("latestVersion"), badge.get("currentVersion"))
                || (state.get("target") == badge.get("latestVersion") && moment < number(state.get("retryAt"))))
        {
            return;
        }
        let (health, error) = self.opencodex_request("/healthz", "GET", None, 3.0, false);
        let current = health.as_ref().and_then(|health| health.get("version")).cloned();
        if active {
            let target = state.get("target").cloned();
            let job_query = state
                .get("jobId")
                .filter(|id| id.is_truthy())
                .map_or_else(String::new, |id| format!("?jobId={}", quote(&py::str(Some(id)))));
            let (result, _) = self.opencodex_request(&format!("/api/update/status{job_query}"), "GET", None, 5.0, true);
            let job =
                py::dict(result.as_ref().and_then(|result| result.get("job"))).as_object().cloned().unwrap_or_default();
            let matched = job.get("latestVersion") == target.as_ref();
            let installed = current.as_ref().is_some_and(Json::is_truthy)
                && target.as_ref().is_some_and(Json::is_truthy)
                && !newer(target.as_ref(), current.as_ref());
            if installed {
                let healthy_since =
                    state.get("healthySince").filter(|v| v.is_truthy()).and_then(Json::python_float).unwrap_or(moment);
                if (matched && job.get("status") == Some(&Json::from("succeeded"))) || moment - healthy_since >= 15.0 {
                    let version = py::str(current.as_ref());
                    finish(&mut state, true, &format!("已自动更新至 {version}"));
                    return;
                }
                save(
                    &mut state,
                    object! { "healthySince" => healthy_since, "message" => "新版本已启动，正在确认运行状态" },
                );
                return;
            }
            if matched && job.get("status") == Some(&Json::from("failed")) {
                finish(&mut state, false, "自动更新未完成，1 小时后重试");
                return;
            }
            if moment - state.get("startedAt").and_then(Json::python_float).unwrap_or(moment) > 1800.0 {
                finish(&mut state, false, "更新结果暂未确认，1 小时后重新检查");
                return;
            }
            let message = if job.get("status") == Some(&Json::from("restarting")) {
                "正在重启并确认版本"
            } else {
                "正在自动更新"
            };
            save(&mut state, object! { "healthySince" => 0, "message" => message });
            return;
        }
        let Some(current) = current.filter(|current| error.is_none() && current.is_truthy()) else { return };
        let target = badge.get("latestVersion").cloned();
        if !newer(target.as_ref(), Some(&current)) {
            return;
        }
        if badge.get("canUpdate") != Some(&Json::Bool(true)) {
            save(
                &mut state,
                object! { "target" => target, "status" => "blocked", "message" => "此安装方式需手动更新", "retryAt" => moment + 3600.0 },
            );
            return;
        }
        let (status, status_error) = self.opencodex_request("/api/update/status", "GET", None, 5.0, true);
        let job =
            py::dict(status.as_ref().and_then(|status| status.get("job"))).as_object().cloned().unwrap_or_default();
        if status_error.as_ref().is_some_and(|error| error.code != 404) {
            return;
        }
        if job.get("status") == Some(&Json::from("running")) || job.get("status") == Some(&Json::from("restarting")) {
            // Track an existing installer, including one started outside this plugin.
            save(
                &mut state,
                object! {
                    "target" => py::or(job.get("latestVersion"), target.clone().unwrap_or(Json::Null)), "status" => "running",
                    "jobId" => job.get("id").cloned().unwrap_or(Json::Null), "startedAt" => moment, "healthySince" => 0, "message" => "正在自动更新",
                },
            );
            return;
        }
        if !self.module_enabled("opencodex") {
            return;
        }
        let intent = object! {
            "target" => target.clone(), "previous" => current, "status" => "running", "startedAt" => moment, "healthySince" => 0,
            "jobId" => Json::Null, "message" => "正在自动更新",
        };
        if !save(&mut state, intent) {
            return;
        }
        let body = Json::Object(object! { "tag" => "latest", "restart" => true });
        let (run, run_error) = self.opencodex_request("/api/update/run", "POST", Some(&body), 30.0, true);
        let started = py::dict(run.as_ref().and_then(|run| run.get("job"))).as_object().cloned().unwrap_or_default();
        if let Some(id) = started.get("id").filter(|id| id.is_truthy()) {
            save(
                &mut state,
                object! { "jobId" => id.clone(), "target" => py::or(started.get("latestVersion"), target.unwrap_or(Json::Null)) },
            );
        } else if let Some(run_error) = run_error {
            // A timeout may mean the proxy is already restarting: keep the intent and look again next time.
            if (400..500).contains(&run_error.code) {
                finish(&mut state, false, "自动更新暂未启动，1 小时后重试");
            } else {
                save(&mut state, object! { "message" => "正在确认更新任务" });
            }
        }
    }
}
