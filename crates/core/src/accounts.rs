//! Read-only provider account resources: balances, Claude API spend, the MiniMax plan and key checks. Only
//! explicitly configured credentials are used; nothing raw is cached.

use std::collections::HashMap;
use std::str::FromStr;

use indexmap::IndexMap;
use rust_decimal::Decimal;

use crate::dashboard::iso;
use crate::files::paths;
use crate::http::{self, JsonFailure, Request};
use crate::json::{Json, Object};
use crate::time::days_from_civil;
use crate::{object, py};

/// Exact decimal arithmetic for money, as the Python reference does it with Decimal before turning it into a float.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Money(pub Decimal);

impl Money {
    /// Decimal(str(value)): numbers by their Python text, numeric strings as written. None where Decimal raises.
    pub fn from_json(json: Option<&Json>) -> Option<Money> {
        let text = match json? {
            value @ (Json::Int(_) | Json::Double(_)) => py::str(Some(value)),
            Json::String(raw) => raw.trim_matches([' ', '\t']).to_string(),
            _ => return None,
        };
        if text.is_empty() || !text.chars().all(|c| c.is_ascii_digit() || "+-.eE".contains(c)) {
            return None;
        }
        let parsed = if text.contains(['e', 'E']) {
            Decimal::from_scientific(&text).ok()?
        } else {
            Decimal::from_str(&text).ok()?
        };
        Some(Money(parsed))
    }

    /// float(Decimal): the nearest double to the exact decimal.
    pub fn double(self) -> f64 {
        self.0.to_string().parse().unwrap_or(0.0)
    }
}

/// A credential from the environment, or from the file its _FILE variable names.
pub fn credential(name: &str, environment: &HashMap<String, String>) -> String {
    let value = environment.get(name).map(|value| value.trim()).unwrap_or("");
    if !value.is_empty() {
        return value.to_string();
    }
    let Some(file) = environment.get(&format!("{name}_FILE")).filter(|file| !file.is_empty()) else {
        return String::new();
    };
    std::fs::read_to_string(paths::expand_user(file)).map(|text| text.trim().to_string()).unwrap_or_default()
}

const ANTHROPIC: &str = "https://api.anthropic.com";

fn anthropic_headers(key: &str) -> Vec<(&'static str, String)> {
    vec![("x-api-key", key.to_string()), ("anthropic-version", "2023-06-01".to_string())]
}

fn percent(value: Option<&Json>) -> Option<f64> {
    value
        .filter(|v| v.as_bool().is_none())
        .and_then(Json::python_float)
        .filter(|n| n.is_finite() && (0.0..=100.0).contains(n))
}

/// isinstance(value, (int, float)), which a bool also passes.
pub fn numeric(value: Option<&Json>) -> Option<f64> {
    match value? {
        Json::Bool(flag) => Some(if *flag { 1.0 } else { 0.0 }),
        other => other.number(),
    }
}

fn state(text: &str) -> Object {
    object! { "state" => text }
}

fn xai_balance(data: &Object) -> Object {
    // The billing ledger is debit-positive: a $10 purchase has total -1000.
    let Some(amount) = Money::from_json(data.get("total").and_then(|total| total.get("val"))) else {
        return state("余额格式待检查");
    };
    object! { "balance" => Money(-amount.0 / Decimal::from(100)).double(), "currency" => "USD", "state" => "已同步" }
}

