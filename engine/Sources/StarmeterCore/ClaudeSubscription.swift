import Foundation

/// Claude subscription usage: the usage response's windows and extra usage, Claude Code's OAuth login (read only,
/// tokens kept in memory), its statusline snapshot and the desktop app's own snapshot. Ports of
/// swiftbar_claude_live.parse_usage, swiftbar_claude_oauth, swiftbar_claude_local and swiftbar_claude.subscription.
/// The OAuth protocol and lifecycle follow steipete/CodexBar 458afc60 (MIT), see lib/CODEXBAR_LICENSE.
public enum ClaudeSubscription {
    public struct UsageError: Error, CustomStringConvertible { public let description: String }

    static let fiveHours = 5.0 * 3600, week = 7.0 * 86400

    /// The windows of a usage response, newest plan format first, and any extra usage it carries.
    public static func parseUsage(_ data: JSON, now: Double = Date().timeIntervalSince1970) throws -> JSONObject {
        guard let fields = data.object else { throw UsageError(description: "用量格式尚未支持") }
        var windows: [JSON] = [], seen = Set<String>()
        func add(_ label: JSON?, _ percent: JSON?, _ reset: JSON?, _ span: Double?) {
            guard let percent, percent.bool == nil, let value = percent.number, value.isFinite, value >= 0, value <= 100 else { return }
            if let reset, !reset.isNull, reset.string == nil { return }
            guard let text = label?.string, !text.isEmpty else { return }
            let key = text + "\u{1F}" + (reset?.string ?? "\u{0}")
            guard seen.insert(key).inserted else { return }
            var window: JSONObject = ["label": .string(String(text.prefix(80))), "used": percent, "resetsAt": reset ?? .null]
            if let span { window["span"] = .int(Int64(span)) }  // Python's 5*3600 and 7*86400 are integers.
            windows.append(.object(window))
        }
        // The plan API lists model and surface limits instead of fixed five_hour keys.
        if let limits = fields["limits"]?.array {
            for row in limits {
                guard let limit = row.object else { continue }
                let scope = limit["scope"]?.object ?? JSONObject()
                let model = scope["model"]?.object ?? JSONObject(), surface = scope["surface"]?.object ?? JSONObject()
                let label = Py.or(model["display_name"], surface["display_name"] ?? .null)
                // The window's length, for the sidebar's pace; a kind not recognised has none.
                let kind = (Py.str(limit["group"] ?? .string("")) + " " + Py.str(limit["kind"] ?? .string(""))).lowercased()
                let span: Double? = ["week", "seven_day"].contains(where: kind.contains) ? week : ["five_hour", "session"].contains(where: kind.contains) ? fiveHours : nil
                add(label.isTruthy ? label : .string(span == week ? "本周" : "当前周期"), limit["percent"], limit["resets_at"], span)
            }
        }
        if windows.isEmpty {
            for (key, label) in [("five_hour", "5 小时"), ("seven_day", "本周"), ("seven_day_opus", "Opus 本周"), ("seven_day_sonnet", "Sonnet 本周")] {
                if let row = fields[key]?.object { add(.string(label), row["utilization"], row["resets_at"], key == "five_hour" ? fiveHours : week) }
            }
        }
        if windows.isEmpty { throw UsageError(description: "尚未返回可识别的订阅用量") }
        var result: JSONObject = ["windows": .array(windows), "updatedAt": .double(now), "stale": false, "state": "在线快照", "source": "live"]
        if let extra = extraUsage(fields) { result["extraUsage"] = .object(extra) }
        return result
    }

    static func isCurrency(_ value: JSON?) -> Bool {
        guard let text = value?.string else { return false }
        return text.utf8.count == 3 && text.utf8.allSatisfy { (0x41...0x5A).contains($0) }
    }

    /// {amount_minor, exponent, currency} → amount and currency; anything else is unknown.
    static func money(_ value: JSON?) -> (Double, String?)? {
        guard let fields = value?.object, case .int(let amount)? = fields["amount_minor"], case .int(let exponent)? = fields["exponent"],
              (0...6).contains(exponent) else { return nil }
        var scale = 1.0
        for _ in 0..<exponent { scale *= 10 }
        return (Double(amount) / scale, isCurrency(fields["currency"]) ? fields["currency"]!.string : nil)
    }

