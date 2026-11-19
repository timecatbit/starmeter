//! Each module's rows: the subscription windows and ChatGPT's accounts; rendering only reads caches, the reads
//! happen in the background worker.

use jiff::tz::TimeZone;

use crate::background::{ACCOUNT_SOURCES, EXTERNAL_TTLS, KEY_TTLS};
use crate::json::{Json, Object};
use crate::opencodex::ENDPOINTS;
use crate::rows::{
    self, MUTED_COLOR, Options, RANK_COLOR, WARN_COLOR, claude_span, g, item, merged, refresh_failed, round, shorten,
    usage_level, used_text, with_failure,
};
use crate::runtime::{RANGES, Runtime, now};
use crate::time::IsoTime;
use crate::{accounts, catalog, claude_subscription, dashboard, object, py, updates};

/// The cached payload of a source, or an empty one.
pub fn object(data: &Object, name: &str) -> Object {
    data.get(name).and_then(Json::as_object).cloned().unwrap_or_default()
}

/// The error message cached beside a source, when there is one.
pub fn failure(data: &Object, name: &str) -> Option<String> {
    data.get(&format!("{name}_error")).and_then(Json::as_str).map(String::from)
}

fn put(data: &mut Object, name: &str, result: (Option<Object>, Option<String>)) {
    data.insert(name, result.0.map(Json::Object).unwrap_or(Json::Null));
    data.insert(format!("{name}_error"), result.1.map(Json::from).unwrap_or(Json::Null));
}

fn muted(size: i64) -> Options {
    object! { "color" => MUTED_COLOR, "size" => size }
}

pub fn chatgpt_window_label(window: &Object) -> String {
    let minutes = py::or(window.get("minutes"), Json::Int(0)).number().unwrap_or(0.0);
    let span = if minutes == 10080.0 {
        "本周".to_string()
    } else if minutes >= 60.0 {
        format!("{} 小时", g(minutes / 60.0))
    } else {
        format!("{} 分钟", g(minutes))
    };
    // The plan's own limit goes by its span alone (the page is already ChatGPT 订阅); another limit keeps its name.
    let name = py::str(window.get("name"));
    if ["codex", "chatgpt"].contains(&name.to_lowercase().as_str()) { span } else { format!("{name} · {span}") }
}

impl Runtime {
    /// Every cache the panel shows, with its error message (null when none).
    pub fn cached_data(&self) -> Object {
        let mut data = Object::new();
        let mut ttls = EXTERNAL_TTLS.to_vec();
        ttls.sort_by(|a, b| a.0.cmp(b.0));
        for (name, ttl) in ttls {
            put(&mut data, name, self.cache_result(&self.read_cache(name), ttl));
        }
        if !self.opencodex_configured() {
            for (name, _, _) in ENDPOINTS {
                data.insert(name, Json::Null);
            }
            data.insert("healthz_error", "未配置 OpenCodex");
        }
        for key in ACCOUNT_SOURCES {
            if !self.default_ready(key) {
                data.insert(key, accounts::account(key, None, &Object::new(), &self.environment, now()));
                data.insert(format!("{key}_error"), Json::Null);
            }
        }
        for (provider, ttl) in KEY_TTLS {
            if !self.module_enabled(provider) {
                continue;
            }
            for entry in self.named_keys(provider) {
                if let Some(id) = entry.get("id").and_then(Json::as_str) {
                    let name = format!("{provider}@{id}");
                    put(&mut data, &name, self.cache_result(&self.read_cache(&name), ttl));
                }
            }
        }
        if !self.chatgpt_configured() {
            data.insert("chatgpt", object! { "state" => "未找到 ChatGPT 客户端登录" });
            data.insert("chatgpt_error", Json::Null);
        }
        let ttl = self.refresh_ttl(60.0);
        put(&mut data, "range_usage", self.cache_result(&self.read_cache(&self.range_cache_name(None)), ttl));
        // A new range and its totals appear together; the previous range stays until then, for the same sources.
        if data.get("range_usage").is_none_or(Json::is_null)
            && !refresh_failed(data.get("range_usage_error").and_then(Json::as_str))
        {
            let view = self.read_cache("panel-view");
            let previous = view
                .get("dashboard")
                .and_then(|dashboard| dashboard.get("rangeKey"))
                .and_then(Json::as_str)
                .map(String::from);
            if let Some(previous) = previous
                && RANGES.iter().any(|(key, _)| *key == previous)
                && previous != self.selected_range()
            {
                let (last, error) = self.cache_result(&self.read_cache(&self.range_cache_name(Some(&previous))), ttl);
                if let Some(last) = last {
                    data.insert("range_usage", last);
                    data.insert("range_usage_error", error.map(Json::from).unwrap_or(Json::Null));
                    data.insert("displayed_range", previous);
                }
            }
        }
        data
    }

