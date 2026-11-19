//! The rows of the other modules: active sessions, Komari, Harness updates and the rest of the homepage.

use crate::dashboard;
use crate::json::{Json, Object};
use crate::rows::{MUTED_COLOR, fmt_tokens, item};
use crate::runtime::Runtime;
use crate::{object, py};

/// An MCP tool by its own name: mcp__ccd_turn__update_plan is update_plan.
pub fn tool_name(name: &str) -> String {
    match name.strip_prefix("mcp__").and(name.rfind("__")) {
        Some(last) if !name[last + 2..].is_empty() => name[last + 2..].to_string(),
        _ => name.to_string(),
    }
}

impl Runtime {
    /// A session's turns as the menu lists them; a turn that sent no request (stopped before the model answered) is
    /// left out.
    pub fn turn_lines(&self, turns: &[Object], unit: &Json, project: Option<&str>) -> Vec<String> {
        let turns: Vec<&Object> = turns.iter().filter(|turn| py::num(turn.get("requests")) != 0.0).collect();
        if turns.is_empty() {
            return vec![];
        }
        let symbol = unit.get("symbol").and_then(Json::as_str).unwrap_or("$");
        let rate = unit.get("rate").and_then(Json::number).unwrap_or(1.0);
        let count = format!("最近 {} 轮", turns.len());
        let heading: Vec<&str> =
            [project, Some(count.as_str())].into_iter().flatten().filter(|part| !part.is_empty()).collect();
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
