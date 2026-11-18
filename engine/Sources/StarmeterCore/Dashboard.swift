import Foundation

/// The homepage's Summary, chart, heatmap and cards from a range bundle: the port of dashboard_payload and its
/// helpers in swiftbar_plugin.py, producing the same JSON the panel reads.
extension Runtime {
    static let shortRanges: Set<String> = ["today", "yesterday", "24h"]
    static let streakRanges: Set<String> = ["7d", "week", "30d", "month", "all"]
    static let weekdays = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
    static let heatmapDays = 360, heatmapRow = 30
    static let groupings: [(String, String)] = [("tool", "工具"), ("model", "模型"), ("project", "项目")]
    static let gateways: Set<String> = ["opencodex", "magpie"]
    static let groupCards = 9
    static let groupFields = ["tokens", "cost", "requests", "unpriced", "output", "credits", "cached", "inputs"]
    static let modelMakers: [(String, String)] = [("claude", "claude"), ("gpt", "codex"), ("chatgpt", "codex"), ("codex", "codex"), ("o1", "codex"),
        ("o3", "codex"), ("o4", "codex"), ("gemini", "gemini"), ("deepseek", "deepseek-harness"), ("kimi", "kimi-api"), ("moonshot", "kimi-api"),
        ("grok", "grok"), ("minimax", "minimax"), ("abab", "minimax"), ("qwen", "qoder"), ("qfmodel", "qoder"), ("qoder", "qoder"),
        ("composer", "cursor"), ("cursor", "cursor")]
    static let magpieClientNames = ["codex": "ChatGPT", "claude": "Claude Code", "claude-code": "Claude Code", "claude-desktop": "Claude",
                                    "opencode": "OpenCode", "dsh": "DeepSeek Harness", "gemini": "Gemini CLI", "gemini-cli": "Gemini CLI",
                                    "agy": "Antigravity", "antigravity": "Antigravity", "pi": "Pi", "cursor": "Cursor", "": "未标注"]
    static let brandLogos: [String: (String, Bool)] = ["ChatGPT": ("openai.svg", true), "Claude": ("claude.svg", false), "MiniMax": ("minimax.svg", false),
        "Gemini": ("gemini.svg", false), "Grok": ("grok.svg", true), "Kimi": ("kimi.svg", true), "Qoder": ("qoder.svg", true), "Cursor": ("cursor.svg", true)]
    static let moduleLogos: [String: (String, Bool)] = ["magpie": ("magpie.svg", true), "claude": ("claude.svg", false), "codex": ("openai.svg", true),
        "opencode": ("opencode.svg", true), "workbuddy": ("workbuddy.svg", true), "qoder": ("qoder.svg", true), "gemini": ("gemini.svg", false), "cursor": ("cursor.svg", true)]
    static let moduleSymbols = ["summary": "chart.xyaxis.line", "heatmap": "square.grid.3x3.fill", "opencodex": "network", "komari": "server.rack",
                                "updates": "arrow.down.circle"]

    // MARK: Icons

    /// The installed assets: the hidden folder beside the entry, else one beside it.
    var assetDir: String {
        let folder = (Runtime.absolute(plugin) as NSString).deletingLastPathComponent
        return Runtime.installed(folder, "assets", marker: "logos") ?? folder + "/assets"
        return folder + "/assets"
    }

    func logo(_ file: String) -> String? {
        memoized("logo:" + file) {
            guard let data = FileManager.default.contents(atPath: assetDir + "/" + file), !data.isEmpty else { return nil as String? }
            return data.base64EncodedString()
        }
    }

    /// A module's icon for the panel: an image (template when monochrome) or a system symbol.
    public func moduleIcon(_ key: String) -> JSONObject {
        if let service = Catalog.byKey[key] {
            var raw: String?, template = false
            if service.brand == "DeepSeek" { raw = logo("deepseek-logo.svg") }
            else if let (file, mono) = Runtime.brandLogos[service.brand] { raw = logo("logos/" + file); template = mono }
            if let raw { return ["image": .string(raw), "template": .bool(template)] }
            return ["symbol": .string(service.category == "订阅" ? "person.crop.circle" : "creditcard")]
        }
        var raw: String?, template = false
        if key == "deepseek-harness" { raw = logo("deepseek-logo.svg") }
        else if let (file, mono) = Runtime.moduleLogos[key] { raw = logo("logos/" + file); template = mono }
        if let raw { return ["image": .string(raw), "template": .bool(template)] }
        return ["symbol": .string(Runtime.moduleSymbols[key] ?? "square.grid.2x2")]
    }

