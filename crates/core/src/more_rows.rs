//! The rows of the other modules: active sessions, Komari, Harness updates and the rest of the homepage.

use jiff::tz::TimeZone;

use crate::json::{Json, Object};
use crate::module_rows::{failure, object};
use crate::rows::{
    BAD_COLOR, KOMARI_ONLINE, MENU_ICON_SIZE, MUTED_COLOR, fmt_tokens, item, merged, pad_display, refresh_failed,
    shorten,
};
use crate::runtime::{PRODUCT, RANGES, Runtime, VERSION, now, quote};
use crate::update_products::{self, Kind};
use crate::{claude_subscription, dashboard, object, py, updates};

/// An MCP tool by its own name: mcp__ccd_turn__update_plan is update_plan.
pub fn tool_name(name: &str) -> String {
    match name.strip_prefix("mcp__").and(name.rfind("__")) {
        Some(last) if !name[last + 2..].is_empty() => name[last + 2..].to_string(),
        _ => name.to_string(),
    }
}

impl Runtime {
    /// A session's turns as the menu lists them, under a heading of its project, how many turns and what the whole
    /// session has cost (`total`); a turn that sent no request (stopped before the model answered) is left out.
    pub fn turn_lines(&self, turns: &[Object], unit: &Json, project: Option<&str>, total: Option<&str>) -> Vec<String> {
        let turns: Vec<&Object> = turns.iter().filter(|turn| py::num(turn.get("requests")) != 0.0).collect();
        if turns.is_empty() {
            return vec![];
        }
        let symbol = unit.get("symbol").and_then(Json::as_str).unwrap_or("$");
        let rate = unit.get("rate").and_then(Json::number).unwrap_or(1.0);
        let count = format!("最近 {} 轮", turns.len());
        let heading: Vec<&str> =
            [project, Some(count.as_str()), total].into_iter().flatten().filter(|part| !part.is_empty()).collect();
        let mut lines = vec![item(&heading.join(" · "), 2, object! { "color" => MUTED_COLOR, "size" => 11 })];
        for turn in turns {
            let when = turn.get("at").filter(|at| at.is_truthy()).and_then(Json::number).map_or_else(
                || "更早开始".to_string(),
                |at| dashboard::format(at, &jiff::tz::TimeZone::system(), "%H:%M"),
            );
            let requests = py::num(turn.get("requests"));
            let unpriced = py::num(turn.get("unpriced"));
            let cost = if requests > unpriced {
                format!("{}{symbol}{:.2}", if unpriced != 0.0 { "已知 " } else { "" }, py::num(turn.get("cost")) * rate)
            } else {
                "费用待定价".to_string()
            };
            let mut named: Vec<(String, f64)> = Vec::new();
            for (name, count) in turn.get("tools").and_then(Json::as_object).into_iter().flat_map(|tools| tools.iter())
            {
                let short = tool_name(name);
                match named.iter_mut().find(|(existing, _)| *existing == short) {
                    Some((_, total)) => *total += py::num(Some(count)),
                    None => named.push((short, py::num(Some(count)))),
                }
            }
            named.sort_by(|(a_name, a), (b_name, b)| {
                b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal).then(a_name.cmp(b_name))
            });
            let tools: Vec<String> =
                named.iter().take(4).map(|(name, count)| format!("{name} ×{}", *count as i64)).collect();
            let mut detail = format!("{} 次请求", requests as i64);
            detail += &if tools.is_empty() {
                " · 未调用工具".to_string()
            } else {
                format!(" · {}", tools.join(" · "))
            };
            if unpriced != 0.0 {
                detail += &format!(" · {} 次未定价", unpriced as i64);
            }
            lines.push(item(
                &format!("{when} · {} Token · {cost}", fmt_tokens(turn.get("tokens"))),
                2,
                object! { "paneldetail" => detail },
            ));
        }
        lines
    }
}

/// A version written x.y.z (with an optional v), or None.
pub fn version_tuple(value: Option<&Json>) -> Option<Vec<i64>> {
    let text = py::str(Some(&py::or(value, Json::from(""))));
    let text = text.trim();
    let text = text.strip_prefix('v').unwrap_or(text);
    let parts: Vec<&str> = text.split('.').collect();
    if parts.len() != 3 || !parts.iter().all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return None;
    }
    Some(parts.iter().map(|part| part.parse().unwrap_or(0)).collect())
}

pub fn update_attention_count(rows: &[Object]) -> usize {
    rows.iter()
        .filter(|row| {
            let field = if row.get("automatic").is_some_and(Json::is_truthy) { "needsAttention" } else { "available" };
            row.get(field).is_some_and(Json::is_truthy)
        })
        .count()
}

fn update_indicator(row: &Object) -> &'static str {
    let status = py::str(Some(&row.get("status").cloned().unwrap_or_else(|| Json::from(""))));
    if row.get("needsAttention").is_some_and(Json::is_truthy) || status.contains("失败") {
        "issue"
    } else if row.get("available").is_some_and(Json::is_truthy) {
        "new"
    } else if status.ends_with("已是最新") {
        "ok"
    } else {
        "unknown"
    }
}

