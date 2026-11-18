import Foundation

extension Runtime {
    // MARK: Komari

    func komariExpiry(_ row: JSONObject) -> (String, Int?) {
        let end = Py.str(Py.or(row["expired_at"], .string("")))
        guard end.count >= 10, let day = Runtime.dayFromISO(String(end.prefix(10))) else { return ("未设置", nil) }
        let year = civilFromDays(day).0
        if year >= 2100 { return ("长期", nil) }
        if year < 1970 { return ("未设置", nil) }
        return (Runtime.iso(day), day - Runtime.dayNumber(Date().timeIntervalSince1970, .current))
    }

    func komariLines(_ data: JSONObject) -> [String] {
        let address = komariBaseURL
        if address.isEmpty {
            return [Runtime.item("Komari 监控", ["badge": "未配置", "sfimage": "server.rack"]),
                    Runtime.item("设置面板地址…", level: 1, act("configure-connection", "komari"))]
        }
        let payload = object(data, "komari"), servers = (payload["servers"]?.array ?? []).compactMap(\.object)
        let error = failure(data, "komari")
        let fetched = payload["_fetchedAt"].flatMap { $0.bool == nil ? $0.number : nil }
        let age = fetched.map { $0 > 0 ? Swift.max(0, Date().timeIntervalSince1970 - $0) : .infinity } ?? .infinity
        let live = payload["statusAvailable"]?.isTruthy == true && age <= 90 && error == nil
        let online = servers.filter { $0["online"] == .bool(true) && $0["online"]?.bool != nil }.count
        let unknown = servers.filter { $0["online"]?.bool == nil }.count
        let summary: String
        if servers.isEmpty { summary = Runtime.refreshFailed(error) ? "连接失败" : payload.isEmpty ? "待同步" : "暂无服务器" }
        else if !live { summary = "状态待更新" }
        else if unknown > 0 { summary = "\(online) 在线 · \(unknown) 未知" }
        else { summary = "\(online)/\(servers.count) 在线" }
        var lines = [Runtime.item("Komari 监控", ["badge": .string(summary), "sfimage": "server.rack", "color": .null,
                                                  "tooltip": .string("Komari · " + summary + " · " + freshness(payload["_fetchedAt"]))])]
        let ordered = servers.enumerated().sorted { a, b in
            let x = -(Py.or(a.element["weight"], .int(0)).pythonFloat ?? 0), y = -(Py.or(b.element["weight"], .int(0)).pythonFloat ?? 0)
            if x != y { return x < y }
            let p = Py.str(Py.or(a.element["name"], .string(""))), q = Py.str(Py.or(b.element["name"], .string("")))
            return p != q ? ClaudeUsage.pythonLess(p, q) : a.offset < b.offset
        }.map(\.element)
        for row in ordered {
            let state: Bool? = live ? row["online"]?.bool : nil
            let (mark, label) = state == true ? ("●", "在线") : state == false ? ("○", "离线") : ("?", "待更新")
            let name = Runtime.shorten(Py.str(Py.or(row["name"], row["uuid"] ?? .null)), 18)
            let (expiry, days) = komariExpiry(row)
            var cpu = "—", ram = "—"
            if state == true {
                cpu = String(format: "%.0f%%", Py.or(row["cpu"], .int(0)).pythonFloat ?? 0)
                let total = Py.or(row["ram_total"], .int(0)).pythonFloat ?? 0
                if total > 0 { ram = String(format: "%.0f%%", (Py.or(row["ram"], .int(0)).pythonFloat ?? 0) * 100 / total) }
            }
            let detail = "\(label) · CPU \(cpu) · 内存 \(ram) · 到期 \(expiry)"
            var expiryText = expiry
            if let days, days <= 7 { expiryText += days < 0 ? " 已到期" : days == 0 ? " 今天到期" : " · \(days)天后" }
            let color = state == true ? Runtime.komariOnline : state == false ? Runtime.badColor : Runtime.mutedColor
            let url = address + "/instance/" + Accounts.quote(Py.str(Py.or(row["uuid"], .string(""))))
            lines.append(Runtime.item("\(mark) \(Runtime.padDisplay(name, 18))  \(label) · \(expiryText)", level: 1,
                ["color": .string(color), "font": "Menlo", "size": 12, "tooltip": .string(detail), "href": .string(url),
                 "panelstatus": .string(state == true ? "ok" : state == false ? "issue" : "unknown")]))
        }
        lines += ["-----", refreshRow(payload, error), Runtime.item("打开 Komari 面板", level: 1, ["href": .string(address)]),
                  Runtime.item("刷新监控数据", level: 1, act("refresh-monitor"))]
        return lines
    }

