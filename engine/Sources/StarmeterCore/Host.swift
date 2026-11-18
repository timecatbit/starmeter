import Foundation

/// What an action needs from the place it runs: questions to the user, notices, files and the clipboard.
/// SwiftBar's plugin runs uses AppleScript dialogs; a host with its own windows (the Windows tray, the
/// standalone app) answers through its own UI.
public protocol Host: AnyObject {
    func confirm(_ message: String, detail: String, accept: String) -> Bool
    /// The text typed, or nil when the user cancelled.
    func textDialog(_ message: String, value: String, hidden: Bool, title: String) -> String?
    /// The value of the option chosen, or nil.
    func choose(title: String, prompt: String, _ first: (label: String, value: String), _ second: (label: String, value: String)) -> String?
    func choosePath(_ message: String, folder: Bool) -> String?
    func savePath(_ message: String, name: String) -> String?
    func error(_ title: String, _ detail: String)
    func notify(_ message: String, subtitle: String)
    func openLocal(_ path: String, text: Bool) throws
    func copy(_ text: String)
}

/// An in-process host with its own windows and refresh queue, as the Windows tray: Python's DESKTOP.
public protocol DesktopHost: Host {
    func saveOverrides(_ values: [(String, String)]) throws
    func configFile() -> String
    func openClaude()
    func updateCLI(binary: String, key: String)
    func submitTest(_ provider: String, _ id: String)
    func requestRefresh(_ mode: String?)
    func saveRange(_ key: String)
    func localProxy(_ base: String) -> Bool
}

public struct HostFailure: Error, CustomStringConvertible {
    public let description: String
    public init(_ description: String) { self.description = description }
}

/// Dialogs through osascript, as the SwiftBar plugin always has; notices titled Starmeter.
public final class AppleScriptHost: Host {
    public init() {}

    static func literal(_ text: String) -> String { "\"" + text.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(of: "\"", with: "\\\"") + "\"" }

    /// A newline inside an AppleScript string is written as `return` joined in.
    static func expression(_ text: String) -> String { text.components(separatedBy: "\n").map(literal).joined(separator: " & return & ") }

    /// The script's output, or nil when it failed or the user cancelled. Dialogs wait as long as the user takes.
    func run(_ source: String) -> String? {
        Shell.run(["/usr/bin/osascript", "-e", source], timeout: 24 * 3600)?.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    public func confirm(_ message: String, detail: String, accept: String) -> Bool {
        let body = detail.isEmpty ? message : message + "\n\n" + detail
        let script = "display dialog " + Self.expression(body) + " with title " + Self.literal(Runtime.product) + " buttons {\"取消\", "
            + Self.literal(accept) + "} default button \"取消\" with icon caution"
        return run(script)?.contains(accept) ?? false
    }

    public func textDialog(_ message: String, value: String, hidden: Bool, title: String) -> String? {
        run("text returned of (display dialog " + Self.expression(message) + " default answer " + Self.literal(value) + (hidden ? " with hidden answer" : "")
            + " buttons {\"取消\", \"保存\"} default button \"保存\" cancel button \"取消\" with title " + Self.literal(title) + ")")
    }

    public func choose(title: String, prompt: String, _ first: (label: String, value: String), _ second: (label: String, value: String)) -> String? {
        let answer = run("choose from list {" + Self.literal(first.label) + ", " + Self.literal(second.label) + "} with title " + Self.literal(title)
                         + " with prompt " + Self.literal(prompt) + " default items {" + Self.literal(first.label) + "}")
        return answer == first.label ? first.value : answer == second.label ? second.value : nil
    }

    public func choosePath(_ message: String, folder: Bool) -> String? {
        run("POSIX path of (choose " + (folder ? "folder" : "file") + " with prompt " + Self.literal(message) + ")")
    }

    public func savePath(_ message: String, name: String) -> String? {
        run("POSIX path of (choose file name with prompt " + Self.literal(message) + " default name " + Self.literal(name) + " default location (path to desktop folder))")
    }

    public func error(_ title: String, _ detail: String) {
        _ = run("display alert " + Self.literal(title) + " message " + Self.literal(detail))
    }

    public func notify(_ message: String, subtitle: String) {
        var script = "display notification " + Self.literal(message) + " with title " + Self.literal(Runtime.product)
        if !subtitle.isEmpty { script += " subtitle " + Self.literal(subtitle) }
        _ = Shell.run(["/usr/bin/osascript", "-e", script], timeout: 5)
    }

    public func openLocal(_ path: String, text: Bool) throws {
        guard Shell.run(["/usr/bin/open"] + (text ? ["-t"] : []) + [path], timeout: 5) != nil else { throw HostFailure("open failed") }
    }

    public func copy(_ text: String) {
        _ = Shell.run(["/usr/bin/pbcopy"], timeout: 5, input: Data(text.utf8))
    }
}

/// Explicit connection overrides in config.local.sh: no copied credentials or inferred monitor addresses.
/// Port of swiftbar_config.py.
public enum Connections {
    public static let keys: Set<String> = {
        var keys: Set<String> = ["OCX_HOME", "OCX_BASE_URL", "OCX_TOKEN_FILE", "OCX_CHATGPT_HOME", "KOMARI_BASE_URL", "DEEPSEEK_API_KEY", "DEEPSEEK_API_KEY_FILE",
                                 "XAI_MANAGEMENT_API_KEY", "XAI_MANAGEMENT_API_KEY_FILE", "XAI_TEAM_ID",
                                 "MINIMAX_API_KEY", "MINIMAX_API_KEY_FILE", "MINIMAX_REGION", "MOONSHOT_REGION", "QODER_CN_ORG_ID"]
        for key in Catalog.apiKeys.keys { for name in Catalog.credentialNames(key) { keys.insert(name); keys.insert(name + "_FILE") } }
        return keys
    }()

