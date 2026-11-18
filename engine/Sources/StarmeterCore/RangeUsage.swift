import Foundation
#if os(Windows)
import WinSDK
#endif

/// The statistics range on screen, read from every selected Harness: the port of range_usage_cached and the
/// helpers around it in swiftbar_plugin.py.
extension Runtime {
    /// OCX_TIMEZONE or TZ when set and known, else the system's zone.
    public func localZone() -> TimeZone {
        for key in ["OCX_TIMEZONE", "TZ"] {
            if let name = env(key) { if let zone = TimeZone(identifier: name) { return zone } else { break } }
        }
        return .current
    }

    public func selectedRange() -> String {
        // A desktop host keeps the range with its settings, not in the cache.
        let value = (desktop != nil ? ClaudeApp.readObject(stateDir + "/view-range.json") : readCache("view-range"))["range"]?.string ?? "today"
        return Runtime.ranges.contains { $0.0 == value } ? value : "today"
    }

    /// A wall-clock time in a zone, as an aware Python datetime is: arithmetic on the wall clock, the offset
    /// looked up when it becomes a moment.
    struct Wall {
        var year: Int, month: Int, day: Int, seconds: Int64, microsecond: Int
        var zone: TimeZone

        init(_ moment: Double, _ zone: TimeZone) {
            let offset = Double(zone.secondsFromGMT(for: Date(timeIntervalSince1970: moment)))
            let local = moment + offset
            let whole = (local).rounded(.down)
            let days = Int((whole / 86400).rounded(.down))
            (year, month, day) = civilFromDays(days)
            seconds = Int64(whole) - Int64(days) * 86400
            microsecond = Int(((local - whole) * 1_000_000).rounded())
            if microsecond >= 1_000_000 { microsecond -= 1_000_000; seconds += 1 }
            self.zone = zone
        }

        init(year: Int, month: Int, day: Int, zone: TimeZone) {
            (self.year, self.month, self.day, seconds, microsecond, self.zone) = (year, month, day, 0, 0, zone)
        }

        var timestamp: Double {
            let time = ISOTime(year: year, month: month, day: day, hour: Int(seconds / 3600), minute: Int(seconds / 60 % 60),
                               second: Int(seconds % 60), microsecond: microsecond)
            return time.timestamp(assuming: zone)
        }

        func addingDays(_ count: Int) -> Wall {
            var copy = self
            (copy.year, copy.month, copy.day) = civilFromDays(daysFromCivil(year, month, day) + count)
            return copy
        }

        func addingSeconds(_ count: Int64) -> Wall {
            var copy = self
            let total = seconds + count
            let shift = Int((Double(total) / 86400).rounded(.down))
            copy = copy.addingDays(shift)
            copy.seconds = total - Int64(shift) * 86400
            return copy
        }

        /// Monday is 0, as datetime.weekday().
        var weekday: Int { ((daysFromCivil(year, month, day) % 7) + 7 + 3) % 7 }
    }

    public func rangeBounds(_ key: String, now: Double, zone: TimeZone) -> (Double, Double) {
        let wall = Wall(now, zone)
        let day = Wall(year: wall.year, month: wall.month, day: wall.day, zone: zone)
        // Yesterday ends just before midnight: every reader counts both ends, and midnight belongs to today.
        if key == "yesterday" { return (day.addingDays(-1).timestamp, day.timestamp - 0.001) }
        let start: Wall?
        switch key {
        case "today": start = day
        case "24h": start = wall.addingSeconds(-86400)
        case "7d": start = day.addingDays(-6)
        case "30d": start = day.addingDays(-29)
        case "week": start = day.addingDays(-day.weekday)
        case "month": start = Wall(year: wall.year, month: wall.month, day: 1, zone: zone)
        default: start = nil
        }
        // The end is this very moment; rebuilding it from the wall clock would pick the first of an hour seen twice.
        return (start?.timestamp ?? 0, now)
    }

