import Foundation

extension Runtime {
    // MARK: API keys: the classic connection plus any number of named keys

    /// The classic connection (when configured) first, then the keys added in the panel.
    func apiKeys(_ provider: String) -> [JSONObject] {
        var rows: [JSONObject] = []
        if defaultReady(provider) {
            let envKey = Catalog.credentialNames(provider).first { !credential($0).isEmpty }
            let path = envKey.flatMap { key in env(key + "_FILE").flatMap { environment[key].map { $0.isEmpty } ?? true ? Runtime.absolute(Paths.expandUser($0)) : nil } }
            let source: JSON
            if let envKey, env(envKey) != nil { source = .string("环境变量 " + envKey) }
            else if provider == "deepseek" && envKey == nil { source = .string(ocxHome + "/config.json") }
            else { source = path.map(JSON.string) ?? .null }
            rows.append(["id": "default", "label": .string(Keys.defaultLabel(stateDir, provider)), "options": .object(JSONObject()),
                         "cache": .string(provider == "deepseek" ? "provider_quotas" : provider), "source": source])
        }
        for entry in namedKeys(provider) {
            var row = entry
            let id = Py.str(entry["id"] ?? .null)
            row["cache"] = .string(provider + "@" + id)
            row["source"] = .string((try? Keys.secretPath(stateDir, provider, id)) ?? "")
            rows.append(row)
        }
        return rows
    }

    func entrySecret(_ provider: String, _ id: String) -> String {
        if id != "default" { return Keys.secret(stateDir, provider, id) }
        if provider == "deepseek" && env("DEEPSEEK_API_KEY") == nil && env("DEEPSEEK_API_KEY_FILE") == nil { return opencodexDeepseekKey() }
        return Catalog.credentialNames(provider).lazy.map(credential).first { !$0.isEmpty } ?? ""
    }

    static func checkID(_ provider: String, _ id: String) -> String { id == "default" ? provider : provider + "@" + id }

    /// An irreversible mark of the configured key, so a check never outlives a key change.
    func keyFingerprint(_ provider: String, _ id: String = "default") -> String {
        let secret = entrySecret(provider, id)
        return secret.isEmpty ? "" : String(SHA256.hex(Array(secret.utf8)).prefix(16))
    }

    func keyCheck(_ provider: String, _ id: String = "default") -> JSONObject? {
        guard let row = readCache("key-checks")[Runtime.checkID(provider, id)]?.object, let key = row["key"], key.isTruthy, key == .string(keyFingerprint(provider, id)) else { return nil }
        return row
    }

    static let keyCheckProviders: Set<String> = ["chatgpt-api", "claude-api", "gemini-api"]

    func deepseekBalance(_ data: JSONObject) -> (String, Double?) {
        for report in (object(data, "provider_quotas")["reports"]?.array ?? []).compactMap(\.object) {
            guard Py.str(Py.or(report["provider"], .string(""))).lowercased() == "deepseek" else { continue }
            if let (symbol, amount) = Runtime.reportBalance(report) { return (symbol + String(format: "%.2f", amount), amount) }
            return ("未知", nil)
        }
        return ("", nil)
    }

