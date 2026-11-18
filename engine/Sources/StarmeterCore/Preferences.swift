import Foundation

/// Local presentation preferences in preferences.json. No credentials or usage data live here. The file is
/// shared with the Python engine, so values are kept as written and read back the same way it reads them.
public enum Preferences {
    public struct Invalid: Error, CustomStringConvertible {
        public let description: String
        init(_ description: String) { self.description = description }
    }

    public static let modules: [(key: String, label: String, category: String)] = [
        ("summary", "Summary", "概览"),
        ("heatmap", "用量热力图", "概览"),
        ("opencodex", "OpenCodex", "Harness"),
        ("magpie", "Magpie", "Harness"),
        ("claude", "Claude", "Harness"),
        ("codex", "ChatGPT", "Harness"),
        ("opencode", "OpenCode", "Harness"),
        ("deepseek-harness", "DeepSeek Harness", "Harness"),
        ("workbuddy", "WorkBuddy", "Harness"),
        ("qoder", "Qoder CN", "Harness"),
        ("gemini", "Gemini CLI", "Harness"),
        ("cursor", "Cursor", "Harness"),
    ] + Catalog.services.map { ($0.key, $0.label, $0.category) } + [
        ("sessions", "活跃会话", "其他"),
        ("komari", "Komari 监控", "其他"),
        ("updates", "Harness 更新", "其他"),
    ]
    /// Usage alert levels in percent; 0 is off.
    public static let alertLevels: [Int64] = [0, 70, 80, 90]
    /// The panel follows macOS/Windows, or stays light or dark.
    public static let themes = ["system", "light", "dark"]
    /// The Harness cards on the homepage, one per tool (the default), per model or per project.
    public static let groupings = ["tool", "model", "project"]
    /// Beside the menu bar star: nothing (the default), today's API-equivalent estimate, or the tightest subscription window.
    public static let menuBar = ["none", "cost", "quota"]
    /// API-equivalent estimates read in US dollars, or in yuan at the day's reference rate (or one the user fixed).
    public static let currencies = ["USD", "CNY"]
    /// How often a pinned panel re-reads the range on screen, in seconds.
    public static let pinIntervals: [Int64] = [1, 2, 5]
    public static let defaultVisible = ["summary", "heatmap", "opencodex", "claude", "deepseek", "chatgpt", "sessions", "komari", "updates"]
    public static let known = Set(modules.map(\.key))
    /// Modules added after release show once for people who keep Summary on; a saved file lists the modules it
    /// already knew, so hiding one of these afterwards sticks.
    public static let introduced = ["heatmap", "sessions"]
    /// A new module that only means something beside these is introduced only to those who show one of them.
    public static let introducedWith = ["sessions": ["claude", "codex"]]
    public static let usage = ["opencodex", "magpie", "claude", "codex", "opencode", "deepseek-harness", "workbuddy", "qoder", "gemini", "cursor"]

    /// A balance alert's amount: a positive finite number, given as a number or its text.
    static func balanceLine(_ value: JSON?) -> Bool { inRange(value, above: 0, below: 1e9) }
    /// Yuan to the dollar, within reason.
    static func exchangeRate(_ value: JSON?) -> Bool { inRange(value, above: 1, below: 20) }
    /// A subscription's monthly price in US dollars, as the user wrote it down.
    static func monthlyPrice(_ value: JSON?) -> Bool { inRange(value, above: 0, below: 1e5) }

    private static func inRange(_ value: JSON?, above low: Double, below high: Double) -> Bool {
        guard let value, value.bool == nil, let number = value.pythonFloat else { return false }
        return low < number && number < high
    }

    /// A balance alert is kept per API and currency, as "kimi-api|USD".
    static func balanceKey(_ key: String) -> Bool {
        let parts = key.split(separator: "|", maxSplits: 1, omittingEmptySubsequences: false)
        let currency = parts.count > 1 ? String(parts[1]) : ""
        return Catalog.balanceAPIs.contains(String(parts[0])) && Catalog.currencies.contains(currency)
    }

