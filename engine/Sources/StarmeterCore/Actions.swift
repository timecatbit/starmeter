import Foundation

/// What the menu's and the panel's rows do. Port of run_action and consume_panel_action in swiftbar_plugin.py.
extension Runtime {
    struct Invalid: Error, CustomStringConvertible {
        let description: String
        init(_ description: String) { self.description = description }
    }

    // MARK: Background work

    /// Runs the entry again, detached, for a background mode; the standalone app and the tray queue their own.
    public func launchBackground(_ mode: String) {
        if macApp { return }
        if let desktop { desktop.requestRefresh(mode); return }
        Shell.detach(["/bin/bash", "-p", plugin, mode])
    }

    public func scheduleBackgroundRefresh() {
        if let desktop { desktop.requestRefresh("--background-refresh"); return }
        if externalLoaders().contains(where: { loaderDue($0.0) }) || !usageRangesDue().isEmpty { launchBackground("--background-refresh") }
    }

    func refreshResource(_ source: String) throws {
        guard Runtime.resourceCaches[source] != nil, moduleEnabled(source) else { throw Invalid("invalid resource") }
        let names = resourceCacheNames(source, externalLoaders().map(\.0))
        if names.isEmpty { return }
        var checks = readCache("resource-checks")
        let now = Date().timeIntervalSince1970
        if let last = checks[source]?.number, 0 <= now - last, now - last < 5 { return }
        checks[source] = .double(now); checks["schema"] = 1
        writeJSONCache(cachePath("resource-checks"), .object(checks))
        invalidateCaches(names)
        scheduleBackgroundRefresh()
    }

    static let connectionOrder = ["app_updates", "claude_subscription", "qoder_subscription", "cursor_subscription", "chatgpt", "komari", "provider_quotas",
                                  "grok", "minimax", "kimi-api", "qoder-api", "claude-api"]

    /// A stable cache folder: data from an old connection is dropped when its settings change.
    public func syncConnectionCache() {
        let previous = readCache("connection-fingerprints")
        var current: JSONObject = ["schema": 1]
        for name in Runtime.connectionOrder {
            let fingerprint = JSON.string(connectionFingerprint(name))
            current[name] = fingerprint
            if previous[name] != fingerprint { unlink(cachePath(name)) }
        }
        current["proxy"] = .string(connectionFingerprint("proxy"))
        if previous["proxy"] != current["proxy"] {
            for name in Runtime.opencodexEndpoints.map(\.0) + ["badge"] { unlink(cachePath(name)) }
            for name in (try? FileManager.default.contentsOfDirectory(atPath: cacheDir)) ?? [] where name.hasPrefix("range_usage_") && name.hasSuffix(".json") {
                unlink(cacheDir + "/" + name)
            }
        }
        writeJSONCache(cachePath("connection-fingerprints"), .object(current))
    }

    // MARK: Small helpers

    /// The native panel installed beside the entry.
    var panelApp: String { ((Runtime.absolute(plugin) as NSString).deletingLastPathComponent as NSString).appendingPathComponent(".opencodex-panel/OpenCodexPanel.app") }

    func namedKeyLoaderNames() -> [String] {
        Runtime.keyTTLs.filter { moduleEnabled($0.0) }.flatMap { provider, _ in namedKeys(provider).map { provider + "@" + Py.str($0["id"] ?? .null) } }
    }

    func confirm(_ message: String, _ detail: String = "", accept: String = "确定") -> Bool { host.confirm(message, detail: detail, accept: accept) }
    func dialogError(_ title: String, _ detail: String) { host.error(title, detail) }
    func textDialog(_ message: String, _ value: String = "", hidden: Bool = false, title: String = "连接设置") -> String? {
        host.textDialog(message, value: value, hidden: hidden, title: title)
    }
    func openLocal(_ path: String, text: Bool = false) throws { try host.openLocal(path, text: text) }

    func saveOverrides(_ values: [(String, String)]) throws {
        if let desktop { try desktop.saveOverrides(values) } else { try Connections.writeOverrides(stateDir, values) }
    }

    func update(_ kind: String, _ key: String? = nil, _ value: String? = nil) throws { _ = try Preferences.update(stateDir, kind: kind, key: key, value: value) }

    static func pythonG(_ value: JSON?) -> String { value?.number.map { Runtime.g($0) } ?? "" }

    func terminal(_ command: String) throws {
        let preferred = Shell.run(["/usr/bin/defaults", "read", "com.ameba.SwiftBar", "Terminal"], timeout: 5)?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        try TerminalJobs.launch(command, preferred: preferred, state: stateDir)
    }

    // MARK: OpenCodex's own commands

    func ocx(_ arguments: [String], timeout: Double = 240) -> String? {
        let binary = Shell.which("ocx", environment: environment) ?? "/opt/homebrew/bin/ocx"
        guard FileManager.default.fileExists(atPath: binary) else { return "找不到 ocx 命令" }
        let process = Process()
        process.executableURL = URL(fileURLWithPath: binary)
        process.arguments = arguments
        let output = Pipe()
        process.standardOutput = output; process.standardError = output; process.standardInput = FileHandle.nullDevice
        do { try process.run() } catch { return "找不到 ocx 命令" }
        var data = Data()
        let reader = DispatchQueue(label: "ocx")
        let done = DispatchSemaphore(value: 0)
        reader.async { data = output.fileHandleForReading.readDataToEndOfFile(); done.signal() }
        if done.wait(timeout: .now() + timeout) == .timedOut { process.terminate(); return "ocx " + arguments.joined(separator: " ") + " 执行超时" }
        process.waitUntilExit()
        if process.terminationStatus != 0 {
            let lines = String(decoding: data, as: UTF8.self).split(whereSeparator: \.isNewline).map(String.init).filter { !$0.trimmingCharacters(in: .whitespaces).isEmpty }
            return lines.last.map { Runtime.shorten($0, 160) } ?? "命令执行失败"
        }
        return nil
    }

    func cliAction(_ arguments: [String], _ message: String) {
        let failure = ocx(arguments), label = "ocx " + arguments.joined(separator: " ")
        notify(failure.map { "失败：" + $0 } ?? message, subtitle: label)
    }