    // Subscriptions

    pub fn claude_subscription_lines(&self, data: &Object) -> Vec<String> {
        let mut payload = data.get("claude_subscription").and_then(Json::as_object).cloned().unwrap_or_default();
        if payload.is_empty() {
            // Rendering reads caches only; the login read runs in the background, only local snapshots are read here.
            payload = if self.settings().get("claudeOAuthUsage").is_some_and(Json::is_truthy) {
                object! { "source" => "claude-oauth", "state" => "待同步" }
            } else {
                self.claude_subscription_load().0.unwrap_or_default()
            };
        }
        let windows: Vec<Object> = payload
            .get("windows")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .filter_map(Json::as_object)
            .cloned()
            .collect();
        let source = payload.get("source").and_then(Json::as_str).unwrap_or("");
        let live = source == "live" || source == "claude-oauth";
        let mut error = failure(data, "claude_subscription");
        if let Some(fetch) = payload.get("fetchError").filter(|fetch| fetch.is_truthy()) {
            error = Some(py::str(Some(fetch)));
        }
        let stale = claude_subscription::claude_usage_stale(&payload);
        let mut value = if !windows.is_empty() && !stale {
            let parts: Vec<String> = windows
                .iter()
                .take(2)
                .map(|window| format!("{} {}%", py::str(window.get("label")), g(py::num(window.get("used")))))
                .collect();
            format!("已用 {}", parts.join(" · "))
        } else {
            py::str(Some(&payload.get("state").cloned().unwrap_or_else(|| Json::from("暂无本机用量快照"))))
        };
        if !windows.is_empty() && stale {
            value = if live { "用量待更新" } else { "本机用量待更新" }.into();
        }
        let failed = refresh_failed(error.as_deref());
        // Like every other module: the last good figures stay, marked as a failed refresh.
        value = with_failure(&value, !windows.is_empty() && !stale, failed);
        let source_name = match source {
            "claude-oauth" => "Code 登录",
            "live" => "App 登录 · 手动查询",
            "claude-code" => "Code 本机回传",
            "app-snapshot" => "App 本机快照",
            _ => "Claude App 本机快照",
        };
        let most = windows.iter().map(|window| py::num(window.get("used"))).fold(None, |best: Option<f64>, used| {
            Some(best.map_or(used, |best| if used > best { used } else { best }))
        });
        let mut rows = vec![
            item(
                "Claude 订阅",
                0,
                merged(&[
                    &object! { "badge" => value },
                    &self.brand_icon("claude-subscription"),
                    &object! { "panelusagelevel" => usage_level(Some(most.unwrap_or(0.0)), stale || failed || windows.is_empty()) },
                ]),
            ),
            item(&format!("来源 · {source_name}"), 1, muted(10)),
        ];
        for window in &windows {
            let resets = window
                .get("resetsAt")
                .filter(|at| at.is_truthy())
                .and_then(Json::as_str)
                .and_then(|text| IsoTime::parse(text).ok())
                .map(|time| time.timestamp(&TimeZone::system()));
            let used = py::num(window.get("used"));
            rows.push(item(
                &py::str(window.get("label")),
                1,
                object! {
                    "badge" => used_text(used),
                    "paneldetail" => self.window_detail(used, resets, claude_span(window), payload.get("updatedAt").and_then(Json::number), stale || failed),
                    "panelusagelevel" => usage_level(Some(used), stale || failed),
                },
            ));
        }
        if let Some(extra) = payload.get("extraUsage").and_then(Json::as_object) {
            if extra.get("enabled").is_some_and(Json::is_truthy) {
                let currency = py::str(Some(&extra.get("currency").cloned().unwrap_or_else(|| Json::from("USD"))));
                let symbol = if currency == "USD" { "$".to_string() } else { format!("{currency} ") };
                let money = |value: f64| format!("{symbol}{value:.2}");
                let used = py::num(extra.get("used"));
                let limit = extra.get("limit").filter(|limit| limit.is_truthy()).and_then(Json::number);
                rows.push(item(
                    "额外用量 · 本月",
                    1,
                    object! {
                        "badge" => format!("{}{}", money(used), limit.map(|limit| format!(" / {}", money(limit))).unwrap_or_default()),
                        "paneldetail" => limit.map(|limit| format!("剩余 {}", money((limit - used).max(0.0)))).unwrap_or_else(|| "未设置每月上限".into()),
                    },
                ));
                if let Some(amount) = extra.get("balance").filter(|b| b.as_bool().is_none()).and_then(Json::number) {
                    rows.push(item("额外用量余额", 1, object! { "badge" => money(amount) }));
                }
            } else {
                rows.push(item("额外用量", 1, merged(&[&object! { "badge" => "未开启" }, &muted(10)])));
            }
        }
        // Failures and stale data are explained once here, not on each row.
        let age = if windows.is_empty() { String::new() } else { self.freshness(payload.get("updatedAt")) };
        if failed {
            let mut detail = error.clone().unwrap_or_default();
            if !windows.is_empty() {
                detail += &format!("\n保留最近一次成功的数据 · {age}");
            }
            rows.push(item(
                "更新时间",
                1,
                object! { "badge" => "刷新失败", "paneldetail" => detail, "color" => WARN_COLOR, "size" => 10 },
            ));
        } else if !windows.is_empty() {
            rows.push(item(
                "更新时间",
                1,
                object! {
                    "badge" => age, "paneldetail" => if stale { Json::from("数据待更新") } else { Json::Null },
                    "color" => MUTED_COLOR, "size" => 10,
                },
            ));
        }
        if self.settings().get("claudeOAuthUsage").is_some_and(Json::is_truthy) {
            rows.push(item(
                "立即刷新用量",
                1,
                merged(&[&object! { "sfimage" => "arrow.clockwise" }, &self.act("retry-claude-usage", &[])]),
            ));
            if failed {
                rows.push(item(
                    "重新授权读取登录…",
                    1,
                    merged(&[&object! { "sfimage" => "lock" }, &self.act("reauthorize-claude-oauth", &[])]),
                ));
            }
        } else {
            rows.push(item(
                "启用 Claude Code 登录查询…",
                1,
                merged(&[
                    &object! { "sfimage" => "arrow.triangle.2.circlepath", "paneldetail" => "主动获取订阅用量，不读取 Cookie" },
                    &self.act("set-claude-oauth", &["true"]),
                ]),
            ));
        }
        let app = updates::installed_app_path("Claude", &self.home);
        let href = app.map(|app| self.file_href(&app)).unwrap_or_else(|| "https://claude.ai/".into());
        rows.push(item("打开 Claude", 1, object! { "sfimage" => "macwindow", "href" => href }));
        rows.push(item(
            "打开订阅用量",
            1,
            object! { "sfimage" => "arrow.up.right", "href" => catalog::by_key("claude-subscription").map_or("", |s| s.url) },
        ));
        rows
    }