    static let header = """
    # 本机高级设置：仅本用户可读，不随插件上传或覆盖。
    # 常用设置请在「主页模块」悬停对应来源；本机客户端默认自动读取。
    # 注释是说明，不代表已配置。无需填写即可使用已发现的本机客户端。
    # API 连接只保存密钥文件路径；在插件中输入的 Key 存于同目录 keys/，仅本用户可读。

    """

    /// shlex.quote: plain words stay, anything else goes in single quotes.
    public static func shellQuote(_ value: String) -> String {
        if value.isEmpty { return "''" }
        let safe = value.unicodeScalars.allSatisfy { $0.isASCII && (CharacterSet.alphanumerics.contains($0) || "@%+=:,./-_".unicodeScalars.contains($0)) }
        return safe ? value : "'" + value.replacingOccurrences(of: "'", with: "'\"'\"'") + "'"
    }

    /// Overrides only the keys given, keeping the user's other settings. The last export wins, empty ones included.
    public static func writeOverrides(_ directory: String, _ values: [(String, String)]) throws {
        guard !values.isEmpty, values.allSatisfy({ keys.contains($0.0) && !$0.1.contains("\n") && !$0.1.contains("\0") }) else {
            throw Preferences.Invalid("invalid connection settings")
        }
        try Files.makeDirectory(directory)
        let path = directory + "/config.local.sh"
        var text = FileManager.default.fileExists(atPath: path) ? (try String(contentsOfFile: path, encoding: .utf8)) : header
        for (key, value) in values {
            let marker = " # managed by SwiftBarToken"
            text = text.components(separatedBy: "\n").enumerated().reduce(into: [String]()) { lines, pair in
                if !(pair.element.hasPrefix("export " + key + "=") && pair.element.hasSuffix(marker)) { lines.append(pair.element) }
            }.joined(separator: "\n")
            while let last = text.last, last.isWhitespace { text.removeLast() }
            text += "\nexport " + key + "=" + shellQuote(value) + marker + "\n"
        }
        try Files.replaceAtomically(path, with: Data(text.utf8), prefix: ".config-")
    }

    /// Only keys typed into the plugin are ours to delete; files the user chose are never removed.
    public static func managedKey(_ directory: String, _ file: String) -> Bool {
        let path = Paths.expandUser(file)
        let parent = ((Paths.resolve(path) ?? Runtime.absolute(path)) as NSString).deletingLastPathComponent
        return parent == (Paths.resolve(directory + "/keys") ?? Runtime.absolute(directory + "/keys"))
    }