    /// What one key shows: its balance, else its spend, else what a key check proved.
    func keyValue(_ provider: String, _ entry: JSONObject, _ data: JSONObject) -> JSONObject {
        let cache = Py.str(entry["cache"] ?? .null), payload = object(data, cache), failed = Runtime.refreshFailed(failure(data, cache))
        if provider == "deepseek" && entry["id"] == .string("default") {
            let (text, amount) = deepseekBalance(data)
            if let amount {
                let currency = text.hasPrefix("$") ? "USD" : "CNY"
                return ["text": .string("余额 " + Runtime.moneyText(amount, currency)), "amount": .double(amount), "currency": .string(currency), "kind": "balance",
                        "failed": .bool(failed), "at": payload["_fetchedAt"] ?? .null]
            }
        }
        if let balance = payload["balance"], balance.bool == nil, let amount = balance.number {
            let currency = Py.str(payload["currency"] ?? .string("USD"))
            return ["text": .string("余额 " + Runtime.moneyText(amount, currency)), "amount": balance, "currency": .string(currency), "kind": "balance",
                    "failed": .bool(failed), "at": payload["_fetchedAt"] ?? .null]
        }
        if let spend = payload["monthCost"], spend.bool == nil, let amount = spend.number {
            return ["text": .string("本月已用 " + Runtime.moneyText(amount, "USD")), "amount": spend, "currency": "USD", "kind": "spend",
                    "failed": .bool(failed), "at": payload["_fetchedAt"] ?? .null]
        }
        let check = Runtime.keyCheckProviders.contains(provider) ? keyCheck(provider, Py.str(entry["id"] ?? .null)) : nil
        if check?["valid"] == .bool(false) && check?["valid"]?.bool != nil { return ["text": "Key 无效"] }
        if payload["permission"] == .bool(false) && payload["permission"]?.bool != nil { return ["text": "无花费权限"] }
        if Runtime.keyTTLs.contains(where: { $0.0 == provider }) {
            let state = payload["state"]
            return ["text": .string(failed ? "刷新失败" : (state.map { !$0.isNull && $0 != .string("已同步") } == true ? Py.str(state!) : "待同步"))]
        }
        if Runtime.keyCheckProviders.contains(provider) {
            guard let check else { return ["text": "待测试"] }
            return ["text": .string(check["valid"]?.bool == true ? "Key 有效" : check["valid"]?.bool == false ? "Key 无效" : "测试未完成")]
        }
        return ["text": "已配置"]
    }

    /// Which account a key's figure belongs to: Qoder CN and Grok keys name their organization or team; the others
    /// count as their own key.
    func keyAccount(_ provider: String, _ entry: JSONObject) -> [String] {
        let (option, variable): (String?, String?) = ["qoder-api": ("org", "QODER_CN_ORG_ID"), "grok": ("team", "XAI_TEAM_ID")][provider].map { ($0.0, $0.1) } ?? (nil, nil)
        var owner = ""
        if let option { owner = Py.str(Py.or(Py.dict(entry["options"]).object?[option], .string(variable != nil && entry["id"] == .string("default") ? environment[variable!] ?? "" : ""))) }
        owner = owner.trimmingCharacters(in: .whitespacesAndNewlines)
        return owner.isEmpty ? ["key", Py.str(entry["id"] ?? .null)] : [option!, owner]
    }

    /// One figure per account: the freshest successful reading among that account's keys.
    static func countedValues(_ values: [JSONObject]) -> [JSONObject] {
        var groups = Ordered<String, [JSONObject]>()
        for (index, value) in values.enumerated() {
            let account = value["account"].map(Py.dumps) ?? "#\(index)"
            groups[account] = (groups[account] ?? []) + [value]
        }
        return groups.items.map { _, group in
            var best = group[0]
            for value in group.dropFirst() {
                let a = (value["failed"]?.isTruthy != true ? 1 : 0, value["at"]?.number ?? 0), b = (best["failed"]?.isTruthy != true ? 1 : 0, best["at"]?.number ?? 0)
                if a > b { best = value }
            }
            return best
        }
    }

    /// The homepage total: balances or spend per currency and account, then how many keys when several.
    static func apiBadge(_ values: [JSONObject]) -> String {
        if values.isEmpty { return "未配置" }
        for (kind, prefix) in [("balance", "余额 "), ("spend", "本月已用 ")] {
            let read = values.filter { $0["kind"] == .string(kind) }
            if read.isEmpty { continue }
            let rows = countedValues(read)
            var totals = Ordered<String, Double>()
            for value in rows { let currency = Py.str(value["currency"] ?? .null); totals[currency] = (totals[currency] ?? 0) + (value["amount"]?.number ?? 0) }
            let count = read.count > 1 ? " (\(read.count))" : ""
            return withFailure(prefix + totals.items.map { moneyText($0.1, $0.0) }.joined(separator: " · ") + count, true, rows.contains { $0["failed"]?.isTruthy == true })
        }
        let texts = Set(values.map { Py.str($0["text"] ?? .null) })
        return texts.count == 1 ? texts.first! : "\(values.count) 把 Key"
    }