/// A session by the title its client gave it, by its project folder when it has none, else as an untitled chat (the
/// card already wears its tool's logo).
pub fn session_name(session: &Object) -> String {
    let truthy = |key: &str| session.get(key).filter(|value| value.is_truthy()).map(|value| py::str(Some(value)));
    truthy("title").or_else(|| truthy("project")).unwrap_or_else(|| "未命名对话".into())
}

impl Runtime {
    // Komari

    fn komari_expiry(&self, row: &Object) -> (String, Option<i64>) {
        let end = py::str(Some(&py::or(row.get("expired_at"), Json::from(""))));
        let day =
            (end.chars().count() >= 10).then(|| dashboard::day_from_iso(&end.chars().take(10).collect::<String>()));
        let Some(Some(day)) = day else { return ("未设置".into(), None) };
        let year = py::civil_from_days(day).0;
        if year >= 2100 {
            return ("长期".into(), None);
        }
        if year < 1970 {
            return ("未设置".into(), None);
        }
        (dashboard::iso(day), Some(day - dashboard::day_number(now(), &TimeZone::system())))
    }

    pub fn komari_lines(&self, data: &Object) -> Vec<String> {
        let address = self.komari_base_url();
        if address.is_empty() {
            return vec![
                item("Komari 监控", 0, object! { "badge" => "未配置", "sfimage" => "server.rack" }),
                item("设置面板地址…", 1, self.act("configure-connection", &["komari"])),
            ];
        }
        let payload = object(data, "komari");
        let servers: Vec<Object> = payload
            .get("servers")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .filter_map(Json::as_object)
            .cloned()
            .collect();
        let error = failure(data, "komari");
        let fetched = payload.get("_fetchedAt").filter(|at| at.as_bool().is_none()).and_then(Json::number);
        let age = fetched.map_or(f64::INFINITY, |at| if at > 0.0 { (now() - at).max(0.0) } else { f64::INFINITY });
        let live = payload.get("statusAvailable").is_some_and(Json::is_truthy) && age <= 90.0 && error.is_none();
        let online = servers.iter().filter(|server| server.get("online") == Some(&Json::Bool(true))).count();
        let unknown = servers.iter().filter(|server| server.get("online").and_then(Json::as_bool).is_none()).count();
        let summary = if servers.is_empty() {
            if refresh_failed(error.as_deref()) {
                "连接失败".to_string()
            } else if payload.is_empty() {
                "待同步".into()
            } else {
                "暂无服务器".into()
            }
        } else if !live {
            "状态待更新".into()
        } else if unknown > 0 {
            format!("{online} 在线 · {unknown} 未知")
        } else {
            format!("{online}/{} 在线", servers.len())
        };
        let tooltip = format!("Komari · {summary} · {}", self.freshness(payload.get("_fetchedAt")));
        let mut lines = vec![item(
            "Komari 监控",
            0,
            object! { "badge" => summary, "sfimage" => "server.rack", "color" => Json::Null, "tooltip" => tooltip },
        )];
        let mut ordered: Vec<(usize, &Object)> = servers.iter().enumerate().collect();
        ordered.sort_by(|(a_index, a), (b_index, b)| {
            let weight = |row: &Object| -py::or(row.get("weight"), Json::Int(0)).python_float().unwrap_or(0.0);
            let name = |row: &Object| py::str(Some(&py::or(row.get("name"), Json::from(""))));
            weight(a)
                .partial_cmp(&weight(b))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| name(a).cmp(&name(b)))
                .then(a_index.cmp(b_index))
        });
        for (_, row) in ordered {
            let state = if live { row.get("online").and_then(Json::as_bool) } else { None };
            let (mark, label) = match state {
                Some(true) => ("●", "在线"),
                Some(false) => ("○", "离线"),
                None => ("?", "待更新"),
            };
            let name =
                shorten(&py::str(Some(&py::or(row.get("name"), row.get("uuid").cloned().unwrap_or(Json::Null)))), 18);
            let (expiry, days) = self.komari_expiry(row);
            let (mut cpu, mut ram) = ("—".to_string(), "—".to_string());
            if state == Some(true) {
                cpu = format!("{:.0}%", py::or(row.get("cpu"), Json::Int(0)).python_float().unwrap_or(0.0));
                let total = py::or(row.get("ram_total"), Json::Int(0)).python_float().unwrap_or(0.0);
                if total > 0.0 {
                    ram = format!(
                        "{:.0}%",
                        py::or(row.get("ram"), Json::Int(0)).python_float().unwrap_or(0.0) * 100.0 / total
                    );
                }
            }
            let detail = format!("{label} · CPU {cpu} · 内存 {ram} · 到期 {expiry}");
            let mut expiry_text = expiry.clone();
            if let Some(days) = days.filter(|days| *days <= 7) {
                expiry_text += &if days < 0 {
                    " 已到期".to_string()
                } else if days == 0 {
                    " 今天到期".to_string()
                } else {
                    format!(" · {days}天后")
                };
            }
            let color = match state {
                Some(true) => KOMARI_ONLINE,
                Some(false) => BAD_COLOR,
                None => MUTED_COLOR,
            };
            let url = format!("{address}/instance/{}", quote(&py::str(Some(&py::or(row.get("uuid"), Json::from(""))))));
            lines.push(item(
                &format!("{mark} {}  {label} · {expiry_text}", pad_display(&name, 18)),
                1,
                object! {
                    "color" => color, "font" => "Menlo", "size" => 12, "tooltip" => detail, "href" => url,
                    "panelstatus" => match state { Some(true) => "ok", Some(false) => "issue", None => "unknown" },
                },
            ));
        }
        lines.push("-----".into());
        lines.push(self.refresh_row("komari", &payload, error.as_deref(), 1));
        lines.push(item("打开 Komari 面板", 1, object! { "href" => address.as_str() }));
        lines
    }

    // Sessions

    /// 会话 as the panel shows it, its figures in the range chosen on this computer.
    pub fn sessions_lines(&self, data: &Object, unit: &Json) -> Vec<String> {
        self.sessions_lines_in(data, unit, &self.selected_range())
    }

    /// 会话 with its figures and note in `selected`: a phone reads the range it chose, the computer keeping its own.
    pub fn sessions_lines_in(&self, data: &Object, unit: &Json, selected: &str) -> Vec<String> {
        let selected = selected.to_string();
        // The sessions active in the range shown: a request in it, or a change in it (one that hasn't asked yet). 全部
        // lists every session read, the last month's.
        let (start, end) = self.range_bounds(&selected, now(), &self.local_zone());
        let active_then = |session: &Object| {
            let asked = session
                .get("spent")
                .and_then(|spent| spent.get("ranges"))
                .and_then(|ranges| ranges.get(&selected))
                .and_then(|figure| figure.get("requests"))
                .and_then(Json::integer)
                .unwrap_or(0);
            let at = session.get("at").and_then(Json::number).unwrap_or(0.0);
            selected == "all" || asked > 0 || (start <= at && at <= end)
        };
        let found: Option<Vec<Object>> =
            object(data, "sessions").get("sessions").and_then(Json::as_array).map(|rows| {
                rows.iter().filter_map(Json::as_object).filter(|session| active_then(session)).cloned().collect()
            });
        let label =
            RANGES.iter().find(|(key, _)| *key == selected && *key != "all").map_or("近 30 天", |(_, label)| *label);
        let error = failure(data, "sessions");
        let badge = match &found {
            Some(found) => {
                let running = found.iter().filter(|s| s.get("state") == Some(&Json::from("running"))).count();
                let idle = found.len() - running;
                let parts: Vec<String> = [
                    if running > 0 { format!("{running} 运行中") } else { String::new() },
                    if idle > 0 { format!("{idle} 空闲") } else { String::new() },
                ]
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect();
                if parts.is_empty() { "没有会话".to_string() } else { parts.join(" · ") }
            }
            None if refresh_failed(error.as_deref()) => "读取失败".into(),
            None => "待同步".into(),
        };
        let mut lines = vec![item("会话", 0, object! { "badge" => badge, "sfimage" => "terminal" })];
        let symbol = unit.get("symbol").and_then(Json::as_str).unwrap_or("$");
        let rate = unit.get("rate").and_then(Json::number).unwrap_or(1.0);
        // A cost as the cards write it: 已知 when some requests are unpriced, 费用待定价 when all are.
        let priced = |figure: Option<&Object>| {
            let field = |key: &str| py::num(figure.and_then(|figure| figure.get(key)));
            let (requests, unpriced) = (field("requests"), field("unpriced"));
            if requests == 0.0 {
                format!("{symbol}{:.2}", 0.0)
            } else if requests > unpriced {
                format!("{}{symbol}{:.2}", if unpriced != 0.0 { "已知 " } else { "" }, field("cost") * rate)
            } else {
                "费用待定价".to_string()
            }
        };
        // Sessions of one name (untitled ones go by their project folder, and a program's runs have no title) are
        // told apart by when each began: the time today, the date as well before.
        let names: Vec<String> = found.iter().flatten().map(session_name).collect();
        let zone = TimeZone::system();
        let today = dashboard::format(now(), &zone, "%Y-%m-%d");
        for (session, name) in found.iter().flatten().zip(&names) {
            let began = session.get("started").and_then(Json::number);
            let name = match began.filter(|_| names.iter().filter(|other| *other == name).count() > 1) {
                Some(at) if dashboard::format(at, &zone, "%Y-%m-%d") == today => {
                    format!("{name} · {}", dashboard::format(at, &zone, "%H:%M"))
                }
                Some(at) => format!("{name} · {}", dashboard::format(at, &zone, "%m-%d %H:%M")),
                None => name.clone(),
            };
            let ago = (now() - session.get("at").and_then(Json::number).unwrap_or(0.0)).max(0.0) as i64;
            let model = session
                .get("model")
                .filter(|model| model.is_truthy())
                .map(|model| dashboard::model_title(&dashboard::model_key(Some(model))))
                .map(|title| title.strip_prefix("Claude ").map(String::from).unwrap_or(title))
                .unwrap_or_default();
            let context = session
                .get("context")
                .filter(|context| !context.is_null())
                .and_then(Json::number)
                .map(|share| format!("上下文 {}%", (share * 100.0).round_ties_even() as i64))
                .unwrap_or_default();
            let effort = py::str(Some(&py::or(session.get("effort"), Json::from(""))));
            let running = session.get("state") == Some(&Json::from("running"));
            let turns: Vec<Object> = session
                .get("turns")
                .and_then(Json::as_array)
                .into_iter()
                .flatten()
                .filter_map(Json::as_object)
                .cloned()
                .collect();
            // A session is running or it is not: running for as long as this turn has, idle since it last changed.
            let latest = turns.iter().find(|turn| py::num(turn.get("requests")) != 0.0);
            // 12 分钟; 3 小时 from an hour on (a session stays listed a day).
            let lasted = |seconds: i64| {
                if seconds < 3600 { format!("{} 分钟", seconds / 60) } else { format!("{} 小时", seconds / 3600) }
            };
            let state = if running {
                let started = latest.and_then(|turn| turn.get("at")).and_then(Json::number);
                match started.map(|at| (now() - at).max(0.0) as i64) {
                    Some(seconds) if seconds >= 60 => format!("运行中 {}", lasted(seconds)),
                    _ => "运行中".to_string(),
                }
            } else if ago < 60 {
                "空闲".to_string()
            } else {
                format!("空闲 {}", lasted(ago))
            };
            // A session whose record keeps no requests (the Gemini app's) counts the user's messages instead.
            let rounds = session
                .get("rounds")
                .and_then(Json::integer)
                .filter(|rounds| *rounds > 0)
                .map(|rounds| format!("{rounds} 轮"))
                .unwrap_or_default();
            let detail: Vec<&str> =
                [state.as_str(), model.as_str(), effort.as_str(), context.as_str(), rounds.as_str()]
                    .into_iter()
                    .filter(|p| !p.is_empty())
                    .collect();
            // The live figure: what the session cost in the range the panel shows, as the other tabs count, rising as
            // it works; 全部 (or a reading from before ranges were kept) the whole session. This turn's tokens until
            // the cost is read. The whole session's cost heads its turns.
            let spent = session.get("spent").and_then(Json::as_object);
            let spent_requests = py::num(spent.and_then(|spent| spent.get("requests")));
            let ranges = spent.and_then(|spent| spent.get("ranges")).and_then(Json::as_object);
            let total = (spent_requests > py::num(spent.and_then(|spent| spent.get("unpriced"))))
                .then(|| format!("累计 {}", priced(spent)));
            let badge = if spent_requests != 0.0 {
                match ranges.filter(|_| selected != "all") {
                    Some(ranges) => priced(ranges.get(&selected).and_then(Json::as_object)),
                    None => priced(spent),
                }
            } else {
                latest.map_or_else(
                    || if running { "运行中".to_string() } else { "空闲".to_string() },
                    |turn| format!("{} Token", fmt_tokens(turn.get("tokens"))),
                )
            };
            let tool = session.get("tool").and_then(Json::as_str).unwrap_or("");
            let options = merged(&[
                &object! {
                    "badge" => badge, "paneldetail" => detail.join(" · "),
                    "panelstatus" => if running { "new" } else { "unknown" },
                    // Two sessions can share a name: the panel tells them apart by this.
                    "panelkey" => session.get("id").cloned().unwrap_or(Json::Null),
                },
                &self.session_icon(tool, session.get("module").and_then(Json::as_str)),
            ]);
            lines.push(item(&name, 1, options));
            let project = if session.get("title").is_some_and(Json::is_truthy) {
                session.get("project").filter(|project| project.is_truthy()).map(|project| py::str(Some(project)))
            } else {
                None
            };
            lines.extend(self.turn_lines(&turns, unit, project.as_deref(), total.as_deref()));
        }
        match found.as_ref() {
            Some(found) if found.is_empty() => {
                lines.push(item(&format!("{label}没有会话"), 1, object! { "color" => MUTED_COLOR, "size" => 11 }))
            }
            // Which sessions are listed and what their figures count.
            Some(_) => {
                let counted = if selected == "all" {
                    "金额为会话累计".to_string()
                } else {
                    format!("金额只算{label}")
                };
                lines.push(item(
                    &format!("{label}活动过的会话 · {counted}"),
                    1,
                    object! { "color" => MUTED_COLOR, "size" => 11 },
                ));
            }
            None => {}
        }
        lines.push("-----".into());
        let on = self.settings().get("sessionAlerts").is_some_and(Json::is_truthy);
        lines.push(item(
            "这一轮完成时提醒",
            1,
            merged(&[
                &object! { "sfimage" => "bell", "checked" => on, "paneldetail" => "运行中的会话这一轮完成时发通知" },
                &self.act("set-session-alerts", &[if on { "false" } else { "true" }]),
            ]),
        ));
        lines
    }

    // Updates

    /// Whether OpenCodex updates itself here: a desktop host only for an OpenCodex on this machine.
    pub fn automatic_opencodex(&self) -> bool {
        self.desktop().is_none_or(|desktop| desktop.local_proxy(&self.base))
    }

    pub fn update_rows(&self, data: &Object) -> Vec<Object> {
        let payload = data
            .get("app_updates")
            .and_then(Json::as_object)
            .cloned()
            .or_else(|| self.read_cache("app_updates").get("payload").and_then(Json::as_object).cloned())
            .unwrap_or_default();
        let selected = self.selected_update_ids();
        let mut rows: Vec<Object> = payload
            .get("apps")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .filter_map(Json::as_object)
            .filter(|row| row.get("id").and_then(Json::as_str).is_some_and(|id| selected.iter().any(|s| s == id)))
            .cloned()
            .collect();
        let badge_record = self.read_cache("badge");
        let badge = data
            .get("badge")
            .and_then(Json::as_object)
            .cloned()
            .or_else(|| badge_record.get("payload").and_then(Json::as_object).cloned())
            .unwrap_or_default();
        let current =
            py::or(object(data, "healthz").get("version"), badge.get("currentVersion").cloned().unwrap_or(Json::Null));
        let mut oc = object! {
            "id" => "opencodex", "name" => "OpenCodex", "href" => updates::update_link("opencodex"), "current" => current.clone(),
            "latest" => badge.get("latestVersion").cloned().unwrap_or(Json::Null),
            "checkedAt" => badge_record.get("fetchedAt").cloned().unwrap_or(Json::Int(0)),
        };
        let available = badge.get("updateAvailable").is_some_and(Json::is_truthy)
            && updates::newer(oc.get("latest"), Some(&current));
        oc.insert("available", available);
        let status = if available {
            "有新版本"
        } else if current.is_truthy() && oc.get("latest").is_some_and(Json::is_truthy) {
            "已是最新"
        } else {
            "等待检查"
        };
        oc.insert("status", status);
        let state = self.auto_update_state();
        let automatic = self.automatic_opencodex();
        oc.insert("automatic", automatic);
        if !automatic {
        } else if state.get("status") == Some(&Json::from("running"))
            || (available && state.get("target") == oc.get("latest"))
        {
            oc.insert("status", py::or(state.get("message"), Json::from("等待自动更新")));
            let attention = state.get("status") == Some(&Json::from("failed"))
                || state.get("status") == Some(&Json::from("blocked"));
            oc.insert("needsAttention", attention);
        } else if available {
            oc.insert("status", "等待自动更新");
        } else {
            oc.insert("status", format!("自动更新已开启 · {status}"));
        }
        // Re-read the local app versions so an update clears its alert at once.
        for row in rows.iter_mut() {
            let Some(id) = row.get("id").and_then(Json::as_str).map(String::from) else { continue };
            let Some(product) = update_products::product(&id)
                .filter(|product| product.kind == Kind::App && updates::manual_reason(&id).is_none())
            else {
                continue;
            };
            let info = self.product_app_info(product);
            if info.is_empty() {
                continue;
            }
            row.insert("current", info.get("CFBundleShortVersionString").cloned().unwrap_or(Json::Null));
            let installed = (id == "antigravity-ide")
                .then(|| self.product_app(product).and_then(|app| update_products::app_commit(&app)))
                .flatten();
            if let (Some(latest), Some(installed)) = (row.get("latestCommit").and_then(Json::as_str), installed) {
                let available = latest != installed;
                row.insert("available", available);
                let status = py::str(Some(&row.get("status").cloned().unwrap_or_else(|| Json::from(""))));
                if !status.contains("失败") {
                    row.insert("status", if available { "有新版本" } else { "已是最新" });
                }
            } else if row.get("latest").is_some_and(Json::is_truthy) {
                row.insert("available", update_products::available(&id, row.get("latest"), row.get("current")));
                if row.get("build").is_some_and(Json::is_truthy) {
                    row.insert("available", updates::newer(row.get("build"), info.get("CFBundleVersion")));
                }
                let status = py::str(Some(&row.get("status").cloned().unwrap_or_else(|| Json::from(""))));
                if !status.contains("失败") {
                    let text =
                        if row.get("available") == Some(&Json::Bool(true)) { "有新版本" } else { "已是最新" };
                    row.insert("status", text);
                }
            }
            if id == "chatgpt" {
                *row = self.apply_chatgpt_release(row.clone(), &info);
            }
        }
        let existing: Vec<String> =
            rows.iter().filter_map(|row| row.get("id").and_then(Json::as_str).map(String::from)).collect();
        for key in &selected {
            if key != "opencodex" && !existing.contains(key) {
                rows.push(object! {
                    "id" => key.as_str(), "name" => updates::update_name(key), "href" => updates::update_link(key),
                    "status" => "等待检查", "available" => false,
                });
            }
        }
        for row in rows.iter_mut() {
            let id = row.get("id").and_then(Json::as_str).unwrap_or("").to_string();
            let product = update_products::product(&id);
            let app = self.update_app_path(&id);
            // An app that updates itself is opened by its row; Magpie's has no updater, so its row opens the release.
            if let Some(app) = &app
                && product.is_some_and(|product| product.updates_itself)
            {
                row.insert("href", self.file_href(app));
                row.insert("app", true);
            }
            if let Some(note) = updates::manual_reason(&id) {
                // No version is published for it: the row says why, beside the version installed.
                let info = product.map(|product| self.product_app_info(product)).unwrap_or_default();
                if let Some(version) = info.get("CFBundleShortVersionString").filter(|version| version.is_truthy()) {
                    row.insert("current", version.clone());
                }
                row.insert("status", if app.is_some() { "在 App 内检查更新" } else { "在官网查看更新" });
                row.insert("available", false);
                row.insert("manual", true);
                row.insert("note", note);
            }
        }
        let mut by_id: std::collections::HashMap<String, Object> = std::collections::HashMap::new();
        by_id.insert("opencodex".into(), oc);
        for row in rows {
            if let Some(id) = row.get("id").and_then(Json::as_str) {
                by_id.insert(id.to_string(), row.clone());
            }
        }
        selected.iter().filter_map(|key| by_id.get(key).cloned()).collect()
    }

    pub fn app_update_lines(&self, data: &Object) -> Vec<String> {
        let mut rows: Vec<(usize, Object)> = self.update_rows(data).into_iter().enumerate().collect();
        // By their light: a newer version first, then up to date, then a check that went wrong, the unchecked last
        // (用户：蓝色的最上面，绿色的在蓝色的下面，空的洞洞在后面); in the order selected within each.
        let rank = |row: &Object| match update_indicator(row) {
            "new" => 0,
            "ok" => 1,
            "issue" => 2,
            _ => 3,
        };
        rows.sort_by_key(|(index, row)| (rank(row), *index));
        let rows: Vec<Object> = rows.into_iter().map(|(_, row)| row).collect();
        // Starmeter's own new version, first among the tools (用户选的方案 A: the mark beside Starmeter always opens 工具更新,
        // which shows it and the tools together).
        let own = self.self_update_in_tools(data);
        let count = update_attention_count(&rows)
            + usize::from(own.first().is_some_and(|line| line.contains("panelattention=true")));
        let running = rows.iter().any(|row| row.get("id") == Some(&Json::from("opencodex")))
            && self.auto_update_state().get("status") == Some(&Json::from("running"));
        let status = if count > 0 {
            Json::from(format!("{count} 项待更新"))
        } else if running {
            Json::from("自动更新中")
        } else {
            Json::Null
        };
        let mut lines = vec![item(
            "工具更新",
            0,
            object! { "badge" => status, "panelattention" => count > 0, "sfimage" => "arrow.down.circle" },
        )];
        lines.extend(own);
        if rows.is_empty() {
            lines.push(item("尚未勾选工具", 1, object! { "color" => MUTED_COLOR, "size" => 11 }));
        }
        for row in &rows {
            let id = py::str(row.get("id"));
            let status = py::str(Some(&row.get("status").cloned().unwrap_or_else(|| Json::from(""))));
            let label = format!("{} · {}", py::str(row.get("name")), status.replace("自动更新已开启 · ", ""));
            let mut detail = if row.get("manual").is_some_and(Json::is_truthy) {
                let note = py::str(Some(&py::or(row.get("note"), Json::from("没有公开的版本接口"))));
                match row.get("current").filter(|current| current.is_truthy()) {
                    Some(current) => format!("{} · {note}", py::str(Some(current))),
                    None => note,
                }
            } else if status == "未安装" {
                "点击打开官网".to_string()
            } else {
                py::str(Some(&py::or(row.get("current"), Json::from("版本待确认"))))
            };
            let available = row.get("available").is_some_and(Json::is_truthy);
            if available {
                detail += &format!(" → {}", py::str(row.get("latest")));
            }
            if let Some(channel) = row.get("channel").filter(|channel| channel.is_truthy()) {
                detail += &format!(" · {}", py::str(Some(channel)));
            }
            if let Some(reason) = row.get("reason").filter(|reason| reason.is_truthy()) {
                detail += &format!("\n{}", py::str(Some(reason)));
            }
            // Claude's new version is handed out in turns, by each computer's own ID: the feed asked with an
            // anonymous one may have it before this Claude does, whose own check still says it is up to date
            // (用户：claude提示有新版本，但是为什么app里显示没有更新？……给个提示，说分批推送).
            if available && id == "claude" {
                detail += "\n新版本由 Anthropic 分批推送，Claude 里可能过几天才提示更新";
            }
            let command = row.get("command").and_then(Json::as_str).filter(|command| !command.is_empty());
            // A row with an update says so at its right, a pill to click (更新), as the homepage's attention rows do.
            let pill = if available {
                object! { "badge" => "更新", "panelattention" => true }
            } else {
                Object::new()
            };
            if available && let Some(command) = command {
                // The CLI's update by its channel: brew upgrade, npm install -g or its own update.
                detail += &format!("\n点击运行 {command}");
                lines.push(item(
                    &label,
                    1,
                    merged(&[
                        &object! { "paneldetail" => detail, "panelstatus" => update_indicator(row) },
                        &pill,
                        &self.act("update-cli", &[&id]),
                    ]),
                ));
            } else {
                if available && row.get("app").is_some_and(Json::is_truthy) {
                    detail += "\n点击打开 App 更新";
                } else if available {
                    detail += "\n点击打开下载页";
                }
                lines.push(item(
                    &label,
                    1,
                    merged(&[
                        &object! {
                            "href" => row.get("href").cloned().unwrap_or(Json::Null), "paneldetail" => detail,
                            "panelstatus" => update_indicator(row),
                        },
                        &pill,
                    ]),
                ));
            }
        }
        lines.push("-----".into());
        if rows.iter().any(|row| !row.get("manual").is_some_and(Json::is_truthy)) {
            lines.push(item(
                "立即检查更新",
                1,
                merged(&[&object! { "sfimage" => "arrow.clockwise" }, &self.act("check-app-updates", &[])]),
            ));
        }
        let ids: Vec<String> =
            rows.iter().filter_map(|row| row.get("id").and_then(Json::as_str).map(String::from)).collect();
        // Every CLI 工具更新 can update keeps its log there, as the action that opens it reads them.
        if crate::host::terminal_jobs::latest_log(&self.state_dir, &ids).is_some() {
            lines.push(item(
                "查看最近 CLI 更新日志",
                1,
                merged(&[&object! { "sfimage" => "doc.text" }, &self.act("open-cli-update-log", &[])]),
            ));
        }
        lines.push(item(
            "选择工具…",
            1,
            merged(&[&object! { "sfimage" => "square.grid.2x2" }, &self.act("panel-modules", &[])]),
        ));
        lines
    }

    pub fn plugin_update(&self, data: &Object) -> Option<Object> {
        let release = object(data, "self_update");
        let latest = version_tuple(release.get("latest"))?;
        let current = version_tuple(Some(&Json::from(VERSION)))?;
        (current < latest).then_some(release)
    }

    pub fn self_update_state(&self) -> Object {
        let state = claude_subscription::read_state(&format!("{}/self-update.json", self.state_dir));
        let Some(moment) = state.get("at").filter(|at| at.as_bool().is_none()).and_then(Json::number) else {
            return Object::new();
        };
        // A run that never reported back within 15 minutes is treated as over.
        if state.get("status") == Some(&Json::from("running")) && now() - moment > 900.0 {
            return Object::new();
        }
        state
    }

    pub fn update_asset_name(&self, version: &str) -> String {
        if self.mac_app {
            let chip = if updates::machine() == "arm64" { "apple-silicon" } else { "intel" };
            format!("starmeter-{version}-macos-{chip}.dmg")
        } else if cfg!(target_os = "linux") && self.is_desktop() {
            // The Linux tray offers the release once it has a Linux package (the AppImage, beside the .deb).
            format!("starmeter-{version}-linux-x64.AppImage")
        } else {
            format!("starmeter-{version}{}.zip", if self.is_desktop() { "-windows-x64" } else { "" })
        }
    }

    /// Starmeter's own new version as a row of 工具更新, at its top: lit as the tools' are, from this version to the
    /// new one, a 更新 pill while it can be had; opened, its own page (the homepage row's: 更新到 X…, 查看更新内容) one
    /// level down. Nothing without a newer release.
    pub fn self_update_in_tools(&self, data: &Object) -> Vec<String> {
        let page = self.self_update_lines(data);
        let Some(release) = self.plugin_update(data).filter(|_| !page.is_empty()) else { return vec![] };
        let latest = py::str(release.get("latest"));
        let state = self.self_update_state();
        let mine = state.get("version") == Some(&Json::from(latest.as_str()));
        let running = state.get("status") == Some(&Json::from("running")) && mine;
        let failed = state.get("status") == Some(&Json::from("failed")) && mine;
        let status = if running {
            "正在更新"
        } else if failed {
            "更新失败"
        } else {
            "有新版本"
        };
        let mut options = object! {
            "paneldetail" => format!("{VERSION} → {latest}"), "panelstatus" => if failed { "issue" } else { "new" },
        };
        if !running && !failed {
            options.insert("badge", "更新");
            options.insert("panelattention", true);
        }
        let mut lines = vec![item(&format!("{PRODUCT} · {status}"), 1, options)];
        // Its page but the 当前 → 最新 line, which the row itself says.
        lines.extend(
            page.into_iter()
                .skip(1)
                .filter(|line| !line.trim_start_matches('-').starts_with("当前 "))
                .map(|line| format!("--{line}")),
        );
        lines
    }

    pub fn self_update_lines(&self, data: &Object) -> Vec<String> {
        let Some(release) = self.plugin_update(data) else { return vec![] };
        let latest = py::str(release.get("latest"));
        let state = self.self_update_state();
        let assets = release.get("assets").and_then(Json::as_array).cloned().unwrap_or_default();
        // The standalone app updates itself in place where it can (its bundle, an installed Windows program, an
        // AppImage); otherwise (a .deb, a portable folder) the row links to the release.
        let in_place = (self.mac_app || self.is_desktop()) && self.app_package(&latest, &assets).is_some();
        if (self.mac_app || self.is_desktop()) && !in_place {
            let asset = self.update_asset_name(&latest);
            let listed = assets.iter().any(|entry| entry.get("name") == Some(&Json::from(asset.as_str())));
            if !listed {
                return vec![];
            }
            return vec![
                item(
                    PRODUCT,
                    0,
                    merged(&[
                        &object! { "badge" => format!("可更新 {latest}"), "panelattention" => true },
                        &self.logo_mark(MENU_ICON_SIZE),
                    ]),
                ),
                item(
                    "打开 GitHub 发布页",
                    1,
                    object! {
                        "href" => format!("https://github.com/timecatbit/starmeter/releases/tag/v{latest}"),
                        "sfimage" => "arrow.up.right",
                    },
                ),
            ];
        }
        let mine = state.get("version") == Some(&Json::from(latest.as_str()));
        let running = state.get("status") == Some(&Json::from("running")) && mine;
        let failed = state.get("status") == Some(&Json::from("failed")) && mine;
        let badge = if running {
            "正在更新".to_string()
        } else if failed {
            "更新失败".into()
        } else {
            format!("可更新 {latest}")
        };
        let mut rows = vec![
            item(
                PRODUCT,
                0,
                merged(&[&object! { "badge" => badge, "panelattention" => !failed }, &self.logo_mark(MENU_ICON_SIZE)]),
            ),
            item(&format!("当前 {VERSION} → 最新 {latest}"), 1, object! { "color" => MUTED_COLOR, "size" => 11 }),
        ];
        if running {
            let note = if in_place {
                "正在下载新版本，校验后自动替换并重新打开"
            } else {
                "正在下载并安装，完成后自动刷新"
            };
            rows.push(item(note, 1, object! { "color" => MUTED_COLOR, "size" => 11 }));
        } else {
            if failed {
                rows.push(item(
                    &py::str(Some(&py::or(state.get("reason"), Json::from("更新失败")))),
                    1,
                    object! {
                        "paneldetail" => format!("日志：{}/logs/self-update.log", self.state_dir), "panelstatus" => "issue",
                    },
                ));
            }
            rows.push(item(
                &format!("{}{latest}…", if failed { "重试更新到 " } else { "更新到 " }),
                1,
                merged(&[&object! { "sfimage" => "arrow.down.circle" }, &self.act("update-self", &[])]),
            ));
        }
        rows.push(item(
            "查看更新内容",
            1,
            object! { "href" => release.get("page").cloned().unwrap_or(Json::Null), "sfimage" => "arrow.up.right" },
        ));
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 查看最近 CLI 更新日志 appears for every CLI 工具更新 can update, as the action that opens it finds their logs:
    /// a Gemini CLI update's log alone is enough.
    #[test]
    fn the_latest_cli_log_entry_is_there_for_any_cli_updated() {
        let home = std::env::temp_dir().join(format!("starmeter-cli-log-{}", crate::runtime::random_hex(6)));
        let state = home.join("state");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(state.join("preferences.json"), r#"{"schema":1,"visible":["gemini"]}"#).unwrap();
        let environment: std::collections::HashMap<String, String> = [
            ("HOME", home.to_string_lossy().into_owned()),
            ("OCX_STATE_DIR", state.to_string_lossy().into_owned()),
            ("OCX_CACHE_DIR", home.join("cache").to_string_lossy().into_owned()),
            ("PATH", home.join("bin").to_string_lossy().into_owned()),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect();
        let runtime = Runtime::new(environment, "starmeter");
        let data = object! {
            "app_updates" => object! { "apps" => Json::Array(vec![
                object! { "id" => "gemini", "name" => "Gemini CLI", "current" => "0.40.0", "latest" => "0.40.0" }.into(),
            ]) },
        };
        let entry = |lines: &[String]| lines.iter().any(|line| line.contains("查看最近 CLI 更新日志"));
        assert!(!entry(&runtime.app_update_lines(&data)), "no log yet");
        let logs = state.join("logs/updates");
        std::fs::create_dir_all(&logs).unwrap();
        std::fs::write(logs.join("gemini-20261123-101500-abc.log"), "updated\n").unwrap();
        assert!(entry(&runtime.app_update_lines(&data)), "Gemini CLI's log");
        let _ = std::fs::remove_dir_all(&home);
    }

    /// Claude's new version goes out in turns, by each computer's own ID: its row says so, as this Claude may still say
    /// it is up to date (用户：claude提示有新版本，但是为什么app里显示没有更新？); no other row says it, nor Claude's when
    /// there is nothing new.
    #[test]
    fn claudes_new_version_says_it_is_handed_out_in_turns() {
        let home = std::env::temp_dir().join(format!("starmeter-claude-turns-{}", crate::runtime::random_hex(6)));
        let state = home.join("state");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(state.join("preferences.json"), r#"{"schema":1,"visible":["claude","magpie"]}"#).unwrap();
        let environment: std::collections::HashMap<String, String> = [
            ("HOME", home.to_string_lossy().into_owned()),
            ("OCX_STATE_DIR", state.to_string_lossy().into_owned()),
            ("OCX_CACHE_DIR", home.join("cache").to_string_lossy().into_owned()),
            ("PATH", home.join("bin").to_string_lossy().into_owned()),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect();
        let runtime = Runtime::new(environment, "starmeter");
        let note = "新版本由 Anthropic 分批推送，Claude 里可能过几天才提示更新";
        let lines = |claude: &str| {
            let data = object! {
                "app_updates" => object! { "apps" => Json::Array(vec![
                    object! { "id" => "claude", "name" => "Claude", "status" => if claude == "7.37095.0" { "有新版本" } else { "已是最新" },
                        "current" => "6.28899.6", "latest" => claude, "available" => claude == "7.37095.0" }.into(),
                    object! { "id" => "magpie", "name" => "Magpie", "status" => "有新版本", "current" => "0.1.4706",
                        "latest" => "0.1.4710", "available" => true }.into(),
                ]) },
            };
            runtime.app_update_lines(&data)
        };
        let said = |lines: &[String], name: &str| {
            lines.iter().filter(|line| line.contains(&format!("{name} ·"))).any(|line| line.contains(note))
        };
        let newer = lines("7.37095.0");
        assert!(said(&newer, "Claude"), "Claude's row says it");
        assert!(!said(&newer, "Magpie"), "not another's");
        assert!(!said(&lines("6.28899.6"), "Claude"), "nothing new, nothing said");
        let _ = std::fs::remove_dir_all(&home);
    }
}
