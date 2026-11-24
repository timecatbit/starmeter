//! The API modules' rows (the classic connection and any number of named keys), the other subscriptions, the
//! payback and status rows, and the whole management list.

use std::collections::HashMap;
use std::path::Path;

use indexmap::IndexMap;

use crate::background::{key_ttl, service_status};
use crate::files::paths;
use crate::json::{Json, Object};
use crate::module_rows::{failure, object};
use crate::opencodex::report_balance;
use crate::rows::{
    BAD_COLOR, MUTED_COLOR, Options, WARN_COLOR, item, merged, minimax_span, money_text, refresh_failed, usage_level,
    used_text, with_failure,
};
use crate::runtime::{self, PENDING_READ, RANGES, Runtime, now};
use crate::{catalog, keys, object, preferences, py};

pub const KEY_CHECK_PROVIDERS: [&str; 3] = ["chatgpt-api", "claude-api", "gemini-api"];

fn muted(size: i64) -> Options {
    object! { "color" => MUTED_COLOR, "size" => size }
}

fn check_id(provider: &str, id: &str) -> String {
    if id == "default" { provider.to_string() } else { format!("{provider}@{id}") }
}

/// The currency of a Kimi key's site, or None for an unknown site.
fn moonshot_currency(region: &str) -> Option<&'static str> {
    match region {
        "china" => Some("CNY"),
        "international" => Some("USD"),
        _ => None,
    }
}

/// A balance or spend: a number that is not a bool.
fn amount(value: Option<&Json>) -> Option<f64> {
    value.filter(|value| value.as_bool().is_none()).and_then(Json::number)
}

/// One figure per account: the freshest successful reading among that account's keys.
pub fn counted_values(values: &[Object]) -> Vec<Object> {
    let mut groups: IndexMap<String, Vec<&Object>> = IndexMap::new();
    for (index, value) in values.iter().enumerate() {
        let account = value.get("account").map(py::dumps).unwrap_or_else(|| format!("#{index}"));
        groups.entry(account).or_default().push(value);
    }
    let rank = |value: &Object| {
        (
            if value.get("failed").is_some_and(Json::is_truthy) { 0 } else { 1 },
            value.get("at").and_then(Json::number).unwrap_or(0.0),
        )
    };
    groups
        .into_values()
        .map(|group| {
            let mut best = group[0];
            for value in &group[1..] {
                let (a, b) = (rank(value), rank(best));
                if a.0 > b.0 || (a.0 == b.0 && a.1 > b.1) {
                    best = value;
                }
            }
            best.clone()
        })
        .collect()
}

/// The homepage total: balances or spend per currency and account, then how many keys when several.
pub fn api_badge(values: &[Object]) -> String {
    if values.is_empty() {
        return "未配置".into();
    }
    for (kind, prefix) in [("balance", "余额 "), ("spend", "本月已用 ")] {
        let read: Vec<Object> =
            values.iter().filter(|value| value.get("kind") == Some(&Json::from(kind))).cloned().collect();
        if read.is_empty() {
            continue;
        }
        let rows = counted_values(&read);
        let mut totals: IndexMap<String, f64> = IndexMap::new();
        for value in &rows {
            *totals.entry(py::str(value.get("currency"))).or_default() +=
                value.get("amount").and_then(Json::number).unwrap_or(0.0);
        }
        let count = if read.len() > 1 { format!(" ({})", read.len()) } else { String::new() };
        let text: Vec<String> = totals.iter().map(|(currency, total)| money_text(*total, currency)).collect();
        let failed = rows.iter().any(|value| value.get("failed").is_some_and(Json::is_truthy));
        return with_failure(&format!("{prefix}{}{count}", text.join(" · ")), true, failed);
    }
    let mut texts: Vec<String> = Vec::new();
    for value in values {
        let text = py::str(value.get("text"));
        if !texts.contains(&text) {
            texts.push(text);
        }
    }
    if texts.len() == 1 { texts.remove(0) } else { format!("{} 把 Key", values.len()) }
}

/// 12.3 倍 below a hundred, whole numbers above.
pub fn times_over(ratio: f64) -> String {
    if ratio < 100.0 { format!("{ratio:.1} 倍") } else { format!("{} 倍", ratio.round_ties_even() as i64) }
}

