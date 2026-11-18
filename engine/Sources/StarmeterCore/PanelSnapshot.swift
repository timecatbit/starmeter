import Foundation

extension Runtime {
    /// shlex.split in POSIX mode, for the options of one row: spaces separate, quotes group, a backslash escapes.
    static func shellSplit(_ text: String) -> [String] {
        var words: [String] = [], current = "", inWord = false
        var quote: Character?
        var characters = Array(text)[...]
        while let c = characters.popFirst() {
            if let q = quote {
                if c == q { quote = nil }
                else if c == "\\" && q == "\"", let next = characters.first, next == "\"" || next == "\\" { current.append(next); characters = characters.dropFirst() }
                else { current.append(c) }
            } else if c == "\"" || c == "'" { quote = c; inWord = true }
            else if c == "\\" { if let next = characters.popFirst() { current.append(next) }; inWord = true }
            else if c == " " || c == "\t" || c == "\n" || c == "\r" { if inWord { words.append(current); current = ""; inWord = false } }
            else { current.append(c); inWord = true }
        }
        if inWord { words.append(current) }
        return words
    }

    /// The panel's entries from menu rows: labels, nesting, options and actions.
    static func panelEntries(_ lines: [String]) -> [JSON] {
        // Entries are built as objects with child indexes, then assembled, since a parent gains children later.
        final class Node { var entry: JSONObject; var children: [Node] = []; init(_ entry: JSONObject) { self.entry = entry } }
        var roots: [Node] = [], parents: [Int: Node] = [:]
        for raw in lines where !raw.isEmpty {
            var level = 0
            var entry: JSONObject
            if raw.allSatisfy({ $0 == "-" }) {
                level = Swift.max(0, (raw.count - 3) / 2)
                entry = ["label": "", "separator": true]
            } else {
                var line = Substring(raw)
                while line.hasPrefix("--") { level += 1; line = line.dropFirst(2) }
                let parts = line.components(separatedBy: " | ")
                let label = parts[0], options = parts.count > 1 ? parts[1...].joined(separator: " | ") : ""
                var attrs: [String: String] = [:]
                for part in shellSplit(options) { if let eq = part.firstIndex(of: "=") { attrs[String(part[..<eq])] = String(part[part.index(after: eq)...]) } }
                entry = ["label": .string(label.trimmingCharacters(in: .whitespacesAndNewlines))]
                if let image = attrs["templateimage"].flatMap({ $0.isEmpty ? nil : $0 }) ?? attrs["image"].flatMap({ $0.isEmpty ? nil : $0 }) {
                    entry["image"] = .string(image); entry["template"] = .bool(!(attrs["templateimage"] ?? "").isEmpty)
                } else if let symbol = attrs["sfimage"], !symbol.isEmpty { entry["symbol"] = .string(symbol) }
                if attrs["panelsection"] == "true" { entry["section"] = true }
                if attrs["panelattention"] == "true" { entry["attention"] = true }
                if let level = attrs["panelusagelevel"], ["normal", "warning", "critical"].contains(level) { entry["usageLevel"] = .string(level) }
                if let light = attrs["panelstatus"], ["new", "ok", "issue", "unknown"].contains(light) { entry["statusLight"] = .string(light) }
                if let radio = attrs["panelradio"], radio == "on" || radio == "off" { entry["radio"] = true; entry["checked"] = .bool(radio == "on") }
                if let badge = attrs["badge"], !badge.isEmpty { entry["value"] = .string(badge) }
                if let fallback = attrs["panelfallback"], !fallback.isEmpty { entry["fallbacks"] = .array(fallback.components(separatedBy: fallbackSeparator).map(JSON.string)) }
                if let detail = attrs["paneldetail"], !detail.isEmpty { entry["detail"] = .string(detail) }
                if let href = attrs["href"], !href.isEmpty { entry["href"] = .string(href) }
                if attrs["param2"] == "--action", let name = attrs["param3"], !name.isEmpty {
                    entry["action"] = .array((3..<20).compactMap { attrs["param\($0)"] }.map(JSON.string))
                }
                if level > 0, let first = label.first, "●○?".contains(first), let tooltip = attrs["tooltip"], !tooltip.isEmpty {
                    // The panel draws the status light; the text mark is only for SwiftBar's own menu.
                    let text = String(label.dropFirst()).trimmingCharacters(in: .whitespacesAndNewlines)
                        .replacingOccurrences(of: "\\s{2,}", with: " · ", options: .regularExpression)
                    entry["label"] = .string(text.components(separatedBy: " · ").first ?? text)
                    entry["detail"] = .string(tooltip)
                }
            }
            let node = Node(entry)
            if level > 0, let parent = parents[level - 1] { parent.children.append(node) } else { roots.append(node) }
            parents[level] = node
            for key in parents.keys where key > level { parents[key] = nil }
        }
        func build(_ node: Node) -> JSON {
            var entry = node.entry
            entry["children"] = .array(node.children.map(build))
            // "children" sits where Python's dict put it: after label (and separator).
            var ordered = JSONObject()
            ordered["label"] = entry["label"]
            if let separator = entry["separator"] { ordered["separator"] = separator }
            ordered["children"] = entry["children"]
            for (key, value) in entry where ordered[key] == nil { ordered[key] = value }
            return .object(ordered)
        }
        return roots.map(build)
    }

