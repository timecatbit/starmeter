import Foundation

/// The requests one gateway logged, by token split, each with the times it finished. Matching uses them up, so
/// it is a class: every source gets a fresh copy from `Harness.requestsFrom`.
public final class GatewayRequests {
    public struct Split: Hashable { public var input: Int64, cached: Int64, writes: Int64, output: Int64 }
    var times: [Split: [Double]] = [:]
    var isEmpty: Bool { times.isEmpty }
    init() {}
}

/// A gateway that may have logged the same requests as a source: its name and its requests, or nil when its log
/// can't be read here (nothing is matched against it then).
public typealias Carrier = (name: String, requests: GatewayRequests?)
/// What a gateway's log holds around a range: per client, in the order they first appear, each token split's finish times.
public typealias GatewayLog = Ordered<String, [GatewayRequests.Split: [Double]]>

extension Harness {
    /// A local record and a gateway's record of one request usually finish within seconds of each other; Codex's
    /// auto-review writes its record a few minutes late. The exact token split is what identifies the call.
    static let matchWindow = 600.0

    /// One request a gateway logged: id, agent, finish time, input with cache reads, cache reads, cache writes, output.
    static func gatewayRow(_ kind: String, _ row: JSONObject, offset: Int64) throws -> (String, String, Double, [Double])? {
        if kind == "magpie" {
            guard let event = try magpieEvent(row, offset: offset) else { return nil }
            let n = { (key: String) in event[key]?.number ?? 0 }
            if n("input") + n("cached") + n("writes") + n("output") <= 0 { return nil }
            return (event["id"]!.string!, Py.str(Py.or(row["agent"], .string(""))),
                    (event["timestamp"]?.number ?? event["timestamp"]?.integer.map(Double.init) ?? 0) + Py.num(row["ms"]) / 1000,
                    [n("input") + n("cached"), n("cached"), n("writes"), n("output")])
        }
        guard let usage = row["usage"]?.object else { return nil }
        guard let time = row["timestamp"] else { throw Py.LineError() }
        return (Py.identity(Py.or(row["requestId"], .int(offset))), "", (Py.num(time) + Py.num(row["durationMs"])) / 1000,
                [Py.num(usage["inputTokens"]), Py.num(usage["cachedInputTokens"]), Py.num(usage["cacheCreationInputTokens"]), Py.num(usage["outputTokens"])])
    }

    /// The requests a gateway (OpenCodex or Magpie) logged around start...end, by client; the client is "" when the
    /// gateway doesn't name it. The log is appended to; when it is rewritten (new file, shorter, or another first
    /// line) the index starts over. Only these counters are kept, never request content.
    public static func gatewayRequests(path: String, cache: String, start: Double, end: Double, kind: String = "opencodex") throws -> GatewayLog {
        guard let state = ClaudeUsage.state(of: path), let handle = FileHandle(forReadingAtPath: path) else { throw Py.FileError("unreadable") }
        var first = Data()
        while first.count < 4096, let byte = try handle.read(upToCount: 1), !byte.isEmpty {
            first.append(byte)
            if byte.first == 0x0A { break }
        }
        try? handle.close()
        let head = Py.identity(.string(String(decoding: first, as: UTF8.self)))
        let database = (cache as NSString).appendingPathComponent("harness-gateway-v2-\(kind)-\(Py.identity(.string(Paths.resolve(path) ?? path)).prefix(12)).sqlite3")
        try Files.makeDirectory(cache)
        let db = try Database(path: database)
        Files.ownerOnly(database)
        try db.execute("CREATE TABLE IF NOT EXISTS log(id INTEGER PRIMARY KEY CHECK(id=1), inode INTEGER, head TEXT, offset INTEGER); CREATE TABLE IF NOT EXISTS requests(id TEXT PRIMARY KEY, agent TEXT, ts REAL, input INTEGER, cached INTEGER, writes INTEGER, output INTEGER); CREATE INDEX IF NOT EXISTS requests_ts ON requests(ts);")
        try transaction(db) {
            let known = try db.first("SELECT inode,head,offset FROM log")
            var offset: Int64 = 0
            if let known, known[0].int == state.inode, known[1].text == head, let saved = known[2].int, saved <= state.size { offset = saved }
            if offset == 0 { try db.run("DELETE FROM requests") }
            let insert = try db.prepare("INSERT OR REPLACE INTO requests VALUES (?,?,?,?,?,?,?)")
            _ = try LineReader.read(path, from: offset) { _, line in
                offset += Int64(line.count)
                guard let row = try? JSON.parse(line, surrogates: true), let fields = row.object else { return }
                do {
                    guard let request = try gatewayRow(kind, fields, offset: offset) else { return }
                    try insert.run([.text(request.0), .text(request.1), .double(request.2)] + request.3.map { .int(Int64($0.rounded(.towardZero))) })
                } catch is Py.LineError { return } catch is Py.FileError { return }
            }
            try db.run("INSERT OR REPLACE INTO log VALUES (1,?,?,?)", [.int(state.inode), .text(head), .int(offset)])
        }
        var clients = GatewayLog()
        try db.each("SELECT agent,ts,input,cached,writes,output FROM requests WHERE ts>=? AND ts<=?", [.double(start - matchWindow), .double(end + matchWindow)]) { row in
            let split = GatewayRequests.Split(input: row[2].int ?? 0, cached: row[3].int ?? 0, writes: row[4].int ?? 0, output: row[5].int ?? 0)
            let agent = row[0].text ?? ""
            var requests = clients[agent] ?? [:]
            requests[split, default: []].append(row[1].double ?? 0)
            clients[agent] = requests
        }
        return clients
    }

