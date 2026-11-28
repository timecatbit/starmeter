//! The background worker: which sources are configured, which reads are due, and running them a few at a time
//! under one lock; plus the small reads it owns (service status pages, the yuan rate, the price tables).

use std::collections::{BTreeSet, VecDeque};
use std::path::Path;
use std::sync::Mutex;

use crate::client_accounts::{chatgpt, qoder};
use crate::http::{self, Request};
use crate::json::{Json, Object};
use crate::runtime::{self, Loaded, PRICING_TABLES, RANGES, Runtime, VERSION, now};
use crate::{accounts, catalog, claude_app, claude_subscription, cursor, files, keys, object, opencodex};
use crate::{platform, preferences, pricing, py, shell};

/// Official service status pages, read only when the user turns 服务状态 on. Each is a public Statuspage
/// summary (its status, the parts not working and the incidents open now); nothing about the user is sent, only a
/// plain User-Agent.
pub mod service_status {
    use super::*;

    pub const PAGES: [(&str, &str, &str, &str); 3] = [
        ("claude", "Claude", "https://status.claude.com/api/v2/summary.json", "https://status.claude.com"),
        ("openai", "OpenAI", "https://status.openai.com/api/v2/summary.json", "https://status.openai.com"),
        ("cursor", "Cursor", "https://status.cursor.com/api/v2/summary.json", "https://status.cursor.com"),
    ];
    pub const MODULE_PAGES: [(&str, &str); 9] = [
        ("claude", "claude"),
        ("claude-subscription", "claude"),
        ("claude-api", "claude"),
        ("codex", "openai"),
        ("opencodex", "openai"),
        ("chatgpt", "openai"),
        ("chatgpt-api", "openai"),
        ("cursor", "cursor"),
        ("cursor-subscription", "cursor"),
    ];
    pub const INDICATORS: [(&str, &str); 5] = [
        ("none", ""),
        ("minor", "服务部分降级"),
        ("major", "服务大范围故障"),
        ("critical", "服务严重故障"),
        ("maintenance", "服务维护中"),
    ];

    /// What an open incident or maintenance is doing, in the panel's words; one not listed (resolved) isn't shown.
    pub const INCIDENT_STATES: [(&str, &str); 6] = [
        ("investigating", "正在调查"),
        ("identified", "已找到原因"),
        ("monitoring", "已修复，观察中"),
        ("in_progress", "维护中"),
        ("verifying", "维护验证中"),
        ("scheduled", "计划维护"),
    ];
    /// A part of the service that isn't working fully.
    pub const COMPONENT_STATES: [(&str, &str); 4] = [
        ("degraded_performance", "性能下降"),
        ("partial_outage", "部分中断"),
        ("major_outage", "大范围中断"),
        ("under_maintenance", "维护中"),
    ];

    /// (name, status document, page) of one page.
    pub fn page(key: &str) -> Option<(&'static str, &'static str, &'static str)> {
        PAGES.iter().find(|page| page.0 == key).map(|page| (page.1, page.2, page.3))
    }