    pub fn qoder_subscription_lines(&self, data: &Object) -> Vec<String> {
        let service = catalog::by_key("qoder-subscription").expect("qoder");
        let payload = object(data, "qoder_subscription");
        let error = failure(data, "qoder_subscription");
        let failed = refresh_failed(error.as_deref());
        let windows: Vec<(&str, Object)> = [("套餐 Credits", "plan"), ("加购 Credits", "addOn")]
            .into_iter()
            .filter_map(|(name, field)| {
                let quota = payload.get(field)?.as_object()?;
                quota.get("total").is_some_and(Json::is_truthy).then(|| (name, quota.clone()))
            })
            .collect();
        let observed = payload.get("observedAt").and_then(Json::number);
        // The app logs a summary only when its Credits panel is opened; an older one is labelled, not passed off.
        let stale = observed.is_none_or(|at| at == 0.0 || now() - at > 600.0);
        let share = |quota: &Object| 100.0 * py::num(quota.get("used")) / py::num(quota.get("total"));
        let most = windows.iter().map(|(_, quota)| share(quota)).fold(None, |best: Option<f64>, used| {
            Some(best.map_or(used, |best| if used > best { used } else { best }))
        });
        let badge = if let Some(most) = most {
            let mut text = used_text(most);
            if stale {
                text += &format!(" · 截至 {}", self.snapshot_time(observed.unwrap_or(0.0)));
            }
            text
        } else if observed.is_some_and(|at| at != 0.0) {
            if payload.get("exceeded").is_some_and(Json::is_truthy) { "额度已用完" } else { "无 Credits 额度" }.into()
        } else if self.connection_ready("qoder-subscription") {
            "暂无本机记录".into()
        } else {
            "未发现 Qoder".into()
        };
        let mut rows = vec![
            item(
                &service.label(),
                0,
                merged(&[
                    &object! { "badge" => with_failure(&badge, !windows.is_empty(), failed) },
                    &self.brand_icon("qoder-subscription"),
                    &object! { "panelusagelevel" => if windows.is_empty() { Json::Null } else { usage_level(most, stale || failed) } },
                ]),
            ),
            item("来源 · Qoder 本机记录", 1, muted(10)),
        ];
        for (name, quota) in &windows {
            let used = share(quota);
            let amount = |field: &str| rows::credits_amount(py::num(quota.get(field)));
            let mut detail = format!("{} / {} · 剩余 {}", amount("used"), amount("total"), amount("remaining"));
            if *name == "套餐 Credits"
                && let Some(resets) = payload.get("resetsAt").filter(|at| at.is_truthy())
            {
                detail += &format!(
                    "\n重置于 {}",
                    dashboard::format(resets.number().unwrap_or(0.0), &self.local_zone(), "%m/%d %H:%M")
                );
            }
            rows.push(item(
                name,
                1,
                object! { "badge" => used_text(used), "paneldetail" => detail, "panelusagelevel" => usage_level(Some(used), stale || failed) },
            ));
        }
        let hint = "在 Qoder 窗口右下角打开 Credits 用量后更新";
        if failed {
            rows.push(item(
                "更新时间",
                1,
                object! { "badge" => "刷新失败", "paneldetail" => error.unwrap_or_default(), "color" => WARN_COLOR, "size" => 10 },
            ));
        } else if let Some(observed) = observed.filter(|at| *at != 0.0) {
            rows.push(item(
                "更新时间",
                1,
                object! {
                    "badge" => self.freshness(Some(&Json::Double(observed))),
                    "paneldetail" => if stale { Json::from(hint) } else { Json::Null }, "color" => MUTED_COLOR, "size" => 10,
                },
            ));
        } else {
            rows.push(item(hint, 1, muted(10)));
        }
        if let Some(app) = self.update_app_path("qoder") {
            rows.push(item("打开 Qoder", 1, object! { "sfimage" => "macwindow", "href" => self.file_href(&app) }));
        }
        rows.push(item("打开订阅用量", 1, object! { "sfimage" => "arrow.up.right", "href" => service.url }));
        rows
    }