    /// A range's cache: the price tables' stamps, the archive setting and the folders read are part of its name,
    /// so a change in any of them reads afresh.
    public func rangeCacheName(_ key: String? = nil) -> String {
        let settings = self.settings()
        var pricing: [JSON] = []
        for name in Runtime.pricingTables {
            let source = Pricing.table(name)["source"]?.object
            pricing += [source?["commit"] ?? .null, source?["committedAt"] ?? .null]
        }
        var scope: [JSON] = [["collector", "ccusage-v2"], ["pricing", .array(pricing)], ["keep", settings["keepCleanedUsage"] ?? .bool(true)]]
        let visible = settings["visible"]?.array ?? []
        for key in Preferences.usage where visible.contains(.string(key)) { scope.append([.string(key), .string(sourcePath(key))]) }
        if visible.contains(.string("claude")) {
            scope.append(["claude-projects", .array(ClaudeApp.projectRoots(home: home, environment: environment).map(JSON.string))])
        }
        return "range_usage_" + (key ?? selectedRange()) + "_" + SHA256.hex(Array(Py.dumps(.array(scope)).utf8)).prefix(12)
    }

    /// Where a source's records live on after its tool cleans them up, while 保留已清理记录的用量 is on.
    public func usageArchive(_ source: String, root: String) -> String? {
        guard !root.isEmpty, settings()["keepCleanedUsage"]?.isTruthy ?? true else { return nil }
        let real = Paths.resolve(Paths.expandUser(root)) ?? Runtime.absolute(Paths.expandUser(root))
        return Archive.location(state: stateDir, source: source, root: String(SHA256.hex(Array(real.utf8)).prefix(12)))
    }

    /// OpenCodex's own log of the requests it carried, when it runs on this Mac; a remote one's isn't here.
    func opencodexRequestLog() -> String? {
        let path = ocxHome + "/usage.jsonl"
        let host = URLComponents(string: base)?.host ?? ""
        return ["127.0.0.1", "localhost", "::1"].contains(host) && FileManager.default.fileExists(atPath: path) ? path : nil
    }

    /// What each selected gateway logged, read once per refresh: nil when its log can't be read here.
    func gatewayLogs(_ enabled: [String], start: Double, end: Double) -> [String: GatewayLog?] {
        var logs: [String: GatewayLog?] = [:]
        if enabled.contains("opencodex") {
            if let log = opencodexRequestLog() { logs["OpenCodex"] = .some(try? Harness.gatewayRequests(path: log, cache: cacheDir, start: start, end: end)) }
            else { logs["OpenCodex"] = .some(nil) }
        }
        let magpie = sourcePath("magpie") + "/usage.jsonl"
        if enabled.contains("magpie"), FileManager.default.fileExists(atPath: magpie),
           let log = try? Harness.gatewayRequests(path: magpie, cache: cacheDir, start: start, end: end, kind: "magpie") {
            logs["Magpie"] = .some(log)
        }
        return logs
    }

    /// Gateways that may have carried this source's requests as well. Every selected source is checked against
    /// every other selected gateway; Summary counts such a request once, from the gateway.
    func gatewayCarriers(_ source: String, _ logs: [String: GatewayLog?]) -> [Carrier] {
        var carriers: [Carrier] = []
        if let entry = logs["OpenCodex"], source != "opencodex" {
            // OpenCodex names its client only now and then, so any selected source's requests may be in its log.
            // Without the log, the local ChatGPT card defers to the proxy; others keep telling sessions by provider name.
            if let log = entry { carriers.append(("OpenCodex", Harness.requestsFrom(log))) }
            else if source == "codex" { carriers.append(("OpenCodex", nil)) }
        }
        if let entry = logs["Magpie"], let log = entry, let agents = Runtime.magpieAgents[source] {
            carriers.append(("Magpie", Harness.requestsFrom(log, agents: agents)))
        }
        return carriers
    }

    // MARK: Claude

    public func claudeUsageLive(rangeKey: String, now: Double, zone: TimeZone, carriers: [Carrier]?) -> JSONObject {
        let root = sourcePath("claude")
        // The desktop App, VS Code, JetBrains and terminal sessions share these transcripts; all of them count.
        let (paths, _) = ClaudeApp.transcriptFiles(root: root, home: home, environment: environment)
        let archive = usageArchive("claude", root: root)
        if paths.isEmpty && !(archive.map { FileManager.default.fileExists(atPath: $0) } ?? false) { return ["error": "未找到 Claude 用量记录"] }
        return (try? claudeUsageRecords(root: root, rangeKey: rangeKey, now: now, zone: zone, files: paths, carriers: carriers, archive: archive))
            ?? ["error": "未找到 Claude 用量记录"]
    }

