import Foundation

/// Claude Code and Codex sessions running or waiting for input now. Only the end of each transcript changed in the
/// last half hour is read. Of a session nothing is kept but its state, the title its client gave it, its project
/// folder's name, model, reasoning effort, how full its context is, and when it last changed; never prompts,
/// replies or paths. Port of swiftbar_sessions.py.
public enum Sessions {
    public typealias Price = (JSON?, JSONObject) -> Double?
    static let active = 1800.0, stalled = 600.0, tail = 512 * 1024, detail = 2 * 1024 * 1024, turns = 8
    static let claudeWindows: [Double] = [200_000, 1_000_000]

    static func text(_ value: JSON?) -> String? {
        guard let string = value?.string?.trimmingCharacters(in: .whitespacesAndNewlines), !string.isEmpty else { return nil }
        return string
    }

    static func number(_ value: JSON?) -> JSON {
        guard let value, value.bool == nil, let number = value.number, number >= 0 else { return .int(0) }
        return value
    }

    /// The JSON objects among the lines in the last `size` bytes of a file; the first is dropped when it was cut.
    static func rowsAtEnd(_ path: String, size: Int = tail) throws -> [JSONObject] {
        guard let handle = FileHandle(forReadingAtPath: path) else { throw Py.FileError("unreadable", kind: "OSError") }
        defer { try? handle.close() }
        let end = try handle.seekToEnd()
        try handle.seek(toOffset: end > UInt64(size) ? end - UInt64(size) : 0)
        let data = try handle.readToEnd() ?? Data()
        var lines = data.split(separator: 0x0A, omittingEmptySubsequences: false)
        if end > UInt64(size) && !lines.isEmpty { lines.removeFirst() }
        return lines.compactMap { line in line.withUnsafeBytes { raw in (try? JSON.parse(raw.bindMemory(to: UInt8.self), surrogates: true))?.object } }
    }

    static func firstRow(_ path: String) throws -> JSON {
        guard let handle = FileHandle(forReadingAtPath: path) else { throw Py.FileError("unreadable", kind: "OSError") }
        defer { try? handle.close() }
        var line = Data()
        while line.count < tail, let byte = try handle.read(upToCount: 1), !byte.isEmpty {
            line.append(byte)
            if byte[0] == 0x0A { break }
        }
        return (try? JSON.parse(line)) ?? .object(JSONObject())
    }

    /// A Claude Code transcript: waiting once the last reply ended its turn, running otherwise. Its title is the
    /// last one the transcript records: the user's or the App's, else the one Claude Code made up.
    static func claude(_ path: String) throws -> JSONObject? {
        var last: JSONObject?, reply: JSONObject?, folder: JSON?, named: String?, made: String?
        for row in try rowsAtEnd(path) {
            if row["type"] == .string("custom-title") { named = text(row["customTitle"]) ?? named }
            else if row["type"] == .string("ai-title") { made = text(row["aiTitle"]) ?? made }
            if row["isSidechain"]?.isTruthy == true { continue }
            if let cwd = row["cwd"], cwd.isTruthy { folder = cwd }
            if row["type"] == .string("assistant") || row["type"] == .string("user") { last = row }
            let message = row["message"]?.object ?? JSONObject()
            if row["type"] == .string("assistant"), message["usage"]?.object != nil { reply = message }
        }
        guard let last else { return nil }
        let message = last["message"]?.object ?? JSONObject()
        let waiting = last["type"] == .string("assistant") && [JSON.string("end_turn"), .string("stop_sequence"), .string("max_tokens"), .string("refusal")]
            .contains(message["stop_reason"] ?? .null) && message["stop_reason"]?.string != nil
        var context: JSON = .null, model: JSON = .null
        if let reply, let usage = reply["usage"]?.object {
            model = reply["model"] ?? .null
            let used = ["input_tokens", "cache_read_input_tokens", "cache_creation_input_tokens", "output_tokens"].reduce(JSON.int(0)) { Harness.add($0, number(usage[$1])) }
            let total = used.number ?? 0
            let window = claudeWindows.first { total <= $0 } ?? claudeWindows.last!
            context = total != 0 ? (total / window < 1 ? .double(total / window) : .double(1)) : .null
        }
        return ["tool": "Claude", "state": .string(waiting ? "waiting" : "running"), "title": (named ?? made).map(JSON.string) ?? .null,
                "project": projectName(folder), "model": model, "effort": .null, "context": context]
    }