    func apiAction(_ path: String, _ method: String, _ body: JSON, _ message: String, _ prefix: String = "操作失败") {
        let (_, failure) = opencodexRequest(path, method: method, body: body, timeout: 30)
        if let failure {
            notify("\(prefix)：\(failure.message.isEmpty ? String(failure.code) : failure.message)", subtitle: path)
        } else {
            invalidateCaches(Runtime.opencodexEndpoints.map(\.0))
            notify(message, subtitle: path)
        }
    }

    // MARK: Connections

    /// Only the plugin's overrides change; client files and the global environment stay as they are.
    func applyClientConnection(_ values: [(String, String)], module: String?) throws {
        try saveOverrides(values)
        let next = updated(values)
        if let module { try update("module", module, "true") }
        next.syncConnectionCache(); next.scheduleBackgroundRefresh()
    }

    func configureClientAction(_ name: String, _ extra: [String]) throws {
        let key = extra.count == 1 ? extra[0] : ""
        if ["choose-client-home", "reset-client-connection", "reveal-client-home"].contains(name) && !["chatgpt", "opencodex"].contains(key) { throw Invalid("invalid client") }
        var values: [(String, String)]
        switch name {
        case "reveal-client-home":
            let root = key == "opencodex" ? ocxHome : codexHome()
            if Harness.isDirectory(root) { try openLocal(root) }
            return
        case "reset-client-connection":
            values = key == "chatgpt" ? [("OCX_CHATGPT_HOME", "")] : [("OCX_HOME", ""), ("OCX_BASE_URL", ""), ("OCX_TOKEN_FILE", "")]
        case "choose-client-home":
            let message = key == "chatgpt" ? "选择包含 auth.json 的 ChatGPT 编程客户端配置目录" : "选择 OpenCodex 配置目录；自动读取其中的运行端口和管理令牌"
            guard let chosen = host.choosePath(message, folder: true) else { return }
            let root = Runtime.absolute(Paths.expandUser(chosen))
            guard Harness.isDirectory(root) else { return }
            if key == "chatgpt" {
                let auth = ClaudeApp.readObject(root + "/auth.json"), tokens = auth["tokens"]?.object ?? JSONObject()
                if !["chatgpt", "chatgptAuthTokens"].contains(auth["auth_mode"]?.string ?? "") && tokens["access_token"]?.isTruthy != true {
                    dialogError("未找到 ChatGPT 登录", "请选择已通过 ChatGPT 账户登录的 ChatGPT 编程客户端配置目录。仅 API Key 登录不能读取订阅额度。"); return
                }
                values = [("OCX_CHATGPT_HOME", root)]
            } else {
                let isFile = { (name: String) in FileManager.default.fileExists(atPath: root + "/" + name) && !Harness.isDirectory(root + "/" + name) }
                guard ["runtime-port.json", "admin-api-token", "config.json"].contains(where: isFile) else {
                    dialogError("未找到 OpenCodex 配置", "请选择包含运行端口、管理令牌或 config.json 的目录。"); return
                }
                values = [("OCX_HOME", root), ("OCX_BASE_URL", ""), ("OCX_TOKEN_FILE", "")]
            }
        case "set-proxy-address":
            if !extra.isEmpty { throw Invalid("invalid proxy address action") }
            guard let chosen = textDialog("OpenCodex 管理地址", base) else { return }
            let url = Runtime.monitorURL(chosen)
            if url.isEmpty { dialogError("地址无效", "请填写实际的 http:// 或 https:// 管理地址。"); return }
            values = [("OCX_BASE_URL", url)]
        case "choose-admin-token":
            if !extra.isEmpty { throw Invalid("invalid token action") }
            guard let chosen = host.choosePath("选择 OpenCodex 管理令牌文件；仅保存文件路径", folder: false) else { return }
            let file = Runtime.absolute(Paths.expandUser(chosen))
            guard FileManager.default.fileExists(atPath: file), !Harness.isDirectory(file) else { return }
            values = [("OCX_TOKEN_FILE", file)]
        default: throw Invalid("invalid connection action")
        }
        try applyClientConnection(values, module: name == "reset-client-connection" ? nil : key == "chatgpt" ? "chatgpt" : "opencodex")
    }

    /// Grok needs its Team ID, Kimi and MiniMax a region, Qoder CN an organization; nil means the user cancelled.
    func askOption(_ key: String, _ current: String = "") -> String? {
        func plain(_ text: String) -> Bool { !text.isEmpty && text.unicodeScalars.allSatisfy { $0.isASCII && (CharacterSet.alphanumerics.contains($0) || $0 == "_" || $0 == "-") } }
        switch key {
        case "grok":
            let team = textDialog("xAI Team ID（需要管理密钥，普通推理 API Key 不适用）", current)
            if let team, !plain(team) { dialogError("Team ID 无效", "请填写 xAI 控制台中的 Team ID。"); return nil }
            return team
        case "minimax": return host.choose(title: "MiniMax 套餐", prompt: "选择密钥所属地区", ("国际站", "international"), ("中国站", "china"))
        case "qoder-api":
            let org = textDialog("Qoder CN 组织 ID（组织设置网址 qoder.cn/organizations/<组织 ID>/settings 中的那一段）", current)
            if let org, !plain(org) { dialogError("组织 ID 无效", "请填写 Qoder CN 组织设置网址中的组织 ID。"); return nil }
            return org
        case "kimi-api":
            return host.choose(title: "Kimi API", prompt: "选择 Key 所属平台：中国站 platform.moonshot.cn，国际站 platform.moonshot.ai", ("中国站", "china"), ("国际站", "international"))
        default: return ""
        }
    }