    public static func defaults() -> JSONObject {
        [
            "schema": 1, "visible": .array(defaultVisible.map(JSON.string)), "appearance": "glass", "theme": "system",
            "paths": .object([:]), "allowCollapse": false, "expandedProviders": [], "revision": 0, "configuredSubscriptions": [],
            "claudeLiveUsage": false, "claudeOAuthUsage": false, "cursorUsage": false, "usageAlert": 90, "resetReminder": false,
            "balanceAlerts": .object([:]), "keepCleanedUsage": true, "subscriptionPrices": .object([:]), "groupBy": "tool",
            "menuBar": "none", "sessionAlerts": false, "serviceStatus": false, "currency": "USD", "cnyRate": .null, "pinInterval": 2,
        ]
    }

    static func file(_ directory: String) -> String { (directory as NSString).appendingPathComponent("preferences.json") }

    public static func read(_ directory: String) -> JSONObject {
        var settings = defaults()
        guard let data = FileManager.default.contents(atPath: file(directory)),
              let value = (try? JSON.parse(data))?.object, value["schema"] == .int(1) else { return settings }

        if let visible = value["visible"]?.array, visible.allSatisfy({ $0.string != nil }) {
            let names = visible.compactMap(\.string)
            // The ChatGPT subscription source now follows the OpenCodex selection; no separate choice.
            var shown: [String] = []
            for name in names where known.contains(name) || name == "codex-quota" {
                let key = name == "codex-quota" ? "chatgpt" : name
                if !shown.contains(key) { shown.append(key) }
            }
            let knownBefore = value["modules"]?.array ?? []
            let new = introduced.filter { key in
                !knownBefore.contains(.string(key)) && !shown.contains(key) && shown.contains("summary")
                    && (introducedWith[key] ?? shown).contains { shown.contains($0) }
            }
            if !new.isEmpty, let at = shown.firstIndex(of: "summary") { shown.insert(contentsOf: new, at: at + 1) }
            settings["visible"] = .array(shown.map(JSON.string))
        }
        if let chosen = value["configuredSubscriptions"]?.array {
            settings["configuredSubscriptions"] = .array(Catalog.manualSubscriptions.filter { chosen.contains(.string($0)) }.map(JSON.string))
        }
        if let appearance = value["appearance"]?.string, ["glass", "solid"].contains(appearance) { settings["appearance"] = .string(appearance) }
        if let theme = value["theme"]?.string, themes.contains(theme) { settings["theme"] = .string(theme) }
        for flag in ["allowCollapse", "claudeLiveUsage", "claudeOAuthUsage", "cursorUsage"] {
            if let on = value[flag]?.bool { settings[flag] = .bool(on) }
        }
        if let level = value["usageAlert"], let number = level.number, alertLevels.contains(where: { Double($0) == number }) {
            settings["usageAlert"] = level
        }
        if let on = value["resetReminder"]?.bool { settings["resetReminder"] = .bool(on) }
        if let on = value["keepCleanedUsage"]?.bool { settings["keepCleanedUsage"] = .bool(on) }
        if let grouping = value["groupBy"]?.string, groupings.contains(grouping) { settings["groupBy"] = .string(grouping) }
        if let shown = value["menuBar"]?.string, menuBar.contains(shown) { settings["menuBar"] = .string(shown) }
        if let on = value["sessionAlerts"]?.bool { settings["sessionAlerts"] = .bool(on) }
        if let on = value["serviceStatus"]?.bool { settings["serviceStatus"] = .bool(on) }
        if let currency = value["currency"]?.string, currencies.contains(currency) { settings["currency"] = .string(currency) }
        if let interval = value["pinInterval"], let number = interval.number, pinIntervals.contains(where: { Double($0) == number }) {
            settings["pinInterval"] = interval
        }
        if exchangeRate(value["cnyRate"]), let rate = value["cnyRate"]?.pythonFloat { settings["cnyRate"] = .double(rate) }
        if let prices = value["subscriptionPrices"]?.object {
            var kept = JSONObject()
            for (key, price) in prices where Catalog.subscriptionUsage[key] != nil && monthlyPrice(price) { kept[key] = price }
            settings["subscriptionPrices"] = .object(kept)
        }
        if let lines = value["balanceAlerts"]?.object {
            // 5.14.0 kept one line per API, asked in the API's usual currency.
            var migrated = JSONObject()
            for (key, line) in lines { migrated[key.contains("|") ? key : key + "|" + (Catalog.balanceCurrencies[key] ?? "")] = line }
            var kept = JSONObject()
            for (key, line) in migrated where balanceKey(key) && balanceLine(line) { kept[key] = line }
            settings["balanceAlerts"] = .object(kept)
        }
        if let expanded = value["expandedProviders"]?.array {
            settings["expandedProviders"] = .array(usage.filter { expanded.contains(.string($0)) }.map(JSON.string))
        } else if let open = value["expandedProvider"]?.string, usage.contains(open) {
            // Migrate 5.1.0 without losing the open section.
            settings["expandedProviders"] = [.string(open)]
        }
        if let revision = value["revision"], let number = revision.integer, number >= 0 { settings["revision"] = revision }
        if let paths = value["paths"]?.object {
            var kept = JSONObject()
            for (key, path) in paths where (usage.contains(key) || key == "claude-app") && !(path.string ?? "").isEmpty { kept[key] = path }
            settings["paths"] = .object(kept)
        }
        return settings
    }

