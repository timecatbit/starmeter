import Foundation

/// Where the engine runs from and what it reads: the environment (config.local.sh is sourced by the entry
/// script), the state and cache folders, settings for one render, and the JSON caches shared with the panel.
/// Port of the top of swiftbar_plugin.py.
public final class Runtime {
    public let environment: [String: String]
    public let home: String
    public let ocxHome: String
    public let tokenFile: String
    public let stateDir: String
    public let cacheDir: String
    public let plugin: String
    public let base: String
    public let macApp: Bool
    /// Settings and local metadata read once per render, as RENDER_MEMO does.
    private var memo: [String: Any]?
    /// Set by an in-process host (the standalone app): whether its panel is open, and on battery.
    public var hostCadence: (() -> (Bool, Bool))?
    /// The network-backed ranges, set as their ports land: OpenCodex's own figures and Cursor's account events.
    public var opencodexRangeUsage: ((String, Double) -> JSONObject)?
    public var cursorRangeUsage: ((Double, Double, Double, [Carrier]) -> JSONObject)?
    private let lock = NSLock()

    public static let version = "5.26.1"
    public static let product = "Starmeter"
    public static let ranges: [(String, String)] = [("today", "今天"), ("yesterday", "昨天"), ("24h", "近 24 小时"), ("7d", "近 7 天"),
                                                   ("30d", "近 30 天"), ("week", "本周"), ("month", "本月"), ("all", "全部")]
    public static let hourlyRanges: Set<String> = ["today", "yesterday", "24h", "7d", "week"]
    static let pricingTables = ["codex-pricing.json", "model-pricing.json"]
    static let magpieAgents: [String: Set<String>] = ["codex": ["codex"], "claude": ["claude", "claude-code", "claude-desktop"], "opencode": ["opencode"],
                                                      "deepseek-harness": ["dsh"], "gemini": ["gemini", "gemini-cli"], "cursor": ["cursor"]]
    public static let pendingRead = "正在后台读取"

    public init(environment: [String: String] = ProcessInfo.processInfo.environment, argv0: String = CommandLine.arguments.first ?? "starmeter") {
        self.environment = environment
        home = Paths.home
        func expand(_ value: String) -> String { Paths.expandUser(value) }
        ocxHome = expand(environment["OCX_HOME"].flatMap { $0.isEmpty ? nil : $0 } ?? home + "/.opencodex")
        tokenFile = expand(environment["OCX_TOKEN_FILE"].flatMap { $0.isEmpty ? nil : $0 } ?? ocxHome + "/admin-api-token")
        plugin = environment["OCX_PLUGIN_PATH"].flatMap { $0.isEmpty ? nil : $0 } ?? argv0
        macApp = environment["OCX_MAC_APP"] == "1"
        if let state = environment["OCX_STATE_DIR"], !state.isEmpty { stateDir = expand(state) }
        else if let cache = environment["OCX_CACHE_DIR"], !cache.isEmpty { stateDir = cache + "/settings" }
        else { stateDir = home + "/Library/Application Support/SwiftBarToken" }
        let scope = SHA256.hex(Array(Runtime.absolute(stateDir).utf8)).prefix(16)
        let parent = environment["OCX_CACHE_DIR"].flatMap { $0.isEmpty ? nil : $0 } ?? NSTemporaryDirectory()
        cacheDir = (parent as NSString).appendingPathComponent("opencodex-swiftbar-v5-\(getuid())-\(scope)")
        base = Runtime.detectBase(environment, home: home)
        if Pricing.folders.isEmpty { Pricing.folders = [Runtime.libDirectory(plugin: plugin), stateDir + "/pricing"] }
    }

    static func absolute(_ path: String) -> String {
        path.hasPrefix("/") ? Paths.normalize(path) : Paths.normalize(FileManager.default.currentDirectoryPath + "/" + path)
    }

    /// The installed lib folder beside the entry, where the price tables ship.
    static func libDirectory(plugin: String) -> String {
        let folder = (absolute(plugin) as NSString).deletingLastPathComponent
        for candidate in [folder + "/.opencodex-panel/lib", folder + "/lib"] where Harness.isDirectory(candidate) { return candidate }
        return folder + "/lib"
    }