    static func connectionNames(_ key: String) -> [String]? {
        var names: [String: [String]] = ["komari": ["KOMARI_BASE_URL"], "deepseek": ["DEEPSEEK_API_KEY", "DEEPSEEK_API_KEY_FILE"],
                                         "grok": ["XAI_MANAGEMENT_API_KEY", "XAI_MANAGEMENT_API_KEY_FILE", "XAI_TEAM_ID"],
                                         "minimax": ["MINIMAX_API_KEY", "MINIMAX_API_KEY_FILE", "MINIMAX_REGION"],
                                         "kimi-api": ["MOONSHOT_API_KEY", "MOONSHOT_API_KEY_FILE", "MOONSHOT_REGION"],
                                         "qoder-api": ["QODER_CN_API_KEY", "QODER_CN_API_KEY_FILE", "QODER_CN_ORG_ID"],
                                         "claude-api": ["ANTHROPIC_API_KEY", "ANTHROPIC_API_KEY_FILE"]]
        for api in Catalog.portalAPIs { names[api] = Catalog.credentialNames(api).flatMap { [$0, $0 + "_FILE"] } }
        return names[key]
    }

    func configureConnection(_ key: String, clear: Bool = false) throws {
        guard let names = Runtime.connectionNames(key) else { throw Invalid("invalid connection") }
        var values = names.map { ($0, "") }
        func set(_ name: String, _ value: String) { if let index = values.firstIndex(where: { $0.0 == name }) { values[index].1 = value } }
        if clear {
            if !confirm("清除连接设置？", "清除插件保存的地址或密钥引用；插件保存的 Key 一并删除，你自己选择的密钥文件不会删除。", accept: "清除设置") { return }
        } else if key == "komari" {
            guard let value = textDialog("Komari 面板地址（http:// 或 https://）", komariBaseURL) else { return }
            let url = Runtime.monitorURL(value)
            if url.isEmpty { dialogError("地址无效", "请填写实际面板地址，不使用示例地址、账号密码或查询参数。"); return }
            set("KOMARI_BASE_URL", url)
        } else {
            guard let file = host.choosePath("选择 " + key + " 的密钥文本文件；只保存路径，不复制或展示密钥。", folder: false),
                  FileManager.default.fileExists(atPath: file), !Harness.isDirectory(file) else { return }
            set(names[1], file)
            let current = environment[["grok": "XAI_TEAM_ID", "qoder-api": "QODER_CN_ORG_ID"][key] ?? ""] ?? ""
            guard let option = askOption(key, current) else { return }
            let chosen = (try? String(contentsOfFile: file, encoding: .utf8))?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
            let options: JSONObject = key == "grok" ? ["team": .string(option)] : key == "qoder-api" ? ["org": .string(option)] : [:]
            if refuseRepeated(key, chosen, options, keys: namedKeys(key)) { return }
            if !option.isEmpty { set(names[2], option) }
        }
        let previous = names.filter { $0.hasSuffix("_FILE") }.map { environment[$0] ?? "" }
        try saveOverrides(values)
        for file in previous where !file.isEmpty && !values.contains(where: { $0.1 == file }) && Connections.managedKey(stateDir, file) {
            unlink(Paths.expandUser(file))
        }
        let next = updated(values)
        next.syncConnectionCache()
        if !clear { try update("module", key, "true") }
        else if key != "deepseek" && Keys.entries(stateDir, key).isEmpty { try update("module", key, "false") }
        // A newly saved key gets one check right away, so the row shows a result instead of nothing.
        if !clear && Runtime.keyCheckProviders.contains(key) { next.dispatchKeyCheck(key) }
        next.scheduleBackgroundRefresh()
    }

    /// A key already here isn't added again: the same secret and, for Grok and Qoder CN, the same team or organization.
    func refuseRepeated(_ provider: String, _ secret: String, _ options: JSONObject, keys: [JSONObject]? = nil) -> Bool {
        let account = keyAccount(provider, ["id": "new", "options": .object(options)])
        let same = (keys ?? apiKeys(provider)).first { entry in
            !secret.isEmpty && entrySecret(provider, Py.str(entry["id"] ?? .null)) == secret && (account.first == "key" || keyAccount(provider, entry) == account)
        }
        if let same { dialogError("这把 Key 已经添加过", "与「" + Py.str(same["label"] ?? .null) + "」是同一把 Key，不再重复添加。") }
        return same != nil
    }

    func addAPIKey(_ provider: String) throws {
        guard let service = Catalog.byKey[provider], service.category == "API" else { throw Invalid("invalid API") }
        guard let name = textDialog("给这把 Key 起个名字", "Key \(apiKeys(provider).count + 1)", title: "添加 Key") else { return }
        if !Keys.validLabel(name) { dialogError("名字无效", "请填写 1–24 个字符的名字。"); return }
        let note = provider == "claude-api" ? "\n管理员 Key 权限很大，建议在控制台设置过期时间；插件只用它读取花费。" : ""
        let prompt = ["grok": "xAI 管理密钥（Management API Key）", "qoder-api": "Qoder CN 组织 API Key（组织管理员在 设置 → 高级 → API 密钥 创建，仅团队版/企业版）"][provider]
            ?? service.label + " Key"
        guard let secret = textDialog(prompt + "\n只保存在本机仅你可读的文件中，不会显示或上传。" + note, hidden: true, title: "添加 Key") else { return }
        if !Keys.validKey(secret) { dialogError("Key 格式无效", "请粘贴完整的 API Key，不含空格或换行。"); return }
        guard let option = askOption(provider) else { return }
        let options: JSONObject = ["grok": ["team": .string(option)], "kimi-api": ["region": .string(option)], "qoder-api": ["org": .string(option)]][provider] ?? [:]
        if refuseRepeated(provider, secret, options) { return }
        let id = try Keys.add(stateDir, provider, name: name, value: secret, options: options)
        try update("module", provider, "true")
        if Runtime.keyCheckProviders.contains(provider) { dispatchKeyCheck(provider, id) }
        scheduleBackgroundRefresh()
    }

    func renameAPIKey(_ provider: String, _ id: String) throws {
        guard let entry = apiKeys(provider).first(where: { $0["id"] == .string(id) }) else { throw Invalid("unknown key") }
        guard let name = textDialog("重命名这把 Key", Py.str(entry["label"] ?? .null), title: "重命名 Key") else { return }
        do { try Keys.rename(stateDir, provider, id: id, name: name) } catch is Preferences.Invalid { dialogError("名字无效", "请填写 1–24 个字符的名字。") }
    }

