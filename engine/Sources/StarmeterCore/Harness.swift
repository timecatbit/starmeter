import Foundation

/// The Harness readers other than Claude's, ported from swiftbar_sources.py: Codex, DeepSeek Harness, WorkBuddy,
/// Qoder CN, Magpie and Gemini CLI logs, OpenCode's database, and the gateways' own logs. Only usage counters
/// are kept, never message bodies. Codex delta/rollout rules are adapted from ccusage (MIT), 615bd802.
/// Each event is kept as the JSON text the Python engine writes, so their index files are interchangeable.
public enum Harness {
    public typealias Event = JSONObject
    public typealias Price = (Event) -> Double?

    static let serviceTiers = ["default": "standard", "standard": "standard", "fast": "fast", "priority": "fast", "flex": "flex", "ultrafast": "ultrafast"]
    /// A source's index version rises when what its events hold changes, so old indexes are rebuilt.
    static let indexVersions = ["magpie": 6, "codex": 6, "qoder": 5]

    /// A ChatGPT app chat with no project folder runs in a folder the app makes for it, Documents/Codex/<day>/<slug>,
    /// named after the first words typed ("wo-d", "https-github-com-…"). Its requests keep the chat they belong to (a
    /// Guardian review its parent chat) and take the chat's title when a range is read; ChatGPT 对话 until it has one.
    static let chatMark = "\u{1}"
    static let chatFolder = try! NSRegularExpression(pattern: #"/Codex/\d{4}-\d{2}-\d{2}/[^/]+/?$"#)

    static func codexProject(_ data: JSON, session: String) throws -> JSON {
        if let cwd = try Py.get(data, "cwd")?.string {
            let path = cwd.trimmingCharacters(in: .whitespacesAndNewlines).replacingOccurrences(of: "\\", with: "/")
            if chatFolder.firstMatch(in: path, range: NSRange(path.startIndex..., in: path)) != nil {
                return .string(chatMark + Py.str(Py.or(try Py.get(data, "parent_thread_id"), .string(session))))
            }
        }
        return Py.projectName(try Py.get(data, "cwd"))
    }

    static func chatProjects(_ events: [Event], titles: [String: String]) -> [Event] {
        events.map { event in
            guard let project = event["project"]?.string, project.hasPrefix(chatMark) else { return event }
            var event = event
            event["project"] = .string(titles[String(project.dropFirst())] ?? "ChatGPT 对话")
            return event
        }
    }

    // MARK: Events

    /// The engine's metrics(): a model name and counters as number() makes them.
    static func metrics(_ model: JSON?, _ inputs: JSON?, _ output: JSON?, _ cached: JSON? = .int(0), _ writes: JSON? = .int(0),
                        _ cost: JSON? = nil) -> Event {
        var event = Event()
        event["model"] = .string(Py.str(Py.or(model, .string("未知模型"))))
        event["input"] = Py.number(inputs)
        event["output"] = Py.number(output)
        event["cached"] = Py.number(cached)
        event["writes"] = Py.number(writes)
        if let cost, let value = cost.pythonFloat, cost.number != nil || cost.bool != nil, value.isFinite, value >= 0 {
            event["cost"] = Py.number(cost)
        } else {
            event["cost"] = .null
        }
        return event
    }

    /// min(a, b) of two number() results: the first unless the second is smaller.
    static func smaller(_ a: JSON, _ b: JSON) -> JSON { (b.number ?? 0) < (a.number ?? 0) ? b : a }

    /// a - b where both came from number(): an int only when both are ints.
    static func minus(_ a: JSON, _ b: JSON) -> JSON {
        if case .int(let x) = a, case .int(let y) = b { return .int(x - y) }
        return .double((a.number ?? 0) - (b.number ?? 0))
    }

    static func plus(_ a: JSON, _ b: JSON) -> JSON {
        if case .int(let x) = a, case .int(let y) = b { return .int(x + y) }
        return .double((a.number ?? 0) + (b.number ?? 0))
    }

    static func codexEvent(_ row: JSONObject, _ state: inout JSONObject) throws -> Event? {
        let kind = row["type"]
        let data = Py.dict(row["payload"])
        if kind == .string("event_msg"), try Py.get(data, "type") == .string("thread_settings_applied") {
            // A settings event without a tier says nothing; an unknown tier must not inherit the old one.
            if let settings = try Py.get(data, "thread_settings")?.object, let tier = settings["service_tier"], !tier.isNull {
                state["tier"] = serviceTiers[Py.str(tier)].map(JSON.string) ?? .null
            }
            return nil
        }
        if kind == .string("session_meta") {
            let session = Py.or(Py.or(try Py.get(data, "id"), try Py.get(data, "session_id") ?? .null), state["session"] ?? .null)
            state["session"] = .string(Py.str(session))
            state["provider"] = .string(Py.str(Py.or(try Py.get(data, "model_provider"), .string(""))))
            state["project"] = try codexProject(data, session: Py.str(session))
        } else if kind == .string("turn_context") {
            state["model"] = .string(Py.str(Py.or(Py.or(try Py.get(data, "model"), state["model"] ?? .null), .string("未知模型"))))
        } else if kind == .string("event_msg"), try Py.get(data, "type") == .string("token_count") {
            let info = Py.dict(try Py.get(data, "info"))
            let total = Py.dict(try Py.get(info, "total_token_usage"))
            let last = Py.dict(try Py.get(info, "last_token_usage"))
            if !total.isTruthy && !last.isTruthy { return nil }
            let previous = state["total"]
            if total.isTruthy { state["total"] = total }
            if total.isTruthy && previous == total { return nil }
            // ccusage prefers a recorded request delta; cumulative totals are a fallback, since their jump can
            // include inherited or compacted context, not just this request.
            let values: JSON
            if last.isTruthy {
                values = last
            } else {
                var delta = JSONObject()
                for key in ["input_tokens", "cached_input_tokens", "output_tokens", "total_tokens"] {
                    let difference = minus(Py.number(try Py.get(total, key)), Py.number(try Py.get(Py.dict(previous), key)))
                    delta[key] = (difference.number ?? 0) > 0 ? difference : .int(0)
                }
                values = .object(delta)
            }
            let inputs = Py.number(try Py.get(values, "input_tokens"))
            let cached = smaller(inputs, Py.number(try Py.get(values, "cached_input_tokens")))
            var result = metrics(state["model"], minus(inputs, cached), try Py.get(values, "output_tokens"), cached)
            if (inputs.number ?? 0) + (result["output"]?.number ?? 0) <= 0 { return nil }
            guard let session = state["session"] else { throw Py.LineError() }
            let identifying: JSON
            if total.isTruthy { identifying = total } else {
                guard let time = row["timestamp"] else { throw Py.LineError() }
                identifying = [time, last]
            }
            result["id"] = .string(Py.identity("codex", session, identifying))
            result["session"] = .string(Py.identity(session))
            guard let time = row["timestamp"] else { throw Py.LineError() }
            result["timestamp"] = try Py.stamp(time)
            result["provider"] = state["provider"] ?? .string("")
            result["tier"] = state["tier"] ?? .null
            result["project"] = state["project"] ?? .null
            return result
        }
        return nil
    }

    static func deepseekEvent(_ row: JSONObject, _ state: inout JSONObject) throws -> Event? {
        let kind = row["type"]
        let data = Py.dict(row["data"])
        if kind == .string("session") {
            let version = try Py.int(row["version"] ?? .int(0))
            guard [2, 3, 4].contains(version) else { throw Py.FileError("不支持的 Harness 日志版本") }
            let seeded = row["isSeeded"]?.isTruthy ?? false
            state["session"] = .string(Py.str(Py.or(row["id"], state["session"] ?? .null)))
            state["isSeeded"] = .bool(seeded)
            state["seeded"] = .bool(seeded)
            state["seedLength"] = .int(try Py.int(row["seedLength"] ?? .int(0)))
        } else if kind == .string("session/end-seed"), try Py.get(data, "inherited")?.isTruthy == true {
            state["seeded"] = .bool(false)
        } else if kind == .string("request/header") {
            let header = Py.dict(try Py.get(data, "header"))
            let config = Py.or(try Py.get(header, "config"), header)
            guard let fields = config.object else { throw Py.LineError() }
            state["model"] = fields["model"] ?? state["model"] ?? .null
            state["provider"] = fields["provider"] ?? state["provider"] ?? .string("")
        } else if kind == .string("llm/retry-started") {
            state["attempt"] = .int(try Py.int(state["attempt"] ?? .int(0)) + 1)
        } else if kind == .string("assistant/message") || kind == .string("assistant/attempt") {
            if state["seeded"]?.isTruthy == true || Py.num(row["seq"]) < (state["seedLength"]?.number ?? 0) { return nil }
            var usage: JSON? = kind == .string("assistant/message") ? try Py.get(data, "usage") : nil
            if usage == nil || usage!.isNull {
                usage = nil
                let stream = Py.or(try Py.get(data, "stream"), .array([]))
                switch stream {
                case .array(let chunks):
                    for item in chunks {
                        let chunk: JSON = item.object != nil ? (item.object!["chunk"] ?? item) : .object(JSONObject())
                        if try Py.get(chunk, "type") == .string("usage") { usage = try Py.get(chunk, "usage") ?? .null }
                    }
                case .string, .object: break
                default: throw Py.LineError()
                }
            }
            guard let usage, usage.object != nil else { return nil }
            let message = Py.dict(try Py.get(data, "message"))
            var result = metrics(Py.or(try Py.get(message, "model"), state["model"] ?? .null), usage["inputTokens"], usage["outputTokens"],
                                 usage["cacheReadTokens"], usage["cacheWriteTokens"])
            guard let session = state["session"] else { throw Py.LineError() }
            result["id"] = .string(Py.identity("dsh", session, try Py.get(data, "turn") ?? .null, try Py.get(data, "step") ?? .null,
                                               state["attempt"] ?? .int(0)))
            result["session"] = .string(Py.identity(session))
            guard let time = row["time"] else { throw Py.LineError() }
            result["timestamp"] = try Py.stamp(time)
            result["provider"] = Py.or(try Py.get(message, "provider"), state["provider"] ?? .string(""))
            return result
        }
        return nil
    }

    /// WorkBuddy project transcripts, including tool calls with the same response ID.
    static func workbuddyEvent(_ row: JSONObject, _ state: inout JSONObject) throws -> Event? {
        guard let kind = row["type"], [JSON.string("message"), .string("function_call"), .string("assistant")].contains(kind) else { return nil }
        let message = Py.dict(row["message"]), provider = Py.dict(row["providerData"])
        let messageRole = try Py.get(message, "role")
        let role = row["role"] ?? messageRole ?? .null
        guard role.isNull || role.string == "assistant" else { return nil }
        let messageUsage = try Py.get(message, "usage")
        let usage = messageUsage?.isTruthy == true ? messageUsage! : Py.or(try Py.get(provider, "usage"), .object(JSONObject()))
        guard let fields = usage.object, !fields.isEmpty else { return nil }
        let inputs = Py.number(fields["input_tokens"])
        let cached = smaller(inputs, Py.number(fields["cache_read_input_tokens"]))
        // WorkBuddy normalizes inclusive prompt/output counters; cache and reasoning are already part of them.
        var event = metrics(Py.or(try Py.get(provider, "model"), try Py.get(message, "model") ?? .null), minus(inputs, cached),
                            fields["output_tokens"], cached)
        let response = Py.or(Py.or(try Py.get(provider, "messageId"), try Py.get(message, "id") ?? .null), row["id"] ?? .null)
        guard response.isTruthy else { return nil }
        event["id"] = .string(Py.identity("workbuddy", response))
        event["session"] = .string(Py.identity(Py.or(row["sessionId"], state["session"] ?? .null)))
        guard let time = row["timestamp"] else { throw Py.LineError() }
        event["timestamp"] = try Py.stamp(time)
        event["provider"] = try Py.get(provider, "provider") ?? .string("")
        return event
    }

    /// Qoder CN transcripts: Claude-style assistant rows. Plan-billed requests record Qoder Credits with every
    /// token count at zero; Credits are Qoder's own unit, never USD and never converted into tokens.
    static func qoderEvent(_ row: JSONObject, _ state: inout JSONObject) throws -> Event? {
        guard row["type"] == .string("assistant") else { return nil }
        let message = Py.dict(row["message"])
        guard let usage = try Py.get(message, "usage")?.object else { return nil }
        let credits = usage["credits"] != nil ? Py.number(usage["credits"]) : Py.number(usage["original_credits"])
        var model = Py.str(Py.or(try Py.get(message, "model"), .string("")))
        // Bring-your-own-key models carry a qoder-custom-<id>/ prefix before the provider's model name.
        if model.hasPrefix("qoder-custom-"), let slash = model.firstIndex(of: "/") { model = String(model[model.index(after: slash)...]) }
        var event = metrics(.string(model), usage["input_tokens"], usage["output_tokens"],
                            Py.or(usage["cache_read_input_tokens"], usage["cached_tokens"] ?? .null), usage["cache_creation_input_tokens"])
        let counted = ["input", "output", "cached", "writes"].reduce(0.0) { $0 + (event[$1]?.number ?? 0) }
        if counted <= 0 && (credits.number ?? 0) <= 0 { return nil }
        let session = Py.str(Py.or(row["sessionId"], state["session"] ?? .null))
        let response = Py.or(try Py.get(message, "id"), row["uuid"] ?? .null)
        let when: JSON
        do { when = try Py.stamp(Py.or(row["timestamp"], try Py.get(message, "timestamp") ?? .null)) } catch is Py.FileError { return nil }
        guard response.isTruthy else { return nil }
        // A streamed reply is rewritten under the same message ID; the newest copy in a file wins.
        event["id"] = .string(Py.identity("qoder", .string(session), response))
        event["session"] = .string(Py.identity(.string(session)))
        event["credits"] = credits
        event["timestamp"] = when
        event["project"] = Py.projectName(row["cwd"])
        return event
    }

    /// Counters from Magpie's log as Magpie totals them: its gateway's own calls and the calls a client reported
    /// to it (source 'reported'), but not its internal probes or refused and handed-on calls.
    static func magpieEvent(_ row: JSONObject, offset: Int64) throws -> Event? {
        let source = row["source"]
        if (source?.isTruthy == true && source != .string("reported")) || row["rejected"]?.isTruthy == true || row["handed_on"]?.isTruthy == true {
            return nil
        }
        guard row["provider"]?.isTruthy == true, row["model"]?.isTruthy == true else { return nil }
        // A request the upstream refused before replying produced no usage to count.
        if Py.num(row["status"]) >= 400 && Py.num(row["out"]) == 0 { return nil }
        // Magpie already splits fresh input, cache reads and writes; price the actual model, not a routing group.
        let model = Py.or(row["served"], row["model"]!)
        var event = metrics(model, row["in"], row["out"], row["cache_read"], row["cache_write"], row["cost"])
        guard let time = row["t"] else { throw Py.LineError() }
        let timestamp = try Py.stamp(time)
        // The tier the upstream reported, else the one Magpie asked for: the local client prices by its request too.
        let tier = Py.or(row["tier_served"], row["tier_sent"] ?? .null)
        if tier.array != nil || tier.object != nil { throw Py.LineError() }
        event["id"] = .string(Py.identity("magpie", .int(offset)))
        event["timestamp"] = timestamp
        event["agent"] = .string(Py.str(Py.or(row["agent"], .string(""))))
        event["reported"] = .bool(source?.string == "reported")
        if let session = row["session"], session.isTruthy { event["session"] = .string(Py.identity(session)) } else { event["session"] = .null }
        event["write1h"] = smaller(event["writes"]!, Py.number(row["cache_write_1h"]))
        event["cacheKnown"] = .bool(!(row["cache_unknown"]?.isTruthy ?? false))
        event["tier"] = tier.string.flatMap { serviceTiers[$0] }.map(JSON.string) ?? .null
        event["fast"] = .bool(tier.string == "fast")
        return event
    }

    static func geminiEvent(_ row: JSONObject, _ state: inout JSONObject) throws -> Event? {
        if let session = row["sessionId"], session.isTruthy { state["session"] = .string(Py.str(session)) }
        guard row["type"] == .string("gemini"), let usage = row["tokens"]?.object, row["id"]?.isTruthy == true else { return nil }
        let inputs = Py.number(usage["input"])
        let cached = smaller(inputs, Py.number(usage["cached"]))
        var output = plus(Py.number(usage["output"]), Py.number(usage["thoughts"]))
        let total = Py.number(usage["total"])
        var tool = Py.number(usage["tool"])
        // Gemini returns thoughts separately from candidate output; its own total keeps a token from counting twice.
        if (total.number ?? 0) >= (inputs.number ?? 0) && (total.number ?? 0) > 0 {
            tool = smaller(tool, minus(total, inputs))
            output = minus(minus(total, inputs), tool)
        }
        var result = metrics(row["model"], plus(minus(inputs, cached), tool), output, cached)
        result["id"] = .string(Py.identity("gemini", row["id"]!))
        result["session"] = .string(Py.identity(state["session"] ?? .null))
        guard let time = row["timestamp"] else { throw Py.FileError("KeyError: timestamp") }
        result["timestamp"] = try Py.stamp(time)
        result["provider"] = "google"
        return result
    }

    // MARK: Files

    static func sortedPaths(_ paths: [String]) -> [String] {
        paths.sorted { a, b in
            a.split(separator: "/").map(String.init).lexicographicallyPrecedes(b.split(separator: "/").map(String.init), by: ClaudeUsage.pythonLess)
        }
    }

    /// Every file under root, as root + relative path, without following linked folders.
    static func walk(_ root: String) -> [String] {
        guard let walker = FileManager.default.enumerator(atPath: root) else { return [] }
        var found: [String] = []
        while let relative = walker.nextObject() as? String {
            let path = root + "/" + relative
            var isDirectory: ObjCBool = false
            if FileManager.default.fileExists(atPath: path, isDirectory: &isDirectory), !isDirectory.boolValue { found.append(path) }
        }
        return found
    }

    static func isDirectory(_ path: String) -> Bool {
        var flag: ObjCBool = false
        return FileManager.default.fileExists(atPath: path, isDirectory: &flag) && flag.boolValue
    }

    static func name(_ path: String) -> String { (path as NSString).lastPathComponent }

    static func jsonlFiles(_ root: String, _ source: String) -> [String] {
        switch source {
        case "magpie":
            let log = root + "/usage.jsonl"
            return FileManager.default.fileExists(atPath: log) && !isDirectory(log) ? [log] : []
        case "codex":
            let roots = ["sessions", "archived_sessions"].map { root + "/" + $0 }.filter(isDirectory)
            var chosen: [String: String] = [:], order: [String] = []
            // Active copy wins over archive; plain copy wins over compressed in each root.
            for folder in roots.isEmpty ? [root] : roots {
                for path in sortedPaths(walk(folder)) where name(path).contains(".jsonl") && (path.hasSuffix(".jsonl") || path.hasSuffix(".jsonl.zst")) {
                    var relative = String(path.dropFirst(folder.count + 1))
                    if relative.hasSuffix(".zst") { relative.removeLast(4) }
                    if chosen[relative] == nil { chosen[relative] = path; order.append(relative) }
                }
            }
            return order.map { chosen[$0]! }
        case "qoder":
            // Main transcripts sort before their subagents/ folder, so a main copy is indexed first.
            let base = isDirectory(root + "/projects") ? root + "/projects" : root
            return sortedPaths(walk(base).filter { $0.hasSuffix(".jsonl") })
        case "workbuddy":
            var roots = [".workbuddy-ai", ".workbuddy"].map { root + "/" + $0 + "/projects" }.filter(isDirectory)
            if roots.isEmpty && Paths.resolve(root) == Paths.resolve(Paths.home) { return [] }
            if roots.isEmpty { roots = [isDirectory(root + "/projects") ? root + "/projects" : root] }
            return sortedPaths(Array(Set(roots.flatMap { walk($0).filter { $0.hasSuffix(".jsonl") } })))
        default:
            // Only one physical generation of each DSH session is authoritative.
            var chosen: [String: (Int, String)] = [:], order: [String] = []
            for path in walk(root) {
                guard let generation = dshGeneration(name(path)) else { continue }
                let parent = (path as NSString).deletingLastPathComponent
                if chosen[parent] == nil { order.append(parent) }
                if chosen[parent] == nil || generation > chosen[parent]!.0 { chosen[parent] = (generation, path) }
            }
            return order.map { chosen[$0]!.1 }
        }
    }

    /// session.jsonl, session.v<N>.jsonl, optionally .zstd: the generation number, or nil for other names.
    static func dshGeneration(_ name: String) -> Int? {
        var rest = Substring(name)
        guard rest.hasPrefix("session") else { return nil }
        rest = rest.dropFirst(7)
        if rest.hasSuffix(".zstd") { rest = rest.dropLast(5) }
        guard rest.hasSuffix(".jsonl") else { return nil }
        rest = rest.dropLast(6)
        if rest.isEmpty { return 0 }
        guard rest.hasPrefix(".v"), rest.count > 2, rest.dropFirst(2).allSatisfy(\.isASCII), let number = Int(rest.dropFirst(2)) else { return nil }
        return number
    }

    static func suffix(_ path: String) -> String {
        let last = name(path)
        guard let dot = last.lastIndex(of: "."), dot != last.startIndex else { return "" }
        return String(last[dot...])
    }

    static func bind(_ value: JSON) -> SQLValue {
        switch value {
        case .int(let number): return .int(number)
        case .double(let number): return .double(number)
        case .bool(let flag): return .int(flag ? 1 : 0)
        case .string(let text): return .text(text)
        default: return .null
        }
    }

    // MARK: Reading a log source

    /// The usage of one log source in start...end, indexing only what changed since the last read. carriers are the
    /// gateways that may have logged the same requests; archive keeps the events of log files that disappear.
    public static func jsonlUsage(source: String, root rootPath: String, cache: String, start: Double, end: Double, zone: TimeZone,
                                  excludeProxy: Bool = false, carriers: [Carrier]? = nil, price: Price? = nil, archive: String? = nil) throws -> JSONObject {
        let root = Paths.normalize(Paths.expandUser(rootPath))
        guard isDirectory(root) else { return ["error": "未找到记录目录"] }
        let archive = archive.flatMap { $0.isEmpty ? nil : $0 }
        let database = (cache as NSString).appendingPathComponent(
            "harness-v\(indexVersions[source] ?? 4)-\(source)-\(Py.identity(.string(Paths.resolve(root) ?? root)).prefix(12)).sqlite3")
        try Files.makeDirectory(cache)
        let db = try Database(path: database)
        Files.ownerOnly(database)
        var errors = 0
        let paths = jsonlFiles(root, source)
        do {
            // Every copy of an event is kept per file and deduplicated when read, so a vanished file (archive or
            // compression move) only drops its own rows.
            try db.execute("CREATE TABLE IF NOT EXISTS files(path TEXT PRIMARY KEY, inode INTEGER, size INTEGER, mtime INTEGER, offset INTEGER, state TEXT); CREATE TABLE IF NOT EXISTS events(id TEXT, file TEXT, ts REAL, data TEXT, PRIMARY KEY(id, file)); CREATE INDEX IF NOT EXISTS events_ts ON events(ts); CREATE INDEX IF NOT EXISTS events_file ON events(file);")
            try db.execute("BEGIN")
            try forgetMissing(db, Set(paths.map { Py.identity(.string($0)) }), archive: archive)
            let insert = try db.prepare("INSERT OR REPLACE INTO events VALUES (?,?,?,?)")
            for path in paths {
                guard let state = ClaudeUsage.state(of: path) else { errors += 1; continue }
                let fileKey = Py.identity(.string(path))
                let previous = try db.first("SELECT inode,size,mtime,offset,state FROM files WHERE path=?", [.text(fileKey)])
                if let previous, previous[0].int == state.inode, previous[1].int == state.size, previous[2].int == state.mtime { continue }
                let compressed = [".zst", ".zstd"].contains(suffix(path))
                let incremental = previous != nil && previous![0].int == state.inode && (previous![1].int ?? .max) < state.size && !compressed
                var fileState: JSONObject
                if incremental, let text = previous![4].text, let saved = (try? JSON.parse(text))?.object {
                    fileState = saved
                } else {
                    var session = name(path)
                    if session.hasSuffix(".zst") { session.removeLast(4) }
                    if session.hasSuffix(".jsonl") { session.removeLast(6) }
                    fileState = ["session": .string(session)]
                }
                var offset = incremental ? (previous![3].int ?? 0) : 0
                try db.execute("SAVEPOINT source_file")
                do {
                    if !incremental { try db.run("DELETE FROM events WHERE file=?", [.text(fileKey)]) }
                    let read: (Int64, UnsafeBufferPointer<UInt8>) throws -> Void = { _, line in
                        offset += Int64(line.count)
                        // Avoid parsing full tool/message content unless it carries usage or metadata.
                        if source == "codex" && !containsAny(line, codexWords) { return }
                        let row: JSON
                        do { row = try JSON.parse(line, surrogates: true) } catch let error as JSONError {
                            if error.encoding { throw Py.FileError("UnicodeDecodeError") }
                            return
                        }
                        guard let fields = row.object else { return }
                        do {
                            if source != "codex", fileState["isSeeded"]?.isTruthy == true, fields["type"] == .string("session/end-seed"),
                               try Py.get(Py.dict(fields["data"]), "inherited")?.isTruthy == true {
                                try db.run("DELETE FROM events WHERE file=?", [.text(fileKey)])
                            }
                            let event: Event?
                            switch source {
                            case "codex": event = try codexEvent(fields, &fileState)
                            case "deepseek-harness": event = try deepseekEvent(fields, &fileState)
                            case "workbuddy": event = try workbuddyEvent(fields, &fileState)
                            case "qoder": event = try qoderEvent(fields, &fileState)
                            default: event = try magpieEvent(fields, offset: offset)
                            }
                            if let event {
                                try insert.run([bind(event["id"]!), .text(fileKey), bind(event["timestamp"] ?? .null), .text(Py.dumps(.object(event)))])
                            }
                        } catch is Py.LineError {
                            return
                        }
                    }
                    if compressed {
                        let data = try Zstd.decompress(file: path)
                        try data.withUnsafeBytes { raw in
                            try LineReader.lines(raw.bindMemory(to: UInt8.self), from: Int(offset), read)
                        }
                    } else {
                        _ = try LineReader.read(path, from: offset, read)
                    }
                    try db.run("INSERT OR REPLACE INTO files VALUES (?,?,?,?,?,?)", [.text(fileKey), .int(state.inode), .int(state.size), .int(state.mtime),
                                                                                    .int(offset), .text(Py.dumps(.object(fileState)))])
                    try db.execute("RELEASE source_file")
                } catch is Py.FileError {
                    try db.execute("ROLLBACK TO source_file"); try db.execute("RELEASE source_file"); errors += 1
                } catch is LineReader.Unreadable {
                    try db.execute("ROLLBACK TO source_file"); try db.execute("RELEASE source_file"); errors += 1
                } catch is Zstd.Failure {
                    try db.execute("ROLLBACK TO source_file"); try db.execute("RELEASE source_file"); errors += 1
                }
            }
            try db.execute("COMMIT")
        } catch {
            try? db.execute("ROLLBACK")
            throw error
        }
        var events = Archive.merged(try uniqueEvents(db, start, end), archive, start, end)
        if source == "codex" { events = chatProjects(events, titles: Sessions.codexTitles(root)) }
        let defaultPrice: Price = source == "codex" ? Pricing.codexCost : Pricing.modelCost
        var result = aggregate(events, start: start, end: end, zone: zone, excludeProxy: excludeProxy, price: price ?? defaultPrice, carriers: carriers)
        if errors > 0 {
            result["warning"] = "部分记录无法读取"
            result["partial"] = true
            if (result["summary"]?.object?["requests"]?.number ?? 0) == 0 { result["error"] = "记录格式或权限待检查" }
        }
        if paths.isEmpty && !(archive.map { FileManager.default.fileExists(atPath: $0) } ?? false) { return ["error": "暂无用量记录"] }
        return result
    }

    static let codexWords = ["\"token_count\"", "\"session_meta\"", "\"turn_context\"", "\"thread_settings_applied\""].map { Array($0.utf8) }

    /// Whether any of the words occurs, in one pass: every word has an underscore, so only the places around each
    /// underscore are compared, instead of a full search per word.
    static func containsAny(_ line: UnsafeBufferPointer<UInt8>, _ words: [[UInt8]]) -> Bool {
        guard let base = line.baseAddress else { return false }
        let anchors = words.map { $0.firstIndex(of: 0x5F)! }
        var at = 0
        while at < line.count, let hit = memchr(base + at, 0x5F, line.count - at) {
            let underscore = base.distance(to: hit.assumingMemoryBound(to: UInt8.self))
            for (word, anchor) in zip(words, anchors) {
                let start = underscore - anchor
                if start >= 0, start + word.count <= line.count, memcmp(base + start, word, word.count) == 0 { return true }
            }
            at = underscore + 1
        }
        return false
    }

    /// Drops what the index holds for files that are gone; with an archive, their events move there first.
    static func forgetMissing(_ db: Database, _ active: Set<String>, archive: String?) throws {
        for row in try db.rows("SELECT path FROM files") {
            guard let key = row[0].text, !active.contains(key) else { continue }
            if let archive { try Archive.keep(archive, try db.rows("SELECT id,ts,data FROM events WHERE file=?", [.text(key)])) }
            try db.run("DELETE FROM events WHERE file=?", [.text(key)])
            try db.run("DELETE FROM files WHERE path=?", [.text(key)])
        }
    }

    /// One copy per event ID, whichever file was indexed first.
    static func uniqueEvents(_ db: Database, _ start: Double, _ end: Double) throws -> [Event] {
        try db.rows("SELECT data FROM events WHERE rowid IN (SELECT MIN(rowid) FROM events GROUP BY id) AND ts>=? AND ts<=?",
                    [.double(start), .double(end)]).compactMap { row in row[0].text.flatMap { try? JSON.parse($0).object } }
    }

    // MARK: Gemini CLI and OpenCode

    /// JSON and JSONL Gemini CLI sessions, usage only, including spent rewinds.
    public static func geminiUsage(root rootPath: String, cache: String, start: Double, end: Double, zone: TimeZone, excludeProxy: Bool = false,
                                   carriers: [Carrier]? = nil, archive: String? = nil) throws -> JSONObject {
        let root = Paths.normalize(Paths.expandUser(rootPath))
        guard isDirectory(root) else { return ["error": "未找到 Gemini CLI 记录"] }
        let archive = archive.flatMap { $0.isEmpty ? nil : $0 }
        let folders = isDirectory(root + "/tmp") ? [root + "/tmp"] : [root]
        var found = Set<String>()
        for folder in folders {
            for path in walk(folder) {
                let file = name(path)
                if file.hasPrefix("session-"), file.contains(".json"), [".json", ".jsonl"].contains(suffix(path)) { found.insert(path) }
                // New subagents may use UUID filenames instead of session-*.
                let relative = path.dropFirst(folder.count + 1).split(separator: "/")
                if file.hasSuffix(".jsonl"), let chats = relative.dropLast().firstIndex(of: "chats"), chats < relative.count - 1 { found.insert(path) }
            }
        }
        let paths = sortedPaths(Array(found))
        if paths.isEmpty && !(archive.map { FileManager.default.fileExists(atPath: $0) } ?? false) { return ["error": "暂无 Gemini CLI 记录"] }
        let target = (cache as NSString).appendingPathComponent("harness-gemini-v2-\(Py.identity(.string(Paths.resolve(root) ?? root)).prefix(12)).sqlite3")
        try Files.makeDirectory(cache)
        let db = try Database(path: target)
        Files.ownerOnly(target)
        try db.execute("CREATE TABLE IF NOT EXISTS files(path TEXT PRIMARY KEY,size INTEGER,mtime INTEGER); CREATE TABLE IF NOT EXISTS events(id TEXT,file TEXT,ts REAL,data TEXT,PRIMARY KEY(id,file)); CREATE INDEX IF NOT EXISTS events_ts ON events(ts); CREATE INDEX IF NOT EXISTS events_file ON events(file);")
        var errors = 0
        // Deleted sessions keep contributing only through the archive, when there is one.
        try transaction(db) { try forgetMissing(db, Set(paths.map { Py.identity(.string($0)) }), archive: archive) }
        for path in paths {
            let fileID = Py.identity(.string(path))
            guard let state = ClaudeUsage.state(of: path) else { errors += 1; continue }
            if let known = try db.first("SELECT size,mtime FROM files WHERE path=?", [.text(fileID)]), known[0].int == state.size, known[1].int == state.mtime { continue }
            if Double(state.mtime) / 1e9 < start { continue }
            var fileState: JSONObject = ["session": .string(((name(path)) as NSString).deletingPathExtension)]
            var events: [Event] = []
            do {
                guard let data = FileManager.default.contents(atPath: path) else { throw Py.FileError("unreadable") }
                var records: [JSON] = []
                if suffix(path) == ".json" {
                    guard let value = try? JSON.parse(data) else { throw Py.FileError("bad JSON") }
                    guard let object = value.object else { throw Py.FileError("not an object") }
                    fileState["session"] = .string(Py.str(Py.or(object["sessionId"], fileState["session"]!)))
                    let messages = Py.or(object["messages"], .array([]))
                    switch messages {
                    case .array(let items): records = items
                    case .string, .object: records = []
                    default: throw Py.FileError("messages not iterable")
                    }
                } else {
                    guard let text = String(data: data, encoding: .utf8) else { throw Py.FileError("UnicodeDecodeError") }
                    var lines = text.components(separatedBy: "\n")
                    lines.removeLast()  // The part after the last newline is still being written.
                    for line in lines {
                        let trimmed = line.hasSuffix("\r") ? String(line.dropLast()) : line
                        if let row = try? JSON.parse(trimmed) { records.append(row) }
                    }
                }
                for row in records {
                    guard let fields = row.object else { continue }
                    if let session = fields["sessionId"], session.isTruthy { fileState["session"] = .string(Py.str(session)) }
                    // Legacy full-history checkpoints and repeated message records share IDs. Rewinds don't refund spent tokens.
                    var checkpoint: JSON = .array([])
                    if let set = fields["$set"]?.object { checkpoint = set["messages"] ?? .array([]) }
                    guard let list = checkpoint.array else { throw Py.FileError("checkpoint is not a list") }
                    let items: [JSON] = [row] + list
                    for item in items {
                        guard let itemFields = item.object else { throw Py.FileError("AttributeError") }
                        do {
                            if let event = try geminiEvent(itemFields, &fileState) { events.append(event) }
                        } catch is Py.LineError { throw Py.FileError("TypeError") }
                    }
                }
                try transaction(db) {
                    try db.run("DELETE FROM events WHERE file=?", [.text(fileID)])
                    for event in events {
                        try db.run("INSERT OR REPLACE INTO events VALUES (?,?,?,?)", [bind(event["id"]!), .text(fileID), bind(event["timestamp"] ?? .null), .text(Py.dumps(.object(event)))])
                    }
                    try db.run("INSERT OR REPLACE INTO files VALUES (?,?,?)", [.text(fileID), .int(state.size), .int(state.mtime)])
                }
            } catch is Py.FileError {
                errors += 1
            }
        }
        var result = aggregate(Archive.merged(try uniqueEvents(db, start, end), archive, start, end), start: start, end: end, zone: zone,
                               excludeProxy: excludeProxy, price: Pricing.modelCost, carriers: carriers)
        if errors > 0 {
            result["partial"] = true
            if (result["summary"]?.object?["requests"]?.number ?? 0) == 0 { result["error"] = "Gemini 记录格式待检查" }
        }
        return result
    }

    static func transaction(_ db: Database, _ body: () throws -> Void) throws {
        try db.execute("BEGIN")
        do { try body(); try db.execute("COMMIT") } catch { try? db.execute("ROLLBACK"); throw error }
    }

    /// OpenCode's own database, read only.
    public static func opencodeUsage(root rootPath: String, start: Double, end: Double, zone: TimeZone, excludeProxy: Bool = false,
                                     carriers: [Carrier]? = nil) throws -> JSONObject {
        let root = Paths.normalize(Paths.expandUser(rootPath))
        let database = FileManager.default.fileExists(atPath: root) && !isDirectory(root) ? root : root + "/opencode.db"
        guard FileManager.default.fileExists(atPath: database), !isDirectory(database) else { return ["error": "未找到 OpenCode 数据库"] }
        let db = try Database(readOnly: Paths.resolve(database) ?? database, timeout: 4)
        try db.execute("PRAGMA query_only=ON")
        var events: [Event] = []
        try db.each("SELECT id,session_id,time_created,data FROM message WHERE time_created>=? AND time_created<=? AND json_extract(data,'$.role')='assistant'",
                    [.int(Int64((start * 1000).rounded(.towardZero))), end.isFinite ? .int(Int64((end * 1000).rounded(.towardZero))) : .double(.infinity)]) { row in
            guard let raw = row[3].text, let data = try? JSON.parse(raw), let fields = data.object else { return }
            let usage = Py.dict(fields["tokens"])
            guard let usageFields = usage.object else { return }
            let cached = Py.dict(usageFields["cache"])
            guard let cachedFields = cached.object else { return }
            var event = metrics(fields["modelID"], usageFields["input"], usageFields["output"], cachedFields["read"], cachedFields["write"], fields["cost"])
            let folder = fields["path"]?.object ?? JSONObject()
            event["id"] = .string(Py.identity(sqlJSON(row[0])))
            event["session"] = .string(Py.identity(sqlJSON(row[1])))
            guard let created = row[2].double else { return }
            event["timestamp"] = .double(created / 1000)
            event["provider"] = fields["providerID"] ?? .string("")
            event["project"] = Py.projectName(Py.or(folder["cwd"], folder["root"] ?? .null))
            events.append(event)
        }
        return aggregate(events, start: start, end: end, zone: zone, excludeProxy: excludeProxy, price: Pricing.modelCost, carriers: carriers)
    }

    static func sqlJSON(_ value: SQLValue) -> JSON {
        switch value {
        case .int(let number): return .int(number)
        case .double(let number): return .double(number)
        case .text(let text): return .string(text)
        default: return .null
        }
    }
}

extension Paths {
    /// What pathlib keeps of a path: no repeated or trailing slashes and no "." parts.
    public static func normalize(_ path: String) -> String {
        guard !path.isEmpty else { return "." }
        #if os(Windows)
        // WindowsPath: either slash separates, the drive or UNC share stays in front, backslashes come out.
        let unified = path.replacingOccurrences(of: "\\", with: "/")
        var prefix = "", rest = Substring(unified)
        if unified.count >= 2, unified[unified.index(after: unified.startIndex)] == ":" { prefix = String(unified.prefix(2)); rest = rest.dropFirst(2) }
        else if unified.hasPrefix("//") { prefix = "\\\\"; rest = rest.dropFirst(2) }
        let rooted = rest.hasPrefix("/"), joined = rest.split(separator: "/").filter { $0 != "." }.joined(separator: "\\")
        if prefix.isEmpty && !rooted { return joined.isEmpty ? "." : joined }
        return prefix + (rooted && !prefix.hasPrefix("\\\\") ? "\\" : "") + joined
        #else
        let absolute = path.hasPrefix("/")
        let parts = path.split(separator: "/").filter { $0 != "." }
        let joined = parts.joined(separator: "/")
        return absolute ? "/" + joined : (joined.isEmpty ? "." : joined)
        #endif
    }

