import Foundation

/// Claude Code's status-line callback, connected only when the user turns on 本机用量回传: it keeps the subscription
/// percentages Claude Code hands its status line, then runs the status line the user had before. No network,
/// cookies or keychain. Port of swiftbar_claude_local.py; the bridge is a copy of this engine.
public enum ClaudeBridge {
    static let bridge = "integrations/claude-statusline"
    static let limit = 2 * 1024 * 1024

    static func read(_ path: String) -> JSONObject {
        guard let handle = FileHandle(forReadingAtPath: path) else { return JSONObject() }
        defer { try? handle.close() }
        guard let raw = try? handle.read(upToCount: limit + 1), raw.count <= limit else { return JSONObject() }
        return (try? JSON.parse(raw))?.object ?? JSONObject()
    }

    static func write(_ path: String, _ value: JSONObject) throws {
        let folder = (path as NSString).deletingLastPathComponent
        try Files.makeDirectory(folder)
        try Files.replaceAtomically(path, with: Data(JSON.object(value).serialized().utf8), prefix: "." + (path as NSString).lastPathComponent)
    }

    public static func settingsPath(environment: [String: String] = ProcessInfo.processInfo.environment) -> String {
        let raw = (environment["CLAUDE_CONFIG_DIR"] ?? "").split(separator: ",", omittingEmptySubsequences: false).first.map { String($0).trimmingCharacters(in: .whitespacesAndNewlines) } ?? ""
        var root = raw.isEmpty ? Paths.home + "/.claude" : Paths.expandUser(raw)
        if (root as NSString).lastPathComponent == "projects" { root = (root as NSString).deletingLastPathComponent }
        return root + "/settings.json"
    }

    static func number(_ value: JSON?) -> Double? {
        switch value { case .int(let n): return Double(n); case .double(let n): return n.isFinite ? n : nil; default: return nil }
    }

    static func normalized(_ data: JSONObject, now: Double) -> JSONObject {
        var windows: [JSON] = []
        if let rates = data["rate_limits"]?.object {
            for (key, label) in [("five_hour", "5 小时"), ("seven_day", "本周")] {
                guard let row = rates[key]?.object, let percent = number(row["used_percentage"]), (0...100).contains(percent),
                      let reset = number(row["resets_at"]), now < reset, reset < now + 366 * 86400 else { continue }
                windows.append(["label": .string(label), "used": row["used_percentage"]!, "resetsAt": .string(ISOTime.utcString(reset))])
            }
        }
        return ["schema": 1, "source": "claude-code", "windows": .array(windows), "updatedAt": .double(now),
                "state": .string(windows.isEmpty ? "等待客户端返回订阅用量" : "本机用量"), "stale": .bool(windows.isEmpty)]
    }

    @discardableResult
    static func publish(_ state: String, _ data: JSONObject, now: Double = Date().timeIntervalSince1970, refresh: Bool = false) throws -> JSONObject {
        let path = state + "/" + ClaudeSubscription.snapshotFile
        let previous = read(path), result = normalized(data, now: now)
        let age = number(previous["updatedAt"]).map { now - $0 } ?? .infinity
        // The status line can run several times a second; never start a polling loop.
        let same = (previous["windows"] ?? .null) == result["windows"]
        if 0 <= age && age < (same ? 60 : 10) && (same || result["windows"]?.isTruthy == true) { return previous }
        try write(path, result)
        if refresh { _ = Shell.run(["/usr/bin/open", "-g", "swiftbar://refreshplugin?name=starmeter"], timeout: 3) }
        return result
    }

    public static func enabled(_ state: String) -> Bool { ClaudeSubscription.localEnabled(state) }

    /// The running engine's own file, copied as the bridge so it doesn't depend on where the plugin lives.
    static var executable: String? { Bundle.main.executablePath }