    /// A Codex rollout: running between task_started and task_complete (or an aborted turn). A subagent's thread
    /// (Guardian reviews among them) works for another session and is left out.
    static func codex(_ path: String) throws -> JSONObject? {
        guard let head = try firstRow(path).object else { throw Py.LineError() }
        let payload = head["payload"]?.object ?? JSONObject()
        if payload["source"]?.object?["subagent"] != nil { return nil }
        var folder: JSON? = head["type"] == .string("session_meta") ? payload["cwd"] : nil
        var state: String?, model: JSON = .null, context: JSON = .null, effort: String?
        for row in try rowsAtEnd(path) {
            let data = row["payload"]?.object ?? JSONObject()
            let kind = data["type"]
            if row["type"] == .string("turn_context") { model = Py.or(data["model"], model); effort = text(data["effort"]) ?? effort }
            if row["type"] == .string("session_meta"), let cwd = data["cwd"], cwd.isTruthy { folder = cwd }
            if kind == .string("task_started") { state = "running" }
            else if kind == .string("task_complete") || kind == .string("turn_aborted") { state = "waiting" }
            else if kind == .string("token_count"), let info = data["info"]?.object {
                let last = Py.or(info["last_token_usage"], .object(JSONObject())).object ?? JSONObject()
                let window = number(info["model_context_window"]).number ?? 0
                let used = Harness.add(number(last["input_tokens"]), number(last["output_tokens"])).number ?? 0
                if window != 0 && used != 0 { context = used / window < 1 ? .double(used / window) : .double(1) }
            }
        }
        guard let state else { return nil }
        return ["tool": "ChatGPT", "state": .string(state), "title": .null, "project": projectName(folder), "model": model,
                "effort": effort.map(JSON.string) ?? .null, "context": context]
    }

