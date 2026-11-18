import Foundation

/// Each module's rows: the subscription windows, accounts, APIs and their keys, Komari, sessions, updates and
/// OpenCodex's management. Rendering only reads caches; the reads happen in the background worker.
extension Runtime {
    /// Every cache the panel shows, with its error message (nil when none).
    public func cachedData() -> JSONObject {
        var data = JSONObject()
        func put(_ name: String, _ result: (JSONObject?, String?)) {
            data[name] = result.0.map(JSON.object) ?? .null
            data[name + "_error"] = result.1.map(JSON.string) ?? .null
        }
        for (name, ttl) in Runtime.externalTTLs.sorted(by: { $0.key < $1.key }) { put(name, cacheResult(readCache(name), ttl: ttl)) }
        if !opencodexConfigured() {
            for (name, _, _) in Runtime.opencodexEndpoints { data[name] = .null }
            data["healthz_error"] = "未配置 OpenCodex"
        }
        for key in Runtime.accountSources where !defaultReady(key) { data[key] = .object(Accounts.account(key, environment: environment)); data[key + "_error"] = .null }
        for (provider, ttl) in Runtime.keyTTLs where moduleEnabled(provider) {
            for entry in namedKeys(provider) { if let id = entry["id"]?.string { put(provider + "@" + id, cacheResult(readCache(provider + "@" + id), ttl: ttl)) } }
        }
        if !chatgptConfigured() { data["chatgpt"] = ["state": "未找到 ChatGPT 客户端登录"]; data["chatgpt_error"] = .null }
        put("range_usage", cacheResult(readCache(rangeCacheName()), ttl: refreshTTL(60)))
        // A new range and its totals appear together; the previous range stays until then, for the same sources.
        if data["range_usage"]?.isNull ?? true, !Runtime.refreshFailed(data["range_usage_error"]?.string) {
            let previous = readCache("panel-view")["dashboard"]?.object?["rangeKey"]?.string
            if let previous, Runtime.ranges.contains(where: { $0.0 == previous }), previous != selectedRange() {
                let (last, error) = cacheResult(readCache(rangeCacheName(previous)), ttl: refreshTTL(60))
                if let last { data["range_usage"] = .object(last); data["range_usage_error"] = error.map(JSON.string) ?? .null; data["displayed_range"] = .string(previous) }
            }
        }
        return data
    }

    func object(_ data: JSONObject, _ name: String) -> JSONObject { data[name]?.object ?? JSONObject() }
    func failure(_ data: JSONObject, _ name: String) -> String? { data[name + "_error"]?.string }

    // MARK: Subscriptions