/// Rows flattened and indented, so the plugin never nests more than two levels.
pub fn nest(lines: &[String], level: usize) -> Vec<String> {
    let prefix = "--".repeat(level);
    let separator = format!("{prefix}---");
    let mut flattened: Vec<String> = Vec::new();
    for line in lines.iter().filter(|line| !line.is_empty()) {
        if line.chars().all(|character| character == '-') {
            if flattened.last().is_some_and(|last| *last != separator) {
                flattened.push(separator.clone());
            }
            continue;
        }
        let mut text = line.as_str();
        while let Some(rest) = text.strip_prefix("--") {
            text = rest;
        }
        flattened.push(format!("{prefix}{text}"));
    }
    while flattened.last() == Some(&separator) {
        flattened.pop();
    }
    flattened
}

impl Runtime {
    // API keys: the classic connection plus any number of named keys

    /// The classic connection (when configured) first, then the keys added in the panel.
    pub fn api_keys(&self, provider: &str) -> Vec<Object> {
        let mut rows = Vec::new();
        if self.default_ready(provider) {
            let env_key =
                catalog::credential_names(provider).into_iter().find(|name| !self.credential(name).is_empty());
            let path = env_key.and_then(|key| {
                let file = self.env(&format!("{key}_FILE"))?;
                self.environment
                    .get(key)
                    .is_none_or(|value| value.is_empty())
                    .then(|| runtime::absolute(&paths::expand_user(file).to_string_lossy()))
            });
            let source = if let Some(key) = env_key.filter(|key| self.env(key).is_some()) {
                Json::from(format!("环境变量 {key}"))
            } else if provider == "deepseek" && env_key.is_none() {
                Json::from(format!("{}/config.json", self.ocx_home))
            } else {
                path.map(Json::from).unwrap_or(Json::Null)
            };
            rows.push(object! {
                "id" => "default", "label" => keys::default_label(Path::new(&self.state_dir), provider), "options" => Object::new(),
                "cache" => if provider == "deepseek" { "provider_quotas".to_string() } else { provider.to_string() }, "source" => source,
            });
        }
        for entry in self.named_keys(provider) {
            let mut row = entry.clone();
            let id = py::str(entry.get("id"));
            row.insert("cache", format!("{provider}@{id}"));
            let path = keys::secret_path(Path::new(&self.state_dir), provider, &id)
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default();
            row.insert("source", path);
            rows.push(row);
        }
        rows
    }

    pub fn entry_secret(&self, provider: &str, id: &str) -> String {
        if id != "default" {
            return keys::secret(Path::new(&self.state_dir), provider, id);
        }
        if provider == "deepseek"
            && self.env("DEEPSEEK_API_KEY").is_none()
            && self.env("DEEPSEEK_API_KEY_FILE").is_none()
        {
            return self.opencodex_deepseek_key();
        }
        catalog::credential_names(provider)
            .into_iter()
            .map(|name| self.credential(name))
            .find(|secret| !secret.is_empty())
            .unwrap_or_default()
    }

    /// An irreversible mark of the configured key, so a check never outlives a key change.
    pub fn key_fingerprint(&self, provider: &str, id: &str) -> String {
        let secret = self.entry_secret(provider, id);
        if secret.is_empty() { String::new() } else { py::sha256_hex(secret.as_bytes())[..16].to_string() }
    }

    pub fn key_check(&self, provider: &str, id: &str) -> Option<Object> {
        let checks = self.read_cache("key-checks");
        let row = checks.get(&check_id(provider, id))?.as_object()?;
        let key = row.get("key").filter(|key| key.is_truthy())?;
        (key == &Json::from(self.key_fingerprint(provider, id))).then(|| row.clone())
    }

    fn deepseek_balance(&self, data: &Object) -> (String, Option<f64>) {
        let quotas = object(data, "provider_quotas");
        for report in quotas.get("reports").and_then(Json::as_array).into_iter().flatten().filter_map(Json::as_object) {
            if py::str(Some(&py::or(report.get("provider"), Json::from("")))).to_lowercase() != "deepseek" {
                continue;
            }
            if let Some((symbol, amount)) = report_balance(Some(report)) {
                return (format!("{symbol}{amount:.2}"), Some(amount));
            }
            return ("未知".into(), None);
        }
        (String::new(), None)
    }

