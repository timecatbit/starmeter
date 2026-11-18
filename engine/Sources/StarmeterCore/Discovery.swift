import Foundation

/// Which modules this machine can feed, for a first install and for 自动检测: by installed apps, CLIs on PATH,
/// log folders that hold records and configured keys. Never parses a conversation or makes a request.
/// Port of swiftbar_discovery.py.
public enum Discovery {
    static func hasRecords(_ root: String?, _ matches: (String, String) -> Bool = { name, _ in name.hasSuffix(".jsonl") }) -> Bool {
        guard let root, !root.isEmpty else { return false }
        let base = Paths.expandUser(root)
        guard Harness.isDirectory(base) else { return false }
        for path in Harness.walk(base) {
            let relative = String(path.dropFirst(base.count + 1))
            if matches(Harness.name(path), relative), let state = ClaudeUsage.state(of: path), state.size > 0 { return true }
        }
        return false
    }

    public static func detect(paths: [String: String], home: String, configuredProxy: Bool, komariURL: String, appRoots: [String], chatgptHome: String?,
                              configuredSubscriptions: [String], environment: [String: String]) -> [String] {
        func which(_ name: String) -> Bool { Shell.which(name, environment: environment) != nil }
        func app(_ names: String...) -> Bool { appRoots.contains { root in names.contains { Harness.isDirectory(root + "/" + $0) } } }
        func credential(_ name: String) -> Bool { !Accounts.credential(name, environment: environment).isEmpty }
        var found: Set<String> = ["summary", "heatmap", "updates"]
        if configuredProxy { found.formUnion(["opencodex", "chatgpt"]) }
        for (source, binary) in [("codex", "codex"), ("opencode", "opencode"), ("gemini", "gemini"), ("deepseek-harness", "dsh")] where which(binary) { found.insert(source) }
        let magpie = (paths["magpie"].flatMap { $0.isEmpty ? nil : $0 } ?? home + "/.config/magpie") + "/usage.jsonl"
        if which("magpie") || (FileManager.default.fileExists(atPath: magpie) && !Harness.isDirectory(magpie)) || app("magpie.app", "Magpie.app") { found.insert("magpie") }
        if app("WorkBuddy.app") { found.insert("workbuddy") }
        // Cursor keeps no usage on disk; the module only offers to read the account's records once turned on.
        let cursorState = (paths["cursor"].flatMap { $0.isEmpty ? nil : $0 } ?? home) + "/state.vscdb"
        if which("cursor-agent") || app("Cursor.app") || FileManager.default.fileExists(atPath: cursorState) { found.formUnion(["cursor", "cursor-subscription"]) }
        let qoder = paths["qoder"].flatMap { $0.isEmpty ? nil : $0 } ?? environment["QODERCN_CONFIG_DIR"].flatMap { $0.isEmpty ? nil : $0 } ?? home + "/.qoder-cn"
        if which("qodercn") || app("Qoder CN.app", "QoderCN.app") || hasRecords(qoder + "/projects") { found.insert("qoder") }
        if Qoder.snapshot(home: home) != nil { found.insert("qoder-subscription") }
        let claudeRoot = paths["claude"].flatMap { $0.isEmpty ? nil : $0 } ?? ClaudeApp.defaultRoot(home: home, environment: environment)
        if app("Claude.app") || !ClaudeApp.registryFiles(claudeRoot).isEmpty || which("claude")
            || ClaudeApp.projectRoots(home: home, environment: environment).contains(where: { hasRecords($0) }) { found.insert("claude") }
        if ClaudeSubscription.desktop(root: claudeRoot)["windows"]?.isTruthy == true { found.insert("claude-subscription") }
        let codex = paths["codex"].flatMap { $0.isEmpty ? nil : $0 } ?? home + "/.codex"
        let folders = ["sessions", "archived_sessions"].contains(where: { Harness.isDirectory(codex + "/" + $0) }) ? [codex + "/sessions", codex + "/archived_sessions"] : [codex]
        if folders.contains(where: { hasRecords($0) { name, _ in name.hasSuffix(".jsonl") || name.hasSuffix(".jsonl.zst") } }) { found.insert("codex") }
        // Active sessions are Claude Code's and Codex's.
        if !found.isDisjoint(with: ["claude", "codex"]) { found.insert("sessions") }
        let opencode = paths["opencode"].flatMap { $0.isEmpty ? nil : $0 } ?? home + "/.local/share/opencode"
        let database = Harness.isDirectory(opencode) ? opencode + "/opencode.db" : opencode
        if let state = ClaudeUsage.state(of: database), !Harness.isDirectory(database), state.size > 0 { found.insert("opencode") }
        let dsh = paths["deepseek-harness"].flatMap { $0.isEmpty ? nil : $0 } ?? Runtime.dshRoot(home: home, environment: environment)
        if hasRecords(dsh, { name, _ in name.hasPrefix("session") && (name.hasSuffix(".jsonl") || name.hasSuffix(".jsonl.zstd")) }) { found.insert("deepseek-harness") }
        let gemini = paths["gemini"].flatMap { $0.isEmpty ? nil : $0 } ?? home + "/.gemini"
        if hasRecords(Harness.isDirectory(gemini + "/tmp") ? gemini + "/tmp" : gemini, { name, relative in
            (name.hasPrefix("session-") && (name.hasSuffix(".json") || name.hasSuffix(".jsonl"))) || (name.hasSuffix(".jsonl") && relative.split(separator: "/").dropLast().contains("chats"))
        }) { found.insert("gemini") }
        let workbuddy = paths["workbuddy"].flatMap { $0.isEmpty ? nil : $0 } ?? home
        var roots = [".workbuddy-ai", ".workbuddy"].map { workbuddy + "/" + $0 + "/projects" }.filter(Harness.isDirectory)
        if roots.isEmpty && workbuddy != home { roots = [Harness.isDirectory(workbuddy + "/projects") ? workbuddy + "/projects" : workbuddy] }
        if roots.contains(where: { hasRecords($0) }) { found.insert("workbuddy") }
        let authRoot = Paths.expandUser(chatgptHome ?? environment["CODEX_HOME"].flatMap { $0.isEmpty ? nil : $0 } ?? home + "/.codex")
        let auth = ClaudeApp.readObject(authRoot + "/auth.json")
        if which("codex") && (auth["auth_mode"] == .string("chatgpt") || auth["auth_mode"] == .string("chatgptAuthTokens") || auth["tokens"]?.object?["access_token"]?.isTruthy == true) {
            found.insert("chatgpt")
        }
        let config = ClaudeApp.readObject((paths["opencodex"].flatMap { $0.isEmpty ? nil : $0 } ?? home + "/.opencodex") + "/config.json")
        let provider = Py.or(config["providers"], .object(JSONObject())).object?["deepseek"]?.object ?? JSONObject()
        var baseURL = Py.str(Py.or(provider["baseUrl"], .string("")))
        while baseURL.hasSuffix("/") { baseURL.removeLast() }
        let official = ["https://api.deepseek.com", "https://api.deepseek.com/v1"].contains(baseURL)
        if credential("DEEPSEEK_API_KEY") || (official && provider["apiKey"]?.isTruthy == true) { found.insert("deepseek") }
        if credential("MINIMAX_API_KEY") { found.insert("minimax") }
        if credential("XAI_MANAGEMENT_API_KEY") && !(environment["XAI_TEAM_ID"] ?? "").trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { found.insert("grok") }
        if credential("QODER_CN_API_KEY") && !(environment["QODER_CN_ORG_ID"] ?? "").trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { found.insert("qoder-api") }
        for key in Catalog.keyOnlyAPIs where Catalog.credentialNames(key).contains(where: credential) { found.insert(key) }
        found.formUnion(configuredSubscriptions.filter(Catalog.manualSubscriptions.contains))
        if !Runtime.monitorURL(komariURL).isEmpty { found.insert("komari") }
        return Preferences.modules.map(\.key).filter(found.contains)
    }