    /// A fresh copy of the requests these clients sent (all of them when agents is nil): matching uses them up.
    public static func requestsFrom(_ clients: GatewayLog, agents: Set<String>? = nil) -> GatewayRequests {
        let carried = GatewayRequests()
        for (agent, requests) in clients.items where agents == nil || agents!.contains(agent) {
            for (split, times) in requests { carried.times[split, default: []] += times }
        }
        return carried
    }

    /// Whether a gateway logged this very request: the same token split, finished within the match window. Each
    /// logged request accounts for one local record only.
    static func carriedBy(_ carried: GatewayRequests, _ event: JSONObject) -> Bool {
        func whole(_ key: String) -> Int64 { Int64(Py.num(event[key]).rounded(.towardZero)) }
        let inputs = whole("input"), cached = whole("cached"), writes = whole("writes"), output = whole("output")
        // Gateways report input with cache reads, and some with cache writes as well.
        var split = GatewayRequests.Split(input: inputs + cached, cached: cached, writes: writes, output: output)
        if (carried.times[split] ?? []).isEmpty, writes != 0 {
            split = GatewayRequests.Split(input: inputs + cached + writes, cached: cached, writes: writes, output: output)
        }
        guard let times = carried.times[split], !times.isEmpty else { return false }
        let when = event["timestamp"]?.number ?? 0
        var nearest = 0
        for index in times.indices where Swift.abs(times[index] - when) < Swift.abs(times[nearest] - when) { nearest = index }
        if Swift.abs(times[nearest] - when) > matchWindow { return false }
        carried.times[split]!.remove(at: nearest)
        return true
    }

    /// The events no gateway carried, and those each gateway did.
    static func splitCarried(_ events: [Event], _ carriers: [Carrier], excludeProxy: Bool = false,
                             view: (Event) -> JSONObject = { $0 }) -> ([Event], [(String, [Event])]) {
        var counted: [Event] = []
        var carried: [(String, [Event])] = []
        for event in events {
            let seen = view(event)
            var by: String?
            if excludeProxy, ["opencodex", "ocx"].contains(Py.str(seen["provider"] ?? .string("")).lowercased()) {
                by = "OpenCodex"
            } else {
                by = carriers.first(where: { carrier in carrier.requests.map { !$0.isEmpty && carriedBy($0, seen) } ?? false })?.name
            }
            if let by {
                if let index = carried.firstIndex(where: { $0.0 == by }) { carried[index].1.append(event) } else { carried.append((by, [event])) }
            } else {
                counted.append(event)
            }
        }
        return (counted, carried)
    }

    /// Summary's share of a source, plus each gateway's share, which Summary adds back when that gateway has
    /// nothing to show (its card can't count those requests then).
    static func summaryParts(_ counted: [Event], _ carried: [(String, [Event])], _ tally: ([Event]) -> JSONObject) -> JSONObject {
        func part(_ events: [Event]) -> JSON {
            let result = tally(events)
            var kept = JSONObject()
            for key in ["summary", "models", "days", "hours", "firstActivityAt"] { kept[key] = result[key] ?? .null }
            return .object(kept)
        }
        var counts = JSONObject(), parts = JSONObject()
        for (name, events) in carried { counts[name] = .int(Int64(events.count)) }
        for (name, events) in carried { parts[name] = part(events) }
        return ["counted": part(counted), "carried": .object(counts), "carriedParts": .object(parts)]
    }