    func resourcePanelEntries(_ data: JSONObject) -> [JSON] {
        let entries = Runtime.panelEntries(managementLines(data))
        let services = Dictionary(uniqueKeysWithValues: Catalog.services.map { ($0.label, $0) })
        let loaders = externalLoaders().map(\.0)
        var groups = Ordered<String, [JSON]>()
        for name in ["订阅", "API", "其他"] { groups[name] = [] }
        for entry in entries {
            guard var fields = entry.object, fields["separator"] == nil else { continue }
            let label = Py.str(fields["label"] ?? .null), service = services[label]
            let source = service?.key ?? (label.hasPrefix("Komari 监控") ? "komari" : label == "Harness 更新" ? "updates" : label == "活跃会话" ? "sessions"
                : label.hasPrefix("OpenCodex 管理") ? "opencodex" : nil)
            if fields["children"]?.array?.isEmpty == false, let source, !resourceCacheNames(source, loaders).isEmpty { fields["onOpenAction"] = ["refresh-source", .string(source)] }
            // Refresh sits beside Summary; SwiftBar's own submenu covers its housekeeping.
            if label == "刷新数据" { continue }
            let group = service?.category ?? "其他"
            groups[group] = (groups[group] ?? []) + [.object(fields)]
        }
        // Groups are separated by a line; their names are obvious from the rows themselves.
        var result: [JSON] = []
        for (_, items) in groups.items where !items.isEmpty {
            if !result.isEmpty { result.append(["label": "", "separator": true, "children": []]) }
            result += items
        }
        return result
    }

    func resourceCacheNames(_ source: String, _ loaders: [String]) -> [String] {
        if source == "opencodex" { return loaders.contains("opencodex") ? Runtime.opencodexEndpoints.map(\.0) : [] }
        if source == "chatgpt" && chatgptSource() == "proxy" { return loaders.contains("opencodex") ? ["healthz", "quota", "accounts", "active"] : [] }
        if source == "updates" { return ["app_updates", "badge"].filter(loaders.contains) }
        guard let name = Runtime.resourceCaches[source] else { return [] }
        return loaders.filter { $0 == name || $0.hasPrefix(source + "@") }
    }

    // MARK: Module options and their connection sidebars

    func sourceLocations(_ key: String) -> [String] {
        let root = sourcePath(key)
        if key == "claude" { return [root] + ClaudeApp.projectRoots(home: home, environment: environment) }
        if key == "workbuddy" {
            let roots = [".workbuddy-ai", ".workbuddy"].map { root + "/" + $0 + "/projects" }.filter(Harness.isDirectory)
            if !roots.isEmpty { return roots }
            if root == home { return [home + "/.workbuddy-ai/projects"] }
            if Harness.isDirectory(root + "/projects") { return [root + "/projects"] }
        }
        if key == "codex" {
            let roots = ["sessions", "archived_sessions"].map { root + "/" + $0 }.filter(Harness.isDirectory)
            if !roots.isEmpty { return roots }
        }
        if key == "gemini" && Harness.isDirectory(root + "/tmp") { return [root + "/tmp"] }
        if key == "qoder" && Harness.isDirectory(root + "/projects") { return [root + "/projects"] }
        return root.isEmpty ? [] : [Runtime.absolute(root)]
    }