/// Qoder CN Teams/Enterprise OpenAPI: the organization's shared add-on Credits still unspent.
fn qoder_packages(fetch: &dyn Fn(&str) -> Result<Object, JsonFailure>, org: &str) -> Result<Object, JsonFailure> {
    let base = format!(
        "https://api.qoder.com.cn/v1/organizations/{}/resource-packages?status=active&maxResults=100",
        quote(org)
    );
    let mut total = Decimal::ZERO;
    let mut count = 0i64;
    let mut token: Option<String> = None;
    for _ in 0..10 {
        let url = format!(
            "{base}{}",
            token.as_ref().map_or_else(String::new, |token| format!("&nextToken={}", quote(token)))
        );
        let data = fetch(&url)?;
        let Some(rows) = data.get("resourcePackages").and_then(Json::as_array) else {
            return Ok(state("余额格式待检查"));
        };
        for row in rows {
            let Some(fields) = row.as_object() else { continue };
            if py::str(Some(fields.get("status").unwrap_or(&Json::from("active")))).to_lowercase() != "active" {
                continue;
            }
            let Some(value) = Money::from_json(fields.get("remainingValue")).filter(|value| value.0 >= Decimal::ZERO)
            else {
                return Ok(state("余额格式待检查"));
            };
            total += value.0;
            count += 1;
        }
        token = data.get("nextToken").and_then(Json::as_str).filter(|token| !token.is_empty()).map(String::from);
        if token.is_none() {
            break;
        }
    }
    Ok(
        object! { "balance" => Money(total).double(), "currency" => "Credits", "packages" => count, "state" => "已同步" },
    )
}

/// available_balance already includes vouchers; a negative cash balance means arrears.
fn moonshot_balance(data: &Object, currency: &str) -> Object {
    let payload = data.get("data").and_then(Json::as_object).cloned().unwrap_or_default();
    if let Some(code) = data.get("code")
        && !(code == &Json::Int(0) || code.is_null())
    {
        return state("凭据或地区待检查");
    }
    if data.get("status").and_then(Json::as_bool) == Some(false) {
        return state("凭据或地区待检查");
    }
    let mut amounts: IndexMap<&str, Money> = IndexMap::new();
    for key in ["available_balance", "voucher_balance", "cash_balance"] {
        if payload.contains_key(key) {
            let Some(amount) = Money::from_json(payload.get(key)) else { return state("余额格式待检查") };
            amounts.insert(key, amount);
        }
    }
    let Some(available) = amounts.get("available_balance") else { return state("余额格式待检查") };
    let mut result = object! { "balance" => available.double(), "currency" => currency, "state" => "已同步" };
    for (key, value) in &amounts {
        if *key != "available_balance" {
            result.insert(if *key == "voucher_balance" { "voucher" } else { "cash" }, value.double());
        }
    }
    result
}

fn error_message(body: &[u8]) -> String {
    let head = &body[..body.len().min(4096)];
    let Some(object) = Json::parse(head).ok().and_then(|value| value.as_object().cloned()) else {
        return String::new();
    };
    let error = py::or(object.get("error"), Json::Object(Object::new()));
    py::str(Some(&py::or(error.get("message"), Json::from(""))))
}

/// Admin cost report: month-to-date and today's (UTC) spend, never a balance. Amounts are decimal cents.
fn anthropic_cost(key: &str, now: f64) -> Result<Object, JsonFailure> {
    let today = (now / 86400.0).floor() as i64;
    let (year, month_number, _) = py::civil_from_days(today);
    let month = days_from_civil(year, month_number, 1);
    let stamp = |day: i64| format!("{}T00:00:00Z", iso(day));
    let (mut total, mut current) = (Decimal::ZERO, Decimal::ZERO);
    let hundred = Decimal::from(100);
    let invalid = |message: &str| JsonFailure::Invalid(py::FileError::new(message));
    let mut page: Option<String> = None;
    for _ in 0..4 {
        let mut query = vec![
            ("starting_at", stamp(month)),
            ("ending_at", stamp(today + 1)),
            ("bucket_width", "1d".to_string()),
            ("limit", "31".to_string()),
        ];
        if let Some(page) = &page {
            query.push(("page", page.clone()));
        }
        let encoded: Vec<String> = query.iter().map(|(key, value)| format!("{key}={}", form_quote(value))).collect();
        let url = format!("{ANTHROPIC}/v1/organizations/cost_report?{}", encoded.join("&"));
        let mut headers = anthropic_headers(key);
        headers.push(("Accept", "application/json".to_string()));
        let response =
            http::send(&url, Request { headers, timeout: 12.0, follow_redirects: false, ..Request::default() })
                .map_err(JsonFailure::Unreachable)?;
        if !(200..300).contains(&response.status) {
            return Err(JsonFailure::Status(http::Status { code: response.status, body: response.body }));
        }
        let head = &response.body[..response.body.len().min(2 * 1024 * 1024)];
        let data = Json::parse(head)
            .ok()
            .and_then(|value| value.as_object().cloned())
            .ok_or_else(|| invalid("invalid cost report"))?;
        let buckets = data.get("data").and_then(Json::as_array).ok_or_else(|| invalid("invalid cost report"))?;
        for bucket in buckets {
            let results = bucket
                .get("results")
                .filter(|results| results.is_truthy())
                .and_then(Json::as_array)
                .cloned()
                .unwrap_or_default();
            for row in &results {
                let fields = row.as_object().ok_or_else(|| invalid("bad row"))?;
                if py::str(Some(&py::or(fields.get("currency"), Json::from("USD")))) != "USD" {
                    return Err(invalid("unexpected currency"));
                }
                let amount = Money::from_json(fields.get("amount")).ok_or_else(|| invalid("invalid amount"))?;
                total += amount.0 / hundred;
                if py::str(Some(bucket.get("starting_at").unwrap_or(&Json::from("")))).starts_with(&iso(today)) {
                    current += amount.0 / hundred;
                }
            }
        }
        page = data.get("next_page").filter(|page| page.is_truthy()).map(|page| py::str(Some(page)));
        if !data.get("has_more").is_some_and(Json::is_truthy) || page.is_none() {
            break;
        }
    }
    Ok(object! {
        "monthCost" => Money(total).double(), "todayCost" => Money(current).double(), "currency" => "USD",
        "month" => iso(month)[..7].to_string(), "state" => "已同步",
    })
}