    // MARK: Small helpers

    /// The dashboard's number(): float(value) within 0..1e20, else 0.0.
    static func figure(_ value: JSON?) -> Double {
        guard let value, let number = value.pythonFloat, number >= 0, number < 1e20 else { return 0 }
        return number
    }

    /// min(1, x) as Python returns it: the integer 1 when x is not smaller.
    static func capped(_ value: Double) -> JSON { value < 1 ? .double(value) : .int(1) }

    /// round(x, 6) as Python rounds: the exact value to six decimals.
    static func round6(_ value: Double) -> Double { Double(String(format: "%.6f", value)) ?? value }

    static func summaryCostValue(_ summary: JSONObject?) -> JSON? {
        guard let summary else { return nil }
        for key in ["estimatedCostUsd", "costUsd", "totalCostUsd", "totalCost", "cost"] {
            if let value = summary[key], !value.isNull, value != .string("") { return value }
        }
        return nil
    }

    static func refreshFailed(_ error: String?) -> Bool { error != nil && error != Runtime.pendingRead }

    /// A gateway's clients by their share of its tokens, largest first; one name per client.
    static func clientRows(_ clients: JSON?) -> [JSON] {
        var merged = Ordered<String, Double>()
        for row in clients?.array ?? [] {
            let agent = row["agent"].map { Py.str(Py.or($0, .string(""))) } ?? ""
            let name = magpieClientNames[agent] ?? Py.str(row["agent"] ?? .null)
            merged[name] = (merged[name] ?? 0) + Swift.max(0, Py.or(row["totalTokens"], .int(0)).pythonFloat ?? 0)
        }
        let total = merged.items.reduce(0) { $0 + $1.1 }
        guard total > 0 else { return [] }
        return stableSorted(merged.items, by: { -$0.1 }).filter { $0.1 > 0 }.map { ["name": .string($0.0), "tokens": .double($0.1), "share": .double($0.1 / total)] }
    }

    /// Python's sorted(key=...): ascending by key, equal keys keep their order.
    static func stableSorted<T>(_ items: [T], by key: (T) -> Double) -> [T] {
        items.enumerated().sorted { a, b in
            let x = key(a.element), y = key(b.element)
            return x != y ? x < y : a.offset < b.offset
        }.map(\.element)
    }

    /// How API-equivalent estimates read: nil for US dollars, else the yuan with its rate and where it came from.
    func currencyUnit(_ data: JSONObject) -> JSON {
        let settings = self.settings()
        guard settings["currency"] == .string("CNY") else { return .null }
        if let rate = settings["cnyRate"], rate.isTruthy, let value = rate.number {
            return ["code": "CNY", "symbol": "¥", "rate": rate, "note": .string(String(format: "手动汇率 %.4f", value))]
        }
        let known = data["exchange_rate"]?.object ?? JSONObject()
        guard Preferences.exchangeRate(known["rate"]), let rate = known["rate"]?.pythonFloat else { return .null }
        return ["code": "CNY", "symbol": "¥", "rate": .double(rate),
                "note": .string(String(format: "欧洲央行 %@ 参考汇率 %.4f", Py.str(known["date"] ?? .string("")), rate))]
    }

    /// Wall-clock text of a moment in the zone, by the strftime pieces the dashboard uses.
    static func format(_ moment: Double, _ zone: TimeZone, _ pattern: String) -> String {
        let wall = Wall(moment, zone)
        let hour = Int(wall.seconds / 3600), minute = Int(wall.seconds / 60 % 60)
        return pattern.replacingOccurrences(of: "%Y", with: String(format: "%04d", wall.year))
            .replacingOccurrences(of: "%m", with: String(format: "%02d", wall.month))
            .replacingOccurrences(of: "%d", with: String(format: "%02d", wall.day))
            .replacingOccurrences(of: "%H", with: String(format: "%02d", hour))
            .replacingOccurrences(of: "%M", with: String(format: "%02d", minute))
    }

    static func dayNumber(_ moment: Double, _ zone: TimeZone) -> Int {
        let wall = Wall(moment, zone)
        return daysFromCivil(wall.year, wall.month, wall.day)
    }