    pub fn module_page(module: &str) -> Option<&'static str> {
        MODULE_PAGES.iter().find(|pair| pair.0 == module).map(|pair| pair.1)
    }

    pub fn indicator(name: &str) -> Option<&'static str> {
        INDICATORS.iter().find(|pair| pair.0 == name).map(|pair| pair.1)
    }

    /// One page's indicator and description, with the incidents and maintenances open now (name, what is being done,
    /// its own page) and the parts not working fully, so a note of trouble says what it is and where to read more
    /// (用户：你不可以给一个信息之后就不管了); or the name of what went wrong.
    pub fn read(key: &str, version: &str) -> Object {
        let Some((_, url, _)) = page(key) else { return object! { "error" => "KeyError" } };
        let headers = vec![("User-Agent", format!("Starmeter/{version}")), ("Accept", "application/json".to_string())];
        let Ok(response) =
            http::send(url, Request { headers, timeout: 10.0, limit: 256 * 1024 + 1, ..Request::default() })
        else {
            return object! { "error" => "URLError" };
        };
        if !(200..300).contains(&response.status) {
            return object! { "error" => "HTTPError" };
        }
        if response.body.len() > 256 * 1024 {
            return object! { "error" => "ValueError" };
        }
        let Some(document) = Json::parse(&response.body).ok().and_then(|value| value.as_object().cloned()) else {
            return object! { "error" => "JSONDecodeError" };
        };
        summary(&document)
    }

    fn clipped(text: &str, most: usize) -> String {
        text.trim().chars().take(most).collect()
    }

    /// A summary document as kept: the status, then up to three open incidents or maintenances and five parts.
    pub fn summary(document: &Object) -> Object {
        let status = py::dict(document.get("status"));
        let (Some(name), Some(description)) =
            (status.get("indicator").and_then(Json::as_str), status.get("description").and_then(Json::as_str))
        else {
            return object! { "error" => "ValueError" };
        };
        if indicator(name).is_none() {
            return object! { "error" => "ValueError" };
        }
        let mut result = object! { "indicator" => name, "description" => clipped(description, 200) };
        let listed = |field: &str| -> Vec<Object> {
            document
                .get(field)
                .and_then(Json::as_array)
                .into_iter()
                .flatten()
                .filter_map(Json::as_object)
                .cloned()
                .collect()
        };
        let incidents: Vec<Json> = listed("incidents")
            .into_iter()
            .chain(listed("scheduled_maintenances"))
            .filter_map(|incident| {
                let state = incident.get("status").and_then(Json::as_str)?;
                INCIDENT_STATES.iter().find(|pair| pair.0 == state && state != "scheduled")?;
                let name = incident.get("name").and_then(Json::as_str).map(|name| clipped(name, 120))?;
                let link = incident
                    .get("shortlink")
                    .and_then(Json::as_str)
                    .filter(|link| link.starts_with("https://"))
                    .map_or(Json::Null, |link| Json::from(clipped(link, 300)));
                Some(object! { "name" => name, "status" => state, "link" => link }.into())
            })
            .take(3)
            .collect();
        if !incidents.is_empty() {
            result.insert("incidents", Json::Array(incidents));
        }
        let parts: Vec<Json> = listed("components")
            .into_iter()
            .filter(|part| part.get("group") != Some(&Json::Bool(true)))
            .filter_map(|part| {
                let state = part.get("status").and_then(Json::as_str)?;
                COMPONENT_STATES.iter().find(|pair| pair.0 == state)?;
                let name = part.get("name").and_then(Json::as_str).map(|name| clipped(name, 60))?;
                Some(object! { "name" => name, "status" => state }.into())
            })
            .take(5)
            .collect();
        if !parts.is_empty() {
            result.insert("components", Json::Array(parts));
        }
        result
    }

    /// What is wrong, a line each: the open incidents and maintenances with what is being done, or else the parts not
    /// working fully; None when the page names neither.
    pub fn trouble(state: &Object) -> Option<String> {
        let named = |field: &str, states: &[(&str, &str)]| -> Vec<String> {
            state
                .get(field)
                .and_then(Json::as_array)
                .into_iter()
                .flatten()
                .filter_map(Json::as_object)
                .filter_map(|item| {
                    let status = item.get("status").and_then(Json::as_str)?;
                    let said = states.iter().find(|pair| pair.0 == status)?.1;
                    Some(format!("{} · {said}", py::str(item.get("name"))))
                })
                .collect()
        };
        let incidents = named("incidents", &INCIDENT_STATES);
        let lines = if incidents.is_empty() { named("components", &COMPONENT_STATES) } else { incidents };
        (!lines.is_empty()).then(|| lines.join("\n"))
    }

    pub fn read_all(wanted: &[String], version: &str) -> Object {
        let mut result = Object::new();
        for key in wanted.iter().filter(|key| page(key).is_some()) {
            result.insert(key.clone(), read(key, version));
        }
        result
    }

    /// A new reading over the last one: a page that failed this time (offline, just woken) keeps the last good answer
    /// it had, marked `kept`; one never read well shows its failure (WAKE-2).
    pub fn merged(previous: &Object, fresh: Object) -> Object {
        let mut out = Object::new();
        for (key, value) in fresh.iter() {
            let failed = value.get("error").is_some_and(Json::is_truthy);
            match previous.get(key).and_then(Json::as_object).filter(|last| failed && last.get("indicator").is_some()) {
                Some(last) => {
                    let mut kept = last.clone();
                    kept.insert("kept", true);
                    out.insert(key.as_str(), Json::Object(kept));
                }
                None => {
                    out.insert(key.as_str(), value.clone());
                }
            }
        }
        out
    }

    /// Whether a page's answer is a failure or a kept one: read again in a minute, not five, and at once on waking.
    pub fn unsettled(payload: &Object) -> bool {
        payload.iter().any(|(_, value)| {
            value.get("error").is_some_and(Json::is_truthy) || value.get("kept").is_some_and(Json::is_truthy)
        })
    }

    /// How long a reading of the pages holds.
    pub fn lifetime(payload: &Object) -> f64 {
        if unsettled(payload) { 60.0 } else { 300.0 }
    }
}

pub const EXTERNAL_TTLS: [(&str, f64); 23] = [
    ("provider_quotas", 300.0),
    ("badge", 3600.0),
    ("komari", 30.0),
    ("app_updates", 21600.0),
    ("self_update", 21600.0),
    ("chatgpt", 300.0),
    ("grok", 300.0),
    ("minimax", 300.0),
    ("kimi-api", 300.0),
    ("qoder-api", 300.0),
    ("claude-api", 1800.0),
    ("claude_subscription", 60.0),
    ("qoder_subscription", 60.0),
    ("cursor_subscription", 300.0),
    ("gemini_subscription", 60.0),
    ("sessions", 30.0),
    ("service_status", 300.0),
    ("exchange_rate", 86400.0),
    ("healthz", 30.0),
    ("ready", 30.0),
    ("quota", 30.0),
    ("accounts", 30.0),
    ("active", 30.0),
];
pub const ACCOUNT_SOURCES: [&str; 5] = ["grok", "minimax", "kimi-api", "qoder-api", "claude-api"];
pub const KEY_TTLS: [(&str, f64); 5] =
    [("deepseek", 300.0), ("kimi-api", 300.0), ("grok", 300.0), ("qoder-api", 300.0), ("claude-api", 1800.0)];