    public static func ensureFile(_ directory: String) throws -> String {
        try Files.makeDirectory(directory)
        let path = directory + "/config.local.sh"
        if case .failed(let reason) = Files.createExclusively(path, contents: Data(header.utf8)) { throw HostFailure(reason) }
        return path
    }
}

/// Usage export: the range on screen as CSV (a row per day and tool) or JSON (totals, models and days per tool).
/// Only counters leave: no paths, keys, prompts or session IDs. Port of swiftbar_export.py.
public enum UsageExport {
    static let columns = ["date", "tool", "requests", "input_tokens", "output_tokens", "cache_read_tokens", "cache_write_tokens",
                          "total_tokens", "cost_usd", "unpriced_requests", "credits", "summary_tokens", "summary_cost_usd"]

    static func number(_ value: JSON?) -> Double {
        guard let value, let number = value.pythonFloat ?? value.string.flatMap({ parsePythonFloat($0.trimmingCharacters(in: .whitespacesAndNewlines)) }) else { return 0 }
        return number >= 0 ? number : 0
    }

    static func counters(_ row: JSONObject) -> JSONObject {
        let writes = row["cacheCreationInputTokens"]
        let reads = Py.or(row["cacheReadInputTokens"], row["cachedInputTokens"] ?? .null)
        return ["requests": .int(Int64(number(row["requests"]))), "input_tokens": .int(Int64(number(row["inputTokens"]))),
                "output_tokens": .int(Int64(number(row["outputTokens"]))), "cache_read_tokens": .int(Int64(number(reads))),
                "cache_write_tokens": writes == nil || writes == .null ? .null : .int(Int64(number(writes))),
                "total_tokens": .int(Int64(number(row["totalTokens"]))), "cost_usd": .double(Runtime.round6(number(row["estimatedCostUsd"]))),
                "unpriced_requests": .int(Int64(number(row["unpricedRequests"]))), "credits": .double(Double(String(format: "%.4f", number(row["credits"]))) ?? 0)]
    }

    static func tools(_ selected: [(String, String, JSONObject)], _ counting: [String: JSONObject]) -> [(String, String, JSONObject, JSONObject)] {
        selected.compactMap { key, name, usage in
            guard usage["summary"]?.isTruthy == true, usage["error"]?.isTruthy != true else { return nil }
            return (key, name, usage, counting[key]?["usage"]?.object ?? JSONObject())
        }
    }

    static func csvField(_ value: JSON) -> String {
        let text: String
        switch value {
        case .null: text = ""
        case .double(let number): text = Py.repr(.double(number))
        default: text = Py.str(value)
        }
        let special = text.contains(",") || text.contains("\"") || text.contains("\n") || text.contains("\r")
        return special ? "\"" + text.replacingOccurrences(of: "\"", with: "\"\"") + "\"" : text
    }

    public static func csvText(_ selected: [(String, String, JSONObject)], _ counting: [String: JSONObject]) -> String {
        var lines = [columns.joined(separator: ",")]
        for (_, name, usage, counted) in tools(selected, counting) {
            var countedDays: [String: JSONObject] = [:]
            for day in counted["days"]?.array ?? [] { if let fields = day.object { countedDays[String(Py.str(fields["date"] ?? .string("")).prefix(10))] = fields } }
            // sorted(key=str(date)): by code points, equal dates keep their order.
            let ordered = (usage["days"]?.array ?? []).compactMap(\.object).enumerated().sorted { a, b in
                let x = Py.str(a.element["date"] ?? .string("")), y = Py.str(b.element["date"] ?? .string(""))
                return x != y ? x.unicodeScalars.lexicographicallyPrecedes(y.unicodeScalars) : a.offset < b.offset
            }.map(\.element)
            for day in ordered {
                let date = String(Py.str(day["date"] ?? .string("")).prefix(10)), part = countedDays[date] ?? JSONObject()
                var row = counters(day)
                row["date"] = .string(date); row["tool"] = .string(name)
                row["summary_tokens"] = .int(Int64(number(part["totalTokens"]))); row["summary_cost_usd"] = .double(Runtime.round6(number(part["estimatedCostUsd"])))
                lines.append(columns.map { csvField(row[$0] ?? .null) }.joined(separator: ","))
            }
        }
        return lines.joined(separator: "\n") + "\n"
    }