    static func iso(_ day: Int) -> String {
        let (y, m, d) = civilFromDays(day)
        return String(format: "%04d-%02d-%02d", y, m, d)
    }

    static func dayFromISO(_ text: String) -> Int? {
        let parts = text.split(separator: "-")
        guard parts.count == 3, let y = Int(parts[0]), let m = Int(parts[1]), let d = Int(parts[2]) else { return nil }
        return daysFromCivil(y, m, d)
    }

    // MARK: Adding up a source's parts

    /// Tallies of one source's records added up: summary fields, models, days and hours.
    static func addUsage(_ parts: [JSONObject]) -> JSONObject {
        var summary = JSONObject()
        var tables: [String: Ordered<String, JSONObject>] = ["models": Ordered(), "days": Ordered(), "hours": Ordered()]
        var firsts: [JSON] = []
        func add(_ target: inout JSONObject, _ row: JSONObject, skip: Set<String>) {
            for (key, value) in row where !skip.contains(key) && value.number != nil {
                target[key] = Harness.add(target[key] ?? .int(0), value)
            }
        }
        for part in parts {
            if let first = part["firstActivityAt"], !first.isNull { firsts.append(first) }
            add(&summary, part["summary"]?.object ?? JSONObject(), skip: ["coverageRatio", "sessions"])
            for (field, key) in [("models", "model"), ("days", "date"), ("hours", "timestamp")] {
                for row in part[field]?.array ?? [] {
                    guard let fields = row.object else { continue }
                    let id = Py.dumps(fields[key] ?? .null)
                    var target = tables[field]![id] ?? [key: fields[key] ?? .null]
                    add(&target, fields, skip: [key])
                    tables[field]![id] = target
                }
            }
        }
        let requests = summary["requests"]?.number ?? 0, unpriced = summary["unpricedRequests"]?.number ?? 0
        summary["coverageRatio"] = .double((requests - unpriced) / Swift.max(1, requests))
        var first: JSON = .null
        for value in firsts where first.isNull || (value.number ?? 0) < (first.number ?? 0) { first = value }
        return ["summary": .object(summary), "models": .array(tables["models"]!.items.map { .object($0.1) }),
                "days": .array(tables["days"]!.items.map { .object($0.1) }), "hours": .array(tables["hours"]!.items.map { .object($0.1) }),
                "firstActivityAt": first]
    }

    /// What Summary adds for each selected source: matched request by request, a gateway counts what it carried and
    /// the client adds only the rest. "usage" is nil when Summary adds nothing for it.
    static func summaryUsage(_ selected: [(String, String, JSONObject)]) -> [String: JSONObject] {
        let available = Set(selected.filter { $0.2["summary"]?.isTruthy == true && $0.2["error"]?.isTruthy != true }.map(\.0))
        var result: [String: JSONObject] = [:]
        for (source, _, usage) in selected {
            var carried = JSONObject()
            for (name, count) in usage["carried"]?.object ?? JSONObject() where count.isTruthy && available.contains(name.lowercased()) { carried[name] = count }
            // A gateway with nothing to show can't count what it carried, so the client counts those again.
            let missing = (usage["carriedParts"]?.object ?? JSONObject()).filter { carried[$0.key] == nil }.compactMap { $0.value.object }
            let counted: JSONObject
            if let base = usage["counted"]?.object, base.isTruthy, !missing.isEmpty { counted = addUsage([base] + missing) }
            else { counted = usage["counted"]?.object ?? JSONObject() }
            var entry: JSONObject
            // Without OpenCodex's own log the proxy stays the summary authority for the local ChatGPT records.
            if source == "codex" && available.contains("opencodex") && usage["proxyMatched"]?.isTruthy != true {
                entry = ["usage": .null, "proxy": true]
            } else if !carried.isEmpty && (counted["summary"]?.object?["requests"]?.isTruthy != true) {
                entry = ["usage": .null]
            } else {
                entry = ["usage": .object(carried.isEmpty ? usage : counted)]
            }
            entry["carried"] = .object(carried)
            entry["counted"] = carried.isEmpty ? .null : .object(counted)
            result[source] = entry
        }
        return result
    }

    // MARK: Model and project cards

    struct Group {
        var fields: [String: Double] = Dictionary(uniqueKeysWithValues: Runtime.groupFields.map { ($0, 0) })
        var tools = Ordered<String, Double>()
        var toolNames: [String: String] = [:]
    }