    func claudeSubscriptionLines(_ data: JSONObject) -> [String] {
        var payload = data["claude_subscription"]?.object ?? JSONObject()
        if payload.isEmpty {
            // Rendering reads caches only; the login read runs in the background, only local snapshots are read here.
            payload = settings()["claudeOAuthUsage"]?.isTruthy == true ? ["source": "claude-oauth", "state": "待同步"] : (claudeSubscriptionLoad().0 ?? JSONObject())
        }
        let windows = (payload["windows"]?.array ?? []).compactMap(\.object)
        let live = payload["source"] == .string("live") || payload["source"] == .string("claude-oauth")
        var error = failure(data, "claude_subscription")
        if let fetch = payload["fetchError"], fetch.isTruthy { error = Py.str(fetch) }
        let stale = Runtime.claudeUsageStale(payload)
        var value = !windows.isEmpty && !stale ? "已用 " + windows.prefix(2).map { Py.str($0["label"] ?? .null) + " " + Runtime.g(Py.num($0["used"])) + "%" }.joined(separator: " · ")
            : Py.str(payload["state"] ?? .string("暂无本机用量快照"))
        if !windows.isEmpty && stale { value = live ? "用量待更新" : "本机用量待更新" }
        let failed = Runtime.refreshFailed(error)
        // Like every other module: the last good figures stay, marked as a failed refresh.
        value = Runtime.withFailure(value, !windows.isEmpty && !stale, failed)
        let source = ["claude-oauth": "Code 登录", "live": "App 登录 · 手动查询", "claude-code": "Code 本机回传", "app-snapshot": "App 本机快照"][payload["source"]?.string ?? ""] ?? "Claude App 本机快照"
        let most = windows.map { Py.num($0["used"]) }.max() ?? 0
        var rows = [Runtime.item("Claude 订阅", Runtime.merged(["badge": .string(value)], brandIcon("claude-subscription"),
                                                              ["panelusagelevel": Runtime.usageLevel(most, stale || failed || windows.isEmpty)])),
                    Runtime.item("来源 · " + source, level: 1, ["color": .string(Runtime.mutedColor), "size": 10])]
        for window in windows {
            let resets = window["resetsAt"].flatMap { $0.isTruthy ? $0.string.flatMap { try? ISOTime.parse($0).timestamp() } : nil }
            rows.append(Runtime.item(Py.str(window["label"] ?? .null), level: 1, ["badge": .string(Runtime.usedText(Py.num(window["used"]))),
                "paneldetail": windowDetail(Py.num(window["used"]), resets, Runtime.claudeSpan(window), payload["updatedAt"]?.number, failed: stale || failed),
                "panelusagelevel": Runtime.usageLevel(Py.num(window["used"]), stale || failed)]))
        }
        if let extra = payload["extraUsage"]?.object {
            if extra["enabled"]?.isTruthy == true {
                let currency = Py.str(extra["currency"] ?? .string("USD"))
                let symbol = currency == "USD" ? "$" : currency + " "
                func money(_ value: Double) -> String { symbol + String(format: "%.2f", value) }
                let used = Py.num(extra["used"]), limit = extra["limit"].flatMap { $0.isTruthy ? $0.number : nil }
                rows.append(Runtime.item("额外用量 · 本月", level: 1, ["badge": .string(money(used) + (limit.map { " / " + money($0) } ?? "")),
                    "paneldetail": .string(limit.map { "剩余 " + money(Swift.max(0, $0 - used)) } ?? "未设置每月上限")]))
                if let balance = extra["balance"], balance.bool == nil, let amount = balance.number { rows.append(Runtime.item("额外用量余额", level: 1, ["badge": .string(money(amount))])) }
            } else {
                rows.append(Runtime.item("额外用量", level: 1, ["badge": "未开启", "color": .string(Runtime.mutedColor), "size": 10]))
            }
        }
        // Failures and stale data are explained once here, not on each row.
        let age = windows.isEmpty ? "" : freshness(payload["updatedAt"])
        if failed {
            rows.append(Runtime.item("更新时间", level: 1, ["badge": "刷新失败", "paneldetail": .string(error! + (windows.isEmpty ? "" : "\n保留最近一次成功的数据 · " + age)),
                                                           "color": .string(Runtime.warnColor), "size": 10]))
        } else if !windows.isEmpty {
            rows.append(Runtime.item("更新时间", level: 1, ["badge": .string(age), "paneldetail": stale ? "数据待更新" : .null, "color": .string(Runtime.mutedColor), "size": 10]))
        }
        if settings()["claudeOAuthUsage"]?.isTruthy == true {
            rows.append(Runtime.item("立即刷新用量", level: 1, Runtime.merged(["sfimage": "arrow.clockwise"], act("retry-claude-usage"))))
            if failed { rows.append(Runtime.item("重新授权读取登录…", level: 1, Runtime.merged(["sfimage": "lock"], act("reauthorize-claude-oauth")))) }
        } else {
            rows.append(Runtime.item("启用 Claude Code 登录查询…", level: 1, Runtime.merged(["sfimage": "arrow.triangle.2.circlepath", "paneldetail": "主动获取订阅用量，不读取 Cookie"],
                                                                                         act("set-claude-oauth", "true"))))
        }
        let app = Updates.installedAppPath("Claude", home: home)
        rows.append(Runtime.item("打开 Claude", level: 1, ["sfimage": "macwindow", "href": .string(app.map(fileHref) ?? "https://claude.ai/")]))
        rows.append(Runtime.item("打开订阅用量", level: 1, ["sfimage": "arrow.up.right", "href": .string(Catalog.byKey["claude-subscription"]!.url)]))
        return rows
    }