    func removeAPIKey(_ provider: String, _ id: String) throws {
        guard let entry = apiKeys(provider).first(where: { $0["id"] == .string(id) }) else { throw Invalid("unknown key") }
        if id == "default" { return try configureConnection(provider, clear: true) }
        if !confirm("删除「" + Py.str(entry["label"] ?? .null) + "」？", "只删除插件保存的这把 Key，不影响服务商账户。", accept: "删除") { return }
        try Keys.remove(stateDir, provider, id: id)
        unlink(cachePath(Py.str(entry["cache"] ?? .null)))
        var checks = readCache("key-checks")
        if checks[Runtime.checkID(provider, id)] != nil {
            checks[Runtime.checkID(provider, id)] = nil
            writeJSONCache(cachePath("key-checks"), .object(checks))
        }
        if !connectionReady(provider) { try update("module", provider, "false") }
        scheduleBackgroundRefresh()
    }

    func dispatchKeyCheck(_ provider: String, _ id: String = "default") {
        if let desktop { desktop.submitTest(provider, id) } else { try? testAPIKey(provider, id) }
    }

    public func testAPIKey(_ provider: String, _ id: String = "default") throws {
        let secret = Runtime.keyCheckProviders.contains(provider) ? entrySecret(provider, id) : ""
        if secret.isEmpty { throw Invalid("invalid key check") }
        var checks = readCache("key-checks")
        checks["schema"] = 1
        var result = Accounts.checkKey(provider, key: secret)
        result["checkedAt"] = .double(Date().timeIntervalSince1970); result["key"] = .string(keyFingerprint(provider, id))
        checks[Runtime.checkID(provider, id)] = .object(result)
        writeJSONCache(cachePath("key-checks"), .object(checks))
    }

    func setBalanceAlert(_ provider: String, _ currency: String) throws {
        let key = provider + "|" + currency
        guard Preferences.balanceKey(key) else { throw Invalid("invalid balance alert") }
        let line = settings()["balanceAlerts"]?.object?[key]
        let unit = ["CNY": "元", "USD": "美元"][currency] ?? " \(currency) "
        guard let answer = textDialog("\(Catalog.byKey[provider]!.label) 余额降到多少\(unit)以下时提醒？留空关闭。", line?.isTruthy == true ? Runtime.pythonG(line) : "",
                                      title: "余额提醒") else { return }
        var value = answer.trimmingCharacters(in: .whitespacesAndNewlines)
        while let first = value.first, first == "¥" || first == "$" { value.removeFirst() }
        do { try update("balance-alert", key, value.replacingOccurrences(of: ",", with: "")) }
        catch is Preferences.Invalid { dialogError("金额无效", "请填写大于 0 的数字，或留空关闭提醒。") }
    }

    func setSubscriptionPrice(_ key: String) throws {
        guard Catalog.subscriptionUsage[key] != nil else { throw Invalid("invalid subscription") }
        let price = settings()["subscriptionPrices"]?.object?[key]
        guard let answer = textDialog("\(Catalog.byKey[key]!.label) 每月多少美元？用来算本月用量是订阅价的几倍。留空清除。", price?.isTruthy == true ? Runtime.pythonG(price) : "",
                                      title: "订阅月费") else { return }
        var value = answer.trimmingCharacters(in: .whitespacesAndNewlines)
        while value.first == "$" { value.removeFirst() }
        value = value.replacingOccurrences(of: ",", with: "")
        if value.hasSuffix("/月") { value.removeLast(2) }
        do { try update("subscription-price", key, value.trimmingCharacters(in: .whitespacesAndNewlines)) }
        catch is Preferences.Invalid { dialogError("金额无效", "请填写大于 0 的数字（美元），或留空清除。") }
        scheduleBackgroundRefresh()
    }

    // MARK: Export

    func exportUsage(_ kind: String) throws {
        guard ["csv", "json"].contains(kind) else { throw Invalid("invalid export") }
        let key = selectedRange(), label = Runtime.ranges.first { $0.0 == key }!.1
        let bundle = readCache(rangeCacheName(key))["payload"]?.object ?? JSONObject()
        guard bundle["range"] == .string(key) else { dialogError("还没有可导出的用量", "“" + label + "”还在读取，读完后再导出。"); return }
        let day = Runtime.format(Date().timeIntervalSince1970, localZone(), "%Y%m%d")
        guard let path = host.savePath("导出“\(label)”的用量", name: "starmeter-\(key)-\(day).\(kind)") else { return }
        let selected = Preferences.modules.filter { Preferences.usage.contains($0.key) && moduleEnabled($0.key) }.map { ($0.key, $0.label, bundle[$0.key]?.object ?? JSONObject()) }
        let counting = Runtime.summaryUsage(selected)
        let text = kind == "csv" ? UsageExport.csvText(selected, counting)
            : UsageExport.jsonText(selected, counting, key: key, label: label, start: bundle["start"], end: bundle["end"], version: Runtime.version)
        // Excel opens a CSV with a byte-order mark as UTF-8, so tool names read correctly.
        var bytes = Data(kind == "csv" ? [0xEF, 0xBB, 0xBF] : [])
        bytes.append(Data(text.utf8))
        try bytes.write(to: URL(fileURLWithPath: path))
        notify("已导出到 " + (path as NSString).lastPathComponent, subtitle: "导出用量")
    }

    // MARK: Self-update

    func setSelfUpdateState(_ values: JSONObject) {
        var state = values
        state["at"] = .double(Date().timeIntervalSince1970)
        writeJSONCache(stateDir + "/self-update.json", .object(state))
    }

    // MARK: The actions