pub fn minimax_quota(data: &Object) -> Object {
    let base = py::dict(data.get("base_resp")).as_object().cloned().unwrap_or_default();
    if base.get("status_code") != Some(&Json::Int(0)) {
        return object! { "state" => "套餐凭据或地区待检查", "windows" => Json::Array(vec![]) };
    }
    let mut windows: Vec<Json> = Vec::new();
    for row in py::or(data.get("model_remains"), Json::Array(vec![])).as_array().into_iter().flatten() {
        let Some(fields) = row.as_object() else { continue };
        let model = py::str(Some(&py::or(fields.get("model_name"), Json::from("套餐"))));
        // Not included in this plan, not 100% available.
        let excluded = ["interval", "weekly"].iter().all(|span| {
            fields.get(&format!("current_{span}_total_count")) == Some(&Json::Int(0))
                && fields.get(&format!("current_{span}_status")) == Some(&Json::Int(3))
        });
        if excluded {
            continue;
        }
        for (span, label, start_key, reset_key) in [
            ("interval", "滚动额度", "start_time", "end_time"),
            ("weekly", "本周额度", "weekly_start_time", "weekly_end_time"),
        ] {
            let status = fields.get(&format!("current_{span}_status"));
            let total = fields.get(&format!("current_{span}_total_count"));
            let mut remaining = percent(fields.get(&format!("current_{span}_remaining_percent"))).map(Json::Double);
            if span == "weekly" && status == Some(&Json::Int(3)) {
                continue; // unlimited, no finite percentage
            }
            if remaining.is_none()
                && let Some(all) = numeric(total).filter(|all| *all > 0.0)
                && let Some(count) = numeric(fields.get(&format!("current_{span}_usage_count")))
                    .filter(|count| *count >= 0.0 && *count <= all)
            {
                // Legacy API used usage_count for REMAINING; explicit percentages win when present.
                remaining = Some(Json::Double(100.0 * count / all));
            }
            let Some(remaining) = remaining else { continue };
            let moment = |value: Option<&Json>| -> Option<Json> {
                let value = value?;
                let number = numeric(Some(value))?;
                if number > 1e11 {
                    Some(Json::Double(number / 1000.0))
                } else if number > 0.0 {
                    Some(value.clone())
                } else {
                    None
                }
            };
            let ends = moment(fields.get(reset_key));
            let starts = moment(fields.get(start_key));
            let mut window = object! { "name" => format!("{model} · {label}"), "remaining" => remaining, "resetsAt" => ends.clone() };
            if let (Some(starts), Some(ends)) = (starts, ends)
                && starts.is_truthy()
                && ends.is_truthy()
                && numeric(Some(&ends)).unwrap_or(0.0) > numeric(Some(&starts)).unwrap_or(0.0)
            {
                window.insert("startsAt", starts);
            }
            windows.push(window.into());
        }
    }
    let state_text = if windows.is_empty() { "暂无可读取的套餐额度" } else { "已同步" };
    object! { "state" => state_text, "windows" => Json::Array(windows) }
}