    public static func jsonText(_ selected: [(String, String, JSONObject)], _ counting: [String: JSONObject], key: String, label: String,
                                start: JSON?, end: JSON?, version: String, now: Date = Date()) -> String {
        func moment(_ stamp: JSON?) -> JSON {
            guard let stamp, stamp.isTruthy, let seconds = stamp.number else { return .null }
            return .string(ISOTime.utcString(seconds))
        }
        var body: JSONObject = ["product": "Starmeter", "version": .string(version), "range": .string(key), "rangeLabel": .string(label),
                                "start": moment(start), "end": moment(end), "exportedAt": .string(ISOTime.utcString(now.timeIntervalSince1970)), "tools": []]
        var list: [JSON] = []
        for (tool, name, usage, counted) in tools(selected, counting) {
            let models: [JSON] = (usage["models"]?.array ?? []).compactMap(\.object).map { row in
                var fields = counters(row); fields["model"] = .string(Py.str(Py.or(row["model"], .string("")))); return .object(fields)
            }
            let days: [JSON] = (usage["days"]?.array ?? []).compactMap(\.object).map { row in
                var fields = counters(row); fields["date"] = .string(String(Py.str(row["date"] ?? .string("")).prefix(10))); return .object(fields)
            }
            list.append(["id": .string(tool), "name": .string(name), "totals": .object(counters(usage["summary"]?.object ?? JSONObject())),
                         "summary": .object(counters(counted["summary"]?.object ?? JSONObject())), "models": .array(models), "days": .array(days)])
        }
        body["tools"] = .array(list)
        return JSON.object(body).serialized(ensureASCII: false, indent: 2) + "\n"
    }
}

/// Terminal tabs for CLI updates, with their output kept in a local log. Port of swiftbar_terminal.py.
public enum TerminalJobs {
    static let cliNames = ["claude-code": "Claude Code", "codex-cli": "Codex"]

    static func literal(_ value: String) -> String {
        "\"" + value.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(of: "\"", with: "\\\"").replacingOccurrences(of: "\n", with: "\" & linefeed & \"") + "\""
    }

    /// The new window's shell prints its own startup lines and echoes the command, so the screen is cleared first;
    /// afterwards the tab waits for Return instead of starting another login shell.
    static func bootstrap(_ command: String) -> String {
        "export PATH=\"/opt/homebrew/bin:/usr/local/bin:$PATH\"\ncd \"$HOME\" || exit 1\n"
            + "if [ -t 1 ]; then printf '\\033[H\\033[2J\\033[3J'; fi\n"
            + command + "\ntoken_terminal_status=$?\n"
            + "if [ -t 0 ]; then printf '\\n按回车键关闭…'; read -r _; fi\nexit \"$token_terminal_status\"\n"
    }

    static func join(_ words: [String]) -> String { words.map(Connections.shellQuote).joined(separator: " ") }

    static func modified(_ path: String) -> Double {
        ((try? FileManager.default.attributesOfItem(atPath: path)[.modificationDate]) as? Date)?.timeIntervalSince1970 ?? 0
    }

    static func isLink(_ path: String) -> Bool { (try? FileManager.default.destinationOfSymbolicLink(atPath: path)) != nil }

    /// Opens the command in a new terminal tab; where Apple Events are refused, as a unique .command file.
    @discardableResult
    public static func launch(_ command: String, preferred: String, state: String) throws -> String {
        let app = ["iTerm", "iTerm2"].contains(preferred) ? "iTerm" : "Terminal"
        let inline = join(["/bin/bash", "-c", bootstrap(command)])
        let script: String
        if app == "iTerm" {
            script = "tell application \"iTerm\"\nactivate\nif (count of windows) = 0 then\n"
                + "set tokenWindow to (create window with default profile command " + literal(inline) + ")\n"
                + "set tokenTab to current tab of tokenWindow\nelse\n"
                + "tell current window to set tokenTab to (create tab with default profile command " + literal(inline) + ")\n"
                + "end if\ntry\nselect tokenTab\nend try\nend tell"
        } else {
            script = "tell application \"Terminal\"\nactivate\ndo script " + literal(inline) + "\nend tell"
        }
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/osascript")
        process.arguments = ["-e", script]
        process.standardOutput = FileHandle.nullDevice; process.standardError = FileHandle.nullDevice
        try process.run()
        let deadline = Date().addingTimeInterval(15)
        while process.isRunning && Date() < deadline { Platform.sleep(0.05) }
        if process.isRunning {
            // Delivery is uncertain: never start the updater twice after a timeout.
            process.terminate()
            throw HostFailure("终端响应超时，请先检查是否已经打开更新标签")
        }
        if process.terminationStatus == 0 { return "direct" }
        let root = state + "/terminal-jobs"
        try Files.makeDirectory(root)
        let old = ((try? FileManager.default.contentsOfDirectory(atPath: root)) ?? []).filter { $0.hasSuffix(".command") }.map { root + "/" + $0 }
            .sorted { modified($0) > modified($1) }
        for path in old.dropFirst(19) where !isLink(path) { try? FileManager.default.removeItem(atPath: path) }
        let name = root + "/starmeter-" + Files.randomName() + ".command"
        guard case .created = Files.createExclusively(name, contents: Data(("#!/bin/bash\n" + bootstrap(command)).utf8)) else { throw HostFailure("无法写入终端命令") }
        Platform.setMode(name, 0o700)
        var opened = Shell.run(["/usr/bin/open", "-a", app, name], timeout: 10) != nil
        if !opened && app != "Terminal" { opened = Shell.run(["/usr/bin/open", "-a", "Terminal", name], timeout: 10) != nil }
        if !opened { throw HostFailure("无法打开终端，请检查 SwiftBar 的终端设置") }
        return "file"
    }

