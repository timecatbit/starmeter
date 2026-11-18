import Foundation

/// The menu SwiftBar draws, and the entry's commands. Port of title, render and main in swiftbar_plugin.py.
extension Runtime {

    /// The star, with the figure the user chose beside it; kept steady while a source reconnects.
    public func title(_ data: JSONObject? = nil) -> String {
        // 20 points: the mark's dots fall on whole pixels there, and it matches the system icons beside it.
        let value = data.map { menuBarValue($0) } ?? ""
        return Runtime.item(value, Runtime.merged(["tooltip": "AI 用量面板", "emojize": "false", "size": .null], logoMark(20)))
    }

    func panelTitle(_ line: String) -> String {
        guard Harness.isDirectory(panelApp) else { return line }
        var options = act("panel")
        options["refresh"] = nil
        return line + " " + options.map { Runtime.asOption($0.key, $0.value) }.joined(separator: " ")
    }

    /// A row for SwiftBar's own menu: the panel-only options dropped, system colors kept for contrast.
    static func nativeMenuLine(_ line: String) -> String {
        guard let range = line.range(of: " | ") else { return line }
        var options = Options()
        for part in shellSplit(String(line[range.upperBound...])) {
            guard let equals = part.firstIndex(of: "=") else { continue }
            options[String(part[..<equals])] = .string(String(part[part.index(after: equals)...]))
        }
        for key in ["color", "sfcolor", "sfcolors", "panelattention", "paneldetail", "panelfallback", "panelsection", "panelstatus", "panelradio", "panelusagelevel"] {
            options[key] = nil
        }
        return String(line[..<range.upperBound]) + options.map { asOption($0.key, $0.value) }.joined(separator: " ")
    }

    func aboutMenuLines() -> [String] {
        [Runtime.item("关于 Starmeter", logoMark(Runtime.menuIconSize)),
         Runtime.item("版本 " + Runtime.version + " · 更新记录", level: 1, ["sfimage": "tag", "href": .string(Runtime.repository + "/blob/main/CHANGELOG.md")]),
         Runtime.item("作者 " + Runtime.author + " · GitHub 仓库", level: 1, ["sfimage": "person.crop.circle", "href": .string(Runtime.repository)])]
    }

    func detectionMenuLines() -> [String] {
        // One clickable row: its confirmation dialog explains what detection reads and what it replaces.
        [Runtime.item("自动检测…", Runtime.merged(["sfimage": "sparkle.magnifyingglass"], act("detect-modules")))]
    }

    /// The module list as SwiftBar rows, for when the native panel isn't installed.
    func moduleMenuLines() -> [String] {
        var rows = [Runtime.item("主页模块", ["sfimage": "square.grid.2x2"])]
        func append(_ entries: [JSON], _ level: Int) {
            for entry in entries.compactMap(\.object) {
                let label = Py.str(entry["label"] ?? .null)
                let action = entry["action"]?.array?.compactMap(\.string) ?? []
                if entry["separator"]?.isTruthy == true { rows.append(String(repeating: "-", count: 2 * level + 3)) }
                else if entry["section"]?.isTruthy == true { rows.append(Runtime.item(label, level: level, ["color": .string(Runtime.mutedColor), "size": 10])) }
                else if let children = entry["children"]?.array, !children.isEmpty {
                    let value = entry["value"].flatMap { $0.isTruthy ? " · " + Py.str($0) : nil } ?? ""
                    rows.append(Runtime.item(label + value, level: level))
                    if action.first == "set-module" {
                        rows.append(Runtime.item("显示在主页", level: level + 1, Runtime.merged(["checked": entry["checked"] ?? false], act(action[0], Array(action.dropFirst())))))
                    }
                    append(children, level + 1)
                } else if !action.isEmpty {
                    rows.append(Runtime.item(label, level: level, Runtime.merged(["checked": entry["checked"] ?? false], act(action[0], Array(action.dropFirst())))))
                } else {
                    rows.append(Runtime.item(label, level: level, ["size": 11]))
                }
            }
        }
        append(moduleOptions(JSONObject()), 1)
        return rows
    }

    /// Rendering only reads caches; network and log reads run in the background worker.
    public func gather() -> JSONObject {
        scheduleBackgroundRefresh()
        return cachedData()
    }

    /// SwiftBar's text for one run. With the native panel installed, its snapshot is written and the menu keeps
    /// only the right-click rows.
    public func render(_ data: JSONObject) -> String {
        let dashboard = dashboardPayload(data, rangeError: data["range_usage_error"]?.string)
        let options = moduleOptions(data)
        let native = [Runtime.item("主页模块…", Runtime.merged(["sfimage": "square.grid.2x2"], act("panel-modules")))] + detectionMenuLines() + aboutMenuLines()
        if environment["OCX_NATIVE_LAYOUT"] == "1" || !Harness.isDirectory(panelApp) {
            var overview: [String] = []
            let providers = (dashboard["providers"]?.array ?? []).compactMap(\.object)
            if moduleEnabled("summary") {
                let valid = providers.filter { $0["available"]?.isTruthy == true && ($0["summaryIncluded"] ?? true).isTruthy }
                let total = valid.reduce(0.0) { sum, provider in
                    let counted = provider["counted"]?.object.flatMap { $0.isEmpty ? nil : $0 } ?? provider
                    return sum + (counted["tokens"]?.pythonFloat ?? 0)
                }
                overview.append(Runtime.item(valid.isEmpty ? "Summary · 数据待同步"
                                             : "Summary · " + Py.str(dashboard["rangeLabel"] ?? .null) + " · " + Runtime.fmtTokens(.double(total)) + " Token"))
            }
            for provider in providers {
                let name = Py.str(provider["name"] ?? .null)
                overview.append(Runtime.item(name + " · " + (provider["available"]?.isTruthy == true ? Runtime.fmtTokens(provider["tokens"]) + " Token" : Py.str(provider["status"] ?? .null))))
            }
            let lines = overview + (overview.isEmpty ? [] : ["---"]) + managementLines(data) + ["---"] + moduleMenuLines()
            return ([title(data), "---"] + lines.map(Runtime.nativeMenuLine)).joined(separator: "\n")
        }
        writeJSONCache(cachePath("panel-view"), .object(panelSnapshot(data, dashboard: dashboard, options: options)))
        return ([panelTitle(title(data)), "---"] + native.map(Runtime.nativeMenuLine)).joined(separator: "\n")
    }