    /// Extra usage from the same response: monthly spend, cap and, when reported, the credit balance.
    static func extraUsage(_ data: JSONObject) -> JSONObject? {
        if let spend = data["spend"]?.object, let enabled = spend["enabled"]?.bool {
            if !enabled { return ["enabled": false] }
            let used = money(spend["used"]), limit = money(spend["limit"]), balance = money(spend["balance"])
            if let used, used.0 >= 0 {
                var result: JSONObject = ["enabled": true, "used": .double(used.0), "limit": limit.flatMap { $0.0 > 0 ? JSON.double($0.0) : nil } ?? .null,
                                          "currency": .string(used.1 ?? "USD")]
                if let balance { result["balance"] = .double(balance.0) }
                return result
            }
        }
        // Older responses only carry extra_usage, in minor units (cents unless decimal_places says otherwise).
        guard let value = data["extra_usage"]?.object, let enabled = value["is_enabled"]?.bool else { return nil }
        if !enabled { return ["enabled": false] }
        var scale = 100.0
        if case .int(let places)? = value["decimal_places"], (0...6).contains(places) { scale = 1; for _ in 0..<places { scale *= 10 } }
        func major(_ raw: JSON?) -> JSON? {
            guard let raw, raw.bool == nil, let number = raw.number, number.isFinite, number >= 0 else { return nil }
            return .double(number / scale)
        }
        guard let used = major(value["used_credits"]) else { return nil }
        let limit = major(value["monthly_limit"])
        // A zero monthly limit means no cap rather than nothing left.
        return ["enabled": true, "used": used, "limit": limit.flatMap { $0.isTruthy ? $0 : nil } ?? .null,
                "currency": .string(isCurrency(value["currency"]) ? value["currency"]!.string! : "USD")]
    }

    // MARK: Small state files

    static func readState(_ path: String) -> JSONObject {
        guard let handle = FileHandle(forReadingAtPath: path) else { return JSONObject() }
        defer { try? handle.close() }
        guard let data = try? handle.read(upToCount: 2 * 1024 * 1024 + 1), data.count <= 2 * 1024 * 1024,
              let object = (try? JSON.parse(data))?.object else { return JSONObject() }
        return object
    }

    static func writeState(_ path: String, _ value: JSONObject) throws {
        try Files.makeDirectory((path as NSString).deletingLastPathComponent)
        try Files.replaceAtomically(path, with: Data(JSON.object(value).serialized().utf8), prefix: "." + (path as NSString).lastPathComponent)
    }

    // MARK: Claude Code's statusline snapshot

    static let snapshotFile = "claude-code-usage.json", backupFile = "integrations/claude-statusline-original.json"

    /// Whether the statusline bridge is what Claude Code runs now.
    public static func localEnabled(_ state: String) -> Bool {
        let meta = readState(state + "/" + backupFile)
        guard let settings = meta["settings"], settings.isTruthy, let command = meta["managedCommand"], command.isTruthy else { return false }
        let status = readState(Py.str(settings))["statusLine"]?.object
        return status?["command"] == command
    }

    /// The bridge's last snapshot, or nil when there is none worth reading.
    public static func localSnapshot(_ state: String, now: Double = Date().timeIntervalSince1970) -> JSONObject? {
        let data = readState(state + "/" + snapshotFile)
        guard data["schema"] == .int(1), data["source"] == .string("claude-code") else { return nil }
        guard let stampValue = data["updatedAt"], stampValue.bool == nil, let stamp = stampValue.number, stamp.isFinite, stamp <= now + 60,
              let rows = data["windows"]?.array else { return nil }
        var windows: [JSON] = [], expired = false
        for row in rows {
            guard let fields = row.object, let label = fields["label"], let value = fields["used"], let resetText = fields["resetsAt"]?.string,
                  let reset = try? ISOTime.parse(resetText) else { continue }
            guard [JSON.string("5 小时"), .string("本周")].contains(label), value.bool == nil, let used = value.number, used.isFinite, used >= 0, used <= 100 else { continue }
            expired = expired || reset.timestamp() <= now
            windows.append(["label": label, "used": value, "resetsAt": .string(resetText)])
        }
        let stale = expired || now - stamp > 600 || windows.isEmpty
        return ["schema": 1, "source": "claude-code", "windows": .array(windows), "updatedAt": stampValue, "stale": .bool(stale),
                "state": .string(!windows.isEmpty && stale ? "本机用量待更新" : !windows.isEmpty ? "本机用量" : "等待客户端返回订阅用量")]
    }