    @discardableResult
    public func runAction(_ name: String, _ extra: [String]) throws -> Int32 {
        func one() -> String { extra.count == 1 ? extra[0] : "" }
        switch name {
        case "panel", "panel-modules":
            writeJSONCache(cachePath("panel-open"), ["schema": 1, "mode": .string(name == "panel-modules" ? "modules" : "home"), "createdAt": .double(Date().timeIntervalSince1970)])
            if Harness.isDirectory(panelApp) {
                // Not "-g": the panel opens active, so its glass and controls don't change look on the first click.
                _ = Shell.run(["/usr/bin/open", "-a", panelApp, "--args", "--plugin", Runtime.absolute(plugin), "--snapshot", cachePath("panel-view")], timeout: 5)
            }
        case "set-module", "set-appearance", "set-theme", "reset-modules":
            switch name {
            case "set-module": try update("module", extra.first ?? "", extra.count > 1 ? extra[1] : "")
            case "set-appearance": try update("appearance", extra.first ?? "")
            case "set-theme": try update("theme", extra.first ?? "")
            default: try update("reset")
            }
            if name == "set-module" && extra.count == 2 && extra[1] == "true" && Runtime.resourceCaches[extra[0]] != nil {
                invalidateCaches(resourceCacheNames(extra[0], externalLoaders().map(\.0)))
            }
            scheduleBackgroundRefresh()
        case "set-subscription":
            guard extra.count == 2 else { throw Invalid("invalid subscription") }
            try update("subscription", extra[0], extra[1])
        case "set-collapse":
            guard extra.count == 1 else { throw Invalid("invalid display action") }
            try update("collapse", extra[0])
        case "set-usage-alert", "set-reset-reminder":
            guard extra.count == 1 else { throw Invalid("invalid alert setting") }
            try update(name == "set-usage-alert" ? "usage-alert" : "reset-reminder", extra[0])
        case "set-balance-alert":
            guard extra.count == 2 else { throw Invalid("invalid balance alert") }
            try setBalanceAlert(extra[0], extra[1])
        case "set-subscription-price":
            guard extra.count == 1 else { throw Invalid("invalid subscription price") }
            try setSubscriptionPrice(extra[0])
        case "set-expanded":
            guard extra.count == 2 else { throw Invalid("invalid expansion action") }
            try update("expanded", extra[0], extra[1])
        case "detect-modules":
            let detected = detectedModules(), current = Set(settings()["visible"]?.array?.compactMap(\.string) ?? [])
            let labels = Dictionary(uniqueKeysWithValues: Preferences.modules.map { ($0.key, $0.label) })
            let added = detected.filter { !current.contains($0) }.map { labels[$0]! }
            let removed = Preferences.modules.filter { current.contains($0.key) && !detected.contains($0.key) }.map(\.label)
            let detail = "检测到 \(detected.count) 个模块。确认后将替换当前勾选。\n\n将启用：\(added.isEmpty ? "无" : added.joined(separator: "、"))\n将取消：\(removed.isEmpty ? "无" : removed.joined(separator: "、"))\n\n只检查本机目录与已有配置，不读取对话正文，不显示 Key。"
            if confirm("自动检测将覆盖当前模块选择", detail, accept: "覆盖当前选项") {
                try Discovery.replaceSelection(stateDir, detected: detected); scheduleBackgroundRefresh()
            }
        case "refresh-view": scheduleBackgroundRefresh()
        case "refresh-source": try refreshResource(one())
        case "choose-client-home", "reset-client-connection", "reveal-client-home", "set-proxy-address", "choose-admin-token":
            try configureClientAction(name, extra)
        case "configure-connection", "clear-connection": try configureConnection(one(), clear: name == "clear-connection")
        case "add-api-key": try addAPIKey(one())
        case "rename-api-key", "remove-api-key":
            guard extra.count == 2 else { throw Invalid("invalid key action") }
            if name == "rename-api-key" { try renameAPIKey(extra[0], extra[1]) } else { try removeAPIKey(extra[0], extra[1]) }
        case "test-api-key":
            guard (1...2).contains(extra.count) else { throw Invalid("invalid key check") }
            try testAPIKey(extra[0], extra.count > 1 ? extra[1] : "default")
        case "auto-source":
            try update("auto-path", one()); scheduleBackgroundRefresh()
        case "open-settings":
            try openLocal(desktop?.configFile() ?? Connections.ensureFile(stateDir), text: true)
        case "open-claude":
            if !extra.isEmpty { throw Invalid("invalid app action") }
            if let desktop { desktop.openClaude(); return 0 }
            guard Shell.run(["/usr/bin/open", "-a", "Claude"], timeout: 5) != nil else { throw Invalid("open failed") }
        case "set-claude-oauth", "reauthorize-claude-oauth":
            let reauthorize = name == "reauthorize-claude-oauth"
            if reauthorize && !extra.isEmpty { throw Invalid("invalid Claude authorization action") }
            let value = reauthorize ? "true" : one()
            guard ["true", "false"].contains(value) else { throw Invalid("invalid Claude OAuth preference") }
            if value == "true" {
                if reauthorize || settings()["claudeOAuthUsage"]?.isTruthy != true {
                    let title = reauthorize ? "重新读取 Claude Code 登录？" : "使用 Claude Code 登录查询用量？"
                    let detail = desktop != nil
                        ? "读取 Claude Code 本机登录文件，仅向 api.anthropic.com 查询用量。不会读取 Cookie，也不会调用模型。凭据不写入插件配置或日志；登录过期时在 Claude Code 重新登录。"
                        : "读取 Claude Code 现有登录，仅向 api.anthropic.com 查询订阅用量。无需 Cookie 或模型调用。登录过期时可启动 Code 自检续期，不发送消息。\n凭据只在内存；不写入插件配置或日志。macOS 可能询问钥匙串权限。"
                    if !confirm(title, detail, accept: reauthorize ? "重新读取" : "启用") { return 0 }
                }
                try update("claude-oauth", "true")
                // Renewal can take half a minute, so the background worker does it, as a retry.
                writeJSONCache(cachePath("claude-retry"), ["schema": 1, "requestedAt": .double(Date().timeIntervalSince1970)])
            } else {
                try update("claude-oauth", "false"); unlink(stateDir + "/claude-oauth-usage.json")
            }
            invalidateCaches(["claude_subscription"])
            if value == "true" { launchBackground("--background-refresh-wait") } else { scheduleBackgroundRefresh() }
        case "set-cursor-usage":
            let value = one()
            guard ["true", "false"].contains(value) else { throw Invalid("invalid Cursor usage preference") }
            if value == "true" && settings()["cursorUsage"]?.isTruthy != true {
                if !confirm("读取 Cursor 用量记录？", "Cursor 不在本机记录 Token。开启后，插件读取 Cursor 桌面版或 Cursor CLI 已保存的登录，只用它向 cursor.com 查询本账户的用量记录（模型、Token 与费用），桌面版和 CLI 的用量都包括在内。\n令牌只在内存中使用，不保存、不刷新、不改写，也不发往其他地址。", accept: "开启") { return 0 }
            }
            try update("cursor-usage", value)
            if value == "true" { try update("module", "cursor", "true") }
            else {
                for file in (try? FileManager.default.contentsOfDirectory(atPath: cacheDir)) ?? [] where file.hasPrefix("cursor_usage_") || file.hasPrefix("cursor_subscription") {
                    unlink(cacheDir + "/" + file)
                }
            }
            invalidateCaches([rangeCacheName(), "cursor_subscription"])
            scheduleBackgroundRefresh()
        case "set-keep-usage":
            let value = one()
            guard ["true", "false"].contains(value) else { throw Invalid("invalid keep usage setting") }
            if value == "false" && settings()["keepCleanedUsage"]?.isTruthy == true {
                if !confirm("不再保留已清理记录的用量？", "Claude Code 等工具会删掉旧记录（Claude Code 默认保留 30 天）。关闭后删除已保留的用量，“全部”和更早的日子只按还在的记录统计，删除后无法恢复。", accept: "关闭并删除") { return 0 }
                Archive.clear(state: stateDir)
            }
            try update("keep-usage", value)
            scheduleBackgroundRefresh()
        case "retry-claude-usage":
            if !extra.isEmpty || settings()["claudeOAuthUsage"]?.isTruthy != true { throw Invalid("invalid Claude refresh action") }
            let now = Date().timeIntervalSince1970, pending = claudeRetryPending()
            if 0 <= now - pending && now - pending < 5 { return 0 }
            writeJSONCache(cachePath("claude-retry"), ["schema": 1, "requestedAt": .double(now)])
            invalidateCaches(["claude_subscription"])
            launchBackground("--background-refresh-wait")
        case "set-claude-live":
            let value = one()
            guard ["true", "false"].contains(value) else { throw Invalid("invalid Claude usage preference") }
            if value == "true" && settings()["claudeLiveUsage"]?.isTruthy != true {
                let detail = "仅在点击“在线查询一次”时读取 Claude App 的钥匙串和 Cookie，向 https://claude.ai 查询订阅用量。平时只读取本机快照，不自动请求。\n凭据只在内存，不写入配置、日志或 GitHub；不查询余额，不调用模型。"
                if !confirm("允许手动查询 Claude 用量？", detail, accept: "允许读取") { return 0 }
            }
            try update("claude-live", value)
            if value == "false" { invalidateCaches(["claude_subscription_manual"]) }
            invalidateCaches(["claude_subscription"]); scheduleBackgroundRefresh()
        case "set-claude-local":
            let value = one()
            if macApp && value == "true" { throw Invalid("standalone status-line setup unavailable") }
            guard ["true", "false"].contains(value) else { throw Invalid("invalid local Claude setting") }
            if value == "true" {
                if !confirm("启用本机用量回传？", "在 " + ClaudeBridge.settingsPath(environment: environment) + " 中连接官方状态栏回调。只保存订阅百分比、重置时间和更新时间，不读取 Cookie 或对话内容。已有状态栏会备份并继续执行，关闭时恢复。\n需等待 Claude Code 产生响应后才会有新数据。", accept: "启用") { return 0 }
                try ClaudeBridge.install(stateDir, environment: environment)
            } else { ClaudeBridge.uninstall(stateDir) }
            invalidateCaches(["claude_subscription"]); scheduleBackgroundRefresh()
        case "refresh-claude-online":
            if !extra.isEmpty { throw Invalid("invalid Claude query") }
            // claude.ai answers only browsers it recognizes; this engine has no such client, so the query stays unavailable.
            dialogError("Claude 在线查询未完成", "在线查询暂不可用，请改用 Claude Code 登录查询。")
        case "update-cli":
            let key = one()
            guard let command = ["claude-code": "claude", "codex-cli": "codex"][key], selectedUpdateIDs().contains(key), moduleEnabled("updates") else { throw Invalid("invalid update target") }
            guard let binary = Shell.which(command, environment: environment) else { throw Invalid("CLI not installed") }
            if let desktop { desktop.updateCLI(binary: binary, key: key); return 0 }
            guard let help = Shell.run([binary, "--help"], timeout: 5) else { throw Invalid("help failed") }
            if help.range(of: "(?m)^\\s+update(?:\\s|\\|)", options: .regularExpression) == nil {
                dialogError("当前版本不支持 update 命令", "请通过官方安装方式更新。"); return 0
            }
            let refresh = macApp ? nil : "swiftbar://refreshplugin?name=" + Accounts.quote(swiftbarName)
            try terminal(try TerminalJobs.updateCommand(binary: binary, key: key, state: stateDir, refreshURL: refresh, zone: localZone()))
        case "open-cli-update-log":
            if !extra.isEmpty || !moduleEnabled("updates") { throw Invalid("invalid update log action") }
            if let path = TerminalJobs.latestLog(stateDir, selectedUpdateIDs()) { try openLocal(path, text: true) }
        case "reveal-source":
            let key = one()
            guard Preferences.usage.contains(key) else { throw Invalid("invalid source") }
            if let first = sourceLocations(key).first(where: { FileManager.default.fileExists(atPath: $0) }) {
                try openLocal(Harness.isDirectory(first) ? first : (first as NSString).deletingLastPathComponent)
            }
        case "choose-source":
            let key = extra.first ?? ""
            guard Preferences.usage.contains(key) else { throw Invalid("invalid source") }
            let prompt = key == "claude" ? "选择 Claude App 数据目录（包含 claude-code-sessions 或 plan-usage-history.json）" : "选择 " + key + " 的用量记录目录"
            if let chosen = host.choosePath(prompt, folder: true), !chosen.isEmpty {
                try update("path", key, chosen); scheduleBackgroundRefresh()
            }
        case "export-usage":
            guard extra.count == 1 else { throw Invalid("invalid export") }
            try exportUsage(extra[0])
        case "set-currency":
            guard extra.count == 1 else { throw Invalid("invalid currency") }
            try update("currency", extra[0])
            if extra[0] == "CNY" { scheduleBackgroundRefresh() }
        case "set-cny-rate":
            if !extra.isEmpty { throw Invalid("invalid exchange rate") }
            let rate = settings()["cnyRate"]
            guard let answer = textDialog("1 美元合多少人民币？留空则每天按欧洲央行参考汇率自动更新。", rate?.isTruthy == true ? Runtime.pythonG(rate) : "", title: "汇率") else { return 0 }
            var value = answer.trimmingCharacters(in: .whitespacesAndNewlines)
            while value.first == "¥" { value.removeFirst() }
            do { try update("cny-rate", value) } catch is Preferences.Invalid { dialogError("汇率无效", "请填写 1 到 20 之间的数字，或留空自动更新。"); return 0 }
            scheduleBackgroundRefresh()
        case "set-service-status":
            guard extra.count == 1 else { throw Invalid("invalid service status setting") }
            try update("service-status", extra[0])
            if extra[0] == "false" { invalidateCaches(["service_status"]) } else { scheduleBackgroundRefresh() }
        case "set-session-alerts":
            guard extra.count == 1 else { throw Invalid("invalid session alert setting") }
            try update("session-alerts", extra[0])
        case "refresh-live":
            if !extra.isEmpty { throw Invalid("invalid live refresh") }
            refreshLive()
        case "set-pin-interval":
            guard extra.count == 1 else { throw Invalid("invalid pin interval") }
            try update("pin-interval", extra[0])
        case "set-menu-bar":
            let key = one()
            guard Preferences.menuBar.contains(key) else { throw Invalid("invalid menu bar value") }
            try update("menu-bar", key)
            if key == "cost" { scheduleBackgroundRefresh() }
        case "set-group":
            let key = one()
            guard Preferences.groupings.contains(key) else { throw Invalid("invalid grouping") }
            try update("group", key)
        case "set-range":
            let key = extra.first ?? "today"
            guard Runtime.ranges.contains(where: { $0.0 == key }) else { throw Invalid("invalid range") }
            if let desktop { desktop.saveRange(key) } else { writeJSONCache(cachePath("view-range"), ["schema": 1, "range": .string(key)]) }
            if desktop != nil || cacheDue(readCache(rangeCacheName(key)), ttl: refreshTTL(60)) { launchBackground("--refresh-range") }
        case "refresh":
            invalidateCaches([rangeCacheName()] + Runtime.externalTTLs.keys.sorted() + namedKeyLoaderNames())
            scheduleBackgroundRefresh()
        case "update-self":
            guard let release = pluginUpdate(["self_update": .object(readCache("self_update")["payload"]?.object ?? JSONObject())]) else {
                notify("Starmeter 已是最新版本 " + Runtime.version, subtitle: ""); return 0
            }
            if selfUpdateState()["status"] == .string("running") { return 0 }
            let latest = Py.str(release["latest"] ?? .null)
            if confirm("更新 Starmeter 到 " + latest + "？", "当前版本 " + Runtime.version + "。将从 GitHub 下载该版本的安装包，校验 SHA-256 后安装；原文件先备份，设置、Key 和统计记录都会保留。", accept: "更新") {
                setSelfUpdateState(["status": "running", "version": .string(latest)])
                Shell.detach(["/bin/bash", "-p", plugin, "--self-update", latest])
            }
        case "check-app-updates":
            invalidateCaches(["app_updates", "badge", "self_update"]); scheduleBackgroundRefresh()
        case "refresh-monitor":
            invalidateCaches(["komari"]); scheduleBackgroundRefresh()
        case "restart":
            if confirm("重新启动 opencodex 代理？", "正在进行的请求会被中断，代理会自动恢复到就绪状态。") { cliAction(["restart"], "代理已重新启动") }
        case "stop":
            if confirm("停止 opencodex 代理？", "代理会停止，并同时把 ChatGPT 编程客户端还原为原生配置。\n需要时点「启动代理」可以重新开启。") { cliAction(["stop"], "代理已停止") }
        case "start":
            notify("正在启动 opencodex…", subtitle: "")
            let failure = ocx(["ensure"]) ?? ocx(["start"])
            notify(failure.map { "启动失败：" + $0 } ?? "代理已启动", subtitle: "")
        case "sync":
            notify("正在同步模型到 ChatGPT…", subtitle: "")
            apiAction("/api/sync", "POST", .object(JSONObject()), "模型已同步到 ChatGPT")
        case "repair":
            if confirm("修复自启动服务？", "会重新注册后台服务，可能需要输入管理员密码。") { cliAction(["service", "repair"], "自启动服务已修复") }
        case "status": try terminal("ocx status")
        case "update":
            let tag = extra.first ?? "latest"
            if confirm("更新 opencodex（通道 " + tag + "）？", "更新需要几分钟，完成后会自动重启代理。") {
                let (_, failure) = opencodexRequest("/api/update/run", method: "POST", body: ["tag": .string(tag), "restart": true], timeout: 30)
                unlink(cachePath("badge"))
                if let failure, failure.message.contains("already_latest") { notify("已是最新版本", subtitle: "OpenCodex 无需更新") }
                else if let failure { notify("操作失败：" + (failure.message.isEmpty ? String(failure.code) : failure.message), subtitle: "/api/update/run") }
                else { notify("更新任务已启动", subtitle: "/api/update/run") }
            }
        case "check-update":
            let (payload, failure) = opencodexRequest("/api/update/check", timeout: 30)
            if let failure { notify("检查更新失败：" + (failure.message.isEmpty ? String(failure.code) : failure.message), subtitle: "") }
            else {
                writeJSONCache(cachePath("badge"), ["schema": 1, "payload": payload.map(JSON.object) ?? .null, "fetchedAt": .double(Date().timeIntervalSince1970)])
                if payload?["updateAvailable"]?.isTruthy == true {
                    notify("发现新版本 " + Py.str(payload?["latestVersion"] ?? .null), subtitle: "当前 " + Py.str(payload?["currentVersion"] ?? .null))
                } else {
                    notify("已是最新版本 " + Py.str(Py.or(payload?["currentVersion"], .string(""))), subtitle: "")
                }
            }
        case "account-use":
            let id = extra.first ?? ""
            if confirm("切换到账号 " + id + "？", "之后的请求会使用这个账号。") {
                apiAction("/api/codex-auth/active", "PUT", ["accountId": .string(id)], "已切换激活账号", "切换失败")
            }
        case "account-pause":
            let id = extra.first ?? "", paused = (extra.count > 1 ? extra[1] : "true") == "true"
            apiAction("/api/codex-auth/accounts/pause", "PUT", ["id": .string(id), "paused": .bool(paused)], paused ? "账号已暂停" : "账号已启用", "操作失败")
        case "pool-strategy":
            let strategy = extra.first ?? ""
            let labels = ["quota": "按额度", "round-robin": "轮询", "fill-first": "用完再换"]
            apiAction("/api/codex-auth/pool-strategy", "PUT", ["strategy": .string(strategy)], "调度策略已改为「\(labels[strategy] ?? strategy)」", "切换失败")
        case "auto-switch":
            let threshold = extra.first ?? ""
            guard let value = threshold.isEmpty ? 80 : Int(threshold) else { throw Invalid("invalid threshold") }
            apiAction("/api/codex-auth/auto-switch", "PUT", ["threshold": .int(Int64(value))], "额度超过 \(threshold)% 时自动切换账号", "设置失败")
        case "failover":
            let threshold = extra.first ?? ""
            guard let value = threshold.isEmpty ? 1 : Int(threshold) else { throw Invalid("invalid threshold") }
            apiAction("/api/codex-auth/failover", "PUT", ["threshold": .int(Int64(value))], "失败换账号阈值已设为 " + (threshold != "0" ? threshold : "不自动换"), "设置失败")
        case "copy-url":
            host.copy(base); notify("已复制管理地址", subtitle: "")
        case "copy-token":
            if let token = adminToken() { host.copy(token); notify("已复制管理令牌", subtitle: "") }
            else { notify("没有找到管理令牌", subtitle: tokenFile) }
        case "open-home": try openLocal(ocxHome)
        case "logs":
            let log = ocxHome + "/service.log"
            if desktop != nil { try openLocal(log, text: true); return 0 }
            try terminal("tail -n 100 -f " + Connections.shellQuote(log))
        default: break
        }
        return 0
    }