    func keyLines(_ provider: String, _ entry: JSONObject, _ data: JSONObject, level: Int) -> [String] {
        let cache = Py.str(entry["cache"] ?? .null), payload = object(data, cache), error = failure(data, cache)
        var rows: [String] = []
        let currency = Py.str(payload["currency"] ?? .string("USD"))
        for (field, name) in [("cash", "现金"), ("voucher", "代金券")] {
            if let value = payload[field], value.bool == nil, let amount = value.number { rows.append(Runtime.item(name, level: level, ["badge": .string(Runtime.moneyText(amount, currency))])) }
        }
        if let today = payload["todayCost"], today.bool == nil, let amount = today.number { rows.append(Runtime.item("今天已用（UTC）", level: level, ["badge": .string(Runtime.moneyText(amount, "USD"))])) }
        if case .int(let packages)? = payload["packages"] {
            // The balance is the organization's shared add-on pool; members' plan Credits are separate.
            rows.append(Runtime.item("组织共享加购 Credits", level: level, ["badge": .string("\(packages) 个有效资源包"), "paneldetail": "不含成员各自的套餐 Credits"]))
            let org = Py.dict(entry["options"]).object?["org"]
            if org?.isTruthy == true || (entry["id"] == .string("default") && env("QODER_CN_ORG_ID") != nil) {
                rows.append(Runtime.item("组织 ID", level: level, ["badge": org?.isTruthy == true ? org! : .string(env("QODER_CN_ORG_ID") ?? ""), "color": .string(Runtime.mutedColor), "size": 10]))
            }
        }
        if payload["permission"] == .bool(false) && payload["permission"]?.bool != nil {
            rows.append(Runtime.item(Py.str(payload["state"] ?? .string("没有查看花费的权限")), level: level, ["color": .string(Runtime.warnColor), "size": 10,
                "paneldetail": .string(payload["reason"] != .string("workspace") ? "需要管理员 Key（sk-ant-admin…），个人组织需先转为团队组织" : "这把 Key 绑定了 Workspace，换用管理员 Key（sk-ant-admin…）")]))
        }
        if Runtime.keyTTLs.contains(where: { $0.0 == provider }) { rows.append(refreshRow(payload, error, level: level)) }
        let id = Py.str(entry["id"] ?? .null)
        if Runtime.keyCheckProviders.contains(provider) {
            let check = keyCheck(provider, id)
            let badge = check.map { Py.str($0["state"] ?? .null) + " · " + freshness($0["checkedAt"]).replacingOccurrences(of: "更新", with: "") } ?? "未测试"
            rows.append(Runtime.item("测试 Key", level: level, Runtime.merged(["badge": .string(badge)], act("test-api-key", provider, id))))
        }
        if let source = entry["source"], source.isTruthy { rows.append(Runtime.item("来源", level: level, ["paneldetail": source, "color": .string(Runtime.mutedColor), "size": 10])) }
        rows.append(Runtime.item("重命名…", level: level, act("rename-api-key", provider, id)))
        // A key read from OpenCodex's own config is not ours to delete.
        if entry["source"] != .string(ocxHome + "/config.json") { rows.append(Runtime.item("删除…", level: level, Runtime.merged(["color": .string(Runtime.badColor)], act("remove-api-key", provider, id)))) }
        return rows
    }