    pub fn cursor_subscription_lines(&self, data: &Object) -> Vec<String> {
        let service = catalog::by_key("cursor-subscription").expect("cursor");
        let enabled = self.settings().get("cursorUsage").is_some_and(Json::is_truthy);
        let payload = if enabled { object(data, "cursor_subscription") } else { Object::new() };
        let error = if enabled { failure(data, "cursor_subscription") } else { None };
        let failed = refresh_failed(error.as_deref());
        let percent = payload.get("percent").filter(|p| !p.is_null()).and_then(Json::number);
        let end = payload.get("cycleEnd").and_then(Json::number);
        let start = payload.get("cycleStart").and_then(Json::number);
        let badge = if !enabled {
            "未开启读取".to_string()
        } else if payload.get("unlimited").is_some_and(Json::is_truthy) {
            "不限量".into()
        } else if let Some(percent) = percent {
            used_text(percent)
        } else if payload.is_empty() {
            "待同步".into()
        } else {
            "暂无套餐用量".into()
        };
        let mut rows = vec![
            item(
                &service.label(),
                0,
                merged(&[
                    &object! { "badge" => with_failure(&badge, percent.is_some(), failed) },
                    &self.brand_icon("cursor-subscription"),
                    &object! { "panelusagelevel" => if percent.is_none() { Json::Null } else { usage_level(percent, failed) } },
                ]),
            ),
            item("来源 · Cursor 账户（cursor.com）", 1, muted(10)),
        ];
        if let Some(percent) = percent {
            let span = match (end, start) {
                (Some(end), Some(start)) if end != 0.0 && start != 0.0 => Some(end - start),
                _ => None,
            };
            rows.push(item(
                "本周期",
                1,
                object! {
                    "badge" => used_text(percent),
                    "paneldetail" => self.window_detail(percent, end, span, payload.get("_fetchedAt").and_then(Json::number), failed),
                    "panelusagelevel" => usage_level(Some(percent), failed),
                },
            ));
            if let Some(limit) = payload.get("limitCents").filter(|limit| limit.is_truthy()) {
                let text =
                    format!("${:.2} / ${:.2}", py::num(payload.get("usedCents")) / 100.0, py::num(Some(limit)) / 100.0);
                rows.push(item("套餐额度", 1, object! { "badge" => text }));
            }
        }
        let extra = payload.get("onDemand").and_then(Json::as_object).cloned().unwrap_or_default();
        if !payload.is_empty() {
            let text = if extra.get("enabled").is_some_and(Json::is_truthy) {
                let mut text = format!("${:.2}", py::num(extra.get("usedCents")) / 100.0);
                if extra.get("limitCents").is_some_and(Json::is_truthy) {
                    text += &format!(" / ${:.2}", py::num(extra.get("limitCents")) / 100.0);
                }
                text
            } else {
                "未开启".into()
            };
            rows.push(item("按需用量", 1, object! { "badge" => text }));
        }
        if enabled {
            rows.push(self.refresh_row(&payload, error.as_deref(), 1));
        } else {
            rows.push(item(
                "读取 Cursor 用量记录…",
                1,
                merged(&[
                    &object! {
                        "sfimage" => "arrow.triangle.2.circlepath",
                        "paneldetail" => "用本机 Cursor 登录查询 cursor.com，令牌只在内存中使用",
                    },
                    &self.act("set-cursor-usage", &["true"]),
                ]),
            ));
        }
        if let Some(app) = self.update_app_path("cursor") {
            rows.push(item("打开 Cursor", 1, object! { "sfimage" => "macwindow", "href" => self.file_href(&app) }));
        }
        rows.push(item("打开用量与账单", 1, object! { "sfimage" => "arrow.up.right", "href" => service.url }));
        rows
    }

