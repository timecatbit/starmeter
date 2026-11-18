import Foundation

/// Qoder CN plan Credits from the desktop app's own log: read only, no network, no login data. Only the Credits
/// fields of the app's account.getQuotaUsage summary are kept. Format as observed by TokenTracker (MIT) for
/// Qoder CN 0.4.x. Port of swiftbar_qoder.py.
public enum Qoder {
    static let maxAge = 7.0 * 86400, maxFiles = 8, scanBytes = 4 * 1024 * 1024
    /// A far-future expiry (Qoder uses 9999-12-31) means no reset date.
    static let noReset = 4102444800.0

    public static func logRoots(home: String, environment: [String: String] = ProcessInfo.processInfo.environment) -> [String] {
        #if os(Windows)
        let base = environment["APPDATA"] ?? home + "\\AppData\\Roaming"
        #else
        let base = home + "/Library/Application Support"
        #endif
        return [base + "/com.qodercn.app.stable/logs", base + "/QoderCN/logs"]
    }

    static func logFiles(home: String, now: Double) -> [String] {
        var found: [(Double, String)] = []
        for root in logRoots(home: home) where Harness.isDirectory(root) {
            for path in Harness.walk(root) where Harness.name(path) == "main.log" {
                guard let state = ClaudeUsage.state(of: path) else { continue }
                let mtime = Double(state.mtime) / 1e9
                if now - mtime <= maxAge { found.append((mtime, path)) }
            }
        }
        return found.sorted { $0.0 != $1.0 ? $0.0 > $1.0 : ClaudeUsage.pythonLess($1.1, $0.1) }.prefix(maxFiles).map(\.1)
    }

    static func amount(_ value: JSON?) -> Double? {
        guard let value, value.bool == nil, let number = value.number, number >= 0, number < 1e12 else { return nil }
        return number
    }

    /// Seconds since 1970 from seconds or milliseconds; nil when unusable.
    static func epoch(_ value: JSON?) -> Double? {
        guard let value, value.bool == nil, let number = value.number, number > 0, number < 1e15 else { return nil }
        return number > 1e10 ? number / 1000 : number
    }

    /// {used,total,remaining} in Credits, or nil when the block is missing or malformed.
    static func quota(_ value: JSON?) -> JSON? {
        guard let fields = value?.object, let used = amount(fields["used"]), let total = amount(fields["total"]) else { return nil }
        let remaining = amount(fields["remaining"]) ?? Swift.max(0, total - used)
        return ["used": .double(used), "total": .double(total), "remaining": .double(remaining)]
    }

    /// The Credits fields of one AccountUsage line, with the time the app logged it.
    static func summary(_ line: String) -> JSONObject? {
        guard line.hasPrefix("["), let close = line.firstIndex(of: "]"), close > line.index(after: line.startIndex),
              let start = line.range(of: "{\"")?.lowerBound, let last = line.lastIndex(of: "}"), start <= last else { return nil }
        let stamp = String(line[line.index(after: line.startIndex)..<close])
        guard let observed = try? ISOTime.parse(stamp).timestamp(assuming: .current),
              let data = try? JSON.parse(String(line[start...last])) else { return nil }
        guard let usage = data.object?["qoderUsage"]?.object else { return nil }
        let plan = quota(usage["userQuota"]), extra = quota(usage["addOnQuota"])
        if plan == nil && extra == nil { return nil }
        let resets = epoch(usage["expiresAt"])
        return ["observedAt": .double(observed), "plan": plan ?? .null, "addOn": extra ?? .null,
                "resetsAt": resets.flatMap { $0 < noReset ? JSON.double($0) : nil } ?? .null, "exceeded": .bool(usage["isQuotaExceeded"] == .bool(true) && usage["isQuotaExceeded"]?.bool == true)]
    }

    /// The newest summary across recent logs, or nil. Lines are read from the end of each file.
    public static func snapshot(home: String, now: Double = Date().timeIntervalSince1970) -> JSONObject? {
        var best: JSONObject?
        for path in logFiles(home: home, now: now) {
            guard let handle = FileHandle(forReadingAtPath: path) else { continue }
            let size = (try? handle.seekToEnd()) ?? 0
            try? handle.seek(toOffset: size > UInt64(scanBytes) ? size - UInt64(scanBytes) : 0)
            let data = (try? handle.readToEnd()) ?? Data()
            try? handle.close()
            let text = String(decoding: data, as: UTF8.self)
            for line in text.split(omittingEmptySubsequences: false, whereSeparator: { $0 == "\n" || $0 == "\r" || $0 == "\r\n" }).reversed() {
                guard line.contains("\"qoderUsage\"") else { continue }
                if let row = summary(String(line)), now - (row["observedAt"]?.number ?? 0) <= maxAge {
                    if best == nil || (row["observedAt"]?.number ?? 0) > (best!["observedAt"]?.number ?? 0) { best = row }
                    break
                }
            }
        }
        return best
    }
}