    func claudeUsageRecords(root: String, rangeKey: String, now: Double, zone: TimeZone, files: [String], carriers: [Carrier]?, archive: String?) throws -> JSONObject {
        let (start, end) = rangeBounds(rangeKey, now: now, zone: zone)
        let real = Paths.resolve(root) ?? Runtime.absolute(root)
        let database = cacheDir + "/claude-app-usage-v4-" + SHA256.hex(Array(real.utf8)).prefix(12) + ".sqlite3"
        let (all, failures) = try ClaudeUsage.readEvents(files: files, database: database, start: start, end: end, archive: archive)
        let events = all.filter { start <= $0.ts && $0.ts <= end }
        func tally(_ rows: [ClaudeUsage.Event]) -> JSONObject { Runtime.claudeTally(rows, rangeKey: rangeKey, zone: zone) }
        var result = tally(events)
        result["source"] = "Claude App 与 Claude Code"
        result["pricingDate"] = .string(ClaudePricing.pricingDate)
        result["day"] = .string(Harness.localDay(now, zone))
        if let carriers, !carriers.isEmpty {
            // Requests a gateway such as Magpie also logged count once in Summary, from the gateway.
            let views: [JSONObject] = events.map { row in
                ["input": .int(row.metrics[0]), "output": .int(row.metrics[1]), "writes": .int(row.metrics[2] + row.metrics[3]),
                 "cached": .int(row.metrics[4]), "timestamp": .double(row.ts)]
            }
            var counted: [ClaudeUsage.Event] = [], carried: [(String, [ClaudeUsage.Event])] = []
            for (event, view) in zip(events, views) {
                if let owner = carriers.first(where: { carrier in carrier.requests.map { !$0.isEmpty && Harness.carriedBy($0, view) } ?? false })?.name {
                    if let at = carried.firstIndex(where: { $0.0 == owner }) { carried[at].1.append(event) } else { carried.append((owner, [event])) }
                } else {
                    counted.append(event)
                }
            }
            for (key, value) in Runtime.summaryParts(counted, carried, tally) { result[key] = value }
        }
        if failures > 0 {
            result["partial"] = true
            result["warning"] = "部分记录无法读取"
        }
        return result
    }