    // MARK: The desktop app's snapshot

    static let desktopWindows: [(String, String)] = [("fh", "5 小时"), ("sd", "本周"), ("so", "Opus 本周"), ("sn", "Sonnet 本周"), ("cw", "Cowork 本周"), ("oa", "其他应用本周")]

    public static func desktop(root: String, now: Double = Date().timeIntervalSince1970) -> JSONObject {
        var result: JSONObject = ["windows": [], "state": "暂无本机用量快照", "stale": true]
        let data = ClaudeApp.readObject(root + "/plan-usage-history.json")
        guard let version = data["version"], [JSON.int(1), .int(2)].contains(version), let samples = data["samples"]?.array else { return result }
        var rows = samples.compactMap(\.object).filter { row in
            guard let t = row["t"], t.bool == nil, let value = t.number, value.isFinite else { return false }
            return 0 < value / 1000 && value / 1000 <= now + 60
        }
        var orgs = Set(rows.compactMap { $0["org"]?.string })
        // A registry may prove an account boundary, but not which of several orgs is active.
        if let account = ClaudeApp.readObject(root + "/config.json")["lastKnownAccountUuid"]?.string, ClaudeApp.isUUID(account) {
            let base = root + "/claude-code-sessions/" + account
            let owned = Set(((try? FileManager.default.contentsOfDirectory(atPath: base)) ?? []).filter { Harness.isDirectory(base + "/" + $0) && ClaudeApp.isUUID($0) })
            if !owned.isEmpty {
                rows = rows.filter { $0["org"]?.string.map(owned.contains) ?? false }
                orgs = Set(rows.map { $0["org"].map(Py.dumps) ?? "null" })
            }
        }
        if orgs.count > 1 { result["state"] = "多个组织，请在 Claude 查看"; return result }
        guard let row = rows.max(by: { ($0["t"]!.number ?? 0) < ($1["t"]!.number ?? 0) }) else { return result }
        let usage: JSONObject?
        if version == .int(2) { usage = row["u"]?.object }
        else { var picked = JSONObject(); for (key, _) in desktopWindows { picked[key] = row[key] ?? .null }; usage = picked }
        guard let usage else { return result }
        let windows: [JSON] = desktopWindows.compactMap { key, label in
            guard let value = usage[key], value.bool == nil, let used = value.number, used.isFinite, used >= 0, used <= 100 else { return nil }
            return ["label": .string(label), "used": value]
        }
        if windows.isEmpty { return result }
        let stamp = (row["t"]!.number ?? 0) / 1000, stale = now - stamp > 1800
        result["windows"] = .array(windows); result["updatedAt"] = .double(stamp); result["stale"] = .bool(stale)
        result["state"] = .string(stale ? "快照待更新" : "已读取本机快照")
        return result
    }

    // MARK: Claude Code's OAuth login

    public struct OAuthError: Error, CustomStringConvertible {
        public let code: String
        public let description: String
        public var retry: Double = 300
    }

    static let usageURL = "https://api.anthropic.com/api/oauth/usage", interval = 300.0, maxResponse = 1024 * 1024

    /// Claude Code files its login under the login name, or claude-code-user when the name has other characters.
    static func keychainAccount() -> String {
        let name = ProcessInfo.processInfo.environment["USER"].flatMap { $0.isEmpty ? nil : $0 } ?? NSUserName()
        return !name.isEmpty && name.utf8.allSatisfy({ (0x41...0x5A).contains($0) || (0x61...0x7A).contains($0) || (0x30...0x39).contains($0) || [0x2E, 0x5F, 0x2D].contains($0) })
            ? name : "claude-code-user"
    }