    func qoderSubscriptionLines(_ data: JSONObject) -> [String] {
        let service = Catalog.byKey["qoder-subscription"]!, payload = object(data, "qoder_subscription")
        let error = failure(data, "qoder_subscription"), failed = Runtime.refreshFailed(error)
        let windows = [("套餐 Credits", payload["plan"]?.object), ("加购 Credits", payload["addOn"]?.object)].compactMap { name, q in
            q.flatMap { $0["total"]?.isTruthy == true ? (name, $0) : nil } }
        let observed = payload["observedAt"]?.number
        // The app logs a summary only when its Credits panel is opened; an older one is labelled, not passed off as current.
        let stale = observed == nil || observed == 0 || Date().timeIntervalSince1970 - observed! > 600
        let most = windows.map { 100 * Py.num($0.1["used"]) / Py.num($0.1["total"]) }.max()
        var badge: String
        if let most { badge = Runtime.usedText(most) + (stale ? " · 截至 " + snapshotTime(observed ?? 0) : "") }
        else if observed != nil && observed != 0 { badge = payload["exceeded"]?.isTruthy == true ? "额度已用完" : "无 Credits 额度" }
        else { badge = connectionReady("qoder-subscription") ? "暂无本机记录" : "未发现 Qoder" }
        var rows = [Runtime.item(service.label, Runtime.merged(["badge": .string(Runtime.withFailure(badge, !windows.isEmpty, failed))], brandIcon("qoder-subscription"),
                                                              ["panelusagelevel": windows.isEmpty ? .null : Runtime.usageLevel(most, stale || failed)])),
                    Runtime.item("来源 · Qoder 本机记录", level: 1, ["color": .string(Runtime.mutedColor), "size": 10])]
        for (name, q) in windows {
            let used = 100 * Py.num(q["used"]) / Py.num(q["total"])
            var detail = Runtime.creditsAmount(Py.num(q["used"])) + " / " + Runtime.creditsAmount(Py.num(q["total"])) + " · 剩余 " + Runtime.creditsAmount(Py.num(q["remaining"]))
            if name == "套餐 Credits", let resets = payload["resetsAt"], resets.isTruthy { detail += "\n重置于 " + Runtime.format(resets.number ?? 0, localZone(), "%m/%d %H:%M") }
            rows.append(Runtime.item(name, level: 1, ["badge": .string(Runtime.usedText(used)), "paneldetail": .string(detail), "panelusagelevel": Runtime.usageLevel(used, stale || failed)]))
        }
        let hint = "在 Qoder 窗口右下角打开 Credits 用量后更新"
        if failed { rows.append(Runtime.item("更新时间", level: 1, ["badge": "刷新失败", "paneldetail": .string(error!), "color": .string(Runtime.warnColor), "size": 10])) }
        else if let observed, observed != 0 {
            rows.append(Runtime.item("更新时间", level: 1, ["badge": .string(freshness(.double(observed))), "paneldetail": stale ? .string(hint) : .null, "color": .string(Runtime.mutedColor), "size": 10]))
        } else { rows.append(Runtime.item(hint, level: 1, ["color": .string(Runtime.mutedColor), "size": 10])) }
        if let app = updateAppPath("qoder") { rows.append(Runtime.item("打开 Qoder", level: 1, ["sfimage": "macwindow", "href": .string(fileHref(app))])) }
        rows.append(Runtime.item("打开订阅用量", level: 1, ["sfimage": "arrow.up.right", "href": .string(service.url)]))
        return rows
    }