    /// What one key shows: its balance, else its spend, else what a key check proved.
    pub fn key_value(&self, provider: &str, entry: &Object, data: &Object) -> Object {
        let cache = py::str(entry.get("cache"));
        let payload = object(data, &cache);
        let failed = refresh_failed(failure(data, &cache).as_deref());
        let at = payload.get("_fetchedAt").cloned().unwrap_or(Json::Null);
        if provider == "deepseek" && entry.get("id") == Some(&Json::from("default")) {
            let (text, value) = self.deepseek_balance(data);
            if let Some(value) = value {
                let currency = if text.starts_with('$') { "USD" } else { "CNY" };
                return object! {
                    "text" => format!("余额 {}", money_text(value, currency)), "amount" => value, "currency" => currency,
                    "kind" => "balance", "failed" => failed, "at" => at,
                };
            }
        }
        if let Some(value) = amount(payload.get("balance")) {
            let currency = py::str(Some(&payload.get("currency").cloned().unwrap_or_else(|| Json::from("USD"))));
            return object! {
                "text" => format!("余额 {}", money_text(value, &currency)), "amount" => payload.get("balance").cloned().unwrap_or(Json::Null),
                "currency" => currency, "kind" => "balance", "failed" => failed, "at" => at,
            };
        }
        if let Some(value) = amount(payload.get("monthCost")) {
            return object! {
                "text" => format!("本月已用 {}", money_text(value, "USD")), "amount" => payload.get("monthCost").cloned().unwrap_or(Json::Null),
                "currency" => "USD", "kind" => "spend", "failed" => failed, "at" => at,
            };
        }
        let id = py::str(entry.get("id"));
        let check = if KEY_CHECK_PROVIDERS.contains(&provider) { self.key_check(provider, &id) } else { None };
        if check.as_ref().and_then(|check| check.get("valid")) == Some(&Json::Bool(false)) {
            return object! { "text" => "Key 无效" };
        }
        if payload.get("permission") == Some(&Json::Bool(false)) {
            return object! { "text" => "无花费权限" };
        }
        if key_ttl(provider).is_some() {
            let state = payload.get("state");
            let text = if failed {
                "刷新失败".to_string()
            } else if let Some(state) = state.filter(|state| !state.is_null() && *state != &Json::from("已同步")) {
                py::str(Some(state))
            } else {
                "待同步".into()
            };
            return object! { "text" => text };
        }
        if KEY_CHECK_PROVIDERS.contains(&provider) {
            let Some(check) = check else { return object! { "text" => "待测试" } };
            let text = match check.get("valid").and_then(Json::as_bool) {
                Some(true) => "Key 有效",
                Some(false) => "Key 无效",
                None => "测试未完成",
            };
            return object! { "text" => text };
        }
        object! { "text" => "已配置" }
    }

    /// Which account a key's figure belongs to: Qoder CN and Grok keys name their organization or team; the others
    /// count as their own key.
    pub fn key_account(&self, provider: &str, entry: &Object) -> Vec<String> {
        let (option, variable) = match provider {
            "qoder-api" => ("org", "QODER_CN_ORG_ID"),
            "grok" => ("team", "XAI_TEAM_ID"),
            _ => return vec!["key".into(), py::str(entry.get("id"))],
        };
        let fallback = if entry.get("id") == Some(&Json::from("default")) {
            self.environment.get(variable).cloned().unwrap_or_default()
        } else {
            String::new()
        };
        let options = py::dict(entry.get("options"));
        let owner = py::str(Some(&py::or(options.get(option), Json::from(fallback)))).trim().to_string();
        if owner.is_empty() { vec!["key".into(), py::str(entry.get("id"))] } else { vec![option.into(), owner] }
    }