    func apiValues(_ provider: String, _ data: JSONObject) -> ([JSONObject], [JSONObject]) {
        let keys = apiKeys(provider)
        var values = keys.map { keyValue(provider, $0, data) }
        for index in values.indices {
            values[index]["label"] = keys[index]["label"] ?? .null
            if values[index]["kind"]?.isTruthy == true { values[index]["account"] = .array(keyAccount(provider, keys[index]).map(JSON.string)) }
        }
        return (keys, values)
    }

    /// The currencies this API's keys hold; a Kimi key's site tells it before the first reading.
    func balanceCurrencies(_ provider: String, _ keys: [JSONObject], _ values: [JSONObject]) -> [String] {
        var found: [String] = []
        for (entry, value) in zip(keys, values) {
            let currency: String
            if value["kind"] == .string("balance") { currency = Py.str(value["currency"] ?? .null) }
            else if provider == "kimi-api" {
                let region = Py.dict(entry["options"]).object?["region"].flatMap { $0.isTruthy ? Py.str($0) : nil } ?? environment["MOONSHOT_REGION"] ?? "china"
                currency = Accounts.moonshotHosts[region]?.1 ?? "CNY"
            } else { currency = Catalog.balanceCurrencies[provider] ?? "" }
            if !found.contains(currency) { found.append(currency) }
        }
        return found
    }

    func apiLines(_ provider: String, _ data: JSONObject) -> [String] {
        let service = Catalog.byKey[provider]!
        let (keys, values) = apiValues(provider, data)
        var first: [String: JSONObject] = [:]
        for (entry, value) in zip(keys, values) { if let account = value["account"] { let key = Py.dumps(account); if first[key] == nil { first[key] = entry } } }
        var rows = [Runtime.item(service.label, Runtime.merged(["badge": .string(Runtime.apiBadge(values))], brandIcon(provider)))]
        for (entry, value) in zip(keys, values) {
            var note: JSON = .null
            if let account = value["account"], let same = first[Py.dumps(account)], same["id"] != entry["id"] {
                let kind = account.array?.first?.string ?? ""
                note = .string("与「\(Py.str(same["label"] ?? .null))」同一\(kind == "org" ? "组织" : "Team")，主页合计只算一次")
            }
            rows.append(Runtime.item(Py.str(entry["label"] ?? .null), level: 1, ["badge": value["text"] ?? .null, "paneldetail": note]))
            rows += keyLines(provider, entry, data, level: 2)
        }
        rows.append(Runtime.item("添加 Key…", level: 1, Runtime.merged(["sfimage": "plus"], act("add-api-key", provider))))
        if Catalog.balanceAPIs.contains(provider) && !keys.isEmpty {
            // One line per currency: Kimi's China and international sites hold yuan and dollars.
            let currencies = balanceCurrencies(provider, keys, values)
            let lines = settings()["balanceAlerts"]?.object ?? JSONObject()
            for currency in currencies {
                let line = lines[provider + "|" + currency]?.number
                let label = currencies.count == 1 ? "余额提醒…" : (["CNY": "人民币", "USD": "美元", "Credits": "Credits "][currency] ?? currency) + "余额提醒…"
                rows.append(Runtime.item(label, level: 1, Runtime.merged(["sfimage": "bell", "badge": .string(line.map { "低于 " + Runtime.moneyText($0, currency) } ?? "关闭")],
                                                                          act("set-balance-alert", provider, currency))))
            }
        }
        rows.append(Runtime.item("打开用量与账单", level: 1, ["href": .string(service.url), "sfimage": "arrow.up.right"]))
        return rows
    }

    /// Subscriptions without a quota API: a manual, unverified mark and the official page.
    func portalServiceLines(_ key: String) -> [String] {
        let service = Catalog.byKey[key]!, configured = connectionReady(key)
        return [Runtime.item(service.label, Runtime.merged(["badge": .string(configured ? "已标记" : "未配置")], brandIcon(key))),
                Runtime.item("手动标记，未验证", level: 1, ["color": .string(Runtime.mutedColor), "size": 10]),
                Runtime.item(configured ? "清除标记" : "标记为已订阅", level: 1, act("set-subscription", key, configured ? "false" : "true")),
                Runtime.item("打开订阅管理", level: 1, ["href": .string(service.url), "sfimage": "arrow.up.right"])]
    }