    /// A source's own card covers all its records. When a gateway logged some of the same requests, "counted"
    /// holds only the rest, which is what Summary adds for this source.
    public static func aggregate(_ all: [Event], start: Double, end: Double, zone: TimeZone, excludeProxy: Bool = false,
                                 price: Price? = nil, carriers: [Carrier]? = nil) -> JSONObject {
        let events = all.filter { event in
            let when = event["timestamp"]?.number ?? -.infinity
            let used = ["input", "output", "cached", "writes"].reduce(0.0) { $0 + Py.num(event[$1]) }
            return start <= when && when <= end && (used > 0 || Py.num(event["credits"]) > 0)
        }
        var result = tally(events, zone: zone, price: price)
        if excludeProxy || !(carriers ?? []).isEmpty {
            let (counted, carried) = splitCarried(events, carriers ?? [], excludeProxy: excludeProxy)
            for (key, value) in summaryParts(counted, carried, { tally($0, zone: zone, price: price) }) { result[key] = value }
        }
        result["proxyMatched"] = .bool((carriers ?? []).last(where: { $0.name == "OpenCodex" }).map { $0.requests != nil } ?? false)
        return result
    }

    /// Totals for the summary, each model, project, day and hour, and Magpie's clients.
    public static func tally(_ events: [Event], zone: TimeZone, price: Price? = nil) -> JSONObject {
        func empty() -> JSONObject {
            ["totalTokens": .int(0), "inputTokens": .int(0), "outputTokens": .int(0), "cachedInputTokens": .int(0),
             "requests": .int(0), "estimatedCostUsd": .int(0), "unpricedRequests": .int(0), "credits": .int(0)]
        }
        var summary = empty()
        var models = Ordered<String, JSONObject>(), projects = Ordered<String, JSONObject>(), days = [String: JSONObject](), hours = [Int64: JSONObject]()
        var clients = Ordered<String, JSONObject>()
        var sessions = Set<String>(), first: JSON = .null
        for event in events {
            let ts = event["timestamp"] ?? .null
            let when = ts.number ?? ts.integer.map(Double.init) ?? 0
            let i = Py.number(event["input"]), o = Py.number(event["output"]), r = Py.number(event["cached"]), w = Py.number(event["writes"])
            let credits = Py.number(event["credits"])
            var cost = event["cost"].flatMap { $0.isNull ? nil : $0 }
            if cost == nil, let price { cost = price(event).map(JSON.double) }
            // A gateway names the client that sent each request (Magpie's agent).
            if let agent = event["agent"] {
                let key = Py.str(agent)
                var client = clients[key] ?? ["totalTokens": .int(0), "requests": .int(0)]
                client["totalTokens"] = add(client["totalTokens"]!, add(add(add(i, o), r), w))
                client["requests"] = add(client["requests"]!, .int(1))
                clients[key] = client
            }
            first = first.isNull ? ts : ((ts.number ?? 0) < (first.number ?? 0) ? ts : first)
            let day = localDay(when, zone)
            if let session = event["session"], !session.isNull { sessions.insert(Py.dumps(session)) }
            let hour = Int64((when / 3600).rounded(.down)) * 3600
            func count(_ target: inout JSONObject) {
                target["totalTokens"] = add(target["totalTokens"]!, add(add(add(i, o), r), w))
                target["inputTokens"] = add(target["inputTokens"]!, add(add(i, r), w))
                target["outputTokens"] = add(target["outputTokens"]!, o)
                target["cachedInputTokens"] = add(target["cachedInputTokens"]!, r)
                target["requests"] = add(target["requests"]!, .int(1))
                target["credits"] = add(target["credits"]!, credits)
                if let known = event["cacheKnown"] {
                    target["cacheEligibleInputTokens"] = add(target["cacheEligibleInputTokens"] ?? .int(0), known.isTruthy ? add(add(i, r), w) : .int(0))
                }
                if let cost { target["estimatedCostUsd"] = add(target["estimatedCostUsd"]!, cost) }
                else { target["unpricedRequests"] = add(target["unpricedRequests"]!, .int(1)) }
            }
            count(&summary)
            let model = Py.str(event["model"])
            var row = models[model] ?? empty(); count(&row); models[model] = row
            let project = event["project"].flatMap { $0.isTruthy ? Py.str($0) : nil } ?? ""
            row = projects[project] ?? empty(); count(&row); projects[project] = row
            row = days[day] ?? empty(); count(&row); days[day] = row
            row = hours[hour] ?? empty(); count(&row); hours[hour] = row
        }
        summary["sessions"] = .int(Int64(sessions.count))
        let requests = summary["requests"]!.number ?? 0, unpriced = summary["unpricedRequests"]!.number ?? 0
        summary["coverageRatio"] = .double((requests - unpriced) / Swift.max(1, requests))
        var result: JSONObject = [
            "summary": .object(summary),
            "models": .array(models.items.map { var row = $0.1; row["model"] = .string($0.0); return .object(row) }),
            "projects": .array(projects.items.map { var row = $0.1; row["project"] = .string($0.0); return .object(row) }),
            "days": .array(days.keys.sorted().map { var row = days[$0]!; row["date"] = .string($0); return .object(row) }),
            "hours": .array(hours.keys.sorted().map { var row = hours[$0]!; row["timestamp"] = .int($0); return .object(row) }),
            "firstActivityAt": first,
        ]
        if !clients.items.isEmpty {
            result["clients"] = .array(clients.items.map { var row = $0.1; row["agent"] = .string($0.0); return .object(row) })
        }
        return result
    }