    /// One model or project row of a tool, added to its group.
    static func groupAdd(_ groups: inout Ordered<String, Group>, _ key: String, _ tool: (id: String, label: String), _ row: JSONObject) {
        func num(_ name: String) -> Double { Swift.max(0, Py.or(row[name], .int(0)).pythonFloat ?? 0) }
        var group = groups[key] ?? Group()
        let tokens = num("totalTokens")
        let cached = num("cacheReadInputTokens") != 0 ? num("cacheReadInputTokens") : num("cachedInputTokens")
        let inputs = row["cacheEligibleInputTokens"] != nil ? num("cacheEligibleInputTokens") : num("inputTokens")
        for (field, value) in [("tokens", tokens), ("cost", num("estimatedCostUsd")), ("requests", num("requests")), ("unpriced", num("unpricedRequests")),
                               ("output", num("outputTokens")), ("credits", num("credits")), ("cached", cached), ("inputs", inputs)] {
            group.fields[field]! += value
        }
        let id = tool.id + "\u{1F}" + tool.label
        group.tools[id] = (group.tools[id] ?? 0) + (tokens != 0 ? tokens : num("credits"))
        group.toolNames[id] = tool.label
        groups[key] = group
    }

    static func groupMerge(_ parts: [Group]) -> Group {
        var total = Group()
        for group in parts {
            for field in groupFields { total.fields[field]! += group.fields[field]! }
            for (tool, amount) in group.tools.items { total.tools[tool] = (total.tools[tool] ?? 0) + amount; total.toolNames[tool] = group.toolNames[tool] }
        }
        return total
    }

    /// A model or project card in the shape of a tool's, with the tools that make it up as its rows.
    static func groupCard(_ id: String, _ name: String, _ mark: JSONObject, _ accent: String?, _ g: Group) -> JSON {
        let weight = g.tools.items.reduce(0) { $0 + $1.1 }
        let divisor = weight != 0 ? weight : 1
        let requests = Int64(g.fields["requests"]!), unpriced = Int64(g.fields["unpriced"]!)
        let tokens = g.fields["tokens"]!, credits = g.fields["credits"]!
        let tools: [JSON] = stableSorted(g.tools.items, by: { -$0.1 }).prefix(3).map { tool, amount in
            var row: JSONObject = ["name": .string(g.toolNames[tool] ?? tool), "tokens": tokens != 0 ? .double(amount) : .int(0),
                                   "share": capped(amount / divisor)]
            if tokens == 0 { row["credits"] = .double(amount) }
            return .object(row)
        }
        let priced = requests > unpriced
        var card: JSONObject = ["id": .string(id), "name": .string(name)]
        for (key, value) in mark { card[key] = value }
        card["accent"] = accent.map(JSON.string) ?? .null
        card["subtitle"] = ""
        card["status"] = .string(priced || credits != 0 ? "已同步" : "费用待定价")
        card["available"] = true
        card["credits"] = credits != 0 ? .double(credits) : .null
        card["clients"] = .null
        card["cost"] = priced ? .double(g.fields["cost"]!) : .null
        card["tokens"] = .double(tokens)
        card["requests"] = .int(requests)
        card["output"] = .double(g.fields["output"]!)
        card["cache"] = g.fields["inputs"]! != 0 ? capped(g.fields["cached"]! / g.fields["inputs"]!) : .null
        card["coverage"] = .null
        card["sessions"] = .int(0)
        card["unpriced"] = .int(unpriced)
        card["models"] = .array(tools)
        card["history"] = .array([])
        card["summaryIncluded"] = false
        card["priceNote"] = 0 < unpriced && unpriced < requests && credits == 0 ? .string("\(unpriced) 次未定价") : .null
        return .object(card)
    }

    /// Groups with requests, the costliest first; past the limit the rest come back merged, with how many.
    static func rankedGroups(_ groups: Ordered<String, Group>) -> ([(String, Group)], Group?, Int) {
        let order = groups.items.filter { $0.1.fields["requests"]! != 0 }.enumerated().sorted { a, b in
            let x = a.element.1.fields, y = b.element.1.fields
            for field in ["cost", "tokens", "credits"] where x[field]! != y[field]! { return x[field]! > y[field]! }
            return a.offset < b.offset
        }.map(\.element)
        let rest = order.count > groupCards + 1 ? Array(order[groupCards...]) : []
        return (Array(order.prefix(order.count - rest.count)), rest.isEmpty ? nil : groupMerge(rest.map(\.1)), rest.count)
    }

