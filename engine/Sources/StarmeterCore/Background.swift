import Foundation

/// Official service status pages, read only when the user turns 服务状态 on. Each is a public Statuspage
/// document; nothing about the user is sent, only a plain User-Agent. Port of swiftbar_status.py.
public enum ServiceStatus {
    public static let pages: [String: (name: String, status: String, link: String)] = [
        "claude": ("Claude", "https://status.claude.com/api/v2/status.json", "https://status.claude.com"),
        "openai": ("OpenAI", "https://status.openai.com/api/v2/status.json", "https://status.openai.com"),
        "cursor": ("Cursor", "https://status.cursor.com/api/v2/status.json", "https://status.cursor.com"),
    ]
    public static let modulePages = ["claude": "claude", "claude-subscription": "claude", "claude-api": "claude", "codex": "openai", "opencodex": "openai",
                                     "chatgpt": "openai", "chatgpt-api": "openai", "cursor": "cursor", "cursor-subscription": "cursor"]
    public static let indicators = ["none": "", "minor": "服务部分降级", "major": "服务大范围故障", "critical": "服务严重故障", "maintenance": "服务维护中"]

    /// One page's indicator and description, or the name of what went wrong.
    public static func read(_ page: String, version: String) -> JSONObject {
        guard let (_, url, _) = pages[page] else { return ["error": "KeyError"] }
        do {
            let response = try HTTP.send(url, headers: ["User-Agent": "Starmeter/" + version, "Accept": "application/json"], timeout: 10, limit: 64 * 1024 + 1)
            guard (200..<300).contains(response.status) else { return ["error": "HTTPError"] }
            if response.body.count > 64 * 1024 { return ["error": "ValueError"] }
            guard let document = (try? JSON.parse(response.body))?.object else { return ["error": "JSONDecodeError"] }
            let status = Py.dict(document["status"]).object ?? JSONObject()
            guard let indicator = status["indicator"]?.string, indicators[indicator] != nil, let description = status["description"]?.string else {
                return ["error": "ValueError"]
            }
            return ["indicator": .string(indicator), "description": .string(String(description.prefix(200)))]
        } catch {
            return ["error": "URLError"]
        }
    }

    public static func readAll(_ wanted: [String], version: String) -> JSONObject {
        JSONObject(wanted.filter { pages[$0] != nil }.map { ($0, .object(read($0, version: version))) })
    }
}

extension Runtime {
    static let externalTTLs: [String: Double] = ["provider_quotas": 300, "badge": 3600, "komari": 30, "app_updates": 21600, "self_update": 21600, "chatgpt": 300,
        "grok": 300, "minimax": 300, "kimi-api": 300, "qoder-api": 300, "claude-api": 1800, "claude_subscription": 60, "qoder_subscription": 60,
        "cursor_subscription": 300, "sessions": 30, "service_status": 300, "exchange_rate": 86400,
        "healthz": 30, "ready": 30, "quota": 30, "accounts": 30, "active": 30]
    static let accountSources = ["grok", "minimax", "kimi-api", "qoder-api", "claude-api"]
    static let keyTTLs: [(String, Double)] = [("deepseek", 300), ("kimi-api", 300), ("grok", 300), ("qoder-api", 300), ("claude-api", 1800)]
    static let resourceCaches = ["deepseek": "provider_quotas", "chatgpt": "chatgpt", "grok": "grok", "minimax": "minimax", "kimi-api": "kimi-api",
        "qoder-api": "qoder-api", "claude-api": "claude-api", "komari": "komari", "updates": "app_updates", "claude-subscription": "claude_subscription",
        "qoder-subscription": "qoder_subscription", "cursor-subscription": "cursor_subscription", "opencodex": "opencodex", "sessions": "sessions"]

    // MARK: What is configured

    public func namedKeys(_ provider: String) -> [JSONObject] { memoized("keys:" + provider) { Keys.entries(stateDir, provider) } }

    func chatgptLoginPresent() -> Bool {
        let auth = ClaudeApp.readObject(codexHome() + "/auth.json")
        let tokens = auth["tokens"]?.object ?? JSONObject()
        let mode = auth["auth_mode"]
        return Shell.which("codex", environment: environment) != nil
            && (mode == .string("chatgpt") || mode == .string("chatgptAuthTokens") || tokens["access_token"]?.isTruthy == true)
    }