    pub fn key_lines(&self, provider: &str, entry: &Object, data: &Object, level: usize) -> Vec<String> {
        let cache = py::str(entry.get("cache"));
        let payload = object(data, &cache);
        let error = failure(data, &cache);
        let mut rows = Vec::new();
        let currency = py::str(Some(&payload.get("currency").cloned().unwrap_or_else(|| Json::from("USD"))));
        for (field, name) in [("cash", "现金"), ("voucher", "代金券")] {
            if let Some(value) = amount(payload.get(field)) {
                rows.push(item(name, level, object! { "badge" => money_text(value, &currency) }));
            }
        }
        if let Some(value) = amount(payload.get("todayCost")) {
            rows.push(item("今天已用（UTC）", level, object! { "badge" => money_text(value, "USD") }));
        }
        if let Some(Json::Int(packages)) = payload.get("packages") {
            // The balance is the organization's shared add-on pool; members' plan Credits are separate.
            rows.push(item(
                "组织共享加购 Credits",
                level,
                object! { "badge" => format!("{packages} 个有效资源包"), "paneldetail" => "不含成员各自的套餐 Credits" },
            ));
            let options = py::dict(entry.get("options"));
            let org = options.get("org").filter(|org| org.is_truthy());
            if org.is_some()
                || (entry.get("id") == Some(&Json::from("default")) && self.env("QODER_CN_ORG_ID").is_some())
            {
                let badge = org.cloned().unwrap_or_else(|| Json::from(self.env("QODER_CN_ORG_ID").unwrap_or("")));
                rows.push(item("组织 ID", level, object! { "badge" => badge, "color" => MUTED_COLOR, "size" => 10 }));
            }
        }
        if payload.get("permission") == Some(&Json::Bool(false)) {
            let detail = if payload.get("reason") != Some(&Json::from("workspace")) {
                "需要管理员 Key（sk-ant-admin…），个人组织需先转为团队组织"
            } else {
                "这把 Key 绑定了 Workspace，换用管理员 Key（sk-ant-admin…）"
            };
            let state =
                py::str(Some(&payload.get("state").cloned().unwrap_or_else(|| Json::from("没有查看花费的权限"))));
            rows.push(item(&state, level, object! { "color" => WARN_COLOR, "size" => 10, "paneldetail" => detail }));
        }
        if key_ttl(provider).is_some() {
            rows.push(self.refresh_row(provider, &payload, error.as_deref(), level));
        }
        let id = py::str(entry.get("id"));
        if KEY_CHECK_PROVIDERS.contains(&provider) {
            let badge = match self.key_check(provider, &id) {
                Some(check) => format!(
                    "{} · {}",
                    py::str(check.get("state")),
                    self.freshness(check.get("checkedAt")).replace("更新", "")
                ),
                None => "未测试".into(),
            };
            rows.push(item(
                "测试 Key",
                level,
                merged(&[&object! { "badge" => badge }, &self.act("test-api-key", &[provider, &id])]),
            ));
        }
        if let Some(source) = entry.get("source").filter(|source| source.is_truthy()) {
            rows.push(item(
                "来源",
                level,
                object! { "paneldetail" => source.clone(), "color" => MUTED_COLOR, "size" => 10 },
            ));
        }
        rows.push(item("重命名…", level, self.act("rename-api-key", &[provider, &id])));
        // A key read from OpenCodex's own config is not ours to delete.
        if entry.get("source") != Some(&Json::from(format!("{}/config.json", self.ocx_home))) {
            rows.push(item(
                "删除…",
                level,
                merged(&[&object! { "color" => BAD_COLOR }, &self.act("remove-api-key", &[provider, &id])]),
            ));
        }
        rows
    }

    pub fn api_values(&self, provider: &str, data: &Object) -> (Vec<Object>, Vec<Object>) {
        let keys = self.api_keys(provider);
        let mut values: Vec<Object> = keys.iter().map(|entry| self.key_value(provider, entry, data)).collect();
        for (value, entry) in values.iter_mut().zip(&keys) {
            value.insert("label", entry.get("label").cloned().unwrap_or(Json::Null));
            if value.get("kind").is_some_and(Json::is_truthy) {
                let account: Vec<Json> = self.key_account(provider, entry).into_iter().map(Json::from).collect();
                value.insert("account", Json::Array(account));
            }
        }
        (keys, values)
    }

    /// The currencies this API's keys hold; a Kimi key's site tells it before the first reading.
    pub fn balance_currencies(&self, provider: &str, keys: &[Object], values: &[Object]) -> Vec<String> {
        let mut found: Vec<String> = Vec::new();
        for (entry, value) in keys.iter().zip(values) {
            let currency = if value.get("kind") == Some(&Json::from("balance")) {
                py::str(value.get("currency"))
            } else if provider == "kimi-api" {
                let options = py::dict(entry.get("options"));
                let region = options
                    .get("region")
                    .filter(|region| region.is_truthy())
                    .map(|region| py::str(Some(region)))
                    .or_else(|| self.environment.get("MOONSHOT_REGION").map(|region| region.trim().to_string()))
                    .filter(|region| !region.is_empty())
                    .unwrap_or_else(|| "china".into());
                moonshot_currency(&region).unwrap_or("CNY").to_string()
            } else {
                catalog::BALANCE_CURRENCIES
                    .iter()
                    .find(|pair| pair.0 == provider)
                    .map(|pair| pair.1.to_string())
                    .unwrap_or_default()
            };
            if !found.contains(&currency) {
                found.push(currency);
            }
        }
        found
    }