    /// Whether a path names its place from the root: a leading slash, or on Windows a drive or a share.
    public static func isAbsolute(_ path: String) -> Bool {
        #if os(Windows)
        let characters = Array(path.prefix(3))
        return path.hasPrefix("/") || path.hasPrefix("\\") || (characters.count == 3 && characters[1] == ":" && (characters[2] == "\\" || characters[2] == "/"))
        #else
        return path.hasPrefix("/")
        #endif
    }
}

extension Database {
    /// Opens a database read only, as sqlite3.connect('file:...?mode=ro', uri=True).
    public convenience init(readOnly path: String, timeout: Double = 10) throws {
        try self.init(path: path, timeout: timeout, flags: SQLiteFlags.readOnly)
    }
}

extension Harness {
    /// Magpie's requests priced by the model actually served: Claude's own prices (with its one-hour cache writes
    /// and fast mode), the Codex table for GPT and Codex models, and the shared table for the rest.
    public static func magpiePrice(_ event: Event) -> Double? {
        let model = Py.str(event["model"]).lowercased().split(separator: "/", omittingEmptySubsequences: false).last.map(String.init) ?? ""
        if model.hasPrefix("claude-") {
            let oneHour = event["write1h"]?.number ?? 0
            let metrics = [Py.num(event["input"]), Py.num(event["output"]), Py.num(event["writes"]) - oneHour, oneHour, Py.num(event["cached"])]
            return ClaudePricing.cost(model: model, amounts: metrics, speed: event["fast"]?.isTruthy == true ? "fast" : "standard")
        }
        // Codex's own reviewer (codex-auto-review) bills as a GPT model too.
        if ["gpt-", "o1", "o3", "o4", "codex-"].contains(where: model.hasPrefix) {
            var renamed = event
            renamed["model"] = .string(model)
            if let cost = Pricing.codexCost(renamed) { return cost }
        }
        return Pricing.modelCost(event)
    }
}