    static let alwaysAllowed: [[String]] = [["refresh-view"], ["refresh"], ["refresh-live"], ["detect-modules"]] + Preferences.groupings.map { ["set-group", $0] }
    static let toggles: Set<String> = ["set-collapse", "set-reset-reminder", "set-cursor-usage", "set-keep-usage", "set-session-alerts", "set-service-status"]
    static let receiptActions: Set<String> = ["set-module", "set-appearance", "set-theme", "reset-modules", "set-collapse", "set-expanded", "detect-modules"]

    /// The panel queues an action in a file; it runs here, through the plugin's own permissions, only if the
    /// rendered panel offered it. A receipt tells the panel how it went.
    public func consumePanelAction() {
        let pending = cachePath("panel-action"), claimed = pending + ".processing-\(getpid())"
        guard rename(pending, claimed) == 0 else { return }
        var requestID: String?, action: [String]?, status = "rejected", reason: JSON = .null
        defer {
            unlink(claimed)
            if let requestID {
                let preferences: JSON = status == "done" && action.map { Runtime.receiptActions.contains($0[0]) } == true ? presentationSettings() : .null
                writeJSONCache(cachePath("panel-action-receipt"), ["schema": 1, "id": .string(requestID), "status": .string(status), "reason": reason,
                                                                  "finishedAt": .double(Date().timeIntervalSince1970), "preferences": preferences])
            }
        }
        guard let data = FileManager.default.contents(atPath: claimed), let record = (try? JSON.parse(data))?.object else {
            status = "failed"; reason = "action-failed"; return
        }
        requestID = record["id"]?.string
        guard let id = requestID, id.unicodeScalars.count <= 64, let values = record["action"]?.array else { requestID = record["id"]?.string; return }
        let created = record["createdAt"].map { $0.number ?? $0.string.flatMap(parsePythonFloat) } ?? 0
        guard let created else { status = "failed"; reason = "action-failed"; return }
        let age = Date().timeIntervalSince1970 - created
        let strings = values.compactMap(\.string)
        guard !values.isEmpty, strings.count == values.count, 0 <= age, age <= 60 else { return }
        action = strings
        // Only an action already present in the rendered panel can be requested.
        let snapshot = readCache("panel-view")
        var nodes = (snapshot["entries"]?.array ?? []) + (snapshot["rangeOptions"]?.array ?? []) + (snapshot["moduleOptions"]?.array ?? [])
        var allowed = Runtime.alwaysAllowed
        let dashboard = snapshot["dashboard"]?.object ?? JSONObject()
        if dashboard["allowCollapse"]?.isTruthy == true {
            for provider in dashboard["providers"]?.array ?? [] {
                guard let id = provider.object?["id"]?.string, Preferences.usage.contains(id) else { continue }
                allowed += [["set-expanded", id, "true"], ["set-expanded", id, "false"]]
            }
        }
        while let node = nodes.popLast() {
            let fields = node.object ?? JSONObject()
            if let open = fields["onOpenAction"]?.array, !open.isEmpty { allowed.append(open.map { Py.str($0) }) }
            if let candidate = fields["action"]?.array?.compactMap(\.string), !candidate.isEmpty {
                allowed.append(candidate)
                // A persistent picker may queue another click before the previous refresh.
                if candidate.count == 2 && Runtime.toggles.contains(candidate[0]) { allowed += [[candidate[0], "true"], [candidate[0], "false"]] }
                if candidate.count == 3 && candidate[0] == "set-module" && Preferences.known.contains(candidate[1]) {
                    allowed += [["set-module", candidate[1], "true"], ["set-module", candidate[1], "false"]]
                }
            }
            nodes += fields["children"]?.array ?? []
        }
        guard allowed.contains(strings) else { return }
        writeJSONCache(cachePath("last-action"), ["action": .string(strings[0]), "receivedAt": .double(Date().timeIntervalSince1970)])
        do {
            // Outside a render: an action reads the settings it has just changed.
            try runAction(strings[0], Array(strings.dropFirst()))
            status = "done"
        } catch {
            status = "failed"; reason = "action-failed"
        }
    }
}