    pub fn api_lines(&self, provider: &str, data: &Object) -> Vec<String> {
        let service = catalog::by_key(provider).expect("service");
        let (keys, values) = self.api_values(provider, data);
        let mut first: HashMap<String, &Object> = HashMap::new();
        for (entry, value) in keys.iter().zip(&values) {
            if let Some(account) = value.get("account") {
                first.entry(py::dumps(account)).or_insert(entry);
            }
        }
        let mut rows = vec![item(
            &service.label(),
            0,
            merged(&[&object! { "badge" => api_badge(&values) }, &self.brand_icon(provider)]),
        )];
        for (entry, value) in keys.iter().zip(&values) {
            let mut note = Json::Null;
            if let Some(account) = value.get("account")
                && let Some(same) = first.get(&py::dumps(account))
                && same.get("id") != entry.get("id")
            {
                let kind = account.as_array().and_then(|parts| parts.first()).and_then(Json::as_str).unwrap_or("");
                note = Json::from(format!(
                    "与「{}」同一{}，主页合计只算一次",
                    py::str(same.get("label")),
                    if kind == "org" { "组织" } else { "Team" }
                ));
            }
            rows.push(item(
                &py::str(entry.get("label")),
                1,
                object! { "badge" => value.get("text").cloned().unwrap_or(Json::Null), "paneldetail" => note },
            ));
            rows.extend(self.key_lines(provider, entry, data, 2));
        }
        rows.push(item(
            "添加 Key…",
            1,
            merged(&[&object! { "sfimage" => "plus" }, &self.act("add-api-key", &[provider])]),
        ));
        if catalog::balance_apis().contains(&provider) && !keys.is_empty() {
            // One line per currency: Kimi's China and international sites hold yuan and dollars.
            let currencies = self.balance_currencies(provider, &keys, &values);
            let lines = self.settings().get("balanceAlerts").and_then(Json::as_object).cloned().unwrap_or_default();
            for currency in &currencies {
                let line = lines.get(&format!("{provider}|{currency}")).and_then(Json::number);
                let label = if currencies.len() == 1 {
                    "余额提醒…".to_string()
                } else {
                    let name = match currency.as_str() {
                        "CNY" => "人民币".to_string(),
                        "USD" => "美元".to_string(),
                        "Credits" => "Credits ".to_string(),
                        other => other.to_string(),
                    };
                    format!("{name}余额提醒…")
                };
                let badge =
                    line.map(|line| format!("低于 {}", money_text(line, currency))).unwrap_or_else(|| "关闭".into());
                rows.push(item(
                    &label,
                    1,
                    merged(&[
                        &object! { "sfimage" => "bell", "badge" => badge },
                        &self.act("set-balance-alert", &[provider, currency]),
                    ]),
                ));
            }
        }
        rows.push(item("打开用量与账单", 1, object! { "href" => service.url, "sfimage" => "arrow.up.right" }));
        rows
    }

    /// Subscriptions without a quota API: a manual, unverified mark and the official page.
    pub fn portal_service_lines(&self, key: &str) -> Vec<String> {
        let service = catalog::by_key(key).expect("service");
        let configured = self.connection_ready(key);
        vec![
            item(
                &service.label(),
                0,
                merged(&[&object! { "badge" => if configured { "已标记" } else { "未配置" } }, &self.brand_icon(key)]),
            ),
            item("手动标记，未验证", 1, muted(10)),
            item(
                if configured { "清除标记" } else { "标记为已订阅" },
                1,
                self.act("set-subscription", &[key, if configured { "false" } else { "true" }]),
            ),
            item("打开订阅管理", 1, object! { "href" => service.url, "sfimage" => "arrow.up.right" }),
        ]
    }