    public func chatgptConfigured() -> Bool { memoized("chatgpt-configured") { chatgptLoginPresent() } }

    /// A checked, configured OpenCodex supplies the ChatGPT accounts; otherwise the local login does.
    public func chatgptSource() -> String { moduleEnabled("opencodex") && opencodexConfigured() ? "proxy" : "client" }

    /// The DeepSeek key in OpenCodex's own config, when that provider points at the official API.
    func opencodexDeepseekKey() -> String {
        let config = ClaudeApp.readObject(ocxHome + "/config.json")
        guard let provider = Py.or(config["providers"], .object(JSONObject())).object?["deepseek"]?.object else { return "" }
        var base = Py.str(Py.or(provider["baseUrl"], .string("")))
        while base.hasSuffix("/") { base.removeLast() }
        guard ["https://api.deepseek.com", "https://api.deepseek.com/v1"].contains(base) else { return "" }
        return Py.str(Py.or(provider["apiKey"], .string(""))).trimmingCharacters(in: .whitespacesAndNewlines)
    }

    func credential(_ name: String) -> String { Accounts.credential(name, environment: environment) }

    /// Configured at all: the classic connection or, for an API, any key added in the panel.
    public func connectionReady(_ key: String) -> Bool {
        if let service = Catalog.byKey[key], service.category == "API", !namedKeys(key).isEmpty { return true }
        return defaultReady(key)
    }

    public func defaultReady(_ key: String) -> Bool {
        let settings = self.settings()
        switch key {
        case "claude-subscription":
            return settings["claudeOAuthUsage"]?.isTruthy == true || ClaudeSubscription.localEnabled(stateDir)
                || ClaudeSubscription.desktop(root: sourcePath("claude"))["windows"]?.isTruthy == true
        case "qoder-subscription": return Qoder.logRoots(home: home, environment: environment).contains(where: Harness.isDirectory)
        case "cursor-subscription": return settings["cursorUsage"]?.isTruthy == true
        case "chatgpt": return chatgptSource() == "proxy" ? opencodexConfigured() : chatgptConfigured()
        case "deepseek":
            if env("DEEPSEEK_API_KEY") != nil || env("DEEPSEEK_API_KEY_FILE") != nil { return !credential("DEEPSEEK_API_KEY").isEmpty }
            return !opencodexDeepseekKey().isEmpty
        case "komari": return !komariBaseURL.isEmpty
        case "grok": return !credential("XAI_MANAGEMENT_API_KEY").isEmpty && !(environment["XAI_TEAM_ID"] ?? "").trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        case "minimax": return !credential("MINIMAX_API_KEY").isEmpty
        case "kimi-api": return !credential("MOONSHOT_API_KEY").isEmpty
        case "qoder-api": return !credential("QODER_CN_API_KEY").isEmpty && !(environment["QODER_CN_ORG_ID"] ?? "").trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        case "claude-api": return !credential("ANTHROPIC_API_KEY").isEmpty
        default:
            if Catalog.manualSubscriptions.contains(key) { return settings["configuredSubscriptions"]?.array?.contains(.string(key)) ?? false }
            if Catalog.portalAPIs.contains(key) { return Catalog.credentialNames(key).contains { !credential($0).isEmpty } }
            return false
        }
    }

    // MARK: Small loaders

    /// The status pages to read: none unless 服务状态 is on, then only those of the modules shown.
    public func statusPages() -> [String] {
        guard settings()["serviceStatus"]?.isTruthy == true else { return [] }
        return Set(ServiceStatus.modulePages.filter { moduleEnabled($0.key) }.map(\.value)).sorted()
    }

    /// The day's ECB reference rate of the yuan to the dollar, from Frankfurter; only a User-Agent is sent.
    public func exchangeRateLoad() -> (JSONObject?, String?) {
        do {
            let response = try HTTP.send("https://api.frankfurter.dev/v1/latest?base=USD&symbols=CNY",
                                         headers: ["User-Agent": "Starmeter/" + Runtime.version, "Accept": "application/json"], timeout: 10)
            guard (200..<300).contains(response.status) else { return (nil, "汇率读取失败 · HTTPError") }
            guard let body = (try? JSON.parse(response.body.prefix(16 * 1024)))?.object else { return (nil, "汇率读取失败 · JSONDecodeError") }
            let rate = Py.dict(body["rates"]).object?["CNY"]
            guard Preferences.exchangeRate(rate), let value = rate?.pythonFloat, let date = body["date"]?.string else { return (nil, "汇率读取失败 · ValueError") }
            return (["rate": .double(value), "date": .string(String(date.prefix(10)))], nil)
        } catch {
            return (nil, "汇率读取失败 · URLError")
        }
    }