    public static func latestLog(_ state: String, _ keys: [String]) -> String? {
        let root = state + "/logs/updates"
        let names = (try? FileManager.default.contentsOfDirectory(atPath: root)) ?? []
        let files = keys.filter { cliNames[$0] != nil }.flatMap { key in
            names.filter { $0.hasPrefix(key + "-") && $0.hasSuffix(".log") }.map { root + "/" + $0 }
        }.filter { !isLink($0) && !Harness.isDirectory($0) && FileManager.default.fileExists(atPath: $0) }
        return files.max { modified($0) < modified($1) }
    }

    /// The update and its log: versions before and after, the exit status kept from the updater, never from tee.
    public static func updateCommand(binary: String, key: String, state: String, refreshURL: String?, zone: TimeZone) throws -> String {
        guard let label = cliNames[key] else { throw Preferences.Invalid("invalid CLI") }
        let root = state + "/logs/updates"
        try Files.makeDirectory(root)
        let old = ((try? FileManager.default.contentsOfDirectory(atPath: root)) ?? []).filter { $0.hasPrefix(key + "-") && $0.hasSuffix(".log") }
            .map { root + "/" + $0 }.sorted { modified($0) > modified($1) }
        for path in old.dropFirst(19) where !isLink(path) { try? FileManager.default.removeItem(atPath: path) }
        let stamp = Runtime.format(Date().timeIntervalSince1970, zone, "%Y%m%d-%H%M%S")
        var path = ""
        for _ in 0..<100 {
            let candidate = root + "/" + key + "-" + stamp + "-" + Files.randomName() + ".log"
            if case .created = Files.createExclusively(candidate, contents: Data()) { path = candidate; break }
        }
        guard !path.isEmpty else { throw HostFailure("无法创建更新日志") }
        let version = join([binary, "--version"]), update = join([binary, "update"])
        return [
            "token_update_log=" + Connections.shellQuote(path),
            "token_before=$(" + version + " 2>/dev/null) || token_before=\"版本读取失败\"",
            "printf '\\n%s\\n%s\\n更新前：%s\\n\\n' " + Connections.shellQuote(label + " 更新") + " " + Connections.shellQuote("$ " + update) + " \"$token_before\" | /usr/bin/tee -a \"$token_update_log\"",
            update + " 2>&1 | /usr/bin/tee -a \"$token_update_log\"",
            "token_update_status=${PIPESTATUS[0]}",
            "token_after=$(" + version + " 2>/dev/null); token_version_status=$?",
            "if [ \"$token_update_status\" -ne 0 ]; then",
            "  printf '\\n更新失败（退出码 %s）。请查看上方日志。\\n当前版本：%s\\n' \"$token_update_status\" \"${token_after:-无法读取}\"",
            "elif [ \"$token_version_status\" -ne 0 ] || [ -z \"$token_after\" ]; then",
            "  printf '\\n更新命令已完成，但无法确认当前版本，请查看上方日志。\\n'",
            "elif [ \"$token_before\" = \"$token_after\" ]; then",
            "  printf '\\n检查完成，版本未变化：%s\\n是否已是最新版本，请以上方更新器日志为准。\\n' \"$token_after\"",
            "else",
            "  printf '\\n更新成功：%s → %s\\n' \"$token_before\" \"$token_after\"",
            "fi | /usr/bin/tee -a \"$token_update_log\"",
            "printf '\\n日志已保存：%s\\n\\n' \"$token_update_log\" | /usr/bin/tee -a \"$token_update_log\"",
            refreshURL.map { join(["/usr/bin/open", "-g", $0]) } ?? ":",
            "(exit \"$token_update_status\")",
        ].joined(separator: "\n")
    }
}

/// The Windows tray (or any host with its own windows) running the engine as a helper: each question goes out as
/// one JSON line on standard output and the host writes back one line, {"value": …}, on standard input.
public final class StreamHost: DesktopHost {
    let state: String
    private let lock = NSLock()