    /// One keychain item, through the security tool so the credential never lands in an argument.
    static func keychainData(_ account: String?) throws -> Data {
        #if os(macOS)
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/security")
        process.arguments = ["find-generic-password"] + (account.map { ["-a", $0] } ?? []) + ["-s", "Claude Code-credentials", "-w"]
        let output = Pipe()
        process.standardOutput = output
        process.standardError = FileHandle.nullDevice
        do { try process.run() } catch { throw OAuthError(code: "keychain", description: "钥匙串读取未完成，请手动重试授权") }
        let data = output.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        if process.terminationStatus == 44 { throw OAuthError(code: "missing", description: "请先在 Claude Code 登录订阅账户") }
        if process.terminationStatus != 0 { throw OAuthError(code: "keychain", description: "钥匙串未授权，请点击授权读取登录") }
        // security prints the value as bare hex digits when any byte is not printable ASCII; JSON is never all hex.
        let trimmed = data.trimmingWhitespace()
        if !trimmed.isEmpty, trimmed.count % 2 == 0, trimmed.allSatisfy({ (0x30...0x39).contains($0) || (0x41...0x46).contains($0) || (0x61...0x66).contains($0) }) {
            var bytes = [UInt8]()
            var index = trimmed.startIndex
            while index < trimmed.endIndex {
                bytes.append(UInt8(String(decoding: trimmed[index..<trimmed.index(index, offsetBy: 2)], as: UTF8.self), radix: 16)!)
                index = trimmed.index(index, offsetBy: 2)
            }
            return Data(bytes)
        }
        return data
        #else
        throw OAuthError(code: "missing", description: "请先在 Claude Code 登录订阅账户")
        #endif
    }

    static func loginDocument(_ raw: Data) -> JSONObject? {
        guard raw.count <= maxResponse else { return nil }
        return (try? JSON.parse(raw))?.object
    }

    /// The access token and its expiry, read only; the refresh token is never kept.
    static func credentials(environment: [String: String] = ProcessInfo.processInfo.environment) throws -> (token: String, expires: Double, owner: String) {
        let configured = environment["CLAUDE_CONFIG_DIR"] ?? ""
        if configured.contains(",") { throw OAuthError(code: "profile", description: "多个 Claude 目录，请指定单一登录来源") }
        let raw = configured.trimmingCharacters(in: .whitespacesAndNewlines)
        let fallback = Paths.home + "/.claude"
        let root = raw.isEmpty ? fallback : (Paths.resolve(Paths.expandUser(raw)) ?? Paths.expandUser(raw))
        var data: JSONObject?, unreadable: String?, legacy = false
        #if os(macOS)
        if root == fallback {
            do {
                data = loginDocument(try keychainData(keychainAccount()))
                if data == nil { unreadable = "Claude Code 钥匙串里的登录无法解析" }
            } catch let error as OAuthError where error.code == "missing" { legacy = true }
        }
        #endif
        if data == nil {
            let path = root + "/.credentials.json"
            if FileManager.default.fileExists(atPath: path) {
                guard let handle = FileHandle(forReadingAtPath: path) else { throw OAuthError(code: "credential", description: "Claude Code 登录文件无法读取") }
                defer { try? handle.close() }
                let content = (try? handle.read(upToCount: maxResponse + 1)) ?? Data()
                data = loginDocument(content)
                if data == nil { throw OAuthError(code: "credential", description: "Claude Code 登录文件无法解析") }
            }
        }
        if data == nil && legacy {
            // Older installs may have used another account name; only without a local file.
            do {
                data = loginDocument(try keychainData(nil))
                if data == nil { unreadable = "Claude Code 钥匙串里的登录无法解析" }
            } catch let error as OAuthError where error.code == "missing" {}
        }
        guard let data else {
            if let unreadable { throw OAuthError(code: "credential", description: unreadable) }
            throw OAuthError(code: "missing", description: "请先在 Claude Code 登录订阅账户")
        }
        // Signed in with an API key, or only MCP logins stored: no subscription login to read.
        guard let oauthValue = data["claudeAiOauth"], !oauthValue.isNull else { throw OAuthError(code: "missing", description: "请先在 Claude Code 登录订阅账户") }
        guard let oauth = oauthValue.object else { throw OAuthError(code: "credential", description: "Claude Code 登录内容无法识别") }
        let token = oauth["accessToken"], expiry = oauth["expiresAt"]
        // Claude Code empties the stored token once the server rejects its refresh token.
        if token == .string("") { throw OAuthError(code: "missing", description: "Claude Code 登录已失效，请在 Claude Code 运行 /login") }
        guard let text = token?.string, (16...4096).contains(text.utf8.count),
              text.utf8.allSatisfy({ (0x41...0x5A).contains($0) || (0x61...0x7A).contains($0) || (0x30...0x39).contains($0) || [0x2E, 0x5F, 0x7E, 0x2D].contains($0) })
        else { throw OAuthError(code: "credential", description: "Claude Code 登录令牌格式无法识别") }
        guard let expiry, expiry.bool == nil, let expires = expiry.number, expires.isFinite else {
            throw OAuthError(code: "credential", description: "Claude Code 登录缺少有效期")
        }
        guard let scopes = oauth["scopes"]?.array, scopes.contains(.string("user:profile")) else {
            throw OAuthError(code: "scope", description: "Claude Code 登录缺少用量权限，请重新登录")
        }
        return (text, expires / 1000, SHA256.hex(Array(text.utf8)))
    }