pub const RESOURCE_CACHES: [(&str, &str); 15] = [
    ("deepseek", "provider_quotas"),
    ("chatgpt", "chatgpt"),
    ("grok", "grok"),
    ("minimax", "minimax"),
    ("kimi-api", "kimi-api"),
    ("qoder-api", "qoder-api"),
    ("claude-api", "claude-api"),
    ("komari", "komari"),
    ("updates", "app_updates"),
    ("claude-subscription", "claude_subscription"),
    ("qoder-subscription", "qoder_subscription"),
    ("cursor-subscription", "cursor_subscription"),
    ("gemini-subscription", "gemini_subscription"),
    ("opencodex", "opencodex"),
    ("sessions", "sessions"),
];

pub fn external_ttl(name: &str) -> Option<f64> {
    EXTERNAL_TTLS.iter().find(|pair| pair.0 == name).map(|pair| pair.1)
}

pub fn key_ttl(provider: &str) -> Option<f64> {
    KEY_TTLS.iter().find(|pair| pair.0 == provider).map(|pair| pair.1)
}

const CURRENT_INDEXES: [&str; 7] = [
    "claude-app-usage-v5-",
    "harness-v4-",
    "harness-v6-codex-",
    "harness-v6-qoder-",
    "harness-v6-magpie-",
    "harness-gemini-v5-",
    "harness-gateway-v2-",
];
const RETIRED_INDEXES: [&str; 3] = ["harness-v4-magpie-", "harness-v4-codex-", "harness-v4-qoder-"];

fn retired_cache(name: &str) -> bool {
    ["startup.json", "usage_today.json", "usage_7d.json"].contains(&name)
        || RANGES.iter().any(|(key, _)| name == format!("cursor_usage_{key}.json"))
}

fn is_index_file(name: &str) -> bool {
    let mut base = name;
    for suffix in ["-journal", "-wal", "-shm"] {
        if let Some(stripped) = base.strip_suffix(suffix) {
            base = stripped;
            break;
        }
    }
    if !base.ends_with(".sqlite3") {
        return false;
    }
    ["claude-app-usage-", "claude-usage-", "harness-"]
        .iter()
        .any(|prefix| base.starts_with(prefix) && base.chars().count() > prefix.chars().count() + ".sqlite3".len())
}

/// One background read: its cache name and how to run it.
pub type LoaderJob<'a> = (String, Box<dyn FnOnce() + Send + 'a>);

impl Runtime {
    // What is configured

    pub fn named_keys(&self, provider: &str) -> Vec<Object> {
        self.memoized(&format!("keys:{provider}"), || keys::entries(Path::new(&self.state_dir), provider))
    }

    fn chatgpt_login_present(&self) -> bool {
        let auth = claude_app::read_object(&format!("{}/auth.json", self.codex_home_absolute()));
        let tokens = py::dict(auth.get("tokens"));
        let mode = auth.get("auth_mode");
        chatgpt::binary(&self.app_roots(), &self.environment).is_some()
            && (mode == Some(&Json::from("chatgpt"))
                || mode == Some(&Json::from("chatgptAuthTokens"))
                || tokens.get("access_token").is_some_and(Json::is_truthy))
    }

    pub fn chatgpt_configured(&self) -> bool {
        self.memoized("chatgpt-configured", || self.chatgpt_login_present())
    }