    /// MiniMax plan quota: the share already used per window, like every other subscription.
    pub fn provider_account_lines(&self, key: &str, data: &Object) -> Vec<String> {
        let service = catalog::by_key(key).expect("service");
        let payload = object(data, key);
        let error = failure(data, key);
        let failed = refresh_failed(error.as_deref());
        let moment = now();
        let windows: Vec<Object> = payload
            .get("windows")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .filter_map(Json::as_object)
            .filter(|window| crate::rows::known_share(window))
            .filter(|window| {
                !window.get("resetsAt").is_some_and(Json::is_truthy)
                    || window.get("resetsAt").and_then(Json::number).unwrap_or(0.0) > moment
            })
            .cloned()
            .collect();
        let most = windows
            .iter()
            .map(|window| 100.0 - py::num(window.get("remaining")))
            .fold(None, |best: Option<f64>, used| {
                Some(best.map_or(used, |best| if used > best { used } else { best }))
            });
        let badge = match most {
            Some(most) => used_text(most),
            None if payload.get("windows").is_some_and(Json::is_truthy) => "待刷新".into(),
            None if self.connection_ready(key) => {
                py::str(Some(&payload.get("state").cloned().unwrap_or_else(|| Json::from("待同步"))))
            }
            None => "未配置".into(),
        };
        let mut rows = vec![item(
            &service.label(),
            0,
            merged(&[
                &object! { "badge" => with_failure(&badge, !windows.is_empty(), failed) },
                &self.brand_icon(key),
                &object! { "panelusagelevel" => if windows.is_empty() { Json::Null } else { usage_level(most, failed) } },
            ]),
        )];
        for window in &windows {
            let used = 100.0 - py::num(window.get("remaining"));
            rows.push(item(
                &py::str(window.get("name")),
                1,
                object! {
                    "badge" => used_text(used), "panelusagelevel" => usage_level(Some(used), failed),
                    "paneldetail" => self.window_detail(used, window.get("resetsAt").and_then(Json::number), minimax_span(window),
                                                        payload.get("_fetchedAt").and_then(Json::number), failed),
                },
            ));
        }
        rows.push(if self.connection_ready(key) {
            self.refresh_row(key, &payload, error.as_deref(), 1)
        } else {
            item("选择密钥文件…", 1, self.act("configure-connection", &[key]))
        });
        rows.push(item("打开用量与账单", 1, object! { "href" => service.url, "sfimage" => "arrow.up.right" }));
        rows
    }

    /// A subscription's or API's row for its service's status page, while 服务状态 is on.
    pub fn status_lines(&self, key: &str, data: &Object) -> Vec<String> {
        let Some(page) = service_status::module_page(key) else { return vec![] };
        if !self.settings().get("serviceStatus").is_some_and(Json::is_truthy) {
            return vec![];
        }
        let Some((name, _, link)) = service_status::page(page) else { return vec![] };
        let state = object(data, "service_status").get(page).and_then(Json::as_object).cloned().unwrap_or_default();
        let trouble = state.get("indicator").and_then(Json::as_str).is_some_and(|indicator| indicator != "none")
            && !state.get("error").is_some_and(Json::is_truthy);
        let (badge, light) = if state.is_empty() {
            ("待同步".to_string(), "unknown")
        } else if state.get("error").is_some_and(Json::is_truthy) {
            ("读取失败".into(), "unknown")
        } else {
            let indicator = py::str(state.get("indicator"));
            let text = service_status::indicator(&indicator).unwrap_or("");
            (
                if text.is_empty() { "运行正常".to_string() } else { text.to_string() },
                if indicator == "none" { "ok" } else { "issue" },
            )
        };
        // In trouble: what is wrong (the incidents, or the parts not working) under it, not just a word (用户：claude 有部分
        // 服务降级，然后呢？); a click opens the status page itself (用户：降级那个提醒是否可以直接看这个网页).
        let (detail, href) = if trouble {
            (
                service_status::trouble(&state)
                    .map_or_else(|| state.get("description").cloned().unwrap_or(Json::Null), Json::from),
                link.to_string(),
            )
        } else if state.get("error").is_some_and(Json::is_truthy) {
            (Json::from("状态页这次没读到，点开在浏览器里看"), link.to_string())
        } else {
            (state.get("description").cloned().unwrap_or(Json::Null), link.to_string())
        };
        vec![item(
            &format!("{name} 服务状态"),
            1,
            object! { "badge" => badge, "href" => href, "panelstatus" => light, "paneldetail" => detail },
        )]
    }

