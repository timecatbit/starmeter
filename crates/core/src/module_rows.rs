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
use crate::{accounts, catalog, claude_subscription, cursor, dashboard, object, py, updates};

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
        let shown = self.selected_range();
        put(
            &mut data,
            "range_usage",
            self.cache_result(&self.with_devices(&shown, self.read_cache(&self.range_cache_name(None))), ttl),
        );
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
                let (last, error) = self.cache_result(
                    &self.with_devices(&previous, self.read_cache(&self.range_cache_name(Some(&previous)))),
                    ttl,
                );
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
        let most = windows.iter().map(|window| py::num(window.get("used"))).fold(None, |best: Option<f64>, used| {
            Some(best.map_or(used, |best| if used > best { used } else { best }))
        });
        // 本周已用: the week's own window (本周); with none, the tightest. The homepage's two bars name the limits.
        let week = windows
            .iter()
            .find(|window| py::str(window.get("label")) == "本周")
            .map(|window| py::num(window.get("used")));
        let headline = rows::headline_used(week, most);
        let mut value = if let Some((_, text)) = headline.as_ref().filter(|_| !stale) {
            text.clone()
        } else {
            py::str(Some(&payload.get("state").cloned().unwrap_or_else(|| Json::from("暂无本机用量快照"))))
        };
        if !windows.is_empty() && stale {
            value = if live { "用量待更新" } else { "本机用量待更新" }.into();
        }
        let failed = refresh_failed(error.as_deref());
        // 订阅额度框架: a window used up with extra usage on, what is left of it is the value, in orange.
        let extra_on = payload
            .get("extraUsage")
            .and_then(Json::as_object)
            .filter(|extra| extra.get("enabled").is_some_and(Json::is_truthy))
            .cloned();
        let on_reserve = !stale && most.is_some_and(|most| most >= 100.0) && extra_on.is_some();
        if let Some(extra) = extra_on.as_ref().filter(|_| on_reserve) {
            let currency = py::str(Some(&extra.get("currency").cloned().unwrap_or_else(|| Json::from("USD"))));
            let symbol = if currency == "USD" { "$".to_string() } else { format!("{currency} ") };
            let left = match extra.get("limit").filter(|limit| limit.is_truthy()).and_then(Json::number) {
                Some(limit) => format!("额外用量剩 {symbol}{:.2}", (limit - py::num(extra.get("used"))).max(0.0)),
                None => "在用额外用量".into(),
            };
            value = left;
        }
        // Like every other module: the last good figures stay, marked as a failed refresh.
        value = with_failure(&value, !windows.is_empty() && !stale, failed);
        let source_name = match source {
            "claude-oauth" => "Code 登录",
            "live" => "App 登录 · 手动查询",
            "claude-code" => "Code 本机回传",
            "app-snapshot" => "App 本机快照",
            _ => "Claude App 本机快照",
        };
        let limits: Vec<(String, f64, bool)> = windows
            .iter()
            .map(|window| {
                (py::str(window.get("label")), py::num(window.get("used")), claude_span(window) == Some(5.0 * 3600.0))
            })
            .collect();
        let mut rows = vec![
            item(
                "Claude 订阅",
                0,
                merged(&[
                    &object! {
                        "badge" => value, "panelgauges" => rows::gauges(&rows::headline_pair(&limits), stale || failed),
                        "paneldetail" => rows::reserve_week(on_reserve, &limits),
                    },
                    &self.brand_icon("claude-subscription"),
                    &object! {
                        "panelusagelevel" => if on_reserve { rows::reserve_level(failed) } else { usage_level(Some(headline.as_ref().map_or(0.0, |(share, _)| *share)), stale || failed || windows.is_empty()) },
                        "panelshare" => rows::reserve_share(on_reserve, most),
                    },
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
        // 立即刷新: with the sign-in read, the Claude retry (its own backoff, CLAUDE-QUOTA-2); otherwise a read again now.
        let oauth = self.settings().get("claudeOAuthUsage").is_some_and(Json::is_truthy);
        let again =
            if oauth { self.act("retry-claude-usage", &[]) } else { self.act("refresh-now", &["claude-subscription"]) };
        if failed {
            let mut detail = error.clone().unwrap_or_default();
            if !windows.is_empty() {
                detail += &format!("\n保留最近一次成功的数据 · {age}");
            }
            rows.push(self.refresh_now_row(again, "刷新失败", Some(&detail), true, 1));
        } else if !windows.is_empty() {
            rows.push(self.refresh_now_row(again, &age, stale.then_some("数据待更新"), false, 1));
        } else if oauth {
            rows.push(item("立即刷新", 1, merged(&[&object! { "sfimage" => "arrow.clockwise" }, &again])));
        }
        if oauth {
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
            let again = self.act("refresh-now", &["qoder-subscription"]);
            rows.push(self.refresh_now_row(again, "刷新失败", Some(&error.unwrap_or_default()), true, 1));
        } else if let Some(observed) = observed.filter(|at| *at != 0.0) {
            let badge = self.freshness(Some(&Json::Double(observed)));
            let again = self.act("refresh-now", &["qoder-subscription"]);
            rows.push(self.refresh_now_row(again, &badge, stale.then_some(hint), false, 1));
        } else {
            rows.push(item(hint, 1, muted(10)));
        }
        if let Some(app) = self.update_app_path("qoder") {
            rows.push(item("打开 Qoder", 1, object! { "sfimage" => "macwindow", "href" => self.file_href(&app) }));
        }
        rows.push(item("打开订阅用量", 1, object! { "sfimage" => "arrow.up.right", "href" => service.url }));
        rows
    }

    /// Gemini 订阅: the quota Antigravity's app shows, asked of it on this computer (antigravity.rs). Gemini's five-hour
    /// and weekly limits, then Claude's with GPT's; with the app closed the last figures stay, marked with when they
    /// were read, and a window past its reset reads as filled again.
    /// Gemini 订阅's bars, each group's line marked with its logos: Gemini's, and Claude's with OpenAI's.
    fn gemini_bars_marked(&self, windows: &[Object]) -> Vec<rows::Bar> {
        let mut bars = gemini_bars(windows);
        let mark = |key: &str| {
            let icon = self.brand_icon(key);
            let image = |name: &str| icon.get(name).and_then(Json::as_str).map(String::from);
            image("image").map(|image| (image, false)).or_else(|| image("templateimage").map(|image| (image, true)))
        };
        for bar in &mut bars {
            let keys: &[&str] = match bar.group.as_str() {
                "Gemini" => &["gemini-subscription"],
                "Claude 和 GPT" => &["claude-api", "chatgpt-api"],
                _ => &[],
            };
            bar.marks = keys.iter().copied().filter_map(mark).collect();
        }
        bars
    }

    pub fn gemini_subscription_lines(&self, data: &Object) -> Vec<String> {
        let service = catalog::by_key("gemini-subscription").expect("gemini");
        let payload = object(data, "gemini_subscription");
        let error = failure(data, "gemini_subscription");
        let closed = error.as_deref() == Some(crate::antigravity::NOT_RUNNING);
        let failed = refresh_failed(error.as_deref()) && !closed;
        let windows = gemini_windows(&payload, now());
        let read_at = payload.get("_fetchedAt").and_then(Json::number);
        // The row's own figure is Gemini's (用户：Gemini 的已用应该是 Gemini 的，而不是最高的那个数字): the tightest of
        // Gemini's own windows; Claude's with GPT's are a separate allowance, on their own line below. Only with no
        // Gemini window read, the tightest of what there is.
        let own: Vec<&Object> =
            windows.iter().filter(|window| window.get("group").and_then(Json::as_str) == Some("gemini")).collect();
        let counted: Vec<&Object> = if own.is_empty() { windows.iter().collect() } else { own };
        let most = counted
            .iter()
            .map(|window| 100.0 - py::num(window.get("remaining")))
            .fold(None, |best: Option<f64>, used| {
                Some(best.map_or(used, |best| if used > best { used } else { best }))
            });
        // 本周已用: Gemini's own week, when it was read.
        let week = windows
            .iter()
            .find(|window| {
                window.get("group").and_then(Json::as_str) == Some("gemini")
                    && window.get("window").and_then(Json::as_str) != Some("5h")
            })
            .map(|window| 100.0 - py::num(window.get("remaining")));
        let headline = rows::headline_used(week, most);
        let most = headline.as_ref().map(|(share, _)| *share);
        let present = self.connection_ready("gemini-subscription");
        let badge = match &headline {
            Some((_, text)) if closed => format!("{text} · 截至 {}", self.snapshot_time(read_at.unwrap_or(0.0))),
            Some((_, text)) => text.clone(),
            None if closed || !present => if present || antigravity_app(&self.home).is_some() {
                "打开 Antigravity 后显示"
            } else {
                "未发现 Antigravity"
            }
            .into(),
            None => "待同步".into(),
        };
        let mut rows = vec![
            item(
                &service.label(),
                0,
                merged(&[
                    &object! {
                        "badge" => with_failure(&badge, !windows.is_empty(), failed),
                        "panelgauges" => rows::gauges(&self.gemini_bars_marked(&windows), closed || failed),
                    },
                    &self.brand_icon("gemini-subscription"),
                    &object! { "panelusagelevel" => if windows.is_empty() { Json::Null } else { usage_level(most, closed || failed) } },
                ]),
            ),
            item("来源 · Antigravity 本机额度", 1, muted(10)),
        ];
        for window in &windows {
            let used = 100.0 - py::num(window.get("remaining"));
            rows.push(item(
                &gemini_window_name(window),
                1,
                object! {
                    "badge" => used_text(used), "panelusagelevel" => usage_level(Some(used), closed || failed),
                    "paneldetail" => self.window_detail(used, window.get("resetsAt").and_then(Json::number), gemini_span(window),
                                                        read_at, closed || failed),
                },
            ));
        }
        if closed && read_at.is_some() {
            rows.push(item(
                "更新时间",
                1,
                object! {
                    "badge" => self.freshness(payload.get("_fetchedAt")), "paneldetail" => "Antigravity 没开，打开后更新",
                    "color" => MUTED_COLOR, "size" => 10,
                },
            ));
        } else if closed || !present {
            rows.push(item(
                if present { "打开 Antigravity 后更新" } else { "装好 Antigravity 并登录后显示额度" },
                1,
                muted(10),
            ));
        } else {
            rows.push(self.refresh_row("gemini-subscription", &payload, error.as_deref(), 1));
        }
        if let Some(app) = antigravity_app(&self.home) {
            rows.push(item(
                "打开 Antigravity",
                1,
                object! { "sfimage" => "macwindow", "href" => self.file_href(&app) },
            ));
        }
        rows.push(item("打开订阅管理", 1, object! { "sfimage" => "arrow.up.right", "href" => service.url }));
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
        // 订阅额度框架: the cycle's included usage used up with on-demand on, what is left of it is the value, in orange.
        let on_demand = payload
            .get("onDemand")
            .and_then(Json::as_object)
            .filter(|extra| extra.get("enabled").is_some_and(Json::is_truthy))
            .cloned();
        let on_reserve = percent.is_some_and(|percent| percent >= 100.0) && on_demand.is_some();
        let badge = if !enabled {
            "未开启读取".to_string()
        } else if payload.get("unlimited").is_some_and(Json::is_truthy) {
            "不限量".into()
        } else if let Some(extra) = on_demand.as_ref().filter(|_| on_reserve) {
            match extra.get("limitCents").filter(|limit| limit.is_truthy()).and_then(Json::number) {
                Some(limit) => format!("按需剩 ${:.2}", ((limit - py::num(extra.get("usedCents"))) / 100.0).max(0.0)),
                None => "在用按需用量".into(),
            }
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
                    &object! {
                        "panelusagelevel" => if percent.is_none() { Json::Null } else if on_reserve { rows::reserve_level(failed) } else { usage_level(percent, failed) },
                        "panelshare" => rows::reserve_share(on_reserve, percent),
                    },
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
            rows.push(self.refresh_row("cursor-subscription", &payload, error.as_deref(), 1));
            // An encrypted desktop login: read from the keychain once the user allows it here, macOS asking them.
            let keychain = self.settings().get("cursorKeychain").is_some_and(Json::is_truthy);
            if keychain || cursor::login_encrypted(&self.home, &self.environment) {
                let detail = if keychain {
                    "用钥匙串里的“Cursor Safe Storage”解开 Cursor 的登录，令牌只在内存中使用"
                } else {
                    "这个版本的 Cursor 把登录加密存进了钥匙串；打开后 macOS 会询问是否允许，选“始终允许”"
                };
                rows.push(item(
                    "从钥匙串读取登录",
                    1,
                    merged(&[
                        &object! { "sfimage" => "lock", "checked" => keychain, "paneldetail" => detail },
                        &self.act("set-cursor-keychain", &[if keychain { "false" } else { "true" }]),
                    ]),
                ));
            }
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
                lines.push(self.refresh_row("chatgpt", &object(data, "quota"), error.as_deref(), 1));
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
            .filter(|window| rows::known_share(window))
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
        // 本周已用: the week's own window; with none, the tightest.
        let week = visible
            .iter()
            .find(|window| chatgpt_window_label(window) == "本周")
            .map(|window| 100.0 - py::num(window.get("remaining")));
        let headline = rows::headline_used(week, most);
        let mut badge = match &headline {
            Some((_, text)) => text.clone(),
            None if self.chatgpt_configured() => {
                py::str(Some(&payload.get("state").cloned().unwrap_or_else(|| Json::from("待同步"))))
            }
            None => "未配置".into(),
        };
        if !windows.is_empty() && visible.is_empty() {
            badge = "待刷新".into();
        }
        // 订阅额度框架: the windows used up (or ordinary use stopped) with Credits left, the Credits go beside the week's
        // share, in orange (用户：GPT 的额度可以写在已用附近); the free resets are on its page (用户：GPT 的重置可以写在里面).
        let credits = payload.get("credits").and_then(Json::as_object);
        let unlimited = credits.is_some_and(|credits| credits.get("unlimited") == Some(&Json::Bool(true)));
        let balance = credits.map(|credits| rows::credits_amount(py::num(credits.get("balance"))));
        let exhausted = most.is_some_and(|most| most >= 100.0) || payload.get("blocked").is_some_and(Json::is_truthy);
        let on_reserve = exhausted && credits.is_some() && !visible.is_empty();
        if on_reserve {
            badge = if unlimited {
                "Credits 不限".to_string()
            } else {
                format!("余 {} Credits", balance.clone().unwrap_or_default())
            };
        }
        let resets = payload.get("resets").and_then(Json::as_object);
        let offered = resets.and_then(|resets| resets.get("count")).and_then(Json::integer).unwrap_or(0);
        let limits: Vec<(String, f64, bool)> = visible
            .iter()
            .map(|window| {
                let name = chatgpt_window_label(window);
                let five_hours = name == "5 小时";
                (name, 100.0 - py::num(window.get("remaining")), five_hours)
            })
            .collect();
        let mut lines = vec![item(
            &label,
            0,
            merged(&[
                &object! {
                    "badge" => with_failure(&badge, !visible.is_empty(), failed),
                    "panelgauges" => rows::gauges(&rows::headline_pair(&limits), failed),
                    "paneldetail" => rows::reserve_week(on_reserve, &limits),
                },
                &self.brand_icon("chatgpt"),
                &object! {
                    "panelusagelevel" => if visible.is_empty() { Json::Null } else if on_reserve { rows::reserve_level(failed) } else { usage_level(headline.as_ref().map(|(share, _)| *share), failed) },
                    "panelshare" => rows::reserve_share(on_reserve, most),
                },
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
        // What is left once the windows are used up, and the free resets: after the windows, as on every plan.
        if credits.is_some() {
            let badge = if unlimited { "不限".to_string() } else { format!("余 {}", balance.unwrap_or_default()) };
            lines.push(item("Credits", 1, object! { "badge" => badge, "paneldetail" => "窗口用完后按用量扣除" }));
        }
        if offered > 0 {
            let detail = match resets.and_then(|resets| resets.get("expiresAt")).and_then(Json::number) {
                Some(at) => {
                    format!("最早 {} 过期 · 在 Codex 里使用", dashboard::format(at, &self.local_zone(), "%m/%d"))
                }
                None => "一次补满窗口 · 在 Codex 里使用".into(),
            };
            lines.push(item(
                "免费重置",
                1,
                object! { "badge" => format!("可用 {offered} 次"), "paneldetail" => detail },
            ));
        }
        if self.chatgpt_configured() {
            lines.push(self.refresh_row("chatgpt", &payload, error.as_deref(), 1));
        }
        lines.push(item("打开 ChatGPT", 1, object! { "href" => "https://chatgpt.com", "sfimage" => "arrow.up.right" }));
        lines
    }

    /// The installed desktop app an update row or sidebar opens, when there is one.
    pub fn update_app_path(&self, key: &str) -> Option<String> {
        updates::update_apps(key).iter().find_map(|name| updates::installed_app_path(name, &self.home))
    }
}

/// Antigravity's app, where it is installed.
pub fn antigravity_app(home: &str) -> Option<String> {
    updates::installed_app_path("Antigravity", home)
}

/// Gemini 订阅's windows as last read: Gemini's first, then Claude's with GPT's, each five hours before the week; a
/// window past its reset has filled again: none of it used and no reset known (as an unused one reads), kept in its
/// place so the row's bars stay a whole grid (用户：空着一格很丑; it used to be left out, its place empty).
pub fn gemini_windows(payload: &Object, now: f64) -> Vec<Object> {
    let mut windows: Vec<Object> = payload
        .get("windows")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .filter_map(Json::as_object)
        .filter(|window| rows::known_share(window))
        .map(|window| {
            let mut window = window.clone();
            if window.get("resetsAt").and_then(Json::number).is_some_and(|at| at <= now) {
                window.insert("remaining", Json::Double(100.0));
                window.insert("resetsAt", Json::Null);
            }
            window
        })
        .collect();
    let rank = |window: &Object| {
        let group = window.get("group").and_then(Json::as_str).unwrap_or("");
        let order = match group {
            "gemini" => 0,
            "3p" => 1,
            _ => 2,
        };
        (order, i32::from(window.get("window").and_then(Json::as_str) != Some("5h")))
    };
    windows.sort_by_key(rank);
    windows
}

/// A window's name: 5 小时 and 本周 for Gemini's own, the group's name after them for the others, so that a narrow
/// column (the command line's) that cuts the name still tells the two apart.
pub fn gemini_window_name(window: &Object) -> String {
    let span = if window.get("window").and_then(Json::as_str) == Some("5h") { "5 小时" } else { "本周" };
    match window.get("group").and_then(Json::as_str) {
        Some("gemini") => span.to_string(),
        Some("3p") => format!("{span} · Claude 和 GPT"),
        _ => format!("{span} · {}", py::str(window.get("groupName"))),
    }
}

/// Gemini 订阅's homepage bars. Its two groups are separate allowances (Gemini's models, and Claude's with GPT's,
/// picked between by the model one uses), so each has a line of its own, every window in it: its five hours on the
/// left, its week on the right, as on every plan's row (a window it lacks leaves its place empty), the group named on
/// the line's first bar; the panel heads the second line with it. A single group draws its two windows on one line.
pub fn gemini_bars(windows: &[Object]) -> Vec<rows::Bar> {
    fn used(window: &Object) -> f64 {
        100.0 - py::num(window.get("remaining"))
    }
    fn five(window: &Object) -> bool {
        window.get("window").and_then(Json::as_str) == Some("5h")
    }
    /// The tightest of some windows, the earlier on a tie (Antigravity gives one of each length).
    fn tightest<'a>(group: impl Iterator<Item = &'a Object>) -> Option<&'a Object> {
        group.fold(None, |best: Option<&Object>, window| match best {
            Some(best) if used(best) >= used(window) => Some(best),
            _ => Some(window),
        })
    }
    let span = |window: &Object| if five(window) { "5 小时" } else { "本周" };
    let mut groups: Vec<(String, Vec<&Object>)> = Vec::new();
    for window in windows {
        let name = match window.get("group").and_then(Json::as_str) {
            Some("gemini") => "Gemini".to_string(),
            Some("3p") => "Claude 和 GPT".to_string(),
            _ => window.get("groupName").and_then(Json::as_str).unwrap_or("").to_string(),
        };
        match groups.iter_mut().find(|group| group.0 == name) {
            Some(group) => group.1.push(window),
            None => groups.push((name, vec![window])),
        }
    }
    if groups.len() < 2 {
        let limits: Vec<(String, f64, bool)> =
            windows.iter().map(|window| (gemini_window_name(window), used(window), five(window))).collect();
        return rows::headline_pair(&limits);
    }
    groups.truncate(2);
    let mut bars: Vec<rows::Bar> = Vec::new();
    for (line, (name, group)) in groups.iter().enumerate() {
        for column in [0, 1] {
            let Some(window) = tightest(group.iter().copied().filter(|window| five(window) == (column == 0))) else {
                continue;
            };
            let first = !bars.iter().any(|bar| bar.place / 2 == line);
            bars.push(rows::Bar::new(line * 2 + column, if first { name } else { "" }, span(window), used(window)));
        }
    }
    bars
}

/// How long a window runs, for the pace it is used at.
pub fn gemini_span(window: &Object) -> Option<f64> {
    match window.get("window").and_then(Json::as_str) {
        Some("5h") => Some(5.0 * 3600.0),
        Some("weekly") => Some(7.0 * 86400.0),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{gemini_bars, gemini_windows};
    use crate::json::{Json, Object};
    use crate::runtime::{Runtime, now};
    use crate::{object, py};

    /// CLAUDE-QUOTA-1: a refresh that failed keeps the same login's last good figures on the homepage, beside 刷新失败,
    /// while the reading is recent and its windows haven't reset; past ten minutes it goes stale as any reading does.
    #[test]
    fn a_failed_refresh_keeps_the_last_good_figures_on_the_homepage() {
        let home = std::env::temp_dir().join(format!("starmeter-claude-row-{}", crate::runtime::random_hex(6)));
        std::fs::create_dir_all(home.join("state")).unwrap();
        let environment: std::collections::HashMap<String, String> = [
            ("HOME", home.to_string_lossy().into_owned()),
            ("OCX_STATE_DIR", home.join("state").to_string_lossy().into_owned()),
            ("OCX_CACHE_DIR", home.join("cache").to_string_lossy().into_owned()),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect();
        let runtime = Runtime::new(environment, "starmeter");
        let tomorrow = jiff::Timestamp::from_second((now() + 86400.0) as i64).unwrap().to_string();
        let kept = |read_ago: f64| -> Object {
            object! {
                "source" => "claude-oauth", "state" => "刷新失败", "stale" => false, "updatedAt" => now() - read_ago,
                "windows" => Json::Array(vec![object! { "label" => "本周", "used" => 39, "resetsAt" => tomorrow.as_str(), "span" => 604800 }.into()]),
                "fetchError" => "网络暂时不可用",
            }
        };
        let first = |payload: Object| -> String {
            runtime
                .claude_subscription_lines(&object! { "claude_subscription" => payload })
                .into_iter()
                .next()
                .unwrap_or_default()
        };
        let line = first(kept(90.0));
        assert!(line.contains("已用 39% · 刷新失败"), "{line}");
        let line = first(kept(900.0));
        assert!(!line.contains("39%") && line.contains("刷新失败"), "{line}");
        let _ = std::fs::remove_dir_all(&home);
    }

    /// A Gemini window past its reset has filled again: 0% used and no reset known, in its place, so the row's bars
    /// stay a whole grid; one still running is as read.
    #[test]
    fn a_gemini_window_past_its_reset_reads_as_filled_again() {
        let now = 1_800_000_000.0;
        let window = |group: &str, span: &str, left: f64, resets: f64| -> Json {
            object! { "group" => group, "window" => span, "remaining" => left, "resetsAt" => resets }.into()
        };
        let payload = object! { "windows" => Json::Array(vec![
            window("gemini", "5h", 90.7, now - 60.0),
            window("gemini", "weekly", 89.5, now + 86400.0),
            window("3p", "5h", 100.0, now - 1.0),
            window("3p", "weekly", 50.3, now + 86400.0),
        ]) };
        let windows = gemini_windows(&payload, now);
        let read: Vec<(f64, Option<f64>)> = windows
            .iter()
            .map(|window| (py::num(window.get("remaining")), window.get("resetsAt").and_then(Json::number)))
            .collect();
        assert_eq!(read, [(100.0, None), (89.5, Some(now + 86400.0)), (100.0, None), (50.3, Some(now + 86400.0))]);
        let bars: Vec<(usize, f64)> = gemini_bars(&windows).iter().map(|bar| (bar.place, bar.used)).collect();
        assert_eq!(bars.len(), 4, "a whole grid: {bars:?}");
        assert!((bars[0].1 - 0.0).abs() < 1e-9 && (bars[2].1 - 0.0).abs() < 1e-9);
    }
}