    /// A checked, configured OpenCodex supplies the ChatGPT accounts; otherwise the local login does.
    pub fn chatgpt_source(&self) -> &'static str {
        if self.module_enabled("opencodex") && self.opencodex_configured() { "proxy" } else { "client" }
    }

    /// The DeepSeek key in OpenCodex's own config, when that provider points at the official API.
    pub fn opencodex_deepseek_key(&self) -> String {
        let config = claude_app::read_object(&format!("{}/config.json", self.ocx_home));
        let providers = py::or(config.get("providers"), Json::Object(Object::new()));
        let Some(provider) = providers.get("deepseek").and_then(Json::as_object) else { return String::new() };
        let base = py::str(Some(&py::or(provider.get("baseUrl"), Json::from(""))));
        let base = base.trim_end_matches('/');
        if !["https://api.deepseek.com", "https://api.deepseek.com/v1"].contains(&base) {
            return String::new();
        }
        py::str(Some(&py::or(provider.get("apiKey"), Json::from("")))).trim().to_string()
    }

    pub fn credential(&self, name: &str) -> String {
        accounts::credential(name, &self.environment)
    }

    fn given_trimmed(&self, name: &str) -> bool {
        !self.environment.get(name).map(|value| value.trim()).unwrap_or("").is_empty()
    }

    /// Configured at all: the classic connection or, for an API, any key added in the panel.
    pub fn connection_ready(&self, key: &str) -> bool {
        if catalog::by_key(key).is_some_and(|service| service.category == "API") && !self.named_keys(key).is_empty() {
            return true;
        }
        self.default_ready(key)
    }

    pub fn default_ready(&self, key: &str) -> bool {
        let settings = self.settings();
        match key {
            "claude-subscription" => {
                settings.get("claudeOAuthUsage").is_some_and(Json::is_truthy)
                    || claude_subscription::local_enabled(&self.state_dir)
                    || claude_subscription::desktop(&self.source_path("claude"), now())
                        .get("windows")
                        .is_some_and(Json::is_truthy)
            }
            "qoder-subscription" => {
                qoder::log_roots(&self.home, &self.environment).iter().any(|root| Path::new(root).is_dir())
            }
            "cursor-subscription" => settings.get("cursorUsage").is_some_and(Json::is_truthy),
            // Antigravity's app has run here and noted where to ask it for the quota.
            "gemini-subscription" => crate::antigravity::present(&self.home),
            "chatgpt" => {
                if self.chatgpt_source() == "proxy" {
                    self.opencodex_configured()
                } else {
                    self.chatgpt_configured()
                }
            }
            "deepseek" => {
                if self.env("DEEPSEEK_API_KEY").is_some() || self.env("DEEPSEEK_API_KEY_FILE").is_some() {
                    return !self.credential("DEEPSEEK_API_KEY").is_empty();
                }
                !self.opencodex_deepseek_key().is_empty()
            }
            "komari" => !self.komari_base_url().is_empty(),
            "grok" => !self.credential("XAI_MANAGEMENT_API_KEY").is_empty() && self.given_trimmed("XAI_TEAM_ID"),
            "minimax" => !self.credential("MINIMAX_API_KEY").is_empty(),
            "kimi-api" => !self.credential("MOONSHOT_API_KEY").is_empty(),
            "qoder-api" => !self.credential("QODER_CN_API_KEY").is_empty() && self.given_trimmed("QODER_CN_ORG_ID"),
            "claude-api" => !self.credential("ANTHROPIC_API_KEY").is_empty(),
            _ => {
                if catalog::manual_subscriptions().contains(&key) {
                    return settings
                        .get("configuredSubscriptions")
                        .and_then(Json::as_array)
                        .is_some_and(|list| list.contains(&Json::from(key)));
                }
                if catalog::portal_apis().contains(&key) {
                    return catalog::credential_names(key).iter().any(|name| !self.credential(name).is_empty());
                }
                false
            }
        }
    }

    // Small loaders

    /// The status pages to read: none unless 服务状态 is on, then only those of the modules shown.
    pub fn status_pages(&self) -> Vec<String> {
        if !self.settings().get("serviceStatus").is_some_and(Json::is_truthy) {
            return Vec::new();
        }
        let pages: BTreeSet<&str> = service_status::MODULE_PAGES
            .iter()
            .filter(|(module, _)| self.module_enabled(module))
            .map(|(_, page)| *page)
            .collect();
        pages.into_iter().map(String::from).collect()
    }

    /// How long the status pages read last hold: none when a page now wanted isn't among them (a module just put on
    /// the homepage), so its status is read at once rather than shown as 待同步 for up to five minutes.
    fn status_lifetime(&self, payload: &Object) -> f64 {
        if self.status_pages().iter().any(|page| !payload.contains_key(page)) {
            0.0
        } else {
            service_status::lifetime(payload)
        }
    }

    /// The day's ECB reference rate of the yuan to the dollar, from Frankfurter; only a User-Agent is sent.
    pub fn exchange_rate_load(&self) -> Loaded {
        let headers = vec![("User-Agent", format!("Starmeter/{VERSION}")), ("Accept", "application/json".to_string())];
        let Ok(response) = http::send(
            "https://api.frankfurter.dev/v1/latest?base=USD&symbols=CNY",
            Request { headers, timeout: 10.0, ..Request::default() },
        ) else {
            return (None, Some("汇率读取失败 · URLError".into()));
        };
        if !(200..300).contains(&response.status) {
            return (None, Some("汇率读取失败 · HTTPError".into()));
        }
        let body = &response.body[..response.body.len().min(16 * 1024)];
        let Some(document) = Json::parse(body).ok().and_then(|value| value.as_object().cloned()) else {
            return (None, Some("汇率读取失败 · JSONDecodeError".into()));
        };
        let rates = py::dict(document.get("rates"));
        let rate = rates.get("CNY");
        let (true, Some(value), Some(date)) = (
            preferences::exchange_rate(rate),
            rate.and_then(Json::python_float),
            document.get("date").and_then(Json::as_str),
        ) else {
            return (None, Some("汇率读取失败 · ValueError".into()));
        };
        (Some(object! { "rate" => value, "date" => date.chars().take(10).collect::<String>() }), None)
    }

    fn pricing_due(&self, now: f64) -> bool {
        if self.environment.get("OCX_PRICING_UPDATES").map(String::as_str) == Some("0") {
            return false;
        }
        let age = now - self.read_cache("pricing-check").get("checkedAt").and_then(Json::python_float).unwrap_or(0.0);
        !(0.0..86400.0).contains(&age)
    }

    /// Once a day: a plain GET for each table, no cookies or identifiers. Only a valid table newer than the one
    /// in use is kept; OCX_PRICING_UPDATES=0 turns this off.
    pub fn refresh_pricing(&self, now: f64) {
        if !self.pricing_due(now) {
            return;
        }
        self.write_json_cache(&self.cache_path("pricing-check"), &object! { "schema" => 1, "checkedAt" => now }.into());
        for name in PRICING_TABLES {
            let headers =
                vec![("User-Agent", format!("Starmeter/{VERSION}")), ("Accept", "application/json".to_string())];
            let address = format!("https://raw.githubusercontent.com/timecatbit/starmeter/main/lib/{name}");
            let Ok(response) = http::send(
                &address,
                Request { headers, timeout: 10.0, limit: 4 * 1024 * 1024 + 1, ..Request::default() },
            ) else {
                continue;
            };
            if !(200..300).contains(&response.status) || response.body.len() > 4 * 1024 * 1024 {
                continue;
            }
            let Ok(data) = Json::parse(&response.body) else { continue };
            if pricing::valid(name, &data)
                && let Some(table) = data.as_object()
                && pricing::committed(table) > pricing::committed(&pricing::table(name))
            {
                self.write_json_cache(&format!("{}/pricing/{name}", self.state_dir), &data);
                pricing::reset();
            }
        }
    }

    /// OpenCodex's own update check, behind its management API.
    pub fn update_status_cached(&self) -> Loaded {
        self.cached_fetch("badge", 3600.0, 60.0, || {
            let (payload, error) = self.opencodex_request("/api/update/check", "GET", None, 4.0, true);
            Ok((payload, error.map(|error| error.message)))
        })
    }

    // The background worker

    /// Every source to read in the background, in the order 5.x listed them, with how to read it.
    pub fn external_loaders(&self) -> Vec<LoaderJob<'_>> {
        let mut loaders: Vec<LoaderJob<'_>> = Vec::new();
        let settings = self.settings();
        fn add<'a>(loaders: &mut Vec<LoaderJob<'a>>, name: &str, body: impl FnOnce() + Send + 'a) {
            loaders.push((name.to_string(), Box::new(body)));
        }
        if self.module_enabled("opencodex") && self.opencodex_configured() {
            add(&mut loaders, "opencodex", || self.opencodex_refresh());
            add(&mut loaders, "badge", || {
                self.update_status_cached();
            });
        }
        if self.module_enabled("deepseek") && self.default_ready("deepseek") {
            add(&mut loaders, "provider_quotas", || {
                self.provider_quotas();
            });
        }
        if self.module_enabled("komari") && !self.komari_base_url().is_empty() {
            add(&mut loaders, "komari", || {
                self.komari_snapshot();
            });
        }
        if self.module_enabled("updates") && !self.live_app_update_ids().is_empty() {
            add(&mut loaders, "app_updates", || {
                self.app_updates_cached();
            });
        }
        if self.environment.get("OCX_SELF_UPDATES").map(String::as_str) != Some("0") {
            add(&mut loaders, "self_update", || {
                self.self_update_cached();
            });
        }
        if self.module_enabled("claude-subscription") {
            add(&mut loaders, "claude_subscription", || {
                self.claude_subscription_cached();
            });
        }
        for source in self.payback_sources() {
            add(&mut loaders, &format!("{}{source}", crate::range_usage::PAYBACK_MONTH), move || {
                self.payback_month(source);
            });
        }
        if self.module_enabled("sessions") {
            add(&mut loaders, "sessions", || {
                self.sessions_cached(None);
            });
        }
        // The yuan rate is read only once 人民币 is chosen without a rate of the user's own.
        if settings.get("currency") == Some(&Json::from("CNY")) && !settings.get("cnyRate").is_some_and(Json::is_truthy)
        {
            add(&mut loaders, "exchange_rate", || {
                self.cached_fetch("exchange_rate", 86400.0, 3600.0, || Ok(self.exchange_rate_load()));
            });
        }
        let pages = self.status_pages();
        if !pages.is_empty() {
            add(&mut loaders, "service_status", move || {
                let previous = self
                    .read_cache("service_status")
                    .get("payload")
                    .and_then(Json::as_object)
                    .cloned()
                    .unwrap_or_default();
                self.cached_fetch("service_status", self.status_lifetime(&previous), 60.0, || {
                    Ok((Some(service_status::merged(&previous, service_status::read_all(&pages, VERSION))), None))
                });
            });
        }
        if self.module_enabled("cursor-subscription") && settings.get("cursorUsage").is_some_and(Json::is_truthy) {
            add(&mut loaders, "cursor_subscription", || {
                self.cached_fetch("cursor_subscription", self.refresh_ttl(300.0), 60.0, || {
                    let keychain = self.settings().get("cursorKeychain").is_some_and(Json::is_truthy);
                    Ok(match cursor::plan(&self.home, &self.environment, now(), keychain) {
                        Ok(plan) => (Some(plan), None),
                        Err(failure) => (None, Some(failure.to_string())),
                    })
                });
            });
        }
        // Asked of Antigravity's app on this computer, every minute while watched (relaxed when closed).
        if self.module_enabled("gemini-subscription") && crate::antigravity::present(&self.home) {
            add(&mut loaders, "gemini_subscription", || {
                self.cached_fetch("gemini_subscription", self.refresh_ttl(60.0), 60.0, || {
                    Ok(match crate::antigravity::quota(&self.home) {
                        Ok(quota) => (Some(quota), None),
                        Err(failure) => (None, Some(failure)),
                    })
                });
            });
        }
        if self.module_enabled("qoder-subscription") {
            add(&mut loaders, "qoder_subscription", || {
                self.cached_fetch("qoder_subscription", self.refresh_ttl(60.0), 60.0, || {
                    let snapshot = qoder::snapshot(&self.home, &self.environment, now());
                    Ok((Some(snapshot.unwrap_or_else(|| object! { "empty" => true })), None))
                });
            });
        }
        // Each read starts the ChatGPT client's app-server, so a failing client waits its normal interval.
        if self.module_enabled("chatgpt") && self.chatgpt_source() == "client" && self.chatgpt_configured() {
            add(&mut loaders, "chatgpt", || {
                self.cached_fetch("chatgpt", 300.0, 300.0, || {
                    let home = self.codex_home_absolute();
                    let binary = chatgpt::binary(&self.app_roots(), &self.environment);
                    Ok((Some(chatgpt::account(binary.as_deref(), Some(&home), &self.environment)), None))
                });
            });
        }
        for key in ACCOUNT_SOURCES {
            if !(self.module_enabled(key) && self.default_ready(key)) {
                continue;
            }
            add(&mut loaders, key, move || {
                self.cached_fetch(key, external_ttl(key).unwrap_or(300.0), 60.0, || {
                    Ok((Some(accounts::account(key, None, &Object::new(), &self.environment, now())), None))
                });
            });
        }
        for (provider, ttl) in KEY_TTLS {
            if !self.module_enabled(provider) {
                continue;
            }
            for entry in self.named_keys(provider) {
                let Some(id) = entry.get("id").and_then(Json::as_str).map(String::from) else { continue };
                let name = format!("{provider}@{id}");
                let options = entry.get("options").and_then(Json::as_object).cloned().unwrap_or_default();
                let cache = name.clone();
                add(&mut loaders, &name, move || {
                    self.cached_fetch(&cache, ttl, 60.0, || {
                        let secret = keys::secret(Path::new(&self.state_dir), provider, &id);
                        Ok((Some(accounts::account(provider, Some(&secret), &options, &self.environment, now())), None))
                    });
                });
            }
        }
        loaders
    }

    fn ttl_for(&self, name: &str) -> f64 {
        if name.starts_with(crate::range_usage::PAYBACK_MONTH) {
            return 600.0;
        }
        external_ttl(name).or_else(|| key_ttl(name.split('@').next().unwrap_or(""))).unwrap_or(300.0)
    }

    /// The tools a subscription with a price written down is weighed against (本月回本) that aren't on the
    /// homepage, so the month's own figures don't have them: each is read on its own.
    pub fn payback_sources(&self) -> Vec<&'static str> {
        let prices = self.settings().get("subscriptionPrices").and_then(Json::as_object).cloned().unwrap_or_default();
        let mut sources = Vec::new();
        for (subscription, source) in catalog::SUBSCRIPTION_USAGE {
            if prices.contains_key(subscription)
                && self.module_enabled(subscription)
                && !self.module_enabled(source)
                && !sources.contains(&source)
            {
                sources.push(source);
            }
        }
        sources
    }

    pub fn loader_due(&self, name: &str) -> bool {
        if name == "claude_subscription" && self.claude_retry_pending() != 0.0 {
            return now() >= self.read_cache(name).get("retryAt").and_then(Json::python_float).unwrap_or(0.0);
        }
        if name == "service_status" {
            let record = self.read_cache(name);
            let payload = record.get("payload").and_then(Json::as_object).cloned().unwrap_or_default();
            return self.cache_due(&record, self.refresh_ttl(self.status_lifetime(&payload)));
        }
        if name != "opencodex" {
            return self.cache_due(&self.read_cache(name), self.refresh_ttl(self.ttl_for(name)));
        }
        let health = self.read_cache("healthz");
        if self.cache_due(&health, self.refresh_ttl(30.0)) {
            return true;
        }
        // While the service is down, its other endpoints wait for the next health retry.
        if health.get("error").is_some_and(Json::is_truthy) || !health.get("payload").is_some_and(Json::is_truthy) {
            return false;
        }
        opencodex::ENDPOINTS.iter().any(|(name, _, _)| self.cache_due(&self.read_cache(name), self.refresh_ttl(30.0)))
    }

    /// The selected range when its figures are due; this month too while a subscription's price is written down;
    /// today too while today's estimate sits beside the star. Also considers remote sources (`opencodex`/`cursor`)
    /// due even when a pinned `refresh_live` recently updated local sources (`LIVE-RANGE-1`).
    pub fn usage_ranges_due(&self) -> Vec<String> {
        let range_due = |range: &str, base: f64| {
            let ttl = self.refresh_ttl(base);
            self.cache_due(&self.read_cache(&self.range_cache_name(Some(range))), ttl)
                || self.remote_usage_due(range, ttl)
        };
        let key = self.selected_range();
        let settings = self.settings();
        let mut due = Vec::new();
        if range_due(&key, 60.0) {
            due.push(key.clone());
        }
        let prices = settings.get("subscriptionPrices").and_then(Json::as_object).cloned().unwrap_or_default();
        let weighed = prices.keys().any(|module| {
            self.module_enabled(module)
                && catalog::SUBSCRIPTION_USAGE
                    .iter()
                    .find(|pair| pair.0 == module.as_str())
                    .is_some_and(|pair| self.module_enabled(pair.1))
        });
        if weighed && key != "month" && range_due("month", 600.0) {
            due.push("month".into());
        }
        if settings.get("menuBar") == Some(&Json::from("cost")) && key != "today" && range_due("today", 60.0) {
            due.push("today".into());
        }
        // 多台电脑合并: every range is kept to ten minutes for this computer's file in the folder.
        if self.devices_root().is_some() {
            for (range, _) in RANGES {
                if range != key && !due.iter().any(|known| known == range) && range_due(range, 600.0) {
                    due.push(range.to_string());
                }
            }
        }
        due
    }

    /// Index reads one at a time, apart from the slower network refreshes.
    pub fn refresh_usage_range(&self, wait: bool) -> bool {
        self.with_lock("range-refresh.lock", wait, || {
            let due = self.usage_ranges_due();
            for key in &due {
                self.range_usage(Some(key), None);
            }
            !due.is_empty()
        })
        .unwrap_or(false)
    }

    /// The ranges the range menu offers, read ahead while it is open (in the menu's order, the one on screen and any
    /// read in the last minute left alone), so the range picked is most likely read already and shows at once. `stop`
    /// says a range was picked meanwhile: its own read goes first.
    pub fn prefetch_ranges(&self, stop: &dyn Fn() -> bool) {
        let shown = self.selected_range();
        for (key, _) in crate::runtime::RANGES {
            if stop() {
                return;
            }
            let ttl = self.refresh_ttl(60.0);
            if key == shown
                || (!self.cache_due(&self.read_cache(&self.range_cache_name(Some(key))), ttl)
                    && !self.remote_usage_due(key, ttl))
            {
                continue;
            }
            self.with_lock("range-refresh.lock", true, || {
                self.range_usage(Some(key), None);
            });
        }
    }

    /// A pinned panel's tick: active sessions refreshed first (never blocked behind range indexing or remote HTTP,
    /// LIVE-RANGE-2), then the range on screen re-read from local logs only.
    pub fn refresh_live(&self) -> bool {
        if self.module_enabled("sessions") {
            self.sessions_cached(Some(0.0));
        }
        self.with_lock("range-refresh.lock", false, || {
            self.range_usage_local(Some(&self.selected_range()), Some(0.0));
        })
        .is_some()
    }

    /// Runs body holding a lock file in the cache folder; None when another process holds it and we don't wait.
    pub fn with_lock<T>(&self, name: &str, wait: bool, body: impl FnOnce() -> T) -> Option<T> {
        let _ = files::make_directory(Path::new(&self.cache_dir), 0o700);
        let lock = platform::lock(&Path::new(&self.cache_dir).join(name), wait)?;
        let result = body();
        drop(lock);
        Some(result)
    }

    /// One worker at a time, however often the menu is refreshed.
    pub fn background_refresh(&self, wait: bool) {
        let started = self
            .with_lock("refresh.lock", wait, || {
                let mut work: Vec<Box<dyn FnOnce() + Send + '_>> = Vec::new();
                for (name, job) in self.external_loaders() {
                    if self.loader_due(&name) {
                        work.push(job);
                    }
                }
                if !self.usage_ranges_due().is_empty() {
                    work.push(Box::new(|| {
                        self.refresh_usage_range(false);
                    }));
                }
                if self.pricing_due(now()) {
                    work.push(Box::new(|| self.refresh_pricing(now())));
                }
                let started = !work.is_empty();
                run_concurrently(work, 4);
                self.after_reads();
                self.collect_cache_garbage(now());
                started
            })
            .unwrap_or(false);
        if started {
            self.refresh_swiftbar();
        }
    }

    /// Back from sleep, with the network up again: what failed while it was away (just before the lid closed, or in a
    /// maintenance wake) is read again now instead of when its backoff ends, and the Claude subscription is read
    /// afresh. A server's rate limit still holds. The app calls this, then refreshes.
    pub fn retry_after_wake(&self) {
        claude_subscription::forget_passing_failures(&self.state_dir);
        let moment = now();
        let mut names: Vec<String> = self.external_loaders().into_iter().map(|(name, _)| name).collect();
        names.push("healthz".into());
        let failed: Vec<&str> = names
            .iter()
            .map(String::as_str)
            .filter(|name| {
                let record = self.read_cache(name);
                let pages = record.get("payload").and_then(Json::as_object);
                *name == "claude_subscription"
                    || (*name == "service_status" && pages.is_some_and(service_status::unsettled))
                    || (record.get("error").is_some_and(Json::is_truthy)
                        && moment < record.get("retryAt").and_then(Json::python_float).unwrap_or(0.0))
            })
            .collect();
        self.invalidate_caches(&failed);
    }

    /// The command line's read beyond the local logs, as an opened panel would make it: the plans' and APIs'
    /// figures, OpenCodex, service status and the yuan rate that are due, and the price tables. The follow-up work of
    /// the app's own refresh (alerts, update checks and notices, the other computers' file) stays with the app.
    pub fn refresh_quotas(&self, wait: bool) -> bool {
        const LEFT_TO_THE_APP: [&str; 4] = ["komari", "app_updates", "self_update", "sessions"];
        self.with_lock("refresh.lock", wait, || {
            let mut work: Vec<Box<dyn FnOnce() + Send + '_>> = Vec::new();
            for (name, job) in self.external_loaders() {
                if !LEFT_TO_THE_APP.contains(&name.as_str()) && self.loader_due(&name) {
                    work.push(job);
                }
            }
            if self.pricing_due(now()) {
                work.push(Box::new(|| self.refresh_pricing(now())));
            }
            let started = !work.is_empty();
            run_concurrently(work, 4);
            started
        })
        .unwrap_or(false)
    }

    /// Hourly at most; only this engine's own cache folder.
    pub fn collect_cache_garbage(&self, now: f64) {
        let age = now - self.read_cache("cache-cleanup").get("cleanedAt").and_then(Json::python_float).unwrap_or(0.0);
        if (0.0..3600.0).contains(&age) {
            return;
        }
        let name = self.range_cache_name(None);
        let current = format!("_{}.json", name.rsplit('_').next().unwrap_or(""));
        let Ok(entries) = std::fs::read_dir(&self.cache_dir) else { return };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            let Some(mtime) = platform::modified(&path) else { continue };
            let file_age = now - mtime;
            let stale = (is_index_file(&name)
                && (!CURRENT_INDEXES.iter().any(|prefix| name.starts_with(prefix))
                    || RETIRED_INDEXES.iter().any(|prefix| name.starts_with(prefix))))
                || retired_cache(&name)
                || (name.starts_with("range_usage_") && (file_age > 86400.0 || !name.ends_with(&current)))
                || (name.starts_with("configured-sources-") && file_age > 86400.0)
                || ((name.starts_with(".cache-") || name.contains(".processing-")) && file_age > 3600.0);
            if stale {
                if path.is_dir() {
                    let _ = std::fs::remove_dir_all(&path);
                } else {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
        self.write_json_cache(&self.cache_path("cache-cleanup"), &object! { "schema" => 1, "cleanedAt" => now }.into());
    }

    /// Asks SwiftBar to redraw the menu; the standalone app and desktop hosts redraw on their own.
    pub fn refresh_swiftbar(&self) {
        if self.mac_app || self.is_desktop() {
            return;
        }
        let address = format!("swiftbar://refreshplugin?name={}", runtime::quote(&self.swiftbar_name()));
        let _ = shell::run(&["/usr/bin/open", "-g", &address], 4.0, None, None);
    }
}

/// Runs jobs on up to `workers` threads and waits for all of them.
pub fn run_concurrently<'a>(jobs: Vec<Box<dyn FnOnce() + Send + 'a>>, workers: usize) {
    if jobs.is_empty() {
        return;
    }
    let queue: Mutex<VecDeque<Box<dyn FnOnce() + Send + 'a>>> = Mutex::new(jobs.into());
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let next = queue.lock().expect("job queue").pop_front();
                    match next {
                        Some(job) => job(),
                        None => return,
                    }
                }
            });
        }
    });
}