    /// The file browser's name where the user will look for it.
    static var revealLabel: String {
        #if os(Windows)
        return "在资源管理器中打开"
        #else
        return "在 Finder 中打开"
        #endif
    }

    static func locationEntry(_ label: String, _ paths: [String]) -> JSON { ["label": .string(label), "detail": .string(paths.joined(separator: "\n")), "children": []] }

    func clientConnectionEntry(_ key: String) -> JSON {
        let proxy = key == "opencodex", configured = proxy ? opencodexConfigured() : connectionReady("chatgpt")
        let root = proxy ? ocxHome : codexHome()
        var children: [JSON] = []
        if !proxy {
            children.append(["label": .string("当前来源 · " + (chatgptSource() == "proxy" ? "OpenCodex 账户" : "本机 ChatGPT 登录")), "children": []])
            children.append(["label": "勾选 OpenCodex 后改用 OpenCodex 账户", "children": []])
            if chatgptSource() == "proxy" { children.append(Runtime.locationEntry("账户管理地址", [base])) }
            children.append(["label": "", "separator": true, "children": []])
        }
        children.append(Runtime.locationEntry(proxy ? "配置目录" : "本机登录目录", [root]))
        if proxy { children += [Runtime.locationEntry("管理地址", [base]), Runtime.locationEntry("管理令牌文件", [tokenFile])] }
        children.append(["label": "选择配置目录…", "symbol": "folder", "action": ["choose-client-home", .string(key)], "children": []])
        if proxy {
            children += [["label": "修改管理地址…", "action": ["set-proxy-address"], "children": []], ["label": "选择管理令牌文件…", "action": ["choose-admin-token"], "children": []]]
        }
        children.append(["label": "恢复自动读取", "action": ["reset-client-connection", .string(key)], "children": []])
        if Harness.isDirectory(root) { children.append(["label": .string(Runtime.revealLabel), "symbol": "folder", "action": ["reveal-client-home", .string(key)], "children": []]) }
        return ["label": .string(proxy ? "OpenCodex" : "ChatGPT 订阅"), "value": .string(configured ? "已配置" : "未配置"), "symbol": .string(proxy ? "network" : "person.crop.circle"),
                "children": .array(children)]
    }

    func cursorConnectionEntry(_ installed: Bool) -> JSON {
        let enabled = settings()["cursorUsage"]?.isTruthy == true
        var children: [JSON] = [["label": "读取 Cursor 用量记录", "detail": "用本机 Cursor 登录查询 cursor.com，令牌只在内存中使用", "checked": .bool(enabled),
                                 "action": ["set-cursor-usage", .string(enabled ? "false" : "true")], "children": []],
                                ["label": "来源 · Cursor 桌面版与 Cursor CLI", "children": []], ["label": "两者同一账户，用量都在账户记录里", "children": []],
                                Runtime.locationEntry("登录来源", [sourcePath("cursor") + "/state.vscdb", Cursor.cliAuth(home: home, environment: environment)])]
        if let app = updateAppPath("cursor") { children.append(["label": "打开 Cursor", "symbol": "macwindow", "href": .string(fileHref(app)), "children": []]) }
        return ["label": "Cursor", "value": .string(enabled ? "已配置" : "未配置"), "symbol": "key", "children": .array(children)]
    }