    /// The menu when reading failed: never a traceback, never a secret.
    func fallbackMenu(_ failure: String) -> String {
        [title(), "---", Runtime.item("数据读取异常 · " + failure), Runtime.item("打开控制台", ["href": .string(base + "/")]),
         Runtime.item("重新读取", act("refresh"))].joined(separator: "\n")
    }

    /// One run of the entry: a background mode, an action, or the menu.
    public func main(_ arguments: [String]) -> Int32 {
        if afterBackgroundReads == nil { afterBackgroundReads = { [unowned self] in self.afterReads() } }
        if selfUpdateLoader == nil { selfUpdateLoader = { [unowned self] in self.selfUpdateCached() } }
        syncConnectionCache()
        let first = arguments.first ?? ""
        switch first {
        case "--initialize-modules":
            if FileManager.default.fileExists(atPath: stateDir + "/preferences.json") { print("保留已有模块选择"); return 0 }
            let detected = detectedModules()
            print(Discovery.initialize(stateDir, detected: detected) ? "已自动选择模块：" + detected.joined(separator: ", ") : "保留已有模块选择")
            return 0
        case "--background-refresh", "--background-refresh-wait":
            backgroundRefresh(wait: first == "--background-refresh-wait")
            return 0
        case "--refresh-range":
            refreshUsageRange(wait: true)
            refreshSwiftBar()
            return 0
        case "--self-update":
            return selfUpdateWorker(arguments.count > 1 ? arguments[1] : "")
        case "--claude-statusline":
            let index = arguments.firstIndex(of: "--state-dir")
            guard let index, index + 1 < arguments.count else { return 2 }
            return ClaudeBridge.run(state: arguments[index + 1])
        case "--action":
            let name = arguments.count > 1 ? arguments[1] : ""
            writeJSONCache(cachePath("last-action"), ["action": .string(name), "receivedAt": .double(Date().timeIntervalSince1970)])
            do { return try runAction(name, Array(arguments.dropFirst(2))) } catch { return 1 }
        case "--refresh":
            return 0
        case "--worker":
            return worker(Array(arguments.dropFirst()))
        case "--refresh-claude-bridge":
            // After an install: a bridge that is on runs from this engine's new copy.
            if ClaudeBridge.enabled(stateDir) { try? ClaudeBridge.install(stateDir, environment: environment) }
            return 0
        default:
            consumePanelAction()
            // Actions above may change settings; the render below sees one consistent snapshot.
            print(self.render { render(gather()) })
            return 0
        }
    }

    /// The standalone app's private worker: the panel's snapshot after setup, an action, or the background reads.
    func worker(_ arguments: [String]) -> Int32 {
        func snapshot() { render { writeJSONCache(cachePath("panel-view"), .object(panelSnapshot(cachedData()))) } }
        switch arguments {
        case ["paths"]:
            print(JSON.object(["snapshot": .string(cachePath("panel-view"))]).serialized(ensureASCII: true))
        case ["smoke-test"]:
            // The test rewrites the module selection, so it runs only in the build's private home.
            guard environment["TOKEN_MONITOR_SMOKE_TEST"] == "1" else { return 2 }
            let root = stateDir + "/smoke-fixture"
            try? Files.makeDirectory(root)
            let record: JSON = ["t": .double(Date().timeIntervalSince1970 - 10), "agent": "codex", "provider": "test", "model": "gpt-5.4", "in": 1230, "out": 4]
            try? Data((record.serialized() + "\n").utf8).write(to: URL(fileURLWithPath: root + "/usage.jsonl"))
            var settings = Preferences.read(stateDir)
            settings["visible"] = ["summary", "heatmap", "magpie"]; settings["paths"] = ["magpie": .string(root)]
            guard (try? Preferences.write(stateDir, settings)) != nil else { return 1 }
            _ = render { rangeUsage("all") }
            writeJSONCache(cachePath("view-range"), ["schema": 1, "range": "all"])
            snapshot()
            let view = readCache("panel-view")["dashboard"]?.object ?? JSONObject()
            let tokens = view["providers"]?.array?.first?.object?["tokens"]?.pythonFloat
            let heat = (view["heatmap"]?.object?["tokens"]?.array ?? []).reduce(0.0) { $0 + ($1.pythonFloat ?? 0) }
            guard tokens == 1234, heat == 1234, Pricing.table("model-pricing.json")["source"]?.object?["commit"]?.isTruthy == true else { return 1 }
            guard (try? ClaudeBridge.publish(stateDir, JSONObject())) != nil else { return 1 }
        case ["prepare"], ["action"], ["refresh"]:
            if arguments == ["prepare"] && !FileManager.default.fileExists(atPath: stateDir + "/preferences.json") {
                Discovery.initialize(stateDir, detected: detectedModules())
            }
            if arguments == ["action"] { consumePanelAction() }
            if arguments == ["refresh"] { backgroundRefresh() }
            // Published before any long read; clicks are acknowledged on their own.
            snapshot()
        default:
            return 2
        }
        return 0
    }
}