    static func retrySeconds(_ header: String?, now: Double = Date().timeIntervalSince1970) -> Double {
        let raw = header ?? ""
        var seconds = Double(raw.trimmingCharacters(in: .whitespaces)) ?? .nan
        if seconds.isNaN {
            let formatter = DateFormatter()
            formatter.locale = Locale(identifier: "en_US_POSIX")
            formatter.dateFormat = "EEE, dd MMM yyyy HH:mm:ss zzz"
            seconds = formatter.date(from: raw).map { $0.timeIntervalSince1970 - now } ?? 300
        }
        return seconds.isFinite ? Swift.max(60, seconds) : 300
    }

    static func queryUsage(_ token: String) throws -> JSONObject {
        let response: HTTP.Response
        do {
            response = try HTTP.send(usageURL, headers: ["Authorization": "Bearer " + token, "anthropic-beta": "oauth-2025-04-20",
                                                         "Accept": "application/json", "User-Agent": "Starmeter"],
                                     timeout: 12, followRedirects: false, limit: maxResponse + 1)
        } catch { throw OAuthError(code: "network", description: "Claude 用量连接失败，稍后重试") }
        switch response.status {
        case 200..<300: break
        case 401: throw OAuthError(code: "expired", description: "Claude Code 登录失效，请刷新登录")
        case 403: throw OAuthError(code: "forbidden", description: "用量接口拒绝访问，请检查 Claude Code 登录")
        case 429: throw OAuthError(code: "limited", description: "请求频繁，稍后自动重试", retry: retrySeconds(response.header("Retry-After")))
        case 300..<400: throw OAuthError(code: "redirect", description: "用量接口发生跳转，已停止查询")
        default: throw OAuthError(code: "service", description: "Claude 用量服务暂不可用")
        }
        guard (response.header("Content-Type") ?? "").contains("application/json") else { throw OAuthError(code: "format", description: "用量接口返回格式异常") }
        if response.body.count > maxResponse { throw OAuthError(code: "format", description: "用量响应过大") }
        do {
            var result = try parseUsage(try JSON.parse(response.body))
            result["source"] = "claude-oauth"
            result["state"] = "已同步"
            return result
        } catch { throw OAuthError(code: "format", description: "用量格式尚未支持") }
    }

    static let renewSeconds = 30.0

    static func renewed(_ before: String) -> Bool {
        guard let credential = try? credentials() else { return false }
        return credential.owner != before && credential.expires > Date().timeIntervalSince1970 + 30
    }