    func serviceConnectionEntry(_ key: String, _ data: JSONObject) -> JSON {
        let service = Catalog.byKey[key], configured = connectionReady(key)
        let value: JSON = .string(configured ? "已配置" : "未配置")
        if let service, service.category == "API" {
            // The same key list as the homepage, plus pointing the classic connection at a key file.
            var children = Runtime.panelEntries(apiLines(key, data)).first?.object?["children"]?.array ?? []
            children.insert(["label": "选择密钥文件…", "action": ["configure-connection", .string(key)], "children": []], at: Swift.max(0, children.count - 1))
            if !configured, let note = Catalog.setupNotes[key] { children.insert(["label": .string(note), "children": []], at: 0) }
            return ["label": .string(service.label), "value": value, "symbol": "key", "children": .array(children)]
        }
        let settings = self.settings()
        if key == "claude-subscription", let service {
            let oauth = settings["claudeOAuthUsage"]?.isTruthy == true, local = ClaudeSubscription.localEnabled(stateDir)
            let oauthRow: JSON = ["label": "Claude Code 登录查询", "checked": .bool(oauth), "action": ["set-claude-oauth", .string(oauth ? "false" : "true")], "children": []]
            let appSnapshot = Runtime.locationEntry("App 快照文件", [sourcePath("claude") + "/plan-usage-history.json"])
            var children: [JSON]
            #if os(Windows)
            children = [oauthRow, Runtime.locationEntry("Code 登录目录", [claudeConfigRoot()]), appSnapshot,
                        ["label": "选择 Claude 数据目录…", "action": ["choose-source", "claude"], "children": []], ["label": "打开订阅用量", "href": .string(service.url), "children": []]]
            #else
            children = [oauthRow, Runtime.locationEntry("Code 登录来源", [claudeConfigRoot()]),
                        ["label": .string(local ? "关闭本机回传" : "启用本机用量回传…"), "action": ["set-claude-local", .string(local ? "false" : "true")], "children": []],
                        local ? Runtime.locationEntry("本机快照", [stateDir + "/" + ClaudeSubscription.snapshotFile]) : appSnapshot]
            if settings["claudeLiveUsage"]?.isTruthy == true { children.append(["label": "清除在线查询授权", "action": ["set-claude-live", "false"], "children": []]) }
            children += [["label": "选择 Claude 数据目录…", "action": ["choose-source", "claude"], "children": []], ["label": "打开 Claude", "action": ["open-claude"], "children": []],
                         ["label": "打开订阅用量", "href": .string(service.url), "children": []]]
            #endif
            return ["label": .string(service.label), "value": value, "symbol": "person.crop.circle", "children": .array(children)]
        }
        var children: [JSON]
        if Catalog.manualSubscriptions.contains(key) {
            children = [["label": "手动标记，未验证套餐", "children": []]]
            if configured { children.append(Runtime.locationEntry("本机标记文件", [stateDir + "/preferences.json"])) }
            children += [["label": .string(configured ? "清除配置标记" : "标记为已配置"), "action": ["set-subscription", .string(key), .string(configured ? "false" : "true")], "children": []],
                         ["label": "打开订阅管理", "symbol": "arrow.up.right", "href": .string(service!.url), "children": []]]
        } else if key == "minimax" {
            let file = environment["MINIMAX_API_KEY_FILE"] ?? ""
            children = configured && !file.isEmpty ? [Runtime.locationEntry("密钥文件", [Runtime.absolute(Paths.expandUser(file))])] : []
            if !configured, let note = Catalog.setupNotes[key] { children.append(["label": .string(note), "children": []]) }
            children.append(["label": "选择密钥文件…", "action": ["configure-connection", .string(key)], "children": []])
            if configured { children.append(["label": "清除连接设置…", "action": ["clear-connection", .string(key)], "children": []]) }
            children.append(["label": "打开用量与账单", "symbol": "arrow.up.right", "href": .string(service!.url), "children": []])
        } else if key == "qoder-subscription" {
            // Read from the Qoder CN app's own log, nothing to set up: where it reads, and how to bring it up to date.
            let roots = Qoder.logRoots(home: home, environment: environment), found = roots.filter(Harness.isDirectory)
            children = [["label": "来源 · Qoder 桌面 App 的本机日志", "children": []],
                        ["label": "在 Qoder 窗口右下角打开 Credits 用量后更新", "children": []],
                        Runtime.locationEntry("日志目录", found.isEmpty ? [roots[0]] : found)]
            if let app = updateAppPath("qoder") { children.append(["label": "打开 Qoder", "symbol": "macwindow", "href": .string(fileHref(app)), "children": []]) }
            children.append(["label": "打开订阅用量", "symbol": "arrow.up.right", "href": .string(service!.url), "children": []])
        } else {
            children = configured ? [Runtime.locationEntry("面板地址", [komariBaseURL])] : []
            children.append(["label": "设置面板地址…", "action": ["configure-connection", .string(key)], "children": []])
            if configured { children.append(["label": "清除连接设置…", "action": ["clear-connection", .string(key)], "children": []]) }
        }
        let symbol = key == "komari" ? "server.rack" : service?.category == "订阅" ? "person.crop.circle" : "key"
        return ["label": .string(service?.label ?? "Komari 监控"), "value": value, "symbol": .string(symbol), "children": .array(children)]
    }