    /// One name per model across tools: lower case, without a provider's path or a date stamp.
    public static func modelKey(_ name: JSON?) -> String {
        var text = Py.str(Py.or(name, .string(""))).trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        text = text.split(separator: "/", omittingEmptySubsequences: false).last.map(String.init) ?? text
        let bytes = Array(text.utf8)
        if bytes.count >= 9, [0x2D, 0x40].contains(bytes[bytes.count - 9]), bytes[bytes.count - 8] == 0x32, bytes[bytes.count - 7] == 0x30,
           bytes[(bytes.count - 6)...].allSatisfy({ (0x30...0x39).contains($0) }) {
            text = String(decoding: bytes[..<(bytes.count - 9)], as: UTF8.self)
        }
        return text.isEmpty ? "未知模型" : text
    }

    /// A model card's name: Claude's family and version (claude-opus-5-5 → Claude Opus 5.5), other names as they are.
    public static func modelTitle(_ key: String) -> String {
        for family in ["opus", "sonnet", "haiku", "fable"] {
            let prefix = "claude-" + family + "-"
            if key.hasPrefix(prefix), key.count > prefix.count {
                return "Claude " + family.prefix(1).uppercased() + family.dropFirst() + " " + key.dropFirst(prefix.count).replacingOccurrences(of: "-", with: ".")
            }
        }
        return key
    }

    func modelCards(_ selected: [(String, String, JSONObject)], _ counting: [String: JSONObject]) -> [JSON] {
        var groups = Ordered<String, Group>()
        for (source, label, _) in selected {
            guard let usage = counting[source]?["usage"]?.object, usage.isTruthy, usage["error"]?.isTruthy != true else { continue }
            for row in usage["models"]?.array ?? [] { if let fields = row.object { Runtime.groupAdd(&groups, Runtime.modelKey(fields["model"]), (label, label), fields) } }
        }
        let (shown, rest, count) = Runtime.rankedGroups(groups)
        var cards = shown.map { key, g -> JSON in
            let maker = Runtime.modelMakers.first { key.hasPrefix($0.0) }?.1
            return Runtime.groupCard("model:" + key, Runtime.modelTitle(key), maker.map(moduleIcon) ?? ["symbol": "cpu"], maker, g)
        }
        if let rest { cards.append(Runtime.groupCard("model:", "其他 \(count) 个模型", ["symbol": "ellipsis.circle"], nil, rest)) }
        return cards
    }

    func projectCards(_ selected: [(String, String, JSONObject)]) -> [JSON] {
        var groups = Ordered<String, Group>()
        for (source, label, usage) in selected {
            guard !Runtime.gateways.contains(source), usage["summary"]?.isTruthy == true, usage["error"]?.isTruthy != true else { continue }
            // A tool that keeps no project lands in 未标注项目 whole.
            var rows = usage["projects"]?.array ?? []
            if rows.isEmpty, var summary = usage["summary"]?.object { summary["project"] = ""; rows = [.object(summary)] }
            for row in rows {
                if let fields = row.object { Runtime.groupAdd(&groups, Py.str(Py.or(fields["project"], .string(""))), (source, label), fields) }
            }
        }
        let unmarked = groups[""]
        groups[""] = nil
        let (shown, rest, count) = Runtime.rankedGroups(groups)
        func lead(_ g: Group) -> String {
            var best: (String, Double)?
            for (tool, amount) in g.tools.items where best == nil || amount > best!.1 { best = (tool, amount) }
            return String(best?.0.split(separator: "\u{1F}").first ?? "")
        }
        var cards = shown.map { Runtime.groupCard("project:" + $0.0, $0.0, ["symbol": "folder"], lead($0.1), $0.1) }
        if let rest { cards.append(Runtime.groupCard("project:*", "其他 \(count) 个项目", ["symbol": "ellipsis.circle"], lead(rest), rest)) }
        if let unmarked, unmarked.fields["requests"]! != 0 {
            cards.append(Runtime.groupCard("project:", "未标注项目", ["symbol": "questionmark.folder"], lead(unmarked), unmarked))
        }
        return cards
    }
}