    /// Lets the CLI's own startup auth check rotate its token. Nothing is typed into it; it stops as soon as the
    /// login changes, or after 30 seconds, and its whole process group goes with it.
    static func renewWithClaude(_ before: String?) throws {
        #if os(macOS)
        guard let binary = Shell.which("claude") else { throw OAuthError(code: "expired", description: "登录已过期，请安装并登录 Claude Code") }
        let directory = NSTemporaryDirectory() + "swiftbar-claude-auth-" + UUID().uuidString
        try FileManager.default.createDirectory(atPath: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(atPath: directory) }
        let settings: JSON = ["disableAllHooks": true, "remoteControlAtStartup": false, "statusLine": ["type": "command", "command": "/usr/bin/true"]]
        var master: Int32 = 0, slave: Int32 = 0
        guard openpty(&master, &slave, nil, nil, nil) == 0 else { throw OAuthError(code: "expired", description: "登录仍未续期，请在 Claude Code 检查登录") }
        defer { close(master) }
        var environment = ProcessInfo.processInfo.environment
        environment["TERM"] = "xterm-256color"
        environment["CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC"] = "1"
        environment["DISABLE_AUTOUPDATER"] = "1"
        let process = Process()
        process.executableURL = URL(fileURLWithPath: binary)
        process.arguments = ["--strict-mcp-config", "--tools", "", "--settings", settings.serialized(ensureASCII: true)]
        process.currentDirectoryURL = URL(fileURLWithPath: directory)
        process.environment = environment
        let handle = FileHandle(fileDescriptor: slave, closeOnDealloc: true)
        process.standardInput = handle; process.standardOutput = handle; process.standardError = handle
        try process.run()
        // Its own process group, so everything it starts ends with it.
        setpgid(process.processIdentifier, process.processIdentifier)
        try? handle.close()
        let deadline = Date().addingTimeInterval(renewSeconds)
        var check = Date().addingTimeInterval(3)
        var buffer = [UInt8](repeating: 0, count: 65536)
        let flags = fcntl(master, F_GETFL)
        _ = fcntl(master, F_SETFL, flags | O_NONBLOCK)
        while process.isRunning && Date() < deadline {
            _ = read(master, &buffer, buffer.count)  // The CLI's screen is discarded, never saved.
            if let before, Date() >= check {
                check = check.addingTimeInterval(2)
                if renewed(before) { break }
            }
            usleep(200_000)
        }
        if process.isRunning {
            kill(-process.processIdentifier, SIGTERM)
            let wait = Date().addingTimeInterval(2)
            while process.isRunning && Date() < wait { usleep(50_000) }
            if process.isRunning { kill(-process.processIdentifier, SIGKILL); process.waitUntilExit() }
        }
        #else
        throw OAuthError(code: "expired", description: "请在 Claude Code 中重新登录，再刷新用量")
        #endif
    }

    static func unavailable(_ message: String) -> JSONObject {
        ["source": "claude-oauth", "windows": [], "state": .string(message), "stale": true]
    }