    func pricingDue(now: Double = Date().timeIntervalSince1970) -> Bool {
        if environment["OCX_PRICING_UPDATES"] == "0" { return false }
        let age = now - (readCache("pricing-check")["checkedAt"]?.pythonFloat ?? 0)
        return !(0 <= age && age < 86400)
    }

    /// Once a day: a plain GET for each table, no cookies or identifiers. Only a valid table newer than the one
    /// in use is kept; OCX_PRICING_UPDATES=0 turns this off.
    func refreshPricing(now: Double = Date().timeIntervalSince1970) {
        guard pricingDue(now: now) else { return }
        writeJSONCache(cachePath("pricing-check"), ["schema": 1, "checkedAt": .double(now)])
        for name in Runtime.pricingTables {
            guard let response = try? HTTP.send("https://raw.githubusercontent.com/timecatbit/starmeter/main/lib/" + name,
                                                headers: ["User-Agent": "Starmeter/" + Runtime.version, "Accept": "application/json"], timeout: 10, limit: 4 * 1024 * 1024 + 1),
                  (200..<300).contains(response.status), response.body.count <= 4 * 1024 * 1024, let data = try? JSON.parse(response.body) else { continue }
            if Pricing.valid(name, data), let table = data.object, Pricing.committed(table) > Pricing.committed(Pricing.table(name)) {
                writeJSONCache(stateDir + "/pricing/" + name, data)
                Pricing.reset()
            }
        }
    }

    /// OpenCodex's own update check, behind its management API.
    func updateStatusCached() -> (JSONObject?, String?) {
        cachedFetch("badge", ttl: 3600) {
            let (payload, error) = opencodexRequest("/api/update/check", timeout: 4)
            return (payload, error?.message)
        }
    }

    // MARK: The background worker

    /// Every source to read in the background, in the order Python lists them, with how to read it.
    public func externalLoaders() -> [(String, () -> Void)] {
        var loaders: [(String, () -> Void)] = []
        let settings = self.settings()
        func add(_ name: String, _ body: @escaping () -> Void) { loaders.append((name, body)) }
        if moduleEnabled("opencodex") && opencodexConfigured() {
            add("opencodex") { self.opencodexRefresh() }
            add("badge") { _ = self.updateStatusCached() }
        }
        if moduleEnabled("deepseek") && defaultReady("deepseek") { add("provider_quotas") { _ = self.providerQuotas() } }
        if moduleEnabled("komari") && !komariBaseURL.isEmpty { add("komari") { _ = self.komariSnapshot() } }
        if moduleEnabled("updates"), let updates = appUpdatesLoader, appUpdatesWanted?() ?? false { add("app_updates", updates) }
        if environment["OCX_SELF_UPDATES"] != "0", let check = selfUpdateLoader { add("self_update", check) }
        if moduleEnabled("claude-subscription") { add("claude_subscription") { _ = self.claudeSubscriptionCached() } }
        if moduleEnabled("sessions") { add("sessions") { _ = self.sessionsCached() } }
        // The yuan rate is read only once 人民币 is chosen without a rate of the user's own.
        if settings["currency"] == .string("CNY") && !(settings["cnyRate"]?.isTruthy ?? false) {
            add("exchange_rate") { _ = self.cachedFetch("exchange_rate", ttl: 86400, retry: 3600) { self.exchangeRateLoad() } }
        }
        let pages = statusPages()
        if !pages.isEmpty { add("service_status") { _ = self.cachedFetch("service_status", ttl: 300) { (ServiceStatus.readAll(pages, version: Runtime.version), nil) } } }
        if moduleEnabled("cursor-subscription") && settings["cursorUsage"]?.isTruthy == true {
            add("cursor_subscription") {
                _ = self.cachedFetch("cursor_subscription", ttl: self.refreshTTL(300)) {
                    do { return (try Cursor.plan(home: self.home), nil) } catch let failure as Cursor.Failure { return (nil, failure.description) }
                }
            }
        }
        if moduleEnabled("qoder-subscription") {
            add("qoder_subscription") { _ = self.cachedFetch("qoder_subscription", ttl: 60) { (Qoder.snapshot(home: self.home) ?? ["empty": true], nil) } }
        }
        // Each read starts the ChatGPT client's app-server, so a failing client waits its normal interval, not a minute.
        if moduleEnabled("chatgpt") && chatgptSource() == "client" && chatgptConfigured() {
            add("chatgpt") { _ = self.cachedFetch("chatgpt", ttl: 300, retry: 300) { (ChatGPT.account(home: self.codexHome(), environment: self.environment), nil) } }
        }
        for key in Runtime.accountSources where moduleEnabled(key) && defaultReady(key) {
            add(key) { _ = self.cachedFetch(key, ttl: Runtime.externalTTLs[key]!) { (Accounts.account(key, environment: self.environment), nil) } }
        }
        for (provider, ttl) in Runtime.keyTTLs where moduleEnabled(provider) {
            for entry in namedKeys(provider) {
                guard let id = entry["id"]?.string else { continue }
                let name = provider + "@" + id, options = entry["options"]?.object ?? JSONObject()
                add(name) {
                    _ = self.cachedFetch(name, ttl: ttl) {
                        (Accounts.account(provider, key: Keys.secret(self.stateDir, provider, id), options: options, environment: self.environment), nil)
                    }
                }
            }
        }
        return loaders
    }