    /// A subscription weighed against this month's API-equivalent cost of its Harness.
    pub fn payback_lines(&self, key: &str) -> Vec<String> {
        let Some(source) = catalog::SUBSCRIPTION_USAGE.iter().find(|pair| pair.0 == key).map(|pair| pair.1) else {
            return vec![];
        };
        let prices = self.settings().get("subscriptionPrices").and_then(Json::as_object).cloned().unwrap_or_default();
        let price = prices.get(key).and_then(Json::number);
        let badge = price.map(|price| format!("{}/月", money_text(price, "USD"))).unwrap_or_else(|| "未填写".into());
        let mut rows = vec![item(
            "订阅月费…",
            1,
            merged(&[
                &object! { "sfimage" => "dollarsign.circle", "badge" => badge },
                &self.act("set-subscription-price", &[key]),
            ]),
        )];
        let Some(price) = price else { return rows };
        let name =
            preferences::MODULES.iter().find(|module| module.key == source).map_or(String::new(), |m| m.label.clone());
        // The month's own figures when the tool is on the homepage; otherwise its month read on its own.
        let record = if self.module_enabled(source) {
            self.with_devices("month", self.read_cache(&self.range_cache_name(Some("month"))))
        } else {
            self.read_cache(&format!("{}{source}", crate::range_usage::PAYBACK_MONTH))
        };
        let usage = record.get("payload").and_then(|payload| payload.get(source)).and_then(Json::as_object).cloned();
        let quiet = |badge: String| object! { "badge" => badge, "color" => MUTED_COLOR, "size" => 11 };
        if !usage.as_ref().and_then(|usage| usage.get("summary")).is_some_and(Json::is_truthy) {
            // No records of the tool this month says why (none found, a folder to choose); else it's on its way.
            let why = usage.as_ref().and_then(|usage| usage.get("error")).filter(|error| error.is_truthy());
            rows.push(item("本月回本", 1, quiet(why.map_or_else(|| "待同步".into(), |error| py::str(Some(error))))));
        } else {
            let summary =
                usage.and_then(|usage| usage.get("summary").and_then(Json::as_object).cloned()).unwrap_or_default();
            let cost = py::or(summary.get("estimatedCostUsd"), Json::Int(0)).python_float().unwrap_or(0.0).max(0.0);
            let unpriced = py::or(summary.get("unpricedRequests"), Json::Int(0)).python_float().unwrap_or(0.0) as i64;
            let mut detail =
                format!("本月 {name} 的 API 等值 {}，订阅 {}/月", money_text(cost, "USD"), money_text(price, "USD"));
            if unpriced != 0 {
                detail += &format!("；其中 {unpriced} 次未定价，未计入");
            }
            rows.push(item(
                "本月回本",
                1,
                object! { "sfimage" => "chart.line.uptrend.xyaxis", "badge" => times_over(cost / price), "paneldetail" => detail },
            ));
        }
        rows
    }

    pub fn resource_lines(&self, data: &Object) -> Vec<String> {
        let mut lines = Vec::new();
        for service in catalog::SERVICES.iter().filter(|service| self.module_enabled(service.key)) {
            let key = service.key;
            match key {
                "chatgpt" => lines.extend(self.chatgpt_lines(data)),
                "claude-subscription" => lines.extend(self.claude_subscription_lines(data)),
                "qoder-subscription" => lines.extend(self.qoder_subscription_lines(data)),
                "cursor-subscription" => lines.extend(self.cursor_subscription_lines(data)),
                "gemini-subscription" => lines.extend(self.gemini_subscription_lines(data)),
                _ if service.category == "API" => lines.extend(self.api_lines(key, data)),
                _ if crate::background::ACCOUNT_SOURCES.contains(&key) => {
                    lines.extend(self.provider_account_lines(key, data));
                }
                _ => lines.extend(self.portal_service_lines(key)),
            }
            lines.extend(self.payback_lines(key));
            lines.extend(self.status_lines(key, data));
        }
        if !lines.is_empty() {
            lines.push("---".into());
        }
        lines
    }

    // OpenCodex management and the whole list