    /// MiniMax plan quota: the share already used per window, like every other subscription.
    func providerAccountLines(_ key: String, _ data: JSONObject) -> [String] {
        let service = Catalog.byKey[key]!, payload = object(data, key), error = failure(data, key), failed = Runtime.refreshFailed(error)
        let now = Date().timeIntervalSince1970
        let windows = (payload["windows"]?.array ?? []).compactMap(\.object).filter { !($0["resetsAt"]?.isTruthy ?? false) || ($0["resetsAt"]?.number ?? 0) > now }
        let most = windows.map { 100 - Py.num($0["remaining"]) }.max()
        let badge = most.map(Runtime.usedText) ?? (payload["windows"]?.isTruthy == true ? "待刷新" : connectionReady(key) ? Py.str(payload["state"] ?? .string("待同步")) : "未配置")
        var rows = [Runtime.item(service.label, Runtime.merged(["badge": .string(Runtime.withFailure(badge, !windows.isEmpty, failed))], brandIcon(key),
                                                              ["panelusagelevel": windows.isEmpty ? .null : Runtime.usageLevel(most, failed)]))]
        for window in windows {
            let used = 100 - Py.num(window["remaining"])
            rows.append(Runtime.item(Py.str(window["name"] ?? .null), level: 1, ["badge": .string(Runtime.usedText(used)), "panelusagelevel": Runtime.usageLevel(used, failed),
                "paneldetail": windowDetail(used, window["resetsAt"]?.number, Runtime.minimaxSpan(window), payload["_fetchedAt"]?.number, failed: failed)]))
        }
        rows.append(connectionReady(key) ? refreshRow(payload, error) : Runtime.item("选择密钥文件…", level: 1, act("configure-connection", key)))
        rows.append(Runtime.item("打开用量与账单", level: 1, ["href": .string(service.url), "sfimage": "arrow.up.right"]))
        return rows
    }

    /// A subscription's or API's row for its service's status page, while 服务状态 is on.
    func statusLines(_ key: String, _ data: JSONObject) -> [String] {
        guard let page = ServiceStatus.modulePages[key], settings()["serviceStatus"]?.isTruthy == true, let (name, _, link) = ServiceStatus.pages[page] else { return [] }
        let state = object(data, "service_status")[page]?.object ?? JSONObject()
        let badge: String, light: String
        if state.isEmpty { (badge, light) = ("待同步", "unknown") }
        else if state["error"]?.isTruthy == true { (badge, light) = ("读取失败", "unknown") }
        else {
            let indicator = Py.str(state["indicator"] ?? .null)
            badge = (ServiceStatus.indicators[indicator] ?? "").nonEmpty ?? "运行正常"
            light = indicator == "none" ? "ok" : "issue"
        }
        return [Runtime.item(name + " 服务状态", level: 1, ["badge": .string(badge), "href": .string(link), "panelstatus": .string(light), "paneldetail": state["description"] ?? .null])]
    }

    static func timesOver(_ ratio: Double) -> String { (ratio < 100 ? String(format: "%.1f", ratio) : String(Int(ratio.rounded(.toNearestOrEven)))) + " 倍" }