    func ttlFor(_ name: String) -> Double {
        Runtime.externalTTLs[name] ?? Runtime.keyTTLs.first { $0.0 == String(name.split(separator: "@").first ?? "") }?.1 ?? 300
    }

    public func loaderDue(_ name: String) -> Bool {
        if name == "claude_subscription" && claudeRetryPending() != 0 { return Date().timeIntervalSince1970 >= (readCache(name)["retryAt"]?.pythonFloat ?? 0) }
        if name != "opencodex" { return cacheDue(readCache(name), ttl: refreshTTL(ttlFor(name))) }
        let health = readCache("healthz")
        if cacheDue(health, ttl: refreshTTL(30)) { return true }
        // While the service is down, its other endpoints wait for the next health retry.
        if health["error"]?.isTruthy == true || !(health["payload"]?.isTruthy ?? false) { return false }
        return Runtime.opencodexEndpoints.contains { cacheDue(readCache($0.0), ttl: refreshTTL(30)) }
    }

    /// The selected range when its figures are due; this month too while a subscription's price is written down;
    /// today too while today's estimate sits beside the star.
    public func usageRangesDue() -> [String] {
        let key = selectedRange(), settings = self.settings()
        var due = cacheDue(readCache(rangeCacheName(key)), ttl: refreshTTL(60)) ? [key] : []
        let weighed = (settings["subscriptionPrices"]?.object ?? JSONObject()).keys.filter { moduleEnabled($0) && Catalog.subscriptionUsage[$0].map(moduleEnabled) == true }
        if !weighed.isEmpty && key != "month" && cacheDue(readCache(rangeCacheName("month")), ttl: refreshTTL(600)) { due.append("month") }
        if settings["menuBar"] == .string("cost") && key != "today" && cacheDue(readCache(rangeCacheName("today")), ttl: refreshTTL(60)) { due.append("today") }
        return due
    }

    /// Index reads one at a time, apart from the slower network refreshes.
    @discardableResult
    public func refreshUsageRange(wait: Bool = false) -> Bool {
        withLock("range-refresh.lock", wait: wait) {
            let due = usageRangesDue()
            for key in due { _ = rangeUsage(key) }
            return !due.isEmpty
        } ?? false
    }

    /// A pinned panel's tick: the range on screen re-read from the local logs, and the active sessions.
    @discardableResult
    public func refreshLive() -> Bool {
        guard withLock("range-refresh.lock", wait: false, { _ = rangeUsage(selectedRange(), ttl: 0) }) != nil else { return false }
        if moduleEnabled("sessions") { _ = sessionsCached(ttl: 0) }
        return true
    }