    // ChatGPT and OpenCodex's accounts

    /// Weekly share already used by each ChatGPT account: name, percent or none, state.
    pub fn account_usage(&self, data: &Object) -> Vec<(String, Option<f64>, String)> {
        let accounts = py::dict(object(data, "accounts").get("accounts")).as_array().cloned().unwrap_or_default();
        let quotas = py::dict(object(data, "quota").get("quotas")).as_object().cloned().unwrap_or_default();
        accounts
            .iter()
            .enumerate()
            .map(|(index, row)| {
                let fields = row.as_object().cloned().unwrap_or_default();
                let id = py::str(Some(&py::or(fields.get("id"), Json::from(""))));
                let quota = py::or(quotas.get(&id), py::or(fields.get("quota"), Json::Object(Object::new())))
                    .as_object()
                    .cloned()
                    .unwrap_or_default();
                let used = quota.get("weeklyPercent").and_then(Json::python_float).map(|used| used.clamp(0.0, 100.0));
                let label = py::str(Some(&py::or(
                    Some(&py::or(
                        Some(&py::or(fields.get("logLabel"), fields.get("email").cloned().unwrap_or(Json::Null))),
                        Json::from(id.as_str()),
                    )),
                    Json::from(format!("账号 {}", index + 1)),
                )));
                let state = if fields.get("paused").is_some_and(Json::is_truthy) {
                    " 暂停"
                } else if fields.get("needsReauth").is_some_and(Json::is_truthy) {
                    " 需登录"
                } else {
                    ""
                };
                (shorten(&label, 14), used, state.to_string())
            })
            .collect()
    }