    /// Only first installs are set up; existing choices survive upgrades and a concurrent manual choice wins.
    @discardableResult
    public static func initialize(_ directory: String, detected: [String]) -> Bool {
        let target = directory + "/preferences.json"
        if FileManager.default.fileExists(atPath: target) { return false }
        var current = Preferences.read(directory)
        let chosen = Set(detected)
        current["visible"] = .array(Preferences.modules.map(\.key).filter(chosen.contains).map(JSON.string))
        try? Files.makeDirectory(directory)
        let temporary = directory + "/.discovery-" + Files.randomName()
        guard case .created = Files.createExclusively(temporary, contents: Data(JSON.object(current).serialized().utf8)) else { return false }
        defer { Platform.remove(temporary) }
        return Platform.hardLink(temporary, target)
    }

    /// Only after the user confirms the detected selection.
    public static func replaceSelection(_ directory: String, detected: [String]) throws {
        var current = Preferences.read(directory)
        let chosen = Set(detected)
        current["visible"] = .array(Preferences.modules.map(\.key).filter(chosen.contains).map(JSON.string))
        try Preferences.write(directory, current)
    }
}

extension Runtime {
    static func dshRoot(home: String, environment: [String: String]) -> String {
        let explicit = (environment["DSH_SESSIONS_DIR"] ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        if !explicit.isEmpty { return Paths.normalize(Paths.expandUser(explicit)) }
        let configured = (environment["DSH_HOME"] ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        return (configured.isEmpty ? home + "/.dsh" : Paths.normalize(Paths.expandUser(configured))) + "/sessions"
    }

    public func detectedModules() -> [String] {
        Discovery.detect(paths: Dictionary(uniqueKeysWithValues: Preferences.usage.map { ($0, sourcePath($0)) }), home: home, configuredProxy: opencodexConfigured(),
                         komariURL: komariBaseURL, appRoots: desktop != nil ? [] : [home + "/Applications", "/Applications"], chatgptHome: codexHome(),
                         configuredSubscriptions: settings()["configuredSubscriptions"]?.array?.compactMap(\.string) ?? [], environment: environment)
    }
}