    public static func install(_ state: String, settings given: String? = nil, environment: [String: String] = ProcessInfo.processInfo.environment) throws {
        let state = Paths.resolve(state) ?? Runtime.absolute(state)
        let target = given ?? settingsPath(environment: environment)
        let settings = Paths.resolve(target) ?? Runtime.absolute(target)
        var current = read(settings)
        if FileManager.default.fileExists(atPath: settings) && current.isEmpty {
            // {} is a valid empty settings object; a malformed file is never replaced.
            guard let data = FileManager.default.contents(atPath: settings), let value = try? JSON.parse(data), value == .object(JSONObject()) else {
                throw HostFailure("Claude 设置格式异常")
            }
        }
        guard let executable else { throw HostFailure("找不到状态栏程序") }
        let bridge = state + "/" + bridge
        let command = TerminalJobs.join([bridge, "--claude-statusline", "--state-dir", state])
        func copyBridge() throws {
            try Files.makeDirectory((bridge as NSString).deletingLastPathComponent)
            let temporary = bridge + ".new-" + Files.randomName()
            try FileManager.default.copyItem(atPath: executable, toPath: temporary)
            Platform.setMode(temporary, 0o700)
            guard Platform.replace(temporary, bridge) else { Platform.remove(temporary); throw HostFailure("无法写入状态栏程序") }
        }
        let backup = state + "/" + ClaudeSubscription.backupFile
        if enabled(state) {
            try copyBridge()
            // An earlier bridge (the Python one) is pointed at this one; what ran before it is kept.
            var meta = read(backup)
            if meta["managedCommand"] != .string(command), var status = current["statusLine"]?.object {
                status["command"] = .string(command)
                current["statusLine"] = .object(status)
                meta["managedCommand"] = .string(command)
                try write(backup, meta)
                try write(settings, current)
            }
            return
        }
        let original = current["statusLine"]
        if let original, original.object?["type"] != .string("command") || original.object?["command"]?.string == nil {
            throw HostFailure("已有状态栏格式尚未支持，保留原设置")
        }
        try write(backup, ["settings": .string(settings), "original": original ?? .null, "managedCommand": .string(command)])
        try copyBridge()
        var status = original?.object ?? JSONObject()
        status["type"] = "command"; status["command"] = .string(command)
        current["statusLine"] = .object(status)
        try write(settings, current)
    }

    public static func uninstall(_ state: String) {
        let meta = read(state + "/" + ClaudeSubscription.backupFile)
        if enabled(state), let settings = meta["settings"]?.string {
            var current = read(settings)
            if let original = meta["original"], !original.isNull { current["statusLine"] = original } else { current["statusLine"] = nil }
            try? write(settings, current)
        }
        Platform.remove(state + "/" + ClaudeSubscription.snapshotFile)
        // The backup stays if the user replaced the integration themselves.
    }

    /// The status-line callback: keep the percentages, then hand the input to the status line that was there before.
    public static func run(state: String) -> Int32 {
        let raw = FileHandle.standardInput.readData(ofLength: limit + 1)
        var result = JSONObject()
        if raw.count <= limit, let data = (try? JSON.parse(raw))?.object { result = (try? publish(state, data, refresh: true)) ?? JSONObject() }
        if let previous = read(state + "/" + ClaudeSubscription.backupFile)["original"]?.object, let command = previous["command"]?.string, !command.isEmpty {
            let process = Process()
            process.executableURL = URL(fileURLWithPath: "/bin/sh")
            process.arguments = ["-c", command]
            let input = Pipe()
            process.standardInput = input
            if (try? process.run()) != nil {
                input.fileHandleForWriting.write(raw); try? input.fileHandleForWriting.close()
                let deadline = Date().addingTimeInterval(4)
                while process.isRunning && Date() < deadline { Platform.sleep(0.02) }
                if process.isRunning { process.terminate() }
            }
        } else {
            let values = (result["windows"]?.array ?? []).compactMap(\.object).map { Py.str($0["label"] ?? .null) + " " + Runtime.g($0["used"]?.number ?? 0) + "%" }
            print("Claude" + (values.isEmpty ? "" : " · " + values.joined(separator: " · ")))
        }
        return 0
    }
}