    // MARK: Sessions

    func sessionName(_ session: JSONObject) -> String {
        session["title"].flatMap { $0.isTruthy ? Py.str($0) : nil } ?? session["project"].flatMap { $0.isTruthy ? Py.str($0) : nil } ?? "未知项目"
    }

    /// An MCP tool by its own name: mcp__<server>__update_plan reads as update_plan.
    func toolName(_ name: String) -> String {
        guard name.hasPrefix("mcp__"), let last = name.range(of: "__", options: .backwards) else { return name }
        let short = String(name[last.upperBound...])
        return short.isEmpty ? name : short
    }

    /// A turn that sent no request (stopped before the model answered) is left out.
    public func turnLines(_ turns: [JSONObject], unit: JSON, project: String?) -> [String] {
        let turns = turns.filter { Py.num($0["requests"]) != 0 }
        if turns.isEmpty { return [] }
        let symbol = unit.object?["symbol"]?.string ?? "$", rate = unit.object?["rate"]?.number ?? 1
        var lines = [Runtime.item([project, "最近 \(turns.count) 轮"].compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: " · "), level: 2,
                                  ["color": .string(Runtime.mutedColor), "size": 11])]
        for turn in turns {
            let when = turn["at"].flatMap { $0.isTruthy ? $0.number : nil }.map { Runtime.format($0, .current, "%H:%M") } ?? "更早开始"
            let requests = Py.num(turn["requests"]), unpriced = Py.num(turn["unpriced"])
            let cost = requests > unpriced ? (unpriced != 0 ? "已知 " : "") + symbol + String(format: "%.2f", Py.num(turn["cost"]) * rate) : "费用待定价"
            var named: [String: Double] = [:]
            for (name, count) in turn["tools"]?.object ?? JSONObject() { named[toolName(name), default: 0] += Py.num(count) }
            let tools = named.map { ($0.key, $0.value) }
                .sorted { $0.1 != $1.1 ? $0.1 > $1.1 : ClaudeUsage.pythonLess($0.0, $1.0) }.prefix(4).map { "\($0.0) ×\(Int($0.1))" }.joined(separator: " · ")
            let detail = "\(Int(requests)) 次请求" + (tools.isEmpty ? " · 未调用工具" : " · " + tools) + (unpriced != 0 ? " · \(Int(unpriced)) 次未定价" : "")
            lines.append(Runtime.item("\(when) · \(Runtime.fmtTokens(turn["tokens"])) Token · \(cost)", level: 2, ["paneldetail": .string(detail)]))
        }
        return lines
    }

    func sessionsLines(_ data: JSONObject, unit: JSON) -> [String] {
        let found = object(data, "sessions")["sessions"]?.array?.compactMap(\.object)
        let error = failure(data, "sessions")
        let badge: String
        if let found {
            let running = found.filter { $0["state"] == .string("running") }.count, waiting = found.count - running
            badge = [running > 0 ? "\(running) 运行中" : "", waiting > 0 ? "\(waiting) 等待输入" : ""].filter { !$0.isEmpty }.joined(separator: " · ").nonEmpty ?? "没有活跃会话"
        } else { badge = Runtime.refreshFailed(error) ? "读取失败" : "待同步" }
        var lines = [Runtime.item("活跃会话", ["badge": .string(badge), "sfimage": "terminal"])]
        for session in found ?? [] {
            let ago = Swift.max(0, Int(Date().timeIntervalSince1970 - (session["at"]?.number ?? 0)))
            let model = session["model"].flatMap { $0.isTruthy ? Runtime.modelTitle(Runtime.modelKey($0)) : nil }.map { $0.hasPrefix("Claude ") ? String($0.dropFirst(7)) : $0 } ?? ""
            let context = session["context"].flatMap { $0.isNull ? nil : $0.number }.map { "上下文 \(Int(($0 * 100).rounded(.toNearestOrEven)))%" } ?? ""
            let detail = [model, Py.str(Py.or(session["effort"], .string(""))), context, ago < 60 ? "刚刚" : "\(ago / 60) 分钟前"].filter { !$0.isEmpty }.joined(separator: " · ")
            let running = session["state"] == .string("running")
            lines.append(Runtime.item(sessionName(session), level: 1, ["badge": .string(running ? "运行中" : "等待输入"), "paneldetail": .string(detail),
                                                                       "panelstatus": .string(running ? "new" : "ok")]))
            lines += turnLines((session["turns"]?.array ?? []).compactMap(\.object), unit: unit,
                               project: session["title"]?.isTruthy == true ? session["project"].flatMap { $0.isTruthy ? Py.str($0) : nil } : nil)
        }
        if let found, found.isEmpty { lines.append(Runtime.item("最近半小时没有 Claude Code 或 Codex 会话", level: 1, ["color": .string(Runtime.mutedColor), "size": 11])) }
        lines.append("-----")
        let on = settings()["sessionAlerts"]?.isTruthy == true
        lines.append(Runtime.item("等待输入时提醒", level: 1, Runtime.merged(["sfimage": "bell", "checked": .bool(on), "paneldetail": "运行中的会话这一轮完成时发通知"],
                                                                         act("set-session-alerts", on ? "false" : "true"))))
        return lines
    }

    // MARK: Updates

    public func updateRows(_ data: JSONObject) -> [JSONObject] {
        let payload = data["app_updates"]?.object ?? readCache("app_updates")["payload"]?.object ?? JSONObject()
        let selected = selectedUpdateIDs()
        var rows = (payload["apps"]?.array ?? []).compactMap(\.object).filter { $0["id"]?.string.map(selected.contains) ?? false }
        let badge = data["badge"]?.object ?? readCache("badge")["payload"]?.object ?? JSONObject()
        let current = Py.or(object(data, "healthz")["version"], badge["currentVersion"] ?? .null)
        var oc: JSONObject = ["id": "opencodex", "name": "OpenCodex", "href": .string(Runtime.updateLinks["opencodex"]!), "current": current,
                              "latest": badge["latestVersion"] ?? .null, "checkedAt": readCache("badge")["fetchedAt"] ?? .int(0)]
        oc["available"] = .bool(badge["updateAvailable"]?.isTruthy == true && Updates.newer(oc["latest"], current))
        oc["status"] = .string(oc["available"] == .bool(true) ? "有新版本" : (current.isTruthy && oc["latest"]?.isTruthy == true ? "已是最新" : "等待检查"))
        let state = autoUpdateState()
        oc["automatic"] = .bool(desktop?.localProxy(base) ?? true)
        if oc["automatic"] != .bool(true) {}
        else if state["status"] == .string("running") || (oc["available"] == .bool(true) && state["target"] == oc["latest"]) {
            oc["status"] = Py.or(state["message"], .string("等待自动更新"))
            oc["needsAttention"] = .bool(state["status"] == .string("failed") || state["status"] == .string("blocked"))
        } else if oc["available"] == .bool(true) { oc["status"] = "等待自动更新" }
        else { oc["status"] = .string("自动更新已开启 · " + Py.str(oc["status"]!)) }
        // Re-read the local app versions so an update clears its alert at once.
        for index in rows.indices {
            guard let id = rows[index]["id"]?.string, id == "claude" || id == "chatgpt" else { continue }
            let info = appInfo(id == "claude" ? "Claude" : "ChatGPT")
            guard !info.isEmpty else { continue }
            rows[index]["current"] = info["CFBundleShortVersionString"]
            if rows[index]["latest"]?.isTruthy == true {
                rows[index]["available"] = .bool(Updates.newer(rows[index]["latest"], rows[index]["current"]))
                if rows[index]["build"]?.isTruthy == true { rows[index]["available"] = .bool(Updates.newer(rows[index]["build"], info["CFBundleVersion"])) }
                if !Py.str(rows[index]["status"] ?? .string("")).contains("失败") { rows[index]["status"] = .string(rows[index]["available"] == .bool(true) ? "有新版本" : "已是最新") }
            }
            if id == "chatgpt" { rows[index] = applyChatGPTRelease(rows[index], info) }
        }
        let existing = Set(rows.compactMap { $0["id"]?.string })
        for key in selected where key != "opencodex" && !existing.contains(key) {
            rows.append(["id": .string(key), "name": .string(Runtime.updateNames[key]!), "href": .string(Runtime.updateLinks[key]!), "status": "等待检查", "available": false])
        }
        for index in rows.indices {
            let id = rows[index]["id"]?.string ?? ""
            if desktop != nil && (id == "claude" || id == "chatgpt") { rows[index]["status"] = "在 App 内检查更新"; rows[index]["available"] = false; rows[index]["manual"] = true }
            let app = updateAppPath(id)
            if let app { rows[index]["href"] = .string(fileHref(app)); rows[index]["app"] = true }
            if Runtime.manualUpdates.contains(id) {
                rows[index]["status"] = .string(app != nil ? "在 App 内检查更新" : "在官网查看更新"); rows[index]["available"] = false; rows[index]["manual"] = true
            }
        }
        var byID: [String: JSONObject] = ["opencodex": oc]
        for row in rows { if let id = row["id"]?.string { byID[id] = row } }
        return selected.compactMap { byID[$0] }
    }

    public static func updateAttentionCount(_ rows: [JSONObject]) -> Int {
        rows.filter { ($0["automatic"]?.isTruthy == true ? $0["needsAttention"] : $0["available"])?.isTruthy == true }.count
    }

    static func updateIndicator(_ row: JSONObject) -> String {
        let status = Py.str(row["status"] ?? .string(""))
        if row["needsAttention"]?.isTruthy == true || status.contains("失败") { return "issue" }
        if row["available"]?.isTruthy == true { return "new" }
        if status.hasSuffix("已是最新") { return "ok" }
        return "unknown"
    }

    func latestCLILog(_ keys: [String]) -> String? {
        let root = stateDir + "/logs/updates"
        var best: (Double, String)?
        for key in keys where key == "claude-code" || key == "codex-cli" {
            for name in (try? FileManager.default.contentsOfDirectory(atPath: root)) ?? [] where name.hasPrefix(key + "-") && name.hasSuffix(".log") {
                let path = root + "/" + name
                guard (try? FileManager.default.destinationOfSymbolicLink(atPath: path)) == nil, let state = ClaudeUsage.state(of: path), !Harness.isDirectory(path) else { continue }
                let mtime = Double(state.mtime) / 1e9
                if best == nil || mtime > best!.0 { best = (mtime, path) }
            }
        }
        return best?.1
    }

    func appUpdateLines(_ data: JSONObject) -> [String] {
        let rows = updateRows(data).enumerated().sorted { ($0.element["available"]?.isTruthy == true ? 0 : 1, $0.offset) < ($1.element["available"]?.isTruthy == true ? 0 : 1, $1.offset) }.map(\.element)
        let count = Runtime.updateAttentionCount(rows)
        let running = rows.contains { $0["id"] == .string("opencodex") } && autoUpdateState()["status"] == .string("running")
        let status: JSON = count > 0 ? .string("\(count) 项待更新") : running ? "自动更新中" : .null
        var lines = [Runtime.item("Harness 更新", ["badge": status, "panelattention": .bool(count > 0), "sfimage": "arrow.down.circle"])]
        if rows.isEmpty { lines.append(Runtime.item("尚未勾选 Harness", level: 1, ["color": .string(Runtime.mutedColor), "size": 11])) }
        for row in rows {
            let id = Py.str(row["id"] ?? .null)
            let label = Py.str(row["name"] ?? .null) + " · " + Py.str(row["status"] ?? .string("")).replacingOccurrences(of: "自动更新已开启 · ", with: "")
            var detail = row["manual"]?.isTruthy == true ? "尚未接入自动版本检查" : Py.str(Py.or(row["current"], .string("版本待确认")))
            if row["available"]?.isTruthy == true { detail += " → " + Py.str(row["latest"] ?? .null) }
            if let channel = row["channel"], channel.isTruthy { detail += " · " + Py.str(channel) }
            if let reason = row["reason"], reason.isTruthy { detail += "\n" + Py.str(reason) }
            if row["available"]?.isTruthy == true && (id == "claude-code" || id == "codex-cli") {
                detail += "\n点击运行 " + (id == "claude-code" ? "claude update" : "codex update")
                lines.append(Runtime.item(label, level: 1, Runtime.merged(["paneldetail": .string(detail), "panelstatus": .string(Runtime.updateIndicator(row))], act("update-cli", id))))
            } else {
                if row["available"]?.isTruthy == true && row["app"]?.isTruthy == true { detail += "\n点击打开 App 更新" }
                lines.append(Runtime.item(label, level: 1, ["href": row["href"] ?? .null, "paneldetail": .string(detail), "panelstatus": .string(Runtime.updateIndicator(row))]))
            }
        }
        lines.append("-----")
        if rows.contains(where: { $0["manual"]?.isTruthy != true }) { lines.append(Runtime.item("立即检查更新", level: 1, Runtime.merged(["sfimage": "arrow.clockwise"], act("check-app-updates")))) }
        if latestCLILog(rows.compactMap { $0["id"]?.string }) != nil { lines.append(Runtime.item("查看最近 CLI 更新日志", level: 1, Runtime.merged(["sfimage": "doc.text"], act("open-cli-update-log")))) }
        lines.append(Runtime.item("选择 Harness…", level: 1, Runtime.merged(["sfimage": "square.grid.2x2"], act("panel-modules"))))
        return lines
    }

    static func versionTuple(_ value: JSON?) -> [Int]? {
        var text = Py.str(Py.or(value, .string(""))).trimmingCharacters(in: .whitespacesAndNewlines)
        if text.hasPrefix("v") { text.removeFirst() }
        let parts = text.split(separator: ".", omittingEmptySubsequences: false)
        guard parts.count == 3, parts.allSatisfy({ !$0.isEmpty && $0.allSatisfy { $0.isASCII && $0.isNumber } }) else { return nil }
        return parts.map { Int($0) ?? 0 }
    }

    func pluginUpdate(_ data: JSONObject) -> JSONObject? {
        let release = object(data, "self_update")
        guard let latest = Runtime.versionTuple(release["latest"]), let current = Runtime.versionTuple(.string(Runtime.version)), current.lexicographicallyPrecedes(latest) else { return nil }
        return release
    }

    func selfUpdateState() -> JSONObject {
        let state = ClaudeSubscription.readState(stateDir + "/self-update.json")
        guard let at = state["at"], at.bool == nil, let moment = at.number else { return JSONObject() }
        // A run that never reported back within 15 minutes is treated as over.
        if state["status"] == .string("running") && Date().timeIntervalSince1970 - moment > 900 { return JSONObject() }
        return state
    }

    func updateAssetName(_ version: String) -> String {
        macApp ? "token-monitor-\(version)-macos-\(Updates.machine == "arm64" ? "apple-silicon" : "intel").dmg" : "token-monitor-\(version)\(desktop != nil ? "-windows-x64" : "").zip"
    }

    func selfUpdateLines(_ data: JSONObject) -> [String] {
        guard let release = pluginUpdate(data) else { return [] }
        let latest = Py.str(release["latest"] ?? .null), state = selfUpdateState()
        if macApp || desktop != nil {
            let asset = updateAssetName(latest)
            guard (release["assets"]?.array ?? []).contains(where: { $0["name"] == .string(asset) }) else { return [] }
            return [Runtime.item(Runtime.product, Runtime.merged(["badge": .string("可更新 " + latest), "panelattention": true], logoMark(Runtime.menuIconSize))),
                    Runtime.item("打开 GitHub 发布页", level: 1, ["href": .string("https://github.com/timecatbit/starmeter/releases/tag/v" + latest), "sfimage": "arrow.up.right"])]
        }
        let running = state["status"] == .string("running") && state["version"] == .string(latest)
        let failed = state["status"] == .string("failed") && state["version"] == .string(latest)
        let badge = running ? "正在更新" : failed ? "更新失败" : "可更新 " + latest
        var rows = [Runtime.item(Runtime.product, Runtime.merged(["badge": .string(badge), "panelattention": .bool(!failed)], logoMark(Runtime.menuIconSize))),
                    Runtime.item("当前 " + Runtime.version + " → 最新 " + latest, level: 1, ["color": .string(Runtime.mutedColor), "size": 11])]
        if running { rows.append(Runtime.item("正在下载并安装，完成后自动刷新", level: 1, ["color": .string(Runtime.mutedColor), "size": 11])) }
        else {
            if failed {
                rows.append(Runtime.item(Py.str(Py.or(state["reason"], .string("更新失败"))), level: 1,
                                         ["paneldetail": .string("日志：" + stateDir + "/logs/self-update.log"), "panelstatus": "issue"]))
            }
            rows.append(Runtime.item((failed ? "重试更新到 " : "更新到 ") + latest + "…", level: 1, Runtime.merged(["sfimage": "arrow.down.circle"], act("update-self"))))
        }
        rows.append(Runtime.item("查看更新内容", level: 1, ["href": release["page"] ?? .null, "sfimage": "arrow.up.right"]))
        return rows
    }
}

extension String {
    var nonEmpty: String? { isEmpty ? nil : self }
}