    fn unfiltered_management_lines(&self, data: &Object) -> Vec<String> {
        let accounts = py::dict(object(data, "accounts").get("accounts")).as_array().cloned().unwrap_or_default();
        let health_failure = failure(data, "healthz");
        let healthy = data.get("healthz").is_some_and(Json::is_truthy) && !refresh_failed(health_failure.as_deref());
        let pending =
            !data.get("healthz").is_some_and(Json::is_truthy) && health_failure.as_deref() == Some(PENDING_READ);
        let readiness = object(data, "ready").get("status").and_then(Json::as_str).map(String::from);
        let status = if healthy {
            match readiness.as_deref() {
                Some("failed") => "启动失败",
                Some("pending" | "starting") => "正在启动",
                _ => "",
            }
        } else if pending {
            "待同步"
        } else {
            "未连接"
        };
        let mut lines = self.resource_lines(data);
        let title =
            if status.is_empty() { "OpenCodex 管理".to_string() } else { format!("OpenCodex 管理 · {status}") };
        lines.push(item(&title, 0, object! { "sfimage" => "slider.horizontal.3" }));
        let mut children = vec![
            item("打开控制台", 0, object! { "href" => format!("{}/", self.base), "sfimage" => "macwindow" }),
            item(
                "查看完整用量",
                0,
                object! { "href" => format!("{}/#usage", self.base), "sfimage" => "chart.bar.xaxis" },
            ),
        ];
        let action = |label: &str, icon: &str, name: &str, extra: Options| {
            item(label, 0, merged(&[&object! { "sfimage" => icon }, &extra, &self.act(name, &[])]))
        };
        if healthy {
            children.push("---".into());
            children.push(item(
                &format!("ChatGPT 账户 · {}", accounts.len()),
                0,
                object! { "panelsection" => true, "color" => MUTED_COLOR, "size" => 11 },
            ));
            children.extend(self.account_quick_lines(data));
            children.push("---".into());
            children.push(action("同步模型到 ChatGPT", "arrow.triangle.2.circlepath", "sync", Options::new()));
            children.push(action("检查更新", "arrow.down.circle", "check-update", Options::new()));
            children.push(action("打开代理日志", "doc.text", "logs", Options::new()));
            children.push(action("重新启动代理…", "arrow.clockwise", "restart", Options::new()));
            children.push(action("停止代理…", "stop.circle", "stop", object! { "color" => BAD_COLOR }));
        } else if !pending {
            children.push("---".into());
            children.push(action("启动 OpenCodex 代理", "play.fill", "start", Options::new()));
            children.push(action("修复自启动服务", "wrench.and.screwdriver", "repair", Options::new()));
            children.push(action("查看 ocx status", "terminal", "status", Options::new()));
        }
        if healthy {
            children.push("---".into());
            children.push(item(
                "数据更新时间",
                0,
                object! { "panelsection" => true, "color" => MUTED_COLOR, "size" => 11 },
            ));
            // OpenCodex's part of the range bundle carries when it was last read.
            let usage = object(data, "range_usage");
            let ocx = usage.get("opencodex").and_then(Json::as_object).cloned().unwrap_or_default();
            let summary_read = if ocx.get("summary").is_some_and(Json::is_truthy) {
                usage.get("_fetchedAt").cloned().unwrap_or(Json::Null)
            } else {
                Json::Null
            };
            let read = py::or(ocx.get("readAt"), summary_read);
            let selected =
                usage.get("range").and_then(Json::as_str).map(String::from).unwrap_or_else(|| self.selected_range());
            let range_label = RANGES.iter().find(|(key, _)| *key == selected).map_or("", |(_, label)| *label);
            let truthy_text = |value: Option<&Json>| value.filter(|v| v.is_truthy()).map(|v| py::str(Some(v)));
            let range_failure = failure(data, "range_usage");
            let ocx_error = range_failure
                .clone()
                .or_else(|| truthy_text(ocx.get("refreshError")))
                .or_else(|| truthy_text(ocx.get("error")));
            let quotas = object(data, "provider_quotas");
            let sources = [
                (format!("用量统计 · {range_label}"), read, ocx_error, range_failure.is_some()),
                (
                    "供应商余额".to_string(),
                    quotas.get("_fetchedAt").cloned().unwrap_or(Json::Null),
                    failure(data, "provider_quotas"),
                    true,
                ),
            ];
            for (label, fetched, error, from_cache) in sources {
                let mut state = self.freshness(Some(&fetched));
                if let Some(error) = error
                    && (!from_cache || refresh_failed(Some(&error)))
                {
                    state = if fetched.is_truthy() {
                        format!("{state} · 本次读取失败")
                    } else {
                        "读取失败".into()
                    };
                }
                children.push(item(&label, 0, object! { "badge" => state, "color" => MUTED_COLOR, "size" => 11 }));
            }
        }
        lines.extend(nest(&children, 1));
        if self.module_enabled("sessions") {
            lines.extend(self.sessions_lines(data, &self.currency_unit(data)));
        }
        lines.extend(self.komari_lines(data));
        lines.extend(self.app_update_lines(data));
        lines.extend(self.self_update_lines(data));
        lines.push("---".into());
        lines.push(item(
            "刷新数据",
            0,
            merged(&[&object! { "sfimage" => "arrow.clockwise" }, &self.act("refresh", &[])]),
        ));
        lines
    }

    pub fn management_lines(&self, data: &Object) -> Vec<String> {
        let mut result: Vec<String> = Vec::new();
        let mut keep = true;
        for row in self.unfiltered_management_lines(data) {
            if row == "---" {
                if result.last().is_some_and(|last| last != "---") {
                    result.push(row);
                }
                continue;
            }
            if !row.starts_with("--") {
                let key = [("OpenCodex 管理", "opencodex"), ("Komari 监控", "komari"), ("工具更新", "updates")]
                    .iter()
                    .find(|(prefix, _)| row.starts_with(prefix))
                    .map(|(_, key)| *key);
                keep = key.is_none_or(|key| self.module_enabled(key));
            }
            if keep {
                result.push(row);
            }
        }
        while result.last().is_some_and(|last| last == "---") {
            result.pop();
        }
        result
    }
}