#[cfg(test)]
mod prefetch_tests {
    use crate::json::Json;
    use crate::object;
    use crate::runtime::{RANGES, Runtime};

    fn runtime(home: &std::path::Path) -> Runtime {
        let state = home.join("state");
        std::fs::create_dir_all(&state).unwrap();
        let prefs =
            object! { "schema" => 1, "visible" => Json::from(vec!["summary".to_string(), "workbuddy".to_string()]) };
        std::fs::write(state.join("preferences.json"), Json::Object(prefs).serialized()).unwrap();
        let environment: std::collections::HashMap<String, String> = [
            ("HOME", home.to_string_lossy().into_owned()),
            ("OCX_STATE_DIR", state.to_string_lossy().into_owned()),
            ("OCX_CACHE_DIR", home.join("cache").to_string_lossy().into_owned()),
            ("WORKBUDDY_USAGE_DIR", home.to_string_lossy().into_owned()),
            ("CLAUDE_CONFIG_DIR", home.join("claude").to_string_lossy().into_owned()),
            ("CODEX_HOME", home.join("codex").to_string_lossy().into_owned()),
            ("OCX_CHATGPT_HOME", home.join("codex").to_string_lossy().into_owned()),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect();
        Runtime::new(environment, "starmeter")
    }

    /// The range menu open: every range but the one on screen read ahead; opened again within the minute, none read
    /// again; a range picked meanwhile stops it.
    #[test]
    fn the_range_menu_has_its_ranges_read_ahead() {
        let home = std::env::temp_dir().join(format!("starmeter-prefetch-{}", crate::runtime::random_hex(6)));
        std::fs::create_dir_all(home.join(".workbuddy/projects/p")).unwrap();
        std::fs::write(
            home.join(".workbuddy/projects/p/w.jsonl"),
            r#"{"type":"message","timestamp":"2026-11-01T10:00:00Z","message":{"role":"assistant","id":"w1","model":"glm-5","usage":{"input_tokens":9,"output_tokens":1}}}"#.to_string() + "\n",
        )
        .unwrap();
        let runtime = runtime(&home);
        let shown = runtime.selected_range();
        let read_at = |key: &str| {
            runtime.read_cache(&runtime.range_cache_name(Some(key))).get("fetchedAt").and_then(Json::python_float)
        };
        // Stopped before it starts: nothing read.
        runtime.prefetch_ranges(&|| true);
        assert!(RANGES.iter().all(|(key, _)| read_at(key).is_none()));
        runtime.prefetch_ranges(&|| false);
        for (key, _) in RANGES {
            assert_eq!(read_at(key).is_some(), key != shown, "{key}");
        }
        let first: Vec<Option<f64>> = RANGES.iter().map(|(key, _)| read_at(key)).collect();
        runtime.prefetch_ranges(&|| false);
        let again: Vec<Option<f64>> = RANGES.iter().map(|(key, _)| read_at(key)).collect();
        assert_eq!(first, again, "read again within the minute");
        let _ = std::fs::remove_dir_all(&home);
    }
}