    /// A subscription weighed against this month's API-equivalent cost of its Harness.
    func paybackLines(_ key: String) -> [String] {
        guard let source = Catalog.subscriptionUsage[key] else { return [] }
        let price = settings()["subscriptionPrices"]?.object?[key]?.number
        var rows = [Runtime.item("订阅月费…", level: 1, Runtime.merged(["sfimage": "dollarsign.circle", "badge": .string(price.map { Runtime.moneyText($0, "USD") + "/月" } ?? "未填写")],
                                                                     act("set-subscription-price", key)))]
        guard let price else { return rows }
        let name = Preferences.modules.first { $0.key == source }!.label
        let usage = moduleEnabled(source) ? readCache(rangeCacheName("month"))["payload"]?.object?[source]?.object : nil
        if !moduleEnabled(source) { rows.append(Runtime.item("本月回本", level: 1, ["badge": .string("需勾选 " + name + " Harness"), "color": .string(Runtime.mutedColor), "size": 11])) }
        else if usage?["summary"]?.isTruthy != true { rows.append(Runtime.item("本月回本", level: 1, ["badge": "待同步", "color": .string(Runtime.mutedColor), "size": 11])) }
        else {
            let summary = usage!["summary"]!.object!
            let cost = Swift.max(0, Py.or(summary["estimatedCostUsd"], .int(0)).pythonFloat ?? 0), unpriced = Int(Py.or(summary["unpricedRequests"], .int(0)).pythonFloat ?? 0)
            let detail = "本月 \(name) 的 API 等值 \(Runtime.moneyText(cost, "USD"))，订阅 \(Runtime.moneyText(price, "USD"))/月" + (unpriced != 0 ? "；其中 \(unpriced) 次未定价，未计入" : "")
            rows.append(Runtime.item("本月回本", level: 1, ["sfimage": "chart.line.uptrend.xyaxis", "badge": .string(Runtime.timesOver(cost / price)), "paneldetail": .string(detail)]))
        }
        return rows
    }

    func resourceLines(_ data: JSONObject) -> [String] {
        var lines: [String] = []
        for service in Catalog.services where moduleEnabled(service.key) {
            let key = service.key
            if key == "chatgpt" { lines += chatgptLines(data) }
            else if key == "claude-subscription" { lines += claudeSubscriptionLines(data) }
            else if key == "qoder-subscription" { lines += qoderSubscriptionLines(data) }
            else if key == "cursor-subscription" { lines += cursorSubscriptionLines(data) }
            else if service.category == "API" { lines += apiLines(key, data) }
            else if Runtime.accountSources.contains(key) { lines += providerAccountLines(key, data) }
            else { lines += portalServiceLines(key) }
            lines += paybackLines(key)
            lines += statusLines(key, data)
        }
        return lines + (lines.isEmpty ? [] : ["---"])
    }

    // MARK: OpenCodex management and the whole list

    /// Rows flattened and indented, so the plugin never nests more than two levels.
    static func nest(_ lines: [String], level: Int = 1) -> [String] {
        let prefix = String(repeating: "--", count: level)
        var flattened: [String] = []
        for var line in lines where !line.isEmpty {
            if line.allSatisfy({ $0 == "-" }) {
                let separator = prefix + "---"
                if let last = flattened.last, last != separator { flattened.append(separator) }
                continue
            }
            while line.hasPrefix("--") { line.removeFirst(2) }
            flattened.append(prefix + line)
        }
        while flattened.last == prefix + "---" { flattened.removeLast() }
        return flattened
    }