    static func detectBase(_ environment: [String: String], home: String) -> String {
        let explicit = (environment["OCX_BASE_URL"] ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        if !explicit.isEmpty {
            var trimmed = explicit
            while trimmed.hasSuffix("/") { trimmed.removeLast() }
            return trimmed
        }
        let home = Paths.expandUser(environment["OCX_HOME"].flatMap { $0.isEmpty ? nil : $0 } ?? home + "/.opencodex")
        if let port = ClaudeApp.readObject(home + "/runtime-port.json")["port"], let number = try? Py.int(port), (1...65535).contains(number) {
            return "http://127.0.0.1:\(number)"
        }
        return "http://127.0.0.1:10100"
    }

    // MARK: One render's view of the settings

    /// Runs body with settings and local metadata read at most once inside it.
    public func render<T>(_ body: () throws -> T) rethrows -> T {
        lock.lock(); memo = [:]; lock.unlock()
        defer { lock.lock(); memo = nil; lock.unlock() }
        return try body()
    }

    func memoized<T>(_ key: String, _ compute: () -> T) -> T {
        lock.lock()
        // Look the key up first: with an optional T, a cast of a missing entry would succeed as nil.
        if let memo, let stored = memo[key] { lock.unlock(); return stored as! T }
        lock.unlock()
        let value = compute()
        lock.lock()
        if memo != nil { memo![key] = value }
        lock.unlock()
        return value
    }

    public func settings() -> JSONObject { memoized("settings") { Preferences.read(stateDir) } }

    public func moduleEnabled(_ key: String) -> Bool { settings()["visible"]?.array?.contains(.string(key)) ?? false }

    public func env(_ key: String) -> String? { environment[key].flatMap { $0.isEmpty ? nil : $0 } }

    public func sourcePath(_ key: String) -> String {
        let custom = settings()["paths"]?.object?[key == "claude" ? "claude-app" : key]?.string
        let codexHome = Paths.expandUser(env("CODEX_HOME") ?? home + "/.codex")
        let defaults: [String: String] = [
            "opencodex": ocxHome,
            "claude": env("CLAUDE_APP_DATA_DIR") ?? ClaudeApp.defaultRoot(home: home, environment: environment),
            "codex": env("CODEX_USAGE_DIR") ?? codexHome,
            "opencode": env("OPENCODE_DATA_DIR") ?? Paths.expandUser(env("XDG_DATA_HOME") ?? home + "/.local/share") + "/opencode",
            "deepseek-harness": dshSessionsRoot(),
            "workbuddy": env("WORKBUDDY_USAGE_DIR") ?? home,
            "qoder": env("QODERCN_CONFIG_DIR") ?? home + "/.qoder-cn",
            "gemini": env("GEMINI_USAGE_DIR") ?? home + "/.gemini",
            "magpie": magpieRoot(),
            "cursor": home + "/Library/Application Support/Cursor/User/globalStorage",
        ]
        return Paths.expandUser(custom.flatMap { $0.isEmpty ? nil : $0 } ?? defaults[key] ?? "")
    }

    func dshSessionsRoot() -> String {
        let explicit = (environment["DSH_SESSIONS_DIR"] ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        if !explicit.isEmpty { return Paths.normalize(Paths.expandUser(explicit)) }
        let configured = (environment["DSH_HOME"] ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        return (configured.isEmpty ? home + "/.dsh" : Paths.normalize(Paths.expandUser(configured))) + "/sessions"
    }

    func magpieRoot() -> String {
        if let configured = env("MAGPIE_USAGE_DIR") { return Paths.normalize(Paths.expandUser(configured)) }
        let xdg = environment["XDG_CONFIG_HOME"] ?? ""
        return (xdg.hasPrefix("/") ? Paths.normalize(xdg) : home + "/.config") + "/magpie"
    }

    // MARK: Caches

    public func cachePath(_ name: String) -> String { (cacheDir as NSString).appendingPathComponent(name + ".json") }

    static func today() -> String {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = .current
        let c = calendar.dateComponents([.year, .month, .day], from: Date())
        return String(format: "%04d-%02d-%02d", c.year!, c.month!, c.day!)
    }

    public func readCache(_ name: String) -> JSONObject {
        guard let data = FileManager.default.contents(atPath: cachePath(name)), let record = (try? JSON.parse(data))?.object,
              record["schema"] == .int(1) else { return JSONObject() }
        if let connection = record["connection"], connection.isTruthy, connection != .string(connectionFingerprint(name)) { return JSONObject() }
        for key in ["fetchedAt", "retryAt"] {
            if let value = record[key], value.number == nil && value.bool == nil { return JSONObject() }
        }
        if let payload = record["payload"], !payload.isNull, payload.object == nil { return JSONObject() }
        if name.hasPrefix("usage_") || name.hasPrefix("range_usage_") || name == "claude_usage", record["day"] != .string(Runtime.today()) {
            return JSONObject()
        }
        return record
    }

    /// Replaced atomically, so a concurrent refresh never reads half a file; skipped when nothing changed.
    public func writeJSONCache(_ path: String, _ payload: JSON) {
        if let data = FileManager.default.contents(atPath: path), let current = try? JSON.parse(data), current == payload { return }
        try? Files.makeDirectory((path as NSString).deletingLastPathComponent)
        try? Files.replaceAtomically(path, with: Data(payload.serialized().utf8), prefix: ".cache-")
    }

    func cacheDue(_ record: JSONObject, ttl: Double) -> Bool {
        let now = Date().timeIntervalSince1970
        let age = now - (record["fetchedAt"]?.pythonFloat ?? 0)
        return now >= (record["retryAt"]?.pythonFloat ?? 0)
            && (record["force"]?.isTruthy == true || !(record["payload"]?.isTruthy ?? false) || age < 0 || age >= ttl)
    }

    /// The cached payload with its read time, marked stale when it failed, was forced or is old; and the error.
    func cacheResult(_ record: JSONObject, ttl: Double) -> (JSONObject?, String?) {
        guard var result = record["payload"]?.object else {
            let error = record["error"].flatMap { $0.isTruthy ? Py.str($0) : nil }
            return (nil, error ?? Runtime.pendingRead)
        }
        result["_fetchedAt"] = record["fetchedAt"] ?? .int(0)
        let age = Date().timeIntervalSince1970 - (result["_fetchedAt"]?.pythonFloat ?? 0)
        if record["error"]?.isTruthy == true || record["force"]?.isTruthy == true || age < 0 || age >= ttl { result["_stale"] = true }
        else { result["_stale"] = nil }
        return (result, record["error"].flatMap { $0.isTruthy ? Py.str($0) : nil })
    }

    /// The payload a loader gives, cached for ttl seconds; a failed read keeps the last good one and retries.
    public func cachedFetch(_ name: String, ttl: Double, retry: Double = 60, _ loader: () throws -> (JSONObject?, String?)) -> (JSONObject?, String?) {
        let fingerprint = connectionFingerprint(name)
        var record = readCache(name)
        if !cacheDue(record, ttl: ttl) { return cacheResult(record, ttl: ttl) }
        var payload: JSONObject?, failure: String?
        do { (payload, failure) = try loader() }
        catch let thrown as Py.FileError { (payload, failure) = (nil, thrown.kind) }
        catch is Py.LineError { (payload, failure) = (nil, "TypeError") }
        catch let thrown { (payload, failure) = (nil, String(describing: type(of: thrown))) }
        if failure == nil, let payload, let message = payload["error"], message.isTruthy { failure = Py.str(message) }
        if failure == nil, let payload {
            record = ["schema": 1, "payload": .object(payload), "fetchedAt": .double(Date().timeIntervalSince1970), "day": .string(Runtime.today())]
        } else {
            // Failed requests retry after a minute, retaining the last successful data.
            record["schema"] = 1
            record["error"] = .string(failure ?? "数据格式异常")
            record["retryAt"] = .double(Date().timeIntervalSince1970 + retry)
            record["day"] = .string(Runtime.today())
        }
        record["connection"] = .string(fingerprint)
        writeJSONCache(cachePath(name), .object(record))
        return cacheResult(record, ttl: ttl)
    }

    /// What a cached source depends on: change it and the cache is read afresh. Sources without settings of
    /// their own depend on the OpenCodex address and token file.
    public func connectionFingerprint(_ name: String) -> String {
        memoized("connection:" + name) {
            SHA256.hex(Array(Py.dumps(.array(connectionParts(name))).utf8))
        }
    }

    /// Which sources have connection settings of their own; filled in as their loaders are ported.
    static var connectionKeys: [String: [String]] = [
        "app_updates": [], "claude_subscription": [], "qoder_subscription": [], "cursor_subscription": [], "chatgpt": ["OCX_CHATGPT_HOME"],
        "komari": ["KOMARI_BASE_URL"], "provider_quotas": ["DEEPSEEK_API_KEY", "DEEPSEEK_API_KEY_FILE"],
        "grok": ["XAI_MANAGEMENT_API_KEY", "XAI_MANAGEMENT_API_KEY_FILE", "XAI_TEAM_ID"],
        "minimax": ["MINIMAX_API_KEY", "MINIMAX_API_KEY_FILE", "MINIMAX_REGION"],
        "kimi-api": ["MOONSHOT_API_KEY", "MOONSHOT_API_KEY_FILE", "MOONSHOT_REGION"],
        "qoder-api": ["QODER_CN_API_KEY", "QODER_CN_API_KEY_FILE", "QODER_CN_ORG_ID"],
        "claude-api": ["ANTHROPIC_API_KEY", "ANTHROPIC_API_KEY_FILE"],
    ]
    /// Extra facts some sources depend on, set by their loaders' ports (app versions, Claude snapshots...).
    var connectionExtras: [String: () -> [JSON]] = [:]

    func connectionParts(_ name: String) -> [JSON] {
        var parts: [JSON] = connectionExtras[name]?() ?? []
        for key in Runtime.connectionKeys[name] ?? [] {
            let value = environment[key] ?? ""
            parts.append(.string(value))
            if key.hasSuffix("_FILE"), !value.isEmpty {
                if let state = ClaudeUsage.state(of: Paths.expandUser(value)) { parts += [.int(state.mtime), .int(state.size)] }
                else { parts.append("unavailable") }
            }
        }
        if Runtime.connectionKeys[name] == nil || name == "provider_quotas" { parts += [.string(base), .string(tokenFile)] }
        return parts
    }
}