    /// Claude's card totals: tokens with cache reads and writes in the input, recorded costs first, hours only
    /// for the ranges whose chart is hourly.
    static func claudeTally(_ rows: [ClaudeUsage.Event], rangeKey: String, zone: TimeZone) -> JSONObject {
        func empty() -> JSONObject {
            ["totalTokens": .int(0), "inputTokens": .int(0), "outputTokens": .int(0), "cacheReadInputTokens": .int(0), "cacheCreationInputTokens": .int(0),
             "requests": .int(0), "estimatedCostUsd": .int(0), "unpricedRequests": .int(0), "recordedCostRequests": .int(0)]
        }
        var summary = empty()
        var days = [String: JSONObject](), hours = [Int64: JSONObject](), models = Ordered<String, JSONObject>(), projects = Ordered<String, JSONObject>()
        var sessions = Set<[UInt8]>(), first: Double?
        for row in rows {
            let m = row.metrics
            let i = m[0], o = m[1], w5 = m[2], w1 = m[3], r = m[4]
            let cost = row.cost ?? ClaudePricing.cost(model: row.model, metrics: m, speed: row.speed ?? "standard", geo: row.geo ?? "global", tier: row.tier ?? "standard")
            first = first.map { Swift.min($0, row.ts) } ?? row.ts
            let day = Harness.localDay(row.ts, zone)
            sessions.insert(row.session ?? [])
            func count(_ target: inout JSONObject) {
                target["totalTokens"] = Harness.add(target["totalTokens"]!, .int(i + o + w5 + w1 + r))
                target["inputTokens"] = Harness.add(target["inputTokens"]!, .int(i + w5 + w1 + r))
                target["outputTokens"] = Harness.add(target["outputTokens"]!, .int(o))
                target["cacheReadInputTokens"] = Harness.add(target["cacheReadInputTokens"]!, .int(r))
                target["cacheCreationInputTokens"] = Harness.add(target["cacheCreationInputTokens"]!, .int(w5 + w1))
                target["requests"] = Harness.add(target["requests"]!, .int(1))
                if let cost { target["estimatedCostUsd"] = Harness.add(target["estimatedCostUsd"]!, .double(cost)) }
                else { target["unpricedRequests"] = Harness.add(target["unpricedRequests"]!, .int(1)) }
                if row.cost != nil { target["recordedCostRequests"] = Harness.add(target["recordedCostRequests"]!, .int(1)) }
            }
            count(&summary)
            var target = models[row.model] ?? empty(); count(&target); models[row.model] = target
            let project = row.project.flatMap { $0.isEmpty ? nil : $0 } ?? ""
            target = projects[project] ?? empty(); count(&target); projects[project] = target
            target = days[day] ?? empty(); count(&target); days[day] = target
            if hourlyRanges.contains(rangeKey) {
                let hour = Int64((row.ts / 3600).rounded(.down)) * 3600
                target = hours[hour] ?? empty(); count(&target); hours[hour] = target
            }
        }
        summary["sessions"] = .int(Int64(sessions.count))
        let requests = summary["requests"]!.number ?? 0, unpriced = summary["unpricedRequests"]!.number ?? 0
        summary["coverageRatio"] = .double((requests - unpriced) / Swift.max(1, requests))
        return [
            "summary": .object(summary),
            "models": .array(models.items.map { var row = $0.1; row["model"] = .string($0.0); return .object(row) }),
            "projects": .array(projects.items.map { var row = $0.1; row["project"] = .string($0.0); return .object(row) }),
            "days": .array(days.keys.sorted().map { var row = days[$0]!; row["date"] = .string($0); return .object(row) }),
            "hours": .array(hours.keys.sorted().map { var row = hours[$0]!; row["timestamp"] = .int($0); return .object(row) }),
            "firstActivityAt": first.map(JSON.double) ?? .null,
        ]
    }

    static func summaryParts<T>(_ counted: [T], _ carried: [(String, [T])], _ tally: ([T]) -> JSONObject) -> JSONObject {
        func part(_ rows: [T]) -> JSON {
            let result = tally(rows)
            var kept = JSONObject()
            for key in ["summary", "models", "days", "hours", "firstActivityAt"] { kept[key] = result[key] ?? .null }
            return .object(kept)
        }
        var counts = JSONObject(), parts = JSONObject()
        for (name, rows) in carried { counts[name] = .int(Int64(rows.count)) }
        for (name, rows) in carried { parts[name] = part(rows) }
        return ["counted": part(counted), "carried": .object(counts), "carriedParts": .object(parts)]
    }

    // MARK: The range bundle

    /// Reads one source for the range; a failure reads as an error, never as zero.
    func collect(_ source: String, key: String, now: Double, zone: TimeZone, logs: [String: GatewayLog?], enabled: [String]) -> JSONObject {
        let (start, end) = rangeBounds(key, now: now, zone: zone)
        let carriers = gatewayCarriers(source, logs)
        switch source {
        case "opencodex": return opencodexConfigured() ? opencodexRangeUsageLive(key, now: now) : ["error": "未配置 OpenCodex"]
        // Cursor's calls never pass a gateway; only the ones it reported to Magpie are in a gateway's log.
        case "cursor": return cursorRangeUsageLive(start, end, now, carriers.filter { $0.name == "Magpie" })
        case "claude": return claudeUsageLive(rangeKey: key, now: now, zone: zone, carriers: carriers)
        default: break
        }
        let root = sourcePath(source)
        guard !root.isEmpty else { return ["error": "请选择记录目录"] }
        let kept = usageArchive(source, root: root)
        let excludeProxy = enabled.contains("opencodex")
        do {
            switch source {
            case "magpie":
                return try Harness.jsonlUsage(source: source, root: root, cache: cacheDir, start: start, end: end, zone: zone, carriers: carriers,
                                              price: Harness.magpiePrice, archive: kept)
            case "gemini":
                return try Harness.geminiUsage(root: root, cache: cacheDir, start: start, end: end, zone: zone, excludeProxy: excludeProxy, carriers: carriers, archive: kept)
            case "opencode":
                return try Harness.opencodeUsage(root: root, start: start, end: end, zone: zone, excludeProxy: excludeProxy, carriers: carriers)
            default:
                return try Harness.jsonlUsage(source: source, root: root, cache: cacheDir, start: start, end: end, zone: zone, excludeProxy: excludeProxy,
                                              carriers: carriers, archive: kept)
            }
        } catch {
            return ["error": "记录暂不可用"]
        }
    }