    func unfilteredManagementLines(_ data: JSONObject) -> [String] {
        let accounts = Py.dict(object(data, "accounts")["accounts"]).array ?? []
        let healthy = data["healthz"]?.isTruthy == true && !Runtime.refreshFailed(failure(data, "healthz"))
        let pending = !(data["healthz"]?.isTruthy ?? false) && failure(data, "healthz") == Runtime.pendingRead
        let readiness = object(data, "ready")["status"]?.string
        let status = healthy ? (readiness == "failed" ? "启动失败" : readiness == "pending" || readiness == "starting" ? "正在启动" : "") : (pending ? "待同步" : "未连接")
        var lines = resourceLines(data) + [Runtime.item("OpenCodex 管理" + (status.isEmpty ? "" : " · " + status), ["sfimage": "slider.horizontal.3"])]
        var children = [Runtime.item("打开控制台", ["href": .string(base + "/"), "sfimage": "macwindow"]),
                        Runtime.item("查看完整用量", ["href": .string(base + "/#usage"), "sfimage": "chart.bar.xaxis"])]
        if healthy {
            children += ["---", Runtime.item("ChatGPT 账户 · \(accounts.count)", ["panelsection": true, "color": .string(Runtime.mutedColor), "size": 11])]
            children += accountQuickLines(data)
            children += ["---", Runtime.item("同步模型到 ChatGPT", Runtime.merged(["sfimage": "arrow.triangle.2.circlepath"], act("sync"))),
                         Runtime.item("检查更新", Runtime.merged(["sfimage": "arrow.down.circle"], act("check-update"))),
                         Runtime.item("打开代理日志", Runtime.merged(["sfimage": "doc.text"], act("logs"))),
                         Runtime.item("重新启动代理…", Runtime.merged(["sfimage": "arrow.clockwise"], act("restart"))),
                         Runtime.item("停止代理…", Runtime.merged(["sfimage": "stop.circle", "color": .string(Runtime.badColor)], act("stop")))]
        } else if !pending {
            children += ["---", Runtime.item("启动 OpenCodex 代理", Runtime.merged(["sfimage": "play.fill"], act("start"))),
                         Runtime.item("修复自启动服务", Runtime.merged(["sfimage": "wrench.and.screwdriver"], act("repair"))),
                         Runtime.item("查看 ocx status", Runtime.merged(["sfimage": "terminal"], act("status")))]
        }
        if healthy {
            children += ["---", Runtime.item("数据更新时间", ["panelsection": true, "color": .string(Runtime.mutedColor), "size": 11])]
            // OpenCodex's part of the range bundle carries when it was last read.
            let usage = object(data, "range_usage"), ocx = usage["opencodex"]?.object ?? JSONObject()
            let read: JSON = Py.or(ocx["readAt"], ocx["summary"]?.isTruthy == true ? usage["_fetchedAt"] ?? .null : .null)
            let rangeLabel = Runtime.ranges.first { $0.0 == (usage["range"]?.string ?? selectedRange()) }?.1 ?? ""
            let ocxError = failure(data, "range_usage") ?? ocx["refreshError"].flatMap { $0.isTruthy ? Py.str($0) : nil } ?? ocx["error"].flatMap { $0.isTruthy ? Py.str($0) : nil }
            for (label, fetched, error, fromCache) in [("用量统计 · " + rangeLabel, read, ocxError, failure(data, "range_usage") != nil),
                                                      ("供应商余额", object(data, "provider_quotas")["_fetchedAt"] ?? .null, failure(data, "provider_quotas"), true)] {
                var state = freshness(fetched)
                if let error, !fromCache || Runtime.refreshFailed(error) { state = fetched.isTruthy ? state + " · 本次读取失败" : "读取失败" }
                children.append(Runtime.item(label, ["badge": .string(state), "color": .string(Runtime.mutedColor), "size": 11]))
            }
        }
        lines += Runtime.nest(children)
        if moduleEnabled("sessions") { lines += sessionsLines(data, unit: currencyUnit(data)) }
        lines += komariLines(data) + appUpdateLines(data) + selfUpdateLines(data) + ["---"]
        lines.append(Runtime.item("刷新数据", Runtime.merged(["sfimage": "arrow.clockwise"], act("refresh"))))
        return lines
    }

    func managementLines(_ data: JSONObject) -> [String] {
        var result: [String] = [], keep = true
        for row in unfilteredManagementLines(data) {
            if row == "---" { if let last = result.last, last != "---" { result.append(row) }; continue }
            if !row.hasPrefix("--") {
                let key = [("OpenCodex 管理", "opencodex"), ("Komari 监控", "komari"), ("Harness 更新", "updates")].first { row.hasPrefix($0.0) }?.1
                keep = key == nil || moduleEnabled(key!)
            }
            if keep { result.append(row) }
        }
        while result.last == "---" { result.removeLast() }
        return result
    }
}