    /// The folder's name; none for the folder the ChatGPT app makes for a chat (Documents/Codex/<day>/<slug>), whose
    /// title the session already shows.
    static func projectName(_ folder: JSON?) -> JSON {
        if let path = folder?.string {
            var trimmed = path.trimmingCharacters(in: .whitespacesAndNewlines)
            while let last = trimmed.last, last == "/" || last == "\\" { trimmed.removeLast() }
            trimmed = trimmed.replacingOccurrences(of: "\\", with: "/")
            if trimmed.range(of: #"/Codex/\d{4}-\d{2}-\d{2}/[^/]+$"#, options: .regularExpression) != nil { return .null }
        }
        return Py.projectName(folder)
    }

    /// Codex's thread names by thread ID, from its session index; the last name given wins.
    static func codexTitles(_ root: String) -> [String: String] {
        guard let data = FileManager.default.contents(atPath: root + "/session_index.jsonl") else { return [:] }
        var names: [String: String] = [:]
        for line in data.split(separator: 0x0A) {
            guard let row = (try? JSON.parse(Data(line)))?.object, let id = row["id"]?.string, let name = text(row["thread_name"]) else { continue }
            names[id] = name
        }
        return names
    }

    static func stamp(_ value: JSON?) -> JSON {
        guard let text = value.map(Py.str), let time = try? ISOTime.parse(text) else { return .null }
        return .double(time.timestamp())
    }

    static func turn(_ at: JSON) -> JSONObject { ["at": at, "requests": 0, "tokens": 0, "cost": 0.0, "unpriced": 0, "tools": .object(JSONObject())] }

    static func countTool(_ turn: inout JSONObject, _ name: String) {
        var tools = turn["tools"]?.object ?? JSONObject()
        tools[name] = Harness.add(tools[name] ?? .int(0), .int(1))
        turn["tools"] = .object(tools)
    }

    static func lastTurns(_ turns: [JSONObject]) -> [JSON] {
        turns.filter { !($0["at"]?.isNull ?? true) || $0["requests"]?.isTruthy == true }.suffix(Sessions.turns).reversed().map(JSON.object)
    }

    /// The last turns of a Claude Code session, newest first: from one prompt to the next. A reply streamed over
    /// several lines counts once; a subagent's work belongs to the turn that started it.
    static func claudeTurns(_ path: String, price: Price) throws -> [JSON] {
        var turns = [turn(.null)], seen = Set<String>()
        for row in try rowsAtEnd(path, size: detail) {
            let message = row["message"]?.object ?? JSONObject()
            let content = message["content"]
            if row["type"] == .string("user") && row["isSidechain"]?.isTruthy != true && row["isMeta"]?.isTruthy != true {
                var kinds = Set<String>()
                for item in content?.array ?? [] { if let kind = item.object?["type"] { kinds.insert(Py.dumps(kind)) } }
                let spoken = content?.string.map { !$0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty } ?? false
                if spoken || (!kinds.isEmpty && !kinds.contains(Py.dumps(.string("tool_result")))) { turns.append(turn(stamp(row["timestamp"]))) }
            } else if row["type"] == .string("assistant") {
                for item in content?.array ?? [] {
                    if let fields = item.object, fields["type"] == .string("tool_use"), let name = fields["name"], name.isTruthy { countTool(&turns[turns.count - 1], Py.str(name)) }
                }
                let key = Py.dumps(Py.or(message["id"], row["uuid"] ?? .null))
                if let usage = message["usage"]?.object, !seen.contains(key) {
                    seen.insert(key)
                    var current = turns[turns.count - 1]
                    current["requests"] = Harness.add(current["requests"]!, .int(1))
                    let tokens = ["input_tokens", "cache_read_input_tokens", "cache_creation_input_tokens", "output_tokens"].reduce(JSON.int(0)) { Harness.add($0, number(usage[$1])) }
                    current["tokens"] = Harness.add(current["tokens"]!, tokens)
                    if let cost = price(message["model"], usage) { current["cost"] = Harness.add(current["cost"]!, .double(cost)) }
                    else { current["unpriced"] = Harness.add(current["unpriced"]!, .int(1)) }
                    turns[turns.count - 1] = current
                }
            }
        }
        return lastTurns(turns)
    }

    /// The last turns of a Codex session, newest first: from task_started to task_complete; each change of the
    /// running token total is one request.
    static func codexTurns(_ path: String, price: Price) throws -> [JSON] {
        var turns = [turn(.null)], model: JSON = .null, total: JSON?
        for row in try rowsAtEnd(path, size: detail) {
            let data = row["payload"]?.object ?? JSONObject()
            let kind = data["type"]
            if row["type"] == .string("turn_context") { model = Py.or(data["model"], model) }
            if kind == .string("task_started") {
                turns.append(turn(stamp(row["timestamp"])))
            } else if row["type"] == .string("response_item"), let kindText = kind?.string,
                      ["function_call", "custom_tool_call", "local_shell_call", "web_search_call"].contains(kindText) {
                let name = data["name"].flatMap { $0.isTruthy ? Py.str($0) : nil } ?? ["local_shell_call": "shell", "web_search_call": "web_search"][kindText]
                if let name { countTool(&turns[turns.count - 1], name) }
            } else if kind == .string("token_count"), let info = data["info"]?.object {
                let running = info["total_token_usage"]
                let last = Py.or(info["last_token_usage"], .object(JSONObject())).object ?? JSONObject()
                if last.isEmpty || running == total { continue }
                total = running
                var current = turns[turns.count - 1]
                current["requests"] = Harness.add(current["requests"]!, .int(1))
                current["tokens"] = Harness.add(current["tokens"]!, Harness.add(number(last["input_tokens"]), number(last["output_tokens"])))
                if let cost = price(model, last) { current["cost"] = Harness.add(current["cost"]!, .double(cost)) }
                else { current["unpriced"] = Harness.add(current["unpriced"]!, .int(1)) }
                turns[turns.count - 1] = current
            }
        }
        return lastTurns(turns)
    }

    static func recent(_ root: String, depth: Int, since: Double, matching: (String) -> Bool) -> [String] {
        var found: [String] = []
        func visit(_ folder: String, _ level: Int) {
            for name in ((try? FileManager.default.contentsOfDirectory(atPath: folder)) ?? []).sorted() {
                let path = folder + "/" + name
                if level < depth { if Harness.isDirectory(path) { visit(path, level + 1) } }
                else if matching(name), !Harness.isDirectory(path), let state = ClaudeUsage.state(of: path), Double(state.mtime) / 1e9 >= since { found.append(path) }
            }
        }
        visit(root, 0)
        return found
    }

    /// Sessions changed in the last half hour, the most recent first; a running one silent for ten minutes is
    /// dropped. With prices, each carries its last turns as well.
    public static func active(claudeRoots: [String], codexRoot: String?, now: Double, prices: [String: Price]? = nil,
                              claudeApp: [String: (title: String?, effort: String?)] = [:]) -> [JSON] {
        var candidates: [(String, String)] = []
        for root in claudeRoots {
            for path in recent(root, depth: 1, since: now - active, matching: { $0.hasSuffix(".jsonl") }) where !path.split(separator: "/").contains("subagents") {
                candidates.append(("Claude", path))
            }
        }
        if let codexRoot {
            // A rollout is filed under the day its thread started, and a thread can still be going days later.
            for path in recent(codexRoot + "/sessions", depth: 3, since: now - active, matching: { $0.hasPrefix("rollout-") && $0.hasSuffix(".jsonl") }) {
                candidates.append(("ChatGPT", path))
            }
        }
        var titles: [String: String]?
        var found: [(Double, JSONObject)] = []
        for (tool, path) in candidates {
            guard let state = ClaudeUsage.state(of: path) else { continue }
            let changed = Double(state.mtime) / 1e9
            guard var session = (try? (tool == "Claude" ? claude(path) : codex(path))) ?? nil else { continue }
            if session["state"] == .string("running") && now - changed > stalled { continue }
            let stem = ((path as NSString).lastPathComponent as NSString).deletingPathExtension
            if tool == "Claude" {
                let app = claudeApp[stem]
                if session["title"]?.isNull ?? true { session["title"] = app?.title.map(JSON.string) ?? .null }
                session["effort"] = app?.effort.map(JSON.string) ?? .null
            } else {
                titles = titles ?? codexTitles(codexRoot ?? "")
                session["title"] = titles?[String(stem.suffix(36))].map(JSON.string) ?? .null
            }
            session["id"] = .string(String(SHA256.hex(Array(path.utf8)).prefix(16)))
            session["at"] = .double(changed)
            // The turns are read here, so the path itself never leaves this function.
            if let prices, let price = prices[tool] {
                session["turns"] = .array((try? (tool == "Claude" ? claudeTurns(path, price: price) : codexTurns(path, price: price))) ?? [])
            }
            found.append((changed, session))
        }
        return found.enumerated().sorted { $0.element.0 != $1.element.0 ? $0.element.0 > $1.element.0 : $0.offset < $1.offset }.map { .object($0.element.1) }
    }

    /// Sessions that were running at the last read and have just finished their turn.
    public static func newlyWaiting(previous: [JSON], current: [JSON], now: Double, recent: Double = 180) -> [JSONObject] {
        var before: [String: JSON] = [:]
        for session in previous { if let fields = session.object, let id = fields["id"] { before[Py.dumps(id)] = fields["state"] ?? .null } }
        return current.compactMap(\.object).filter { session in
            session["state"] == .string("waiting") && before[Py.dumps(session["id"] ?? .null)] == .string("running")
                && now - (session["at"]?.number ?? 0) <= recent
        }
    }
}

extension Runtime {
    /// A request's estimated cost for session detail, by tool, as the cards price it; nil when unpriced.
    public func sessionPrices() -> [String: Sessions.Price] {
        let claude: Sessions.Price = { model, usage in
            guard let metrics = try? ClaudeUsage.normalize(.object(usage)) else { return nil }
            return ClaudePricing.cost(model: Py.str(Py.or(model, .string(""))), metrics: metrics, speed: usage["speed"]?.string ?? "standard")
        }
        let chatgpt: Sessions.Price = { model, usage in
            let inputs = Py.number(usage["input_tokens"])
            let cached = Harness.smaller(inputs, Py.number(usage["cached_input_tokens"]))
            return Pricing.codexCost(Harness.metrics(model, Harness.minus(inputs, cached), usage["output_tokens"], cached))
        }
        return ["Claude": claude, "ChatGPT": chatgpt]
    }