    /// Runs body holding a lock file in the cache folder; nil when another process holds it and we don't wait.
    func withLock<T>(_ name: String, wait: Bool, _ body: () -> T) -> T? {
        try? Files.makeDirectory(cacheDir)
        let descriptor = open(cacheDir + "/" + name, O_CREAT | O_RDWR | O_APPEND, 0o600)
        guard descriptor >= 0 else { return nil }
        defer { close(descriptor) }
        if flock(descriptor, LOCK_EX | (wait ? 0 : LOCK_NB)) != 0 { return nil }
        return body()
    }

    /// One worker at a time, however often the menu is refreshed.
    public func backgroundRefresh(wait: Bool = false) {
        var started = false
        _ = withLock("refresh.lock", wait: wait) {
            let loaders = externalLoaders()
            var work: [() -> Void] = loaders.filter { loaderDue($0.0) }.map(\.1)
            if !usageRangesDue().isEmpty { work.append { self.refreshUsageRange() } }
            if pricingDue() { work.append { self.refreshPricing() } }
            started = !work.isEmpty
            let queue = OperationQueue()
            queue.maxConcurrentOperationCount = 4
            for job in work { queue.addOperation(job) }
            queue.waitUntilAllOperationsAreFinished()
            afterBackgroundReads?()
            collectCacheGarbage()
        }
        if started { refreshSwiftBar() }
    }

    static let currentIndexes = ["claude-app-usage-v4-", "harness-v4-", "harness-v5-codex-", "harness-v5-qoder-", "harness-v6-magpie-", "harness-gemini-v2-", "harness-gateway-v2-"]
    static let retiredIndexes = ["harness-v4-magpie-", "harness-v4-codex-", "harness-v4-qoder-"]
    static let retiredCaches = ["startup.json", "usage_today.json", "usage_7d.json"] + ranges.map { "cursor_usage_\($0.0).json" }

    static func isIndexFile(_ name: String) -> Bool {
        var base = name
        for suffix in ["-journal", "-wal", "-shm"] where base.hasSuffix(suffix) { base.removeLast(suffix.count); break }
        guard base.hasSuffix(".sqlite3") else { return false }
        return ["claude-app-usage-", "claude-usage-", "harness-"].contains { base.hasPrefix($0) && base.count > $0.count + ".sqlite3".count }
    }

    /// Hourly at most; only this engine's own cache folder.
    public func collectCacheGarbage(now: Double = Date().timeIntervalSince1970) {
        let age = now - (readCache("cache-cleanup")["cleanedAt"]?.pythonFloat ?? 0)
        if 0 <= age && age < 3600 { return }
        let current = "_" + (rangeCacheName().split(separator: "_").last.map(String.init) ?? "") + ".json"
        guard let names = try? FileManager.default.contentsOfDirectory(atPath: cacheDir) else { return }
        for name in names {
            let path = cacheDir + "/" + name
            var info = stat()
            guard lstat(path, &info) == 0 else { continue }
            #if os(macOS)
            let mtime = Double(info.st_mtimespec.tv_sec) + Double(info.st_mtimespec.tv_nsec) / 1e9
            #else
            let mtime = Double(info.st_mtim.tv_sec) + Double(info.st_mtim.tv_nsec) / 1e9
            #endif
            let fileAge = now - mtime
            let stale = (Runtime.isIndexFile(name) && (!Runtime.currentIndexes.contains(where: name.hasPrefix) || Runtime.retiredIndexes.contains(where: name.hasPrefix)))
                || Runtime.retiredCaches.contains(name)
                || (name.hasPrefix("range_usage_") && (fileAge > 86400 || !name.hasSuffix(current)))
                || (name.hasPrefix("configured-sources-") && fileAge > 86400)
                || ((name.hasPrefix(".cache-") || name.contains(".processing-")) && fileAge > 3600)
            if stale { try? FileManager.default.removeItem(atPath: path) }
        }
        writeJSONCache(cachePath("cache-cleanup"), ["schema": 1, "cleanedAt": .double(now)])
    }

    /// Asks SwiftBar to redraw the menu; the standalone app redraws on its own.
    public func refreshSwiftBar() {
        guard !macApp else { return }
        _ = Shell.run(["/usr/bin/open", "-g", "swiftbar://refreshplugin?name=" + Accounts.quote(swiftbarName)], timeout: 4)
    }
}