    pub fn account_quick_lines(&self, data: &Object) -> Vec<String> {
        let accounts = py::dict(object(data, "accounts").get("accounts")).as_array().cloned().unwrap_or_default();
        let active = object(data, "active");
        let quotas = py::dict(object(data, "quota").get("quotas")).as_object().cloned().unwrap_or_default();
        let pinned = active.get("pinnedAccountId");
        if accounts.is_empty() {
            let error = failure(data, "accounts");
            return vec![item(
                if error.is_some() { "账号读取失败" } else { "暂无可用账号" },
                0,
                object! { "color" => if error.is_some() { WARN_COLOR } else { MUTED_COLOR } },
            )];
        }
        let mut lines = Vec::new();
        let mut current: Option<Object> = None;
        for row in &accounts {
            let fields = row.as_object().cloned().unwrap_or_default();
            let id = py::str(Some(&py::or(fields.get("id"), Json::from(""))));
            let name = shorten(
                &py::str(Some(&py::or(
                    Some(&py::or(
                        Some(&py::or(fields.get("logLabel"), fields.get("email").cloned().unwrap_or(Json::Null))),
                        Json::from(id.as_str()),
                    )),
                    Json::from("账号"),
                ))),
                25,
            );
            let quota = py::or(quotas.get(&id), py::or(fields.get("quota"), Json::Object(Object::new())))
                .as_object()
                .cloned()
                .unwrap_or_default();
            let quota_text = quota
                .get("weeklyPercent")
                .and_then(Json::python_float)
                .map(|used| format!(" · {}", used_text(used)))
                .unwrap_or_default();
            if fields.get("paused").is_some_and(Json::is_truthy) {
                lines.push(item(
                    &format!("启用 · {name}{quota_text}"),
                    0,
                    merged(&[&object! { "color" => RANK_COLOR }, &self.act("account-pause", &[&id, "false"])]),
                ));
            } else if pinned == Some(&Json::from(id.as_str())) {
                current = Some(fields.clone());
                lines.push(item(
                    &format!("{name}{quota_text}"),
                    0,
                    object! { "color" => RANK_COLOR, "checked" => true, "panelradio" => "on" },
                ));
            } else {
                // One active account at a time: the panel shows these rows as a radio group.
                lines.push(item(
                    &format!("{name}{quota_text}"),
                    0,
                    merged(&[
                        &object! { "color" => RANK_COLOR, "panelradio" => "off" },
                        &self.act("account-use", &[&id]),
                    ]),
                ));
            }
        }
        if let Some(current) = current {
            let id = py::str(Some(&py::or(current.get("id"), Json::from(""))));
            lines.push(item(
                "暂停当前账号",
                0,
                merged(&[&object! { "color" => MUTED_COLOR }, &self.act("account-pause", &[&id, "true"])]),
            ));
        }
        lines
    }

    /// While OpenCodex is unreachable its account quota isn't being read, whatever that cache says.
    pub fn proxy_quota_error(&self, data: &Object) -> Option<String> {
        let health = failure(data, "healthz");
        if refresh_failed(health.as_deref()) { health } else { failure(data, "quota") }
    }