    func codexHome() -> String {
        Runtime.absolute(Paths.expandUser(env("OCX_CHATGPT_HOME") ?? env("CODEX_HOME") ?? home + "/.codex"))
    }

    /// Claude Code and Codex sessions running or waiting now; with the alert on, one that has just finished its turn is told of.
    public func sessionsCached(ttl: Double? = nil) -> (JSONObject?, String?) {
        let previous = readCache("sessions")["payload"]?.object?["sessions"]?.array ?? []
        return cachedFetch("sessions", ttl: ttl ?? refreshTTL(30)) {
            let now = Date().timeIntervalSince1970
            let found = Sessions.active(claudeRoots: ClaudeApp.projectRoots(home: home, environment: environment), codexRoot: codexHome(), now: now,
                                        prices: sessionPrices(), claudeApp: ClaudeApp.sessionSettings(root: sourcePath("claude"), since: now - Sessions.active))
            if settings()["sessionAlerts"]?.isTruthy == true {
                for session in Sessions.newlyWaiting(previous: previous, current: found, now: now).prefix(3) {
                    let name = session["title"].flatMap { $0.isTruthy ? Py.str($0) : nil } ?? session["project"].flatMap { $0.isTruthy ? Py.str($0) : nil } ?? "未知项目"
                    _ = postNotice("会话等待输入", "\(Py.str(session["tool"] ?? .null)) · \(name) 这一轮已完成")
                }
            }
            return (["sessions": .array(found)], nil)
        }
    }

