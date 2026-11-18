import Foundation

/// Claude Desktop's own registry of the Claude Code sessions it runs, and where every Claude Code client keeps
/// its transcripts. Never reads credentials. Port of swiftbar_claude.py.
public enum ClaudeApp {
    static func isUUID(_ text: String) -> Bool {
        let bytes = Array(text.utf8)
        guard bytes.count == 36 else { return false }
        for (index, byte) in bytes.enumerated() {
            if [8, 13, 18, 23].contains(index) { if byte != 0x2D { return false }; continue }
            guard (0x30...0x39).contains(byte) || (0x61...0x66).contains(byte) || (0x41...0x46).contains(byte) else { return false }
        }
        return true
    }

    /// A small JSON object file, or an empty object when it is missing, too large or not an object.
    public static func readObject(_ path: String, limit: Int = 10 * 1024 * 1024) -> JSONObject {
        guard let handle = FileHandle(forReadingAtPath: path) else { return JSONObject() }
        defer { try? handle.close() }
        guard let data = try? handle.read(upToCount: limit + 1), data.count <= limit,
              let object = (try? JSON.parse(data))?.object else { return JSONObject() }
        return object
    }

    public static func defaultRoot(home: String, environment: [String: String] = ProcessInfo.processInfo.environment) -> String {
        #if os(Windows)
        let roaming = environment["APPDATA"] ?? home + "\\AppData\\Roaming"
        let local = environment["LOCALAPPDATA"] ?? home + "\\AppData\\Local"
        var roots = [roaming + "\\Claude"]
        let packages = local + "\\Packages"
        for name in ((try? FileManager.default.contentsOfDirectory(atPath: packages)) ?? []).sorted()
        where name.hasPrefix("Claude_") || name.hasPrefix("AnthropicPBC.Claude_") {
            roots.append(packages + "\\" + name + "\\LocalCache\\Roaming\\Claude")
        }
        var best: (Date, String)?
        for root in roots {
            let stamps = ["config.json", "claude-code-sessions", "plan-usage-history.json"].compactMap {
                (try? FileManager.default.attributesOfItem(atPath: root + "\\" + $0))?[.modificationDate] as? Date
            }
            if let newest = stamps.max(), best == nil || newest > best!.0 { best = (newest, root) }
        }
        return best?.1 ?? roots[0]
        #else
        return home + "/Library/Application Support/Claude"
        #endif
    }

    /// Only the app's registry, never recursive scans of Cowork workspaces.
    static func registryFiles(_ root: String) -> [String] {
        let base = root + "/claude-code-sessions"
        let account = readObject(root + "/config.json")["lastKnownAccountUuid"]?.string
        func children(_ path: String) -> [String] {
            ((try? FileManager.default.contentsOfDirectory(atPath: path)) ?? []).map { path + "/" + $0 }
        }
        let folders: [String]
        if let account, isUUID(account) { folders = children(base + "/" + account) }
        else { folders = children(base).flatMap(children) }
        return Harness.sortedPaths(folders.flatMap(children).filter { $0.hasSuffix(".json") && !Harness.isDirectory($0) })
    }

    static func registeredIDs(_ root: String) -> Set<String> {
        var ids = Set<String>()
        for path in registryFiles(root) {
            let row = readObject(path)
            guard row["sessionId"]?.isTruthy == true else { continue }
            var candidates = ["sessionId", "cliSessionId", "unarchivedCliSessionId", "historyOnlyCliSessionId"].compactMap { row[$0] }
            if let previous = row["previousCliSessionIds"]?.array { candidates += previous }
            ids.formUnion(candidates.compactMap(\.string).filter(isUUID))
        }
        return ids
    }

    /// The ccusage-compatible folders where every Claude Code client keeps its transcripts.
    public static func projectRoots(home: String, environment: [String: String] = ProcessInfo.processInfo.environment) -> [String] {
        let configured = (environment["CLAUDE_CONFIG_DIR"] ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        var roots: [String]
        if !configured.isEmpty {
            roots = configured.split(separator: ",").map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }.filter { !$0.isEmpty }
                .map { Paths.normalize(Paths.expandUser($0)) }
        } else {
            let xdg = environment["XDG_CONFIG_HOME"].flatMap { $0.isEmpty ? nil : $0 } ?? home + "/.config"
            roots = [home + "/.claude", Paths.normalize(xdg) + "/claude"]
        }
        var seen = Set<String>(), result: [String] = []
        for root in roots {
            let projects = (root as NSString).lastPathComponent == "projects" ? root : root + "/projects"
            if seen.insert(projects).inserted { result.append(projects) }
        }
        return result
    }

    /// Every transcript that counts: all project folders, plus the app's own folder for sessions it registered.
    public static func transcriptFiles(root: String, home: String, environment: [String: String] = ProcessInfo.processInfo.environment) -> (paths: [String], ids: Set<String>) {
        let ids = registeredIDs(root)
        var paths = Set<String>()
        for base in projectRoots(home: home, environment: environment) where Harness.isDirectory(base) {
            paths.formUnion(Harness.walk(base).filter { $0.hasSuffix(".jsonl") })
        }
        let app = root + "/claude-code-sessions"
        if !ids.isEmpty, Harness.isDirectory(app) {
            for path in Harness.walk(app) where path.hasSuffix(".jsonl") {
                let relative = path.dropFirst(app.count + 1).split(separator: "/").map(String.init)
                let stem = ((relative.last ?? "") as NSString).deletingPathExtension
                if ids.contains(stem) || relative.dropLast().contains(where: ids.contains) { paths.insert(path) }
            }
        }
        return (Harness.sortedPaths(Array(paths)), ids)
    }

    /// The app's title and reasoning effort for each session it touched since `since`, by transcript session ID.
    public static func sessionSettings(root: String, since: Double) -> [String: (title: String?, effort: String?)] {
        var found: [String: (title: String?, effort: String?)] = [:]
        for path in registryFiles(root) {
            guard let state = ClaudeUsage.state(of: path), Double(state.mtime) / 1e9 >= since else { continue }
            let row = readObject(path)
            guard let session = row["cliSessionId"]?.string, isUUID(session) else { continue }
            let title = row["title"]?.string?.trimmingCharacters(in: .whitespacesAndNewlines)
            found[session] = (title?.isEmpty == false ? title : nil, row["effort"]?.string.flatMap { $0.isEmpty ? nil : $0 })
        }
        return found
    }
}