/// Official /user/balance; a negative total (arrears) is kept, never hidden.
fn deepseek_balance(data: &Object) -> Object {
    let rows: Vec<&Object> =
        data.get("balance_infos").and_then(Json::as_array).into_iter().flatten().filter_map(Json::as_object).collect();
    let row = rows.iter().find(|row| row.get("currency") == Some(&Json::from("CNY"))).or(rows.first());
    let Some(row) = row else { return state("余额格式待检查") };
    let Some(amount) = Money::from_json(row.get("total_balance")) else { return state("余额格式待检查") };
    object! {
        "balance" => amount.double(), "currency" => py::str(Some(&py::or(row.get("currency"), Json::from("CNY")))), "state" => "已同步",
    }
}

/// One provider account. An explicit key (one of several named keys) comes with its own options; otherwise the
/// classic single connection is read from the environment.
pub fn account(
    source: &str,
    key: Option<&str>,
    options: &Object,
    environment: &HashMap<String, String>,
    now: f64,
) -> Object {
    if key == Some("") {
        return state("密钥文件不可用");
    }
    let given = |name: &str| environment.get(name).cloned().unwrap_or_default();
    let result: Result<Object, JsonFailure> = (|| match source {
        "deepseek" if key.is_some() => {
            Ok(deepseek_balance(&http::get_json("https://api.deepseek.com/user/balance", key.unwrap_or(""))?))
        }
        "grok" => {
            let secret = key.map(String::from).unwrap_or_else(|| credential("XAI_MANAGEMENT_API_KEY", environment));
            let team = py::str(Some(&py::or(options.get("team"), Json::from(given("XAI_TEAM_ID"))))).trim().to_string();
            if secret.is_empty() || team.is_empty() {
                return Ok(state("未配置 xAI 管理密钥与 Team ID"));
            }
            let url = format!("https://management-api.x.ai/v1/billing/teams/{}/prepaid/balance", quote(&team));
            Ok(xai_balance(&http::get_json(&url, &secret)?))
        }
        "qoder-api" => {
            let secret = key.map(String::from).unwrap_or_else(|| credential("QODER_CN_API_KEY", environment));
            let org =
                py::str(Some(&py::or(options.get("org"), Json::from(given("QODER_CN_ORG_ID"))))).trim().to_string();
            if secret.is_empty() || org.is_empty() {
                return Ok(state("未配置 Qoder 组织 API Key 与组织 ID"));
            }
            qoder_packages(&|url| http::get_json(url, &secret), &org)
        }
        "minimax" => {
            let secret = credential("MINIMAX_API_KEY", environment);
            if secret.is_empty() {
                return Ok(object! { "state" => "未配置 MiniMax 套餐密钥", "windows" => Json::Array(vec![]) });
            }
            // A cleared setting is kept as empty: the default, as no setting is.
            let region = environment
                .get("MINIMAX_REGION")
                .map(|region| region.trim().to_string())
                .filter(|region| !region.is_empty())
                .unwrap_or_else(|| "international".into());
            if !["international", "china"].contains(&region.as_str()) {
                return Ok(object! { "state" => "MiniMax 地区配置待检查", "windows" => Json::Array(vec![]) });
            }
            let host = if region == "international" { "https://www.minimax.io" } else { "https://api.minimaxi.com" };
            Ok(minimax_quota(&http::get_json(&format!("{host}/v1/token_plan/remains"), &secret)?))
        }
        "kimi-api" => {
            let secret = key.map(String::from).unwrap_or_else(|| credential("MOONSHOT_API_KEY", environment));
            if secret.is_empty() {
                return Ok(state("未配置 Kimi API Key"));
            }
            let region = options
                .get("region")
                .filter(|region| region.is_truthy())
                .map(|region| py::str(Some(region)))
                .or_else(|| environment.get("MOONSHOT_REGION").map(|region| region.trim().to_string()))
                .filter(|region| !region.is_empty())
                .unwrap_or_else(|| "china".into());
            let (host, currency) = match region.as_str() {
                "china" => ("https://api.moonshot.cn", "CNY"),
                "international" => ("https://api.moonshot.ai", "USD"),
                _ => return Ok(state("Kimi 地区配置待检查")),
            };
            Ok(moonshot_balance(&http::get_json(&format!("{host}/v1/users/me/balance"), &secret)?, currency))
        }
        "claude-api" => {
            let secret = key.map(String::from).unwrap_or_else(|| credential("ANTHROPIC_API_KEY", environment));
            if secret.is_empty() {
                return Ok(state("未配置 Claude API Key"));
            }
            match anthropic_cost(&secret, now) {
                // Spend needs Admin API access: workspace keys get 401, accounts without the admin role 403.
                Err(JsonFailure::Status(status)) if status.code == 403 => Ok(
                    object! { "state" => "Key 所属账户没有查看花费的权限", "permission" => false, "reason" => "role" },
                ),
                Err(JsonFailure::Status(status))
                    if status.code == 401 && error_message(&status.body).contains("Admin API") =>
                {
                    Ok(
                        object! { "state" => "这个 Key 绑定了 Workspace，只能调用模型", "permission" => false, "reason" => "workspace" },
                    )
                }
                other => other,
            }
        }
        _ => Ok(state("不支持的账户来源")),
    })();
    match result {
        Ok(object) => object,
        Err(JsonFailure::Status(status)) if status.code == 401 || status.code == 403 => state("凭据或查询权限待检查"),
        Err(JsonFailure::Status(status)) => {
            object! { "error" => if status.code == 429 { "请求频繁，稍后重试" } else { "服务暂不可用" } }
        }
        Err(_) => object! { "error" => "暂时无法读取数据" },
    }
}