    /// A notification from Starmeter: the standalone app's own, or SwiftBar's.
    @discardableResult
    public func postNotice(_ subtitle: String, _ body: String) -> Bool {
        if let desktop { desktop.notify(body, subtitle: subtitle); return true }
        if macApp {
            guard readCache("notifications-enabled")["allowed"] == .bool(true) else { return false }
            let path = cachePath("notice-" + UUID().uuidString.replacingOccurrences(of: "-", with: "").lowercased())
            writeJSONCache(path, ["subtitle": .string(subtitle), "body": .string(body)])
            return FileManager.default.fileExists(atPath: path)
        }
        let name = swiftbarName
        var parts = URLComponents()
        parts.queryItems = [URLQueryItem(name: "plugin", value: name), URLQueryItem(name: "title", value: Runtime.product), URLQueryItem(name: "subtitle", value: subtitle),
                            URLQueryItem(name: "body", value: body), URLQueryItem(name: "href", value: "swiftbar://refreshplugin?name=" + Accounts.quote(name))]
        let url = "swiftbar://notify?" + (parts.percentEncodedQuery ?? "")
        return Shell.run(["/usr/bin/open", "-g", url], timeout: 5) != nil
    }

    /// SwiftBar's name for the plugin: the file name up to its first dot.
    public var swiftbarName: String { String((plugin as NSString).lastPathComponent.split(separator: ".").first ?? "starmeter") }
}