    public init(state: String) { self.state = state }

    static func emit(_ message: JSONObject) {
        FileHandle.standardOutput.write(Data((JSON.object(message).serialized(ensureASCII: true) + "\n").utf8))
    }

    /// Sends a request and waits for its answer; questions from parallel reads are asked one at a time.
    @discardableResult
    func ask(_ request: JSONObject) -> JSON {
        lock.lock(); defer { lock.unlock() }
        Self.emit(request)
        guard let line = readLine(strippingNewline: true), let answer = (try? JSON.parse(line))?.object else { return .null }
        return answer["value"] ?? .null
    }

    public func confirm(_ message: String, detail: String, accept: String) -> Bool {
        ask(["host": "confirm", "message": .string(message), "detail": .string(detail), "accept": .string(accept)]) == .bool(true)
    }
    public func textDialog(_ message: String, value: String, hidden: Bool, title: String) -> String? {
        ask(["host": "text", "message": .string(message), "value": .string(value), "hidden": .bool(hidden), "title": .string(title)]).string
    }
    public func choose(title: String, prompt: String, _ first: (label: String, value: String), _ second: (label: String, value: String)) -> String? {
        ask(["host": "choose", "title": .string(title), "prompt": .string(prompt),
             "options": [[.string(first.label), .string(first.value)], [.string(second.label), .string(second.value)]]]).string
    }
    public func choosePath(_ message: String, folder: Bool) -> String? { ask(["host": "choosePath", "message": .string(message), "folder": .bool(folder)]).string }
    public func savePath(_ message: String, name: String) -> String? { ask(["host": "savePath", "message": .string(message), "name": .string(name)]).string }
    public func error(_ title: String, _ detail: String) { ask(["host": "error", "title": .string(title), "detail": .string(detail)]) }
    public func notify(_ message: String, subtitle: String) { ask(["host": "notify", "message": .string(message), "subtitle": .string(subtitle)]) }
    public func openLocal(_ path: String, text: Bool) throws {
        guard ask(["host": "openLocal", "path": .string(path), "text": .bool(text)]) == .bool(true) else { throw HostFailure("open failed") }
    }
    public func copy(_ text: String) { ask(["host": "copy", "text": .string(text)]) }
    public func saveOverrides(_ values: [(String, String)]) throws {
        let fields = JSONObject(values.map { ($0.0, JSON.string($0.1)) })
        guard ask(["host": "saveOverrides", "values": .object(fields)]) == .bool(true) else { throw HostFailure("settings could not be saved") }
    }
    /// The tray's own settings file, JSON rather than a shell script.
    public func configFile() -> String {
        let path = state + "/config.local.json"
        if !FileManager.default.fileExists(atPath: path) { try? Files.replaceAtomically(path, with: Data("{}".utf8), prefix: ".config-") }
        return path
    }
    public func openClaude() { ask(["host": "openClaude"]) }
    public func updateCLI(binary: String, key: String) { ask(["host": "updateCLI", "binary": .string(binary), "key": .string(key)]) }
    public func submitTest(_ provider: String, _ id: String) { ask(["host": "submitTest", "provider": .string(provider), "id": .string(id)]) }
    public func requestRefresh(_ mode: String?) { ask(["host": "refresh", "mode": mode.map(JSON.string) ?? .null]) }
    public func saveRange(_ key: String) {
        try? Files.makeDirectory(state)
        try? Files.replaceAtomically(state + "/view-range.json", with: Data(JSON.object(["range": .string(key)]).serialized().utf8), prefix: ".range-")
    }
    public func localProxy(_ base: String) -> Bool { ["127.0.0.1", "localhost", "::1"].contains(URLComponents(string: base)?.host ?? "") }
}