    /// One read at a time; the server's backoff always holds, our own only until the user retries. Credentials
    /// stay read only, and another login's figures are never shown.
    public static func oauthFetch(state: String, consent: Bool = false, force: Bool = false, requestedAt: Double = 0) -> (JSONObject, String?) {
        guard consent else { return (unavailable("未启用 Claude Code 登录查询"), nil) }
        let path = state + "/claude-oauth-usage.json"
        try? Files.makeDirectory(state)
        let lockPath = state + "/claude-oauth.lock"
        let lock = Platform.lock(lockPath, wait: true)
        defer { withExtendedLifetime(lock) {} }
        var cached = readState(path)
        let now = Date().timeIntervalSince1970
        let forced = force || requestedAt > (cached["checkedAt"]?.number ?? 0)
        func number(_ key: String) -> Double { cached[key]?.pythonFloat ?? 0 }
        // Revoked keychain permission must not produce repeated background prompts.
        if cached["paused"]?.isTruthy == true && !force {
            let error = Py.str(cached["error"] ?? .null)
            return (unavailable(error), error)
        }
        // An explicit retry skips our own backoff, but a server rate limit always holds.
        if now < number("retryAt") && (!force || cached["limited"]?.isTruthy == true) {
            let error = cached["error"].map(Py.str) ?? "稍后重试"
            return (cached["payload"]?.object.flatMap { $0.isTruthy ? $0 : nil } ?? unavailable(error), error)
        }
        var owner: String?
        do {
            var credential = try credentials()
            owner = credential.owner
            if JSON.string(credential.owner) != cached["owner"] { cached = ["lastRenewAt": cached["lastRenewAt"] ?? .int(0)] }  // Never another login's quota.
            if cached["renewFailedOwner"] == .string(credential.owner) && !force {
                throw OAuthError(code: "expired", description: "登录续期未完成，请在 Claude Code 检查登录")
            }
            if credential.expires <= now + 30 {
                if now - number("lastRenewAt") < 300 { throw OAuthError(code: "expired", description: "登录仍未续期，请在 Claude Code 检查登录") }
                cached["lastRenewAt"] = .double(now); cached["checkedAt"] = .double(now); try? writeState(path, cached)
                try renewWithClaude(owner)
                credential = try credentials(); owner = credential.owner
                if credential.expires <= Date().timeIntervalSince1970 + 30 { throw OAuthError(code: "expired", description: "登录仍未续期，请在 Claude Code 检查登录") }
                cached["payload"] = nil
            }
            var payload = cached["payload"]?.object
            let age = now - (payload?["updatedAt"]?.number ?? 0)
            if let payload, payload.isTruthy, 0 <= age, age < (forced ? 30 : interval) { return (payload, nil) }
            do { payload = try queryUsage(credential.token) } catch let error as OAuthError {
                if error.code != "expired" || now - number("lastRenewAt") < 300 { throw error }
                cached["lastRenewAt"] = .double(now); cached["checkedAt"] = .double(now); try? writeState(path, cached)
                try renewWithClaude(owner)
                credential = try credentials(); owner = credential.owner
                cached["payload"] = nil
                if credential.expires <= Date().timeIntervalSince1970 { throw error }
                payload = try queryUsage(credential.token)
            }
            try? writeState(path, ["schema": 1, "owner": .string(credential.owner), "payload": .object(payload!), "checkedAt": .double(Date().timeIntervalSince1970),
                                   "lastRenewAt": cached["lastRenewAt"] ?? .int(0)])
            return (payload!, nil)
        } catch let error as OAuthError {
            var payload = owner != nil && JSON.string(owner!) == cached["owner"] ? cached["payload"]?.object : nil
            if var kept = payload, kept.isTruthy { kept["stale"] = true; kept["state"] = "刷新失败"; payload = kept } else { payload = nil }
            let paused = ["keychain", "missing", "scope", "credential", "profile"].contains(error.code)
            try? writeState(path, ["schema": 1, "owner": owner.map(JSON.string) ?? .null, "payload": payload.map(JSON.object) ?? .null, "checkedAt": .double(now),
                                   "lastRenewAt": cached["lastRenewAt"] ?? .int(0), "error": .string(error.description), "paused": .bool(paused),
                                   "renewFailedOwner": error.code == "expired" ? owner.map(JSON.string) ?? .null : .null, "limited": .bool(error.code == "limited"),
                                   "retryAt": paused ? .int(0) : .double(Date().timeIntervalSince1970 + error.retry)])
            return (payload ?? unavailable(error.description), error.description)
        } catch {
            let message = "Claude 登录查询未完成，稍后重试"
            try? writeState(path, ["schema": 1, "checkedAt": .double(now), "error": .string(message), "retryAt": .double(Date().timeIntervalSince1970 + 300)])
            return (unavailable(message), message)
        }
    }
}

extension Data {
    func trimmingWhitespace() -> Data {
        var bytes = self[...]
        while let first = bytes.first, [0x20, 0x09, 0x0A, 0x0D, 0x0B, 0x0C].contains(first) { bytes = bytes.dropFirst() }
        while let last = bytes.last, [0x20, 0x09, 0x0A, 0x0D, 0x0B, 0x0C].contains(last) { bytes = bytes.dropLast() }
        return Data(bytes)
    }
}

extension Shell {
    /// The first executable of that name on PATH, as shutil.which.
    public static func which(_ name: String, environment: [String: String] = ProcessInfo.processInfo.environment) -> String? {
        #if os(Windows)
        let separator: Character = ";", suffixes = ["", ".exe", ".cmd", ".bat"]
        #else
        let separator: Character = ":", suffixes = [""]
        #endif
        for folder in (environment["PATH"] ?? "").split(separator: separator) {
            for suffix in suffixes {
                let path = String(folder) + "/" + name + suffix
                if FileManager.default.isExecutableFile(atPath: path) && !Harness.isDirectory(path) { return path }
            }
        }
        return nil
    }
}

extension Runtime {
    public func invalidateCaches(_ names: [String]) {
        for name in names {
            var record = readCache(name)
            record["force"] = true
            record["retryAt"] = 0
            record["schema"] = 1
            writeJSONCache(cachePath(name), .object(record))
        }
    }