    public static func write(_ directory: String, _ value: JSONObject) throws {
        var value = value
        value["revision"] = .int(max(timeNanoseconds(), (read(directory)["revision"]?.integer ?? 0) + 1))
        value["modules"] = .array(modules.map { .string($0.key) })
        try Files.makeDirectory(directory)
        let data = Data(JSON.object(value).serialized().utf8)
        // read() fell back to defaults for a file it couldn't use; keep the user's original before replacing it.
        try Files.replaceAtomically(file(directory), with: data, prefix: ".preferences-") {
            try Files.keepUnreadable(file(directory))
        }
    }

    @discardableResult
    public static func update(_ directory: String, kind: String, key: String? = nil, value: String? = nil) throws -> JSONObject {
        var current = read(directory)
        func flag(_ name: String, _ message: String) throws {
            guard let key, ["true", "false"].contains(key) else { throw Invalid(message) }
            current[name] = .bool(key == "true")
        }
        func listed(_ name: String) -> [String] { current[name]?.array?.compactMap(\.string) ?? [] }
        func object(_ name: String) -> JSONObject { current[name]?.object ?? JSONObject() }

        switch kind {
        case "module":
            let name = key == "codex-quota" ? "chatgpt" : key  // Existing queued callbacks remain valid.
            guard let name, known.contains(name), let value, ["true", "false"].contains(value) else { throw Invalid("invalid module preference") }
            var visible = Set(listed("visible"))
            if value == "true" { visible.insert(name) } else { visible.remove(name) }
            current["visible"] = .array(modules.map(\.key).filter(visible.contains).map(JSON.string))
        case "claude-live": try flag("claudeLiveUsage", "invalid Claude live usage setting")
        case "claude-oauth": try flag("claudeOAuthUsage", "invalid Claude OAuth setting")
        case "cursor-usage": try flag("cursorUsage", "invalid Cursor usage setting")
        case "usage-alert":
            guard let key, let level = alertLevels.first(where: { String($0) == key }) else { throw Invalid("invalid usage alert") }
            current["usageAlert"] = .int(level)
        case "reset-reminder": try flag("resetReminder", "invalid reset reminder")
        case "keep-usage": try flag("keepCleanedUsage", "invalid keep usage setting")
        case "pin-interval":
            guard let key, let interval = pinIntervals.first(where: { String($0) == key }) else { throw Invalid("invalid pin interval") }
            current["pinInterval"] = .int(interval)
        case "currency":
            guard let key, currencies.contains(key) else { throw Invalid("invalid currency") }
            current["currency"] = .string(key)
        case "cny-rate":
            if key == nil || key == "" {
                current["cnyRate"] = .null
            } else if exchangeRate(key.map(JSON.string)), let rate = key.flatMap(parsePythonFloat) {
                current["cnyRate"] = .double(rate)
            } else {
                throw Invalid("invalid exchange rate")
            }
        case "service-status": try flag("serviceStatus", "invalid service status setting")
        case "session-alerts": try flag("sessionAlerts", "invalid session alert setting")
        case "menu-bar":
            guard let key, menuBar.contains(key) else { throw Invalid("invalid menu bar value") }
            current["menuBar"] = .string(key)
        case "group":
            guard let key, groupings.contains(key) else { throw Invalid("invalid grouping") }
            current["groupBy"] = .string(key)
        case "subscription-price":
            guard let key, Catalog.subscriptionUsage[key] != nil else { throw Invalid("invalid subscription") }
            var prices = object("subscriptionPrices")
            if value == nil || value == "" {
                prices[key] = nil
            } else if monthlyPrice(value.map(JSON.string)), let price = value.flatMap(parsePythonFloat) {
                prices[key] = .double(price)
            } else {
                throw Invalid("invalid subscription price")
            }
            current["subscriptionPrices"] = .object(prices)
        case "balance-alert":
            guard balanceKey(key ?? "None") else { throw Invalid("invalid balance alert") }
            var lines = object("balanceAlerts")
            if value == nil || value == "" || value == "0" {
                lines[key!] = nil
            } else if balanceLine(value.map(JSON.string)), let line = value.flatMap(parsePythonFloat) {
                lines[key!] = .double(line)
            } else {
                throw Invalid("invalid balance alert")
            }
            current["balanceAlerts"] = .object(lines)
        case "subscription":
            guard let key, Catalog.manualSubscriptions.contains(key), let value, ["true", "false"].contains(value) else {
                throw Invalid("invalid subscription")
            }
            var chosen = Set(listed("configuredSubscriptions"))
            if value == "true" { chosen.insert(key) } else { chosen.remove(key) }
            current["configuredSubscriptions"] = .array(Catalog.manualSubscriptions.filter(chosen.contains).map(JSON.string))
        case "appearance":
            guard let key, ["glass", "solid"].contains(key) else { throw Invalid("invalid appearance") }
            current["appearance"] = .string(key)
        case "theme":
            guard let key, themes.contains(key) else { throw Invalid("invalid theme") }
            current["theme"] = .string(key)
        case "collapse":
            try flag("allowCollapse", "invalid collapse setting")
            // Turning folding on starts with every card folded.
            if key == "true" { current["expandedProviders"] = [] }
        case "expanded":
            guard let key, usage.contains(key), let value, ["true", "false"].contains(value) else { throw Invalid("invalid expansion preference") }
            var expanded = Set(listed("expandedProviders"))
            if value == "true" { expanded.insert(key) } else { expanded.remove(key) }
            current["expandedProviders"] = .array(usage.filter(expanded.contains).map(JSON.string))
        case "reset":
            current["visible"] = .array(defaultVisible.map(JSON.string))
            current["appearance"] = "glass"
            current["theme"] = "system"
            current["allowCollapse"] = false
            current["expandedProviders"] = []
        case "auto-path":
            guard let key, usage.contains(key) else { throw Invalid("invalid data source") }
            var paths = object("paths")
            paths[key == "claude" ? "claude-app" : key] = nil
            current["paths"] = .object(paths)
        case "path":
            guard let key, usage.contains(key), let value, !value.isEmpty, Paths.exists(Paths.expandUser(value)),
                  let resolved = Paths.resolve(Paths.expandUser(value)) else { throw Invalid("invalid data directory") }
            var paths = object("paths")
            paths[key == "claude" ? "claude-app" : key] = .string(resolved)
            current["paths"] = .object(paths)
        default:
            throw Invalid("invalid preference action")
        }
        try write(directory, current)
        return read(directory)
    }
}