/// Validity only, for providers without a balance API; the response body is discarded.
pub fn check_key(source: &str, key: &str) -> Object {
    let (mut url, mut headers) = match source {
        "chatgpt-api" => {
            ("https://api.openai.com/v1/models".to_string(), vec![("Authorization", format!("Bearer {key}"))])
        }
        "claude-api" => (format!("{ANTHROPIC}/v1/models?limit=1"), anthropic_headers(key)),
        "gemini-api" => (
            "https://generativelanguage.googleapis.com/v1beta/models?pageSize=1".to_string(),
            vec![("x-goog-api-key", key.to_string())],
        ),
        _ => return object! { "valid" => Json::Null, "state" => "服务暂不可用" },
    };
    // Admin keys cannot call model endpoints; the organization lookup proves them instead.
    if source == "claude-api" && key.starts_with("sk-ant-admin") {
        url = format!("{ANTHROPIC}/v1/organizations/me");
    }
    headers.push(("Accept", "application/json".to_string()));
    let request = Request { headers, timeout: 8.0, follow_redirects: false, limit: 4096, ..Request::default() };
    let Ok(response) = http::send(&url, request) else {
        return object! { "valid" => Json::Null, "state" => "无法连接服务" };
    };
    if (200..300).contains(&response.status) {
        return object! { "valid" => true, "state" => if url.ends_with("/me") { "管理员 Key 有效" } else { "Key 有效" } };
    }
    // A personal or service key without a workspace is accepted but must name one for model calls.
    if source == "claude-api"
        && response.status == 400
        && error_message(&response.body).contains("not scoped to a workspace")
    {
        return object! { "valid" => true, "state" => "Key 有效 · 个人 Key" };
    }
    if [400, 401, 403].contains(&response.status) {
        return object! { "valid" => false, "state" => "Key 无效或无权限" };
    }
    object! { "valid" => Json::Null, "state" => if response.status == 429 { "暂时限流，稍后再试" } else { "服务暂不可用" } }
}

/// urllib.parse.quote(value, safe=''): every byte but letters, digits and _.-~ escaped.
pub fn quote(value: &str) -> String {
    crate::runtime::quote(value)
}

/// urlencode's quote_plus: spaces become +.
pub fn form_quote(value: &str) -> String {
    quote(value).replace("%20", "+")
}