    /// What the Claude subscription depends on: its sources' settings and the files they read.
    func claudeSubscriptionParts() -> [JSON] {
        let settings = self.settings(), root = sourcePath("claude")
        var parts: [JSON] = [.string(root), settings["claudeLiveUsage"] ?? false, settings["claudeOAuthUsage"] ?? false,
                             .string(environment["CLAUDE_CONFIG_DIR"] ?? ""), "oauth-v1"]
        for file in ["plan-usage-history.json", "config.json", stateDir + "/" + ClaudeSubscription.snapshotFile] {
            let path = Paths.isAbsolute(file) ? file : root + "/" + file
            if let state = ClaudeUsage.state(of: path) { parts += [.int(state.mtime), .int(state.size)] } else { parts.append("unavailable") }
        }
        return parts
    }

    /// A retry the user asked for (立即刷新用量) that no result has acknowledged yet.
    func claudeRetryPending() -> Double {
        guard settings()["claudeOAuthUsage"]?.isTruthy == true else { return 0 }
        let requested = readCache("claude-retry")["requestedAt"]?.pythonFloat ?? 0
        let handled = readCache("claude_subscription")["payload"]?.object?["handledRetryAt"]?.pythonFloat ?? 0
        return requested > handled ? requested : 0
    }

    /// Too old to show as current: marked stale, older than ten minutes, or past a window's reset.
    static func claudeUsageStale(_ payload: JSONObject) -> Bool {
        let now = Date().timeIntervalSince1970
        if payload["stale"]?.isTruthy ?? true || now - (payload["updatedAt"]?.pythonFloat ?? 0) > 600 { return true }
        for row in payload["windows"]?.array ?? [] {
            guard let reset = row["resetsAt"], reset.isTruthy else { continue }
            guard let text = reset.string, let time = try? ISOTime.parse(text), time.timestamp() > now else { return true }
        }
        return false
    }

    func claudeSubscriptionLoad() -> (JSONObject?, String?) {
        let settings = self.settings()
        // Code OAuth is independent of browser cookies and does not invoke a model.
        if settings["claudeOAuthUsage"]?.isTruthy == true {
            let requested = readCache("resource-checks")["claude-subscription"]?.pythonFloat ?? 0
            // 立即刷新用量 is an explicit retry: renew once even after a failed renewal for this login.
            let retry = claudeRetryPending()
            var (payload, error) = ClaudeSubscription.oauthFetch(state: stateDir, consent: true, force: retry != 0, requestedAt: Swift.max(requested, retry))
            let handled = retry != 0 ? JSON.double(retry) : (readCache("claude_subscription")["payload"]?.object?["handledRetryAt"] ?? .int(0))
            if handled.isTruthy { payload["handledRetryAt"] = handled }
            if let error { payload["fetchError"] = .string(error) }
            // Publish errors as their own state; don't reuse a different App account.
            return (payload, nil)
        }
        var app = ClaudeSubscription.desktop(root: sourcePath("claude"))
        app["source"] = "app-snapshot"
        app["stale"] = .bool(Runtime.claudeUsageStale(app))
        let code = ClaudeSubscription.localEnabled(stateDir) ? ClaudeSubscription.localSnapshot(stateDir) : nil
        var candidates = [app, code].compactMap { $0 }.filter { $0["windows"]?.isTruthy == true }
        let manual = readCache("claude_subscription_manual")
        if settings["claudeLiveUsage"]?.isTruthy == true, manual["root"] == .string(sourcePath("claude")),
           var payload = manual["payload"]?.object, payload["windows"]?.isTruthy == true {
            payload["stale"] = .bool(Runtime.claudeUsageStale(payload))
            candidates.append(payload)
        }
        func newest(_ items: [JSONObject]) -> JSONObject? {
            var best: JSONObject?
            for item in items where best == nil || (item["updatedAt"]?.pythonFloat ?? 0) > (best!["updatedAt"]?.pythonFloat ?? 0) { best = item }
            return best
        }
        if let fresh = newest(candidates.filter { $0["stale"]?.isTruthy != true }) { return (fresh, nil) }
        if let any = newest(candidates) { return (any, nil) }
        return (code ?? app, nil)
    }

    public func claudeSubscriptionCached() -> (JSONObject?, String?) {
        // An older request can finish after the user clicks retry: keep it pending until a result acknowledges it.
        if claudeRetryPending() != 0 { invalidateCaches(["claude_subscription"]) }
        return cachedFetch("claude_subscription", ttl: 60) { claudeSubscriptionLoad() }
    }
}