    /// a + b as Python adds them: ints stay ints, anything with a float is a float.
    static func add(_ a: JSON, _ b: JSON) -> JSON {
        if case .int(let x) = a, case .int(let y) = b { return .int(x + y) }
        return .double((a.number ?? 0) + (b.number ?? 0))
    }

    /// The local calendar day of a moment, as datetime.fromtimestamp(ts, zone).date().isoformat().
    public static func localDay(_ seconds: Double, _ zone: TimeZone) -> String {
        let offset = Double(zone.secondsFromGMT(for: Date(timeIntervalSince1970: seconds)))
        let (y, m, d) = civilFromDays(Int(((seconds + offset) / 86400).rounded(.down)))
        return String(format: "%04d-%02d-%02d", y, m, d)
    }
}

/// A dictionary that remembers the order keys were first added, like a Python dict.
public struct Ordered<Key: Hashable, Value> {
    public private(set) var keys: [Key] = []
    private var storage: [Key: Value] = [:]
    public init() {}
    public subscript(key: Key) -> Value? {
        get { storage[key] }
        set {
            if let newValue { if storage.updateValue(newValue, forKey: key) == nil { keys.append(key) } }
            else if storage.removeValue(forKey: key) != nil { keys.removeAll { $0 == key } }
        }
    }
    public var items: [(Key, Value)] { keys.map { ($0, storage[$0]!) } }
}

/// Usage that outlives its logs: when a log file disappears, the counters the index held for it move to a durable
/// archive, and the readers add them back, the index winning for any request it still has. Counters, model names,
/// times and hashed IDs only, never content.
public enum Archive {
    public static func location(state: String, source: String, root: String) -> String {
        (state as NSString).appendingPathComponent("archive/\(source)-v1-\(root).sqlite3")
    }

    /// Turning the setting off forgets everything kept so far.
    public static func clear(state: String) {
        try? FileManager.default.removeItem(atPath: (state as NSString).appendingPathComponent("archive"))
    }

    static func connect(_ path: String) throws -> Database {
        try Files.makeDirectory((path as NSString).deletingLastPathComponent)
        let db = try Database(path: path)
        Files.ownerOnly(path)
        try db.execute("CREATE TABLE IF NOT EXISTS events(id TEXT PRIMARY KEY, ts REAL, data TEXT)")
        try db.execute("CREATE INDEX IF NOT EXISTS events_ts ON events(ts)")
        return db
    }

    /// Archives (id, timestamp, event JSON) rows; the first copy of an event stays.
    static func keep(_ path: String, _ rows: [[SQLValue]]) throws {
        guard !rows.isEmpty else { return }
        let db = try connect(path)
        try Harness.transaction(db) {
            let insert = try db.prepare("INSERT OR IGNORE INTO events VALUES (?,?,?)")
            for row in rows { try insert.run(row) }
        }
    }

    static func events(_ path: String?, _ start: Double, _ end: Double) -> [JSONObject] {
        guard let path, FileManager.default.fileExists(atPath: path), let db = try? connect(path),
              let rows = try? db.rows("SELECT data FROM events WHERE ts>=? AND ts<=?", [.double(start), .double(end)]) else { return [] }
        return rows.compactMap { $0[0].text.flatMap { try? JSON.parse($0).object } }
    }

    /// The index's events plus the archived ones it no longer has.
    static func merged(_ live: [JSONObject], _ path: String?, _ start: Double, _ end: Double) -> [JSONObject] {
        let seen = Set(live.map { Py.dumps($0["id"] ?? .null) })
        return live + events(path, start, end).filter { !seen.contains(Py.dumps($0["id"] ?? .null)) }
    }
}