    func cursorSubscriptionLines(_ data: JSONObject) -> [String] {
        let service = Catalog.byKey["cursor-subscription"]!, enabled = settings()["cursorUsage"]?.isTruthy == true
        let payload = enabled ? object(data, "cursor_subscription") : JSONObject(), error = enabled ? failure(data, "cursor_subscription") : nil
        let failed = Runtime.refreshFailed(error)
        let percent = payload["percent"].flatMap { $0.isNull ? nil : $0.number }
        let end = payload["cycleEnd"]?.number, start = payload["cycleStart"]?.number
        let badge: String
        if !enabled { badge = "未开启读取" }
        else if payload["unlimited"]?.isTruthy == true { badge = "不限量" }
        else if let percent { badge = Runtime.usedText(percent) }
        else { badge = payload.isEmpty ? "待同步" : "暂无套餐用量" }
        var rows = [Runtime.item(service.label, Runtime.merged(["badge": .string(Runtime.withFailure(badge, percent != nil, failed))], brandIcon("cursor-subscription"),
                                                              ["panelusagelevel": percent == nil ? .null : Runtime.usageLevel(percent, failed)])),
                    Runtime.item("来源 · Cursor 账户（cursor.com）", level: 1, ["color": .string(Runtime.mutedColor), "size": 10])]
        if let percent {
            let span: Double? = (end ?? 0) != 0 && (start ?? 0) != 0 ? end! - start! : nil
            rows.append(Runtime.item("本周期", level: 1, ["badge": .string(Runtime.usedText(percent)), "paneldetail": windowDetail(percent, end, span, payload["_fetchedAt"]?.number, failed: failed),
                                                       "panelusagelevel": Runtime.usageLevel(percent, failed)]))
            if let limit = payload["limitCents"], limit.isTruthy {
                rows.append(Runtime.item("套餐额度", level: 1, ["badge": .string(String(format: "$%.2f / $%.2f", Py.num(payload["usedCents"]) / 100, Py.num(limit) / 100))]))
            }
        }
        let extra = payload["onDemand"]?.object ?? JSONObject()
        if !payload.isEmpty {
            let text = extra["enabled"]?.isTruthy == true ? String(format: "$%.2f", Py.num(extra["usedCents"]) / 100) + (extra["limitCents"]?.isTruthy == true ? String(format: " / $%.2f", Py.num(extra["limitCents"]) / 100) : "") : "未开启"
            rows.append(Runtime.item("按需用量", level: 1, ["badge": .string(text)]))
        }
        if enabled { rows.append(refreshRow(payload, error)) }
        else {
            rows.append(Runtime.item("读取 Cursor 用量记录…", level: 1, Runtime.merged(["sfimage": "arrow.triangle.2.circlepath", "paneldetail": "用本机 Cursor 登录查询 cursor.com，令牌只在内存中使用"],
                                                                                      act("set-cursor-usage", "true"))))
        }
        if let app = updateAppPath("cursor") { rows.append(Runtime.item("打开 Cursor", level: 1, ["sfimage": "macwindow", "href": .string(fileHref(app))])) }
        rows.append(Runtime.item("打开用量与账单", level: 1, ["sfimage": "arrow.up.right", "href": .string(service.url)]))
        return rows
    }

    // MARK: ChatGPT and OpenCodex's accounts

    /// Weekly share already used by each ChatGPT account: name, percent or nil, state.
    func accountUsage(_ data: JSONObject) -> [(String, Double?, String)] {
        let accounts = Py.dict(object(data, "accounts")["accounts"]).array ?? []
        let quotas = Py.dict(object(data, "quota")["quotas"]).object ?? JSONObject()
        return accounts.enumerated().map { index, row in
            let fields = row.object ?? JSONObject()
            let id = Py.str(Py.or(fields["id"], .string("")))
            let quota = Py.or(quotas[id], Py.or(fields["quota"], .object(JSONObject()))).object ?? JSONObject()
            let used = quota["weeklyPercent"]?.pythonFloat.map { Swift.min(100, Swift.max(0, $0)) }
            let label = Py.str(Py.or(Py.or(Py.or(fields["logLabel"], fields["email"] ?? .null), .string(id)), .string("账号 \(index + 1)")))
            let state = fields["paused"]?.isTruthy == true ? " 暂停" : fields["needsReauth"]?.isTruthy == true ? " 需登录" : ""
            return (Runtime.shorten(label, 14), used, state)
        }
    }