    /// Claude Code's configuration folder: the first of CLAUDE_CONFIG_DIR, else ~/.claude.
    func claudeConfigRoot() -> String {
        let raw = (environment["CLAUDE_CONFIG_DIR"] ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        if raw.isEmpty { return Paths.home + "/.claude" }
        let first = Paths.expandUser(String(raw.split(separator: ",", omittingEmptySubsequences: false)[0]))
        return Paths.resolve(first) ?? Runtime.absolute(first)
    }

    func moduleConnections(_ data: JSONObject) -> [String: JSON] {
        let scope = SHA256.hex(Array(Py.dumps(.array(Preferences.usage.map { [.string($0), .string(sourcePath($0))] })).utf8)).prefix(12)
        let (detected, _) = cachedFetch("configured-sources-" + scope, ttl: 300) { (["modules": .array(detectedModules().map(JSON.string))], nil) }
        let found = Set(detected?["modules"]?.array?.compactMap(\.string) ?? [])
        var connections: [String: JSON] = [:]
        let paths = settings()["paths"]?.object ?? JSONObject()
        for (key, label, _) in Preferences.modules {
            if key == "cursor" { connections[key] = cursorConnectionEntry(found.contains(key)); continue }
            guard Preferences.usage.contains(key) && key != "opencodex" else { continue }
            let custom = paths[key == "claude" ? "claude-app" : key]?.isTruthy == true
            var configured = found.contains(key) || (custom && FileManager.default.fileExists(atPath: sourcePath(key)))
            if key == "deepseek-harness" { configured = Harness.isDirectory(sourcePath(key)) }
            var children: [JSON] = [["label": .string(configured ? (custom ? "来源 · 自定记录目录" : "来源 · 自动读取本机客户端") : "未发现客户端或用量记录"), "children": []],
                                    ["label": "选择记录目录…", "action": ["choose-source", .string(key)], "children": []],
                                    ["label": "恢复自动查找", "action": ["auto-source", .string(key)], "children": []]]
            if configured {
                let locations = sourceLocations(key)
                let exists = locations.contains { FileManager.default.fileExists(atPath: $0) }
                if !locations.isEmpty { children.insert(Runtime.locationEntry(exists ? "读取目录" : "默认目录 · 尚未生成", locations), at: 1) }
                if exists { children.insert(["label": .string(Runtime.revealLabel), "symbol": "folder", "action": ["reveal-source", .string(key)], "children": []], at: 2) }
            }
            if key == "magpie" {
                children[0] = ["label": .string(configured ? "来源 · Magpie 本机网关记录" : "未发现 Magpie 或网关记录"), "children": []]
                if configured { children.insert(["label": "只读取 usage.jsonl", "children": []], at: 1) }
                if let app = updateAppPath(key) { children.append(["label": "打开 Magpie", "symbol": "macwindow", "href": .string(fileHref(app)), "children": []]) }
            }
            if key == "codex" {
                children[0] = ["label": "来源 · 编程客户端的本机记录", "children": []]
                children.insert(["label": "仅编程会话，不含普通聊天", "children": []], at: 1)
            }
            if key == "claude" {
                children[0] = ["label": "来源 · Claude 桌面 App 与 Claude Code", "children": []]
                children.insert(["label": "含 VS Code、JetBrains 与终端里的会话", "children": []], at: 1)
                children = children.map { child in
                    guard child.object?["action"] == ["choose-source", "claude"], var fields = child.object else { return child }
                    fields["label"] = "选择 Claude 数据目录…"
                    return .object(fields)
                }
                children.append(["label": "打开 Claude", "symbol": "macwindow", "action": ["open-claude"], "children": []])
            }
            connections[key] = ["label": .string(label), "value": .string(configured ? "已配置" : "未配置"), "symbol": "folder", "children": .array(children)]
        }
        for service in Catalog.services {
            if service.key == "chatgpt" { connections[service.key] = clientConnectionEntry("chatgpt") }
            else if service.key == "cursor-subscription" {
                var entry = cursorConnectionEntry(true).object!
                entry["label"] = .string(service.label)
                connections[service.key] = .object(entry)
            } else { connections[service.key] = serviceConnectionEntry(service.key, data) }
        }
        connections["opencodex"] = clientConnectionEntry("opencodex")
        connections["komari"] = serviceConnectionEntry("komari", data)
        return connections
    }

    static let detectionHelpEntries: [JSON] = [["label": "确认后覆盖当前模块选择；不读取对话正文，不显示 Key", "children": []], ["label": "", "separator": true, "children": []],
                                               ["label": "开始检测…", "symbol": "sparkle.magnifyingglass", "action": ["detect-modules"], "children": []]]

    static let menuBarLabels: [(String, String)] = [("none", "只显示星星"), ("cost", "今天的 API 估算"), ("quota", "最紧张的订阅额度")]

    public func moduleOptions(_ data: JSONObject) -> [JSON] {
        let settings = self.settings(), connections = moduleConnections(data)
        func option(_ key: String) -> JSON { settings[key] ?? .null }
        var rows: [JSON] = [("system", "跟随系统"), ("light", "浅色"), ("dark", "深色")].map {
            ["label": .string($0.1), "checked": .bool(option("theme") == .string($0.0)), "radio": true, "action": ["set-theme", .string($0.0)], "children": []] }
        rows.append(["label": "", "separator": true, "children": []])
        rows += [("glass", "玻璃材质"), ("solid", "实色背景")].map {
            ["label": .string($0.1), "checked": .bool(option("appearance") == .string($0.0)), "radio": true, "action": ["set-appearance", .string($0.0)], "children": []] }
        let level = option("usageAlert")
        var alertRows: [JSON] = (Preferences.alertLevels.dropFirst() + [0]).map { value in
            ["label": .string(value != 0 ? "\(value)%" : "关闭"), "checked": .bool(level == .int(value)), "radio": true, "action": ["set-usage-alert", .string(String(value))], "children": []] }
        let reminder = option("resetReminder").isTruthy
        alertRows += [["label": "", "separator": true, "children": []],
                      ["label": "重置前提醒", "detail": .string("每周窗口用了不到 \(Alerts.reminderUnder)%，重置前 \(Int(Alerts.reminderLead) / 3600) 小时提醒"), "checked": .bool(reminder),
                       "action": ["set-reset-reminder", .string(reminder ? "false" : "true")], "children": []]]
        let cny = option("currency") == .string("CNY"), rate = option("cnyRate")
        let pin = Int(option("pinInterval").number ?? 2), menuBar = option("menuBar").string ?? "none", alert = option("usageAlert").number ?? 0
        let service = option("serviceStatus").isTruthy, keep = option("keepCleanedUsage").isTruthy
        // Grouped by what they are for, each group under a small title: what the panel shows, pinning and alerts, the
        // data kept and exported, and the module setup.
        rows += [["label": "显示", "section": true, "children": []],
                 ["label": "金额显示", "symbol": .string(cny ? "yensign.circle" : "dollarsign.circle"), "value": .string(cny ? "人民币" : "美元"), "children": [
                    ["label": "美元（$）", "checked": .bool(option("currency") == .string("USD")), "radio": true, "action": ["set-currency", "USD"], "children": []],
                    ["label": "人民币（¥）", "checked": .bool(cny), "radio": true, "action": ["set-currency", "CNY"], "children": []],
                    ["label": "", "separator": true, "children": []],
                    ["label": "汇率…", "value": .string(rate.isTruthy ? String(format: "%.4f · 手动", rate.number ?? 0) : "每天自动更新"),
                     "detail": "API 等值估算按这个汇率换成人民币；订阅月费与 API 实际花费仍是美元", "action": ["set-cny-rate"], "children": []]]],
                 ["label": "菜单栏显示", "symbol": "menubar.rectangle", "value": .string(Runtime.menuBarLabels.first { $0.0 == menuBar }?.1 ?? ""), "children": .array(Runtime.menuBarLabels.map { key, label in
                    ["label": .string(label), "checked": .bool(menuBar == key), "radio": true, "action": ["set-menu-bar", .string(key)], "children": []] })],
                 ["label": "钉住与提醒", "section": true, "children": []],
                 ["label": "钉住时刷新", "symbol": "pin", "value": .string("每 \(pin) 秒"), "children": .array(Preferences.pinIntervals.map { seconds in
                    ["label": .string("每 \(seconds) 秒"), "checked": .bool(option("pinInterval") == .int(seconds)), "radio": true, "action": ["set-pin-interval", .string(String(seconds))], "children": []] })],
                 ["label": "用量提醒", "detail": "订阅窗口用到所选比例时通知一次", "symbol": "bell", "value": .string(alert != 0 ? "\(Int(alert))%" : "关闭"), "children": .array(alertRows)],
                 ["label": "服务状态", "detail": "每 5 分钟查一次 Claude、OpenAI、Cursor 的官方状态页，出问题时标在卡片上", "symbol": "waveform.path.ecg", "checked": .bool(service),
                  "action": ["set-service-status", .string(service ? "false" : "true")], "children": []],
                 ["label": "数据", "section": true, "children": []],
                 ["label": "保留已清理记录的用量", "detail": "工具删掉旧记录后仍计入“全部”等统计", "symbol": "archivebox", "checked": .bool(keep),
                  "action": ["set-keep-usage", .string(keep ? "false" : "true")], "children": []],
                 ["label": "导出用量", "symbol": "square.and.arrow.up", "value": .string(Runtime.ranges.first { $0.0 == selectedRange() }!.1), "children": [
                    ["label": "CSV（每天每个工具一行）…", "action": ["export-usage", "csv"], "children": []], ["label": "JSON…", "action": ["export-usage", "json"], "children": []]]],
                 ["label": "模块与配置", "section": true, "children": []],
                 ["label": "自动检测", "symbol": "sparkle.magnifyingglass", "children": .array(Runtime.detectionHelpEntries)],
                 ["label": "高级配置文件…", "symbol": "doc.text", "action": ["open-settings"], "children": []],
                 ["label": "恢复默认模块", "action": ["reset-modules"], "children": []]]
        let collapse = option("allowCollapse").isTruthy
        var options: [JSON] = [["label": "允许折叠", "detail": "各模块独立展开", "symbol": "rectangle.compress.vertical", "checked": .bool(collapse),
                                "action": ["set-collapse", .string(collapse ? "false" : "true")], "children": []],
                               ["label": "外观与设置", "symbol": "slider.horizontal.3", "children": .array(rows)], ["label": "", "separator": true, "children": []]]
        var group: String?
        let visible = settings["visible"]?.array ?? []
        for (key, label, category) in Preferences.modules {
            if category != group { options.append(["label": .string(Catalog.groupLabels[category] ?? category), "section": true, "children": []]); group = category }
            let enabled = visible.contains(.string(key))
            let connection = connections[key]?.object ?? JSONObject()
            var row: JSONObject = ["label": .string(label), "checked": .bool(enabled), "action": ["set-module", .string(key), .string(enabled ? "false" : "true")],
                                   "children": connection["children"] ?? []]
            for (field, value) in moduleIcon(key) { row[field] = value }
            if let value = connection["value"], value.isTruthy { row["value"] = value }
            options.append(.object(row))
        }
        return options
    }

    public func rangeOptions(_ key: String? = nil) -> [JSON] {
        let chosen = key ?? selectedRange()
        return Runtime.ranges.map { ["label": .string($0.1), "action": ["set-range", .string($0.0)], "checked": .bool($0.0 == chosen), "children": []] }
    }

    func presentationSettings() -> JSON {
        let settings = self.settings()
        return .object(JSONObject(["revision", "visible", "appearance", "theme", "allowCollapse", "expandedProviders"].map { ($0, settings[$0] ?? .null) }))
    }

    /// The figure beside the star, or "": today's estimate or the tightest subscription window, never a state.
    public func menuBarValue(_ data: JSONObject?) -> String {
        let choice = settings()["menuBar"]?.string ?? "none"
        if choice == "cost" {
            let bundle = readCache(rangeCacheName("today"))["payload"]?.object ?? JSONObject()
            guard bundle["range"] == .string("today") else { return "" }
            let selected = Preferences.modules.filter { Preferences.usage.contains($0.key) && moduleEnabled($0.key) }.map { ($0.key, $0.label, bundle[$0.key]?.object ?? JSONObject()) }
            let parts = Runtime.summaryUsage(selected)
            let costs = selected.compactMap { key, _, _ -> Double? in
                guard let usage = parts[key]?["usage"]?.object, usage["summary"]?.isTruthy == true, usage["error"]?.isTruthy != true,
                      let value = Runtime.summaryCostValue(usage["summary"]?.object) else { return nil }
                return value.pythonFloat
            }
            if costs.isEmpty { return "" }
            let unit = currencyUnit(data ?? JSONObject()).object
            let total = costs.reduce(0, +) * (unit?["rate"]?.pythonFloat ?? 1)
            let symbol = unit?["symbol"]?.string ?? "$"
            return symbol + (total < 100 ? String(format: "%.2f", total) : Runtime.grouped(Int64(total.rounded(.toNearestOrEven))))
        }
        if choice == "quota", let data {
            let used = alertWindows(data).filter { $0["current"]?.isTruthy == true }.compactMap { $0["used"]?.number }
            return used.max().map { "\(Int($0.rounded(.toNearestOrEven)))%" } ?? ""
        }
        return ""
    }

    /// format(n, ','): thousands separated by commas.
    static func grouped(_ value: Int64) -> String {
        let digits = String(value.magnitude)
        var out = ""
        for (index, digit) in digits.enumerated() {
            if index > 0 && (digits.count - index) % 3 == 0 { out.append(",") }
            out.append(digit)
        }
        return (value < 0 ? "-" : "") + out
    }

    /// Everything the panel shows, in the file the panel reads.
    public func panelSnapshot(_ data: JSONObject, dashboard given: JSONObject? = nil, options chosen: [JSON]? = nil, now: Double = Date().timeIntervalSince1970) -> JSONObject {
        let dashboard = given ?? dashboardPayload(data, rangeError: data["range_usage_error"]?.string, now: now)
        var options = chosen ?? moduleOptions(data)
        if macApp {
            func withoutNewBridge(_ entries: [JSON]) -> [JSON] {
                entries.filter { $0.object?["action"] != ["set-claude-local", "true"] }.map { entry in
                    var fields = entry.object!
                    fields["children"] = .array(withoutNewBridge(fields["children"]?.array ?? []))
                    return .object(fields)
                }
            }
            options = withoutNewBridge(options)
        }
        let settings = self.settings()
        return ["schema": 1, "project": ["name": .string(Runtime.product), "url": "https://github.com/timecatbit/starmeter"], "dashboard": .object(dashboard),
                "menuBarText": .string(menuBarValue(data)), "pinInterval": settings["pinInterval"] ?? 2, "entries": .array(resourcePanelEntries(data)),
                "rangeOptions": .array(rangeOptions(dashboard["rangeKey"]?.string)), "moduleOptions": .array(options),
                "appearance": settings["appearance"] ?? "glass", "theme": settings["theme"] ?? "system", "preferences": presentationSettings(),
                "updateCount": .int(Int64(Runtime.updateAttentionCount(updateRows(data))))]
    }
}