/// ChatGPT's rate-limit windows from the official read-only App Server: never refreshes or logs in, never calls a model.
public enum ChatGPT {
    public static func account(binary given: String? = nil, home: String? = nil, environment: [String: String] = ProcessInfo.processInfo.environment) -> JSONObject {
        guard let binary = given ?? Shell.which("codex", environment: environment) else { return ["state": "未安装 ChatGPT 编程客户端", "windows": []] }
        var env = environment
        if let home { env["CODEX_HOME"] = home }
        let process = Process()
        process.executableURL = URL(fileURLWithPath: binary)
        process.arguments = ["app-server"]
        process.environment = env
        let input = Pipe(), output = Pipe()
        process.standardInput = input
        process.standardOutput = output
        process.standardError = FileHandle.nullDevice
        do { try process.run() } catch { return ["error": "无法启动 ChatGPT 编程客户端"] }
        defer {
            if process.isRunning { process.terminate() }
            let wait = Date().addingTimeInterval(2)
            while process.isRunning && Date() < wait { usleep(20_000) }
            if process.isRunning { kill(process.processIdentifier, SIGKILL); process.waitUntilExit() }
            try? input.fileHandleForWriting.close()
        }
        // One deadline for the whole handshake; frames are read on their own thread.
        let deadline = Date().addingTimeInterval(12)
        let lines = LineQueue()
        let reader = output.fileHandleForReading
        Thread.detachNewThread {
            var buffer = Data()
            while true {
                let chunk = reader.availableData
                if chunk.isEmpty { lines.finish(); return }
                buffer.append(chunk)
                while let newline = buffer.firstIndex(of: 0x0A) {
                    let line = buffer[buffer.startIndex..<newline]
                    lines.push(line.count > 1024 * 1024 ? Data() : Data(line))
                    buffer.removeSubrange(buffer.startIndex...newline)
                }
            }
        }
        var sequence: Int64 = 0
        func send(_ value: JSON) -> Bool {
            let data = Data((value.serialized(ensureASCII: true) + "\n").utf8)
            do { try input.fileHandleForWriting.write(contentsOf: data); return true } catch { return false }
        }
        func call(_ method: String, _ params: JSON = .object(JSONObject())) -> JSON? {
            sequence += 1
            let id = sequence
            guard send(["id": .int(id), "method": .string(method), "params": params]) else { return nil }
            while let line = lines.pop(until: deadline) {
                guard let response = (try? JSON.parse(line))?.object else { continue }
                if response["id"] == .int(id) { return response["result"] ?? .null }
            }
            return nil
        }
        guard call("initialize", ["clientInfo": ["name": "starmeter", "title": "Starmeter", "version": "5.0.0"]]).map({ !$0.isNull }) == true else {
            return ["error": "客户端接口不可用"]
        }
        _ = send(["method": "initialized"])
        guard let account = call("account/read", ["refreshToken": false])?.object else { return ["error": "无法读取客户端登录状态"] }
        let kind = Py.dict(account["account"]).object?["type"]
        guard kind == .string("chatgpt") || kind == .string("chatgptAuthTokens") else { return ["state": "需 ChatGPT 账户登录", "windows": []] }
        guard let result = call("account/rateLimits/read")?.object else { return ["error": "额度暂不可用"] }
        var buckets = Py.or(result["rateLimitsByLimitId"], .object(JSONObject())).object ?? JSONObject()
        if buckets.isEmpty, let limits = result["rateLimits"], limits.isTruthy { buckets = ["codex": limits] }
        var windows: [JSON] = []
        for (key, bucket) in buckets {
            guard let fields = bucket.object else { continue }
            for slot in ["primary", "secondary"] {
                guard let window = fields[slot]?.object, let used = window["usedPercent"], let value = Accounts.numeric(used) else { continue }
                let left = Harness.minus(.int(100), used.bool != nil ? .int(Int64(value)) : used)
                let capped: JSON = (left.number ?? 0) >= 100 ? .int(100) : left
                windows.append(["name": .string(Py.str(Py.or(fields["limitName"], .string(key)))), "slot": .string(slot),
                                "remaining": (capped.number ?? 0) <= 0 ? .int(0) : capped,
                                "minutes": Py.number(window["windowDurationMins"]), "resetsAt": Py.number(window["resetsAt"])])
            }
        }
        return ["state": .string(windows.isEmpty ? "暂无可读取额度" : "已同步"), "windows": .array(windows),
                "plan": .string(Py.str(Py.or(Py.dict(account["account"]).object?["planType"], .string(""))))]
    }

    /// Lines handed from the reading thread to the caller, waiting at most until a deadline.
    final class LineQueue {
        private var lines: [Data] = []
        private var finished = false
        private let condition = NSCondition()

        func push(_ line: Data) { condition.lock(); lines.append(line); condition.signal(); condition.unlock() }
        func finish() { condition.lock(); finished = true; condition.broadcast(); condition.unlock() }

        func pop(until deadline: Date) -> Data? {
            condition.lock(); defer { condition.unlock() }
            while lines.isEmpty && !finished { if !condition.wait(until: deadline) { return nil } }
            return lines.isEmpty ? nil : lines.removeFirst()
        }
    }
}