    pub fn chatgpt_lines(&self, data: &Object) -> Vec<String> {
        let label = catalog::by_key("chatgpt").expect("chatgpt").label();
        if self.chatgpt_source() == "proxy" {
            let usage = self.account_usage(data);
            let known: Vec<f64> = usage.iter().filter_map(|(_, used, _)| *used).collect();
            let error = self.proxy_quota_error(data);
            let failed = refresh_failed(error.as_deref());
            let mut badge = if usage.is_empty() {
                if self.opencodex_configured() { "待同步" } else { "未配置" }.to_string()
            } else if usage.len() == 1 {
                format!("{}{}", usage[0].1.map(used_text).unwrap_or_else(|| "额度待同步".into()), usage[0].2)
            } else {
                let parts: Vec<String> = usage
                    .iter()
                    .map(|(name, used, state)| {
                        let share = used.map(|used| format!("{}%", g(round(used, 1)))).unwrap_or_else(|| "—".into());
                        format!("{name} {share}{state}")
                    })
                    .collect();
                format!("已用 {}", parts.join(" · "))
            };
            let compact = format!("{} 个账户", usage.len());
            if usage.len() > 2 || badge.chars().count() > 32 {
                badge = compact.clone();
            }
            let fallback = if usage.len() > 1 && badge != compact {
                Json::from(with_failure(&compact, true, failed))
            } else {
                Json::Null
            };
            let most = known.iter().copied().fold(None, |best: Option<f64>, used| {
                Some(best.map_or(used, |best| if used > best { used } else { best }))
            });
            let mut lines = vec![item(
                &label,
                0,
                merged(&[
                    &object! { "badge" => with_failure(&badge, !usage.is_empty(), failed), "panelfallback" => fallback },
                    &self.brand_icon("chatgpt"),
                    &object! { "panelusagelevel" => if known.is_empty() { Json::Null } else { usage_level(most, failed) } },
                ]),
            )];
            lines.push(item("来源 · OpenCodex 账户 · 编程工具额度", 1, muted(10)));
            for (name, used, state) in &usage {
                lines.push(item(
                    name,
                    1,
                    object! {
                        "badge" => format!("{}{state}", used.map(used_text).unwrap_or_else(|| "额度待同步".into())),
                        "panelusagelevel" => if used.is_none() { Json::Null } else { usage_level(*used, failed) },
                    },
                ));
            }
            if self.opencodex_configured() {
                lines.push(self.refresh_row(&object(data, "quota"), error.as_deref(), 1));
            }
            lines.push(item(
                "打开账户管理",
                1,
                object! { "href" => format!("{}/", self.base), "sfimage" => "arrow.up.right" },
            ));
            return lines;
        }
        let payload = object(data, "chatgpt");
        let windows: Vec<Object> = payload
            .get("windows")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .filter_map(Json::as_object)
            .cloned()
            .collect();
        let error = failure(data, "chatgpt");
        let failed = refresh_failed(error.as_deref());
        let moment = now();
        let visible: Vec<&Object> = windows
            .iter()
            .filter(|window| {
                !window.get("resetsAt").is_some_and(Json::is_truthy)
                    || window.get("resetsAt").and_then(Json::number).unwrap_or(0.0) > moment
            })
            .collect();
        let most = visible
            .iter()
            .map(|window| 100.0 - py::num(window.get("remaining")))
            .fold(None, |best: Option<f64>, used| {
                Some(best.map_or(used, |best| if used > best { used } else { best }))
            });
        let mut badge = match most {
            Some(most) => used_text(most),
            None if self.chatgpt_configured() => {
                py::str(Some(&payload.get("state").cloned().unwrap_or_else(|| Json::from("待同步"))))
            }
            None => "未配置".into(),
        };
        if !windows.is_empty() && visible.is_empty() {
            badge = "待刷新".into();
        }
        let mut lines = vec![item(
            &label,
            0,
            merged(&[
                &object! { "badge" => with_failure(&badge, !visible.is_empty(), failed) },
                &self.brand_icon("chatgpt"),
                &object! { "panelusagelevel" => if visible.is_empty() { Json::Null } else { usage_level(most, failed) } },
            ]),
        )];
        lines.push(item("来源 · 本机登录 · 编程工具额度", 1, muted(10)));
        for window in &visible {
            let minutes = py::or(window.get("minutes"), Json::Int(0)).number().unwrap_or(0.0);
            let used = 100.0 - py::num(window.get("remaining"));
            lines.push(item(
                &chatgpt_window_label(window),
                1,
                object! {
                    "badge" => used_text(used), "panelusagelevel" => usage_level(Some(used), failed),
                    "paneldetail" => self.window_detail(used, window.get("resetsAt").and_then(Json::number), Some(minutes * 60.0),
                                                        payload.get("_fetchedAt").and_then(Json::number), failed),
                },
            ));
        }
        if visible.is_empty()
            && !failed
            && let Some(state) = payload.get("state").filter(|state| state.is_truthy())
        {
            lines.push(item(&py::str(Some(state)), 1, muted(10)));
        }
        if self.chatgpt_configured() {
            lines.push(self.refresh_row(&payload, error.as_deref(), 1));
        }
        lines.push(item("打开 ChatGPT", 1, object! { "href" => "https://chatgpt.com", "sfimage" => "arrow.up.right" }));
        lines
    }

    /// The installed desktop app an update row or sidebar opens, when there is one.
    pub fn update_app_path(&self, key: &str) -> Option<String> {
        updates::update_apps(key).iter().find_map(|name| updates::installed_app_path(name, &self.home))
    }
}