    func accountQuickLines(_ data: JSONObject) -> [String] {
        let accounts = Py.dict(object(data, "accounts")["accounts"]).array ?? []
        let active = object(data, "active"), quotas = Py.dict(object(data, "quota")["quotas"]).object ?? JSONObject()
        let pinned = active["pinnedAccountId"]
        if accounts.isEmpty {
            let error = failure(data, "accounts")
            return [Runtime.item(error != nil ? "账号读取失败" : "暂无可用账号", ["color": .string(error != nil ? Runtime.warnColor : Runtime.mutedColor)])]
        }
        var lines: [String] = [], current: JSONObject?
        for row in accounts {
            let fields = row.object ?? JSONObject()
            let id = Py.str(Py.or(fields["id"], .string("")))
            let name = Runtime.shorten(Py.str(Py.or(Py.or(Py.or(fields["logLabel"], fields["email"] ?? .null), .string(id)), .string("账号"))), 25)
            let quota = Py.or(quotas[id], Py.or(fields["quota"], .object(JSONObject()))).object ?? JSONObject()
            let quotaText = quota["weeklyPercent"]?.pythonFloat.map { " · " + Runtime.usedText($0) } ?? ""
            if fields["paused"]?.isTruthy == true {
                lines.append(Runtime.item("启用 · " + name + quotaText, Runtime.merged(["color": .string(Runtime.rankColor)], act("account-pause", id, "false"))))
            } else if pinned == .string(id) && pinned?.string != nil {
                current = fields
                lines.append(Runtime.item(name + quotaText, ["color": .string(Runtime.rankColor), "checked": true, "panelradio": "on"]))
            } else {
                // One active account at a time: the panel shows these rows as a radio group.
                lines.append(Runtime.item(name + quotaText, Runtime.merged(["color": .string(Runtime.rankColor), "panelradio": "off"], act("account-use", id))))
            }
        }
        if let current { lines.append(Runtime.item("暂停当前账号", Runtime.merged(["color": .string(Runtime.mutedColor)], act("account-pause", Py.str(Py.or(current["id"], .string(""))), "true")))) }
        return lines
    }

    static func chatgptWindowLabel(_ window: JSONObject) -> String {
        let minutes = Py.or(window["minutes"], .int(0)).number ?? 0
        let span = minutes == 10080 ? "本周" : minutes >= 60 ? g(minutes / 60) + " 小时" : g(minutes) + " 分钟"
        // The plan's own limit goes by its span alone (the page is already ChatGPT 订阅); another limit keeps its name.
        let name = Py.str(window["name"] ?? .null)
        return ["codex", "chatgpt"].contains(name.lowercased()) ? span : name + " · " + span
    }

    /// While OpenCodex is unreachable its account quota isn't being read, whatever that cache says.
    func proxyQuotaError(_ data: JSONObject) -> String? {
        let health = failure(data, "healthz")
        return Runtime.refreshFailed(health) ? health : failure(data, "quota")
    }