    /// Every selected Harness for one range, cached a minute (longer while the panel is closed). Each source keeps
    /// the time of its own last good read, and a failed read keeps its last good figures, marked as such.
    public func rangeUsage(_ key: String? = nil, ttl: Double? = nil) -> (JSONObject?, String?) {
        let key = key ?? selectedRange()
        let name = rangeCacheName(key)
        return cachedFetch(name, ttl: ttl ?? refreshTTL(60)) {
            let zone = localZone(), now = Date().timeIntervalSince1970
            let (start, end) = rangeBounds(key, now: now, zone: zone)
            let enabled = Preferences.usage.filter(moduleEnabled)
            // Python gives 全部 a start of the integer 0.
            var bundle: JSONObject = ["range": .string(key), "start": key == "all" ? .int(0) : .double(start), "end": .double(end)]
            let logs = gatewayLogs(enabled, start: start, end: end)
            let record = readCache(name)
            let previous = record["payload"]?.object ?? JSONObject()
            var results = [JSONObject](repeating: JSONObject(), count: enabled.count)
            let lock = NSLock()
            DispatchQueue.concurrentPerform(iterations: enabled.count) { index in
                let result = collect(enabled[index], key: key, now: now, zone: zone, logs: logs, enabled: enabled)
                lock.lock(); results[index] = result; lock.unlock()
            }
            for (source, result) in zip(enabled, results) {
                let last = previous[source]?.object ?? JSONObject()
                if result["error"]?.isTruthy == true && last["summary"]?.isTruthy == true {
                    var kept = last
                    kept["refreshError"] = result["error"]
                    kept["readAt"] = Py.or(last["readAt"], record["fetchedAt"] ?? .null)
                    bundle[source] = .object(kept)
                } else if result["error"]?.isTruthy == true {
                    bundle[source] = .object(result)
                } else {
                    var fresh = result
                    fresh["readAt"] = .double(Date().timeIntervalSince1970)
                    bundle[source] = .object(fresh)
                }
            }
            return (bundle, nil)
        }
    }

    /// Closed panels use a relaxed cadence; visible ones keep their normal TTL.
    public func refreshTTL(_ base: Double) -> Double {
        let (visible, battery) = memoized("cadence") { refreshCadence() }
        return visible ? base : Swift.max(base, battery ? 300 : 180)
    }

    func refreshCadence() -> (Bool, Bool) {
        if let cadence = hostCadence { return cadence() }
        #if os(Windows)
        // The tray says whether its panel is open; the battery comes from Windows.
        if environment["OCX_PANEL_VISIBLE"] == "1" { return (true, false) }
        var power = SYSTEM_POWER_STATUS()
        return (false, GetSystemPowerStatus(&power) && power.ACLineStatus == 0)
        #else
        let state = readCache("panel-state")
        if state["visible"] == .bool(true), let pid = state["pid"]?.integer, pid > 0, state["pid"]?.bool == nil, Platform.alive(Int(pid)) { return (true, false) }
        var power = readCache("power-state")
        let now = Date().timeIntervalSince1970
        if now - (power["checkedAt"]?.pythonFloat ?? 0) > 300 {
            let output = Shell.run(["/usr/bin/pmset", "-g", "batt"], timeout: 2)
            if let output {
                power = ["schema": 1, "battery": .bool(output.contains("'Battery Power'")), "checkedAt": .double(now)]
                writeJSONCache(cachePath("power-state"), .object(power))
            }
        }
        return (false, power["battery"]?.isTruthy ?? false)
        #endif
    }
}
