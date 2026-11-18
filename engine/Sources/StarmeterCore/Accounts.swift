import Foundation

/// Read-only provider account resources: balances, Claude API spend, the MiniMax plan and key checks. Only
/// explicitly configured credentials are used; nothing raw is cached. Port of swiftbar_accounts.py.
public enum Accounts {
    /// A credential from the environment, or from the file its _FILE variable names.
    public static func credential(_ name: String, environment: [String: String] = ProcessInfo.processInfo.environment) -> String {
        let value = (environment[name] ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        if !value.isEmpty { return value }
        guard let file = environment[name + "_FILE"], !file.isEmpty,
              let data = FileManager.default.contents(atPath: Paths.expandUser(file)), let text = String(data: data, encoding: .utf8) else { return "" }
        return text.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    static let moonshotHosts = ["china": ("https://api.moonshot.cn", "CNY"), "international": ("https://api.moonshot.ai", "USD")]
    static let anthropic = "https://api.anthropic.com"

    static func anthropicHeaders(_ key: String) -> [String: String] { ["x-api-key": key, "anthropic-version": "2023-06-01"] }

    static func percent(_ value: JSON?) -> Double? {
        guard let value, value.bool == nil, let number = value.pythonFloat, number.isFinite, number >= 0, number <= 100 else { return nil }
        return number
    }

    /// isinstance(value, (int, float)), which a bool also passes.
    static func numeric(_ value: JSON?) -> Double? {
        guard let value else { return nil }
        if let flag = value.bool { return flag ? 1 : 0 }
        return value.number
    }

    static func xaiBalance(_ data: JSONObject) -> JSONObject {
        // The billing ledger is debit-positive: a $10 purchase has total -1000.
        guard let value = data["total"]?.object?["val"], let amount = Money(value) else { return ["state": "余额格式待检查"] }
        return ["balance": .double(Money(-amount.value / 100).double), "currency": "USD", "state": "已同步"]
    }

    /// Qoder CN Teams/Enterprise OpenAPI: the organization's shared add-on Credits still unspent.
    static func qoderPackages(_ fetch: (String) throws -> JSONObject, org: String) throws -> JSONObject {
        let base = "https://api.qoder.com.cn/v1/organizations/" + quote(org) + "/resource-packages?status=active&maxResults=100"
        var total = Decimal(0), count = 0
        var token: String?
        for _ in 0..<10 {
            let data = try fetch(base + (token.map { "&nextToken=" + quote($0) } ?? ""))
            guard let rows = data["resourcePackages"]?.array else { return ["state": "余额格式待检查"] }
            for row in rows {
                guard let fields = row.object, Py.str(fields["status"] ?? .string("active")).lowercased() == "active" else { continue }
                guard let value = Money(fields["remainingValue"]), value.value >= 0 else { return ["state": "余额格式待检查"] }
                total += value.value
                count += 1
            }
            token = data["nextToken"]?.string.flatMap { $0.isEmpty ? nil : $0 }
            if token == nil { break }
        }
        return ["balance": .double(Money(total).double), "currency": "Credits", "packages": .int(Int64(count)), "state": "已同步"]
    }

    /// available_balance already includes vouchers; a negative cash balance means arrears.
    static func moonshotBalance(_ data: JSONObject, currency: String) -> JSONObject {
        let payload = data["data"]?.object ?? JSONObject()
        if let code = data["code"], !(code == .int(0) || code.isNull) { return ["state": "凭据或地区待检查"] }
        if data["status"] == .bool(false) && data["status"]?.bool != nil { return ["state": "凭据或地区待检查"] }
        var amounts = Ordered<String, Money>()
        for key in ["available_balance", "voucher_balance", "cash_balance"] where payload[key] != nil {
            guard let amount = Money(payload[key]) else { return ["state": "余额格式待检查"] }
            amounts[key] = amount
        }
        guard let available = amounts["available_balance"] else { return ["state": "余额格式待检查"] }
        var result: JSONObject = ["balance": .double(available.double), "currency": .string(currency), "state": "已同步"]
        for (key, value) in amounts.items where key != "available_balance" {
            result[key == "voucher_balance" ? "voucher" : "cash"] = .double(value.double)
        }
        return result
    }

    static func errorMessage(_ body: Data) -> String {
        guard let object = (try? JSON.parse(body.prefix(4096)))?.object else { return "" }
        return Py.str(Py.or(Py.or(object["error"], .object(JSONObject())).object?["message"], .string("")))
    }

    /// Admin cost report: month-to-date and today's (UTC) spend, never a balance. Amounts are decimal cents.
    static func anthropicCost(_ key: String, now: Double = Date().timeIntervalSince1970) throws -> JSONObject {
        let today = Int((now / 86400).rounded(.down))
        let (y, m, _) = civilFromDays(today)
        let month = daysFromCivil(y, m, 1)
        func stamp(_ day: Int) -> String { Runtime.iso(day) + "T00:00:00Z" }
        var total = Decimal(0), current = Decimal(0)
        var page: String?
        for _ in 0..<4 {
            var query = [("starting_at", stamp(month)), ("ending_at", stamp(today + 1)), ("bucket_width", "1d"), ("limit", "31")]
            if let page { query.append(("page", page)) }
            let url = anthropic + "/v1/organizations/cost_report?" + query.map { $0.0 + "=" + formQuote($0.1) }.joined(separator: "&")
            let response = try HTTP.send(url, headers: anthropicHeaders(key).merging(["Accept": "application/json"]) { $1 }, timeout: 12, followRedirects: false)
            guard (200..<300).contains(response.status) else { throw HTTP.Status(code: response.status, body: response.body) }
            guard let data = (try? JSON.parse(response.body.prefix(2 * 1024 * 1024)))?.object, let buckets = data["data"]?.array else {
                throw Py.FileError("invalid cost report")
            }
            for bucket in buckets {
                for row in (bucket.object?["results"]).flatMap({ $0.isTruthy ? $0.array : nil }) ?? [] {
                    guard let fields = row.object else { throw Py.FileError("bad row") }
                    guard Py.str(Py.or(fields["currency"], .string("USD"))) == "USD" else { throw Py.FileError("unexpected currency") }
                    guard let amount = Money(fields["amount"]) else { throw Py.FileError("invalid amount") }
                    total += amount.value / 100
                    if Py.str(bucket.object?["starting_at"] ?? .string("")).hasPrefix(Runtime.iso(today)) { current += amount.value / 100 }
                }
            }
            page = data["next_page"].flatMap { $0.isTruthy ? Py.str($0) : nil }
            if data["has_more"]?.isTruthy != true || page == nil { break }
        }
        return ["monthCost": .double(Money(total).double), "todayCost": .double(Money(current).double), "currency": "USD",
                "month": .string(String(Runtime.iso(month).prefix(7))), "state": "已同步"]
    }

    static func minimaxQuota(_ data: JSONObject) -> JSONObject {
        let base = Py.dict(data["base_resp"]).object ?? JSONObject()
        guard base["status_code"] == .int(0) else { return ["state": "套餐凭据或地区待检查", "windows": []] }
        var windows: [JSON] = []
        for row in Py.or(data["model_remains"], .array([])).array ?? [] {
            guard let fields = row.object else { continue }
            let model = Py.str(Py.or(fields["model_name"], .string("套餐")))
            // Not included in this plan, not 100% available.
            if ["interval", "weekly"].allSatisfy({ fields["current_\($0)_total_count"] == .int(0) && fields["current_\($0)_status"] == .int(3) }) { continue }
            for (span, label, startKey, resetKey) in [("interval", "滚动额度", "start_time", "end_time"), ("weekly", "本周额度", "weekly_start_time", "weekly_end_time")] {
                let status = fields["current_\(span)_status"], total = fields["current_\(span)_total_count"]
                var remaining = percent(fields["current_\(span)_remaining_percent"]).map(JSON.double)
                if span == "weekly" && status == .int(3) { continue }  // unlimited, no finite percentage
                if remaining == nil, let all = numeric(total), all > 0, let count = numeric(fields["current_\(span)_usage_count"]), count >= 0, count <= all {
                    // Legacy API used usage_count for REMAINING; explicit percentages win when present.
                    remaining = .double(100 * count / all)
                }
                guard let remaining else { continue }
                func moment(_ value: JSON?) -> JSON? {
                    guard let value, let number = numeric(value) else { return nil }
                    return number > 1e11 ? .double(number / 1000) : number > 0 ? value : nil
                }
                let ends = moment(fields[resetKey]), starts = moment(fields[startKey])
                var window: JSONObject = ["name": .string(model + " · " + label), "remaining": remaining, "resetsAt": ends ?? .null]
                if let starts, let ends, starts.isTruthy, ends.isTruthy, (numeric(ends) ?? 0) > (numeric(starts) ?? 0) { window["startsAt"] = starts }
                windows.append(.object(window))
            }
        }
        return ["state": .string(windows.isEmpty ? "暂无可读取的套餐额度" : "已同步"), "windows": .array(windows)]
    }

    /// Official /user/balance; a negative total (arrears) is kept, never hidden.
    static func deepseekBalance(_ data: JSONObject) -> JSONObject {
        let rows = (data["balance_infos"]?.array ?? []).compactMap(\.object)
        let row = rows.first { $0["currency"] == .string("CNY") } ?? rows.first
        guard let row, let amount = Money(row["total_balance"]) else { return ["state": "余额格式待检查"] }
        return ["balance": .double(amount.double), "currency": .string(Py.str(Py.or(row["currency"], .string("CNY")))), "state": "已同步"]
    }

    /// One provider account. An explicit key (one of several named keys) comes with its own options; otherwise
    /// the classic single connection is read from the environment.
    public static func account(_ source: String, key: String? = nil, options: JSONObject = JSONObject(),
                               environment: [String: String] = ProcessInfo.processInfo.environment) -> JSONObject {
        if key == "" { return ["state": "密钥文件不可用"] }
        func env(_ name: String) -> String { (environment[name] ?? "").trimmingCharacters(in: .whitespacesAndNewlines) }
        do {
            switch source {
            case "deepseek" where key != nil:
                return deepseekBalance(try HTTP.getJSON("https://api.deepseek.com/user/balance", key: key!))
            case "grok":
                let secret = key ?? credential("XAI_MANAGEMENT_API_KEY", environment: environment)
                let team = Py.str(Py.or(options["team"], .string(environment["XAI_TEAM_ID"] ?? ""))).trimmingCharacters(in: .whitespacesAndNewlines)
                guard !secret.isEmpty, !team.isEmpty else { return ["state": "未配置 xAI 管理密钥与 Team ID"] }
                return xaiBalance(try HTTP.getJSON("https://management-api.x.ai/v1/billing/teams/" + quote(team) + "/prepaid/balance", key: secret))
            case "qoder-api":
                let secret = key ?? credential("QODER_CN_API_KEY", environment: environment)
                let org = Py.str(Py.or(options["org"], .string(environment["QODER_CN_ORG_ID"] ?? ""))).trimmingCharacters(in: .whitespacesAndNewlines)
                guard !secret.isEmpty, !org.isEmpty else { return ["state": "未配置 Qoder 组织 API Key 与组织 ID"] }
                return try qoderPackages({ try HTTP.getJSON($0, key: secret) }, org: org)
            case "minimax":
                let secret = credential("MINIMAX_API_KEY", environment: environment)
                guard !secret.isEmpty else { return ["state": "未配置 MiniMax 套餐密钥", "windows": []] }
                let region = environment["MINIMAX_REGION"] ?? "international"
                guard ["international", "china"].contains(region) else { return ["state": "MiniMax 地区配置待检查", "windows": []] }
                let host = region == "international" ? "https://www.minimax.io" : "https://api.minimaxi.com"
                return minimaxQuota(try HTTP.getJSON(host + "/v1/token_plan/remains", key: secret))
            case "kimi-api":
                let secret = key ?? credential("MOONSHOT_API_KEY", environment: environment)
                guard !secret.isEmpty else { return ["state": "未配置 Kimi API Key"] }
                let region = options["region"].flatMap { $0.isTruthy ? Py.str($0) : nil } ?? environment["MOONSHOT_REGION"] ?? "china"
                guard let (host, currency) = moonshotHosts[region] else { return ["state": "Kimi 地区配置待检查"] }
                return moonshotBalance(try HTTP.getJSON(host + "/v1/users/me/balance", key: secret), currency: currency)
            case "claude-api":
                let secret = key ?? credential("ANTHROPIC_API_KEY", environment: environment)
                guard !secret.isEmpty else { return ["state": "未配置 Claude API Key"] }
                do { return try anthropicCost(secret) } catch let status as HTTP.Status {
                    // Spend needs Admin API access: workspace keys get 401, accounts without the admin role 403.
                    if status.code == 403 { return ["state": "Key 所属账户没有查看花费的权限", "permission": false, "reason": "role"] }
                    if status.code == 401 && errorMessage(status.body).contains("Admin API") {
                        return ["state": "这个 Key 绑定了 Workspace，只能调用模型", "permission": false, "reason": "workspace"]
                    }
                    throw status
                }
            default:
                return ["state": "不支持的账户来源"]
            }
        } catch let status as HTTP.Status {
            if status.code == 401 || status.code == 403 { return ["state": "凭据或查询权限待检查"] }
            return ["error": .string(status.code == 429 ? "请求频繁，稍后重试" : "服务暂不可用")]
        } catch {
            return ["error": "暂时无法读取数据"]
        }
    }

    /// Validity only, for providers without a balance API; the response body is discarded.
    public static func checkKey(_ source: String, key: String) -> JSONObject {
        var url: String, headers: [String: String]
        switch source {
        case "chatgpt-api": (url, headers) = ("https://api.openai.com/v1/models", ["Authorization": "Bearer " + key])
        case "claude-api": (url, headers) = (anthropic + "/v1/models?limit=1", anthropicHeaders(key))
        case "gemini-api": (url, headers) = ("https://generativelanguage.googleapis.com/v1beta/models?pageSize=1", ["x-goog-api-key": key])
        default: return ["valid": .null, "state": "服务暂不可用"]
        }
        // Admin keys cannot call model endpoints; the organization lookup proves them instead.
        if source == "claude-api" && key.hasPrefix("sk-ant-admin") { url = anthropic + "/v1/organizations/me" }
        headers["Accept"] = "application/json"
        do {
            let response = try HTTP.send(url, headers: headers, timeout: 8, followRedirects: false, limit: 4096)
            if (200..<300).contains(response.status) { return ["valid": true, "state": .string(url.hasSuffix("/me") ? "管理员 Key 有效" : "Key 有效")] }
            // A personal or service key without a workspace is accepted but must name one for model calls.
            if source == "claude-api" && response.status == 400 && errorMessage(response.body).contains("not scoped to a workspace") {
                return ["valid": true, "state": "Key 有效 · 个人 Key"]
            }
            if [400, 401, 403].contains(response.status) { return ["valid": false, "state": "Key 无效或无权限"] }
            return ["valid": .null, "state": .string(response.status == 429 ? "暂时限流，稍后再试" : "服务暂不可用")]
        } catch {
            return ["valid": .null, "state": "无法连接服务"]
        }
    }

    /// urllib.parse.quote(value, safe=''): every byte but letters, digits and _.-~ escaped.
    static func quote(_ value: String) -> String {
        var allowed = CharacterSet(charactersIn: "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_.-~")
        allowed.remove(charactersIn: "")
        return value.addingPercentEncoding(withAllowedCharacters: allowed) ?? value
    }

    /// urlencode's quote_plus: spaces become +.
    static func formQuote(_ value: String) -> String {
        quote(value).replacingOccurrences(of: "%20", with: "+")
    }
}