    func chatgptLines(_ data: JSONObject) -> [String] {
        let label = Catalog.byKey["chatgpt"]!.label
        if chatgptSource() == "proxy" {
            let usage = accountUsage(data), known = usage.compactMap(\.1)
            let error = proxyQuotaError(data), failed = Runtime.refreshFailed(error)
            var badge: String
            if usage.isEmpty { badge = opencodexConfigured() ? "待同步" : "未配置" }
            else if usage.count == 1 { badge = (usage[0].1.map(Runtime.usedText) ?? "额度待同步") + usage[0].2 }
            else { badge = "已用 " + usage.map { $0.0 + " " + ($0.1.map { Runtime.g(Runtime.round($0, 1)) + "%" } ?? "—") + $0.2 }.joined(separator: " · ") }
            let compact = "\(usage.count) 个账户"
            if usage.count > 2 || badge.count > 32 { badge = compact }
            var lines = [Runtime.item(label, Runtime.merged(["badge": .string(Runtime.withFailure(badge, !usage.isEmpty, failed)),
                "panelfallback": usage.count > 1 && badge != compact ? .string(Runtime.withFailure(compact, true, failed)) : .null], brandIcon("chatgpt"),
                ["panelusagelevel": known.isEmpty ? .null : Runtime.usageLevel(known.max(), failed)]))]
            lines.append(Runtime.item("来源 · OpenCodex 账户 · 编程工具额度", level: 1, ["color": .string(Runtime.mutedColor), "size": 10]))
            for (name, used, state) in usage {
                lines.append(Runtime.item(name, level: 1, ["badge": .string((used.map(Runtime.usedText) ?? "额度待同步") + state),
                                                           "panelusagelevel": used == nil ? .null : Runtime.usageLevel(used, failed)]))
            }
            if opencodexConfigured() { lines.append(refreshRow(object(data, "quota"), error)) }
            lines.append(Runtime.item("打开账户管理", level: 1, ["href": .string(base + "/"), "sfimage": "arrow.up.right"]))
            return lines
        }
        let payload = object(data, "chatgpt"), windows = (payload["windows"]?.array ?? []).compactMap(\.object)
        let error = failure(data, "chatgpt"), failed = Runtime.refreshFailed(error)
        let now = Date().timeIntervalSince1970
        let visible = windows.filter { !($0["resetsAt"]?.isTruthy ?? false) || ($0["resetsAt"]?.number ?? 0) > now }
        let most = visible.map { 100 - Py.num($0["remaining"]) }.max()
        var badge = most.map(Runtime.usedText) ?? (chatgptConfigured() ? Py.str(payload["state"] ?? .string("待同步")) : "未配置")
        if !windows.isEmpty && visible.isEmpty { badge = "待刷新" }
        var lines = [Runtime.item(label, Runtime.merged(["badge": .string(Runtime.withFailure(badge, !visible.isEmpty, failed))], brandIcon("chatgpt"),
                                                       ["panelusagelevel": visible.isEmpty ? .null : Runtime.usageLevel(most, failed)]))]
        lines.append(Runtime.item("来源 · 本机登录 · 编程工具额度", level: 1, ["color": .string(Runtime.mutedColor), "size": 10]))
        for window in visible {
            let minutes = Py.or(window["minutes"], .int(0)).number ?? 0, used = 100 - Py.num(window["remaining"])
            lines.append(Runtime.item(Runtime.chatgptWindowLabel(window), level: 1, ["badge": .string(Runtime.usedText(used)), "panelusagelevel": Runtime.usageLevel(used, failed),
                "paneldetail": windowDetail(used, window["resetsAt"]?.number, minutes * 60, payload["_fetchedAt"]?.number, failed: failed)]))
        }
        if visible.isEmpty && !failed, let state = payload["state"], state.isTruthy {
            lines.append(Runtime.item(Py.str(state), level: 1, ["color": .string(Runtime.mutedColor), "size": 10]))
        }
        if chatgptConfigured() { lines.append(refreshRow(payload, error)) }
        lines.append(Runtime.item("打开 ChatGPT", level: 1, ["href": "https://chatgpt.com", "sfimage": "arrow.up.right"]))
        return lines
    }
}

extension Runtime {
    /// The installed desktop app an update row or sidebar opens, when there is one.
    func updateAppPath(_ key: String) -> String? {
        for name in Runtime.updateApps[key] ?? [] { if let path = Updates.installedAppPath(name, home: home) { return path } }
        return nil
    }
}
