import Foundation

/// The plugin's own updates from its GitHub releases, and notices of new app versions.
/// Port of the self-update and notify_app_updates parts of swiftbar_plugin.py.
extension Runtime {
    public static let repository = "https://github.com/timecatbit/starmeter"
    static let releasesAPI = "https://api.github.com/repos/timecatbit/starmeter/releases/latest"
    static let author = "minicoke"

    struct SelfUpdateError: Error, CustomStringConvertible {
        let description: String
        init(_ description: String) { self.description = description }
    }

    var pluginDir: String { (Runtime.absolute(plugin) as NSString).deletingLastPathComponent }

    static var userAgent: [String: String] { ["User-Agent": "Starmeter/" + version] }

    func download(_ url: String, limit: Int) throws -> Data {
        let response = try HTTP.send(url, headers: Runtime.userAgent, timeout: 30, limit: limit + 1)
        guard (200..<300).contains(response.status) else { throw HTTP.Status(code: response.status, body: Data()) }
        if response.body.count > limit { throw SelfUpdateError("安装包大小异常") }
        return response.body
    }

    static func search(_ pattern: String, _ text: String) -> [String]? {
        guard let expression = try? NSRegularExpression(pattern: pattern),
              let match = expression.firstMatch(in: text, range: NSRange(text.startIndex..., in: text)) else { return nil }
        return (0..<match.numberOfRanges).map { index in Range(match.range(at: index), in: text).map { String(text[$0]) } ?? "" }
    }

    /// A release found through the public pages when the anonymous API is rate-limited. The asset list the release
    /// page loads names each file with the SHA-256 GitHub computed, as the API would.
    func selfUpdateWeb() -> JSONObject? {
        do {
            let answer = try HTTP.send(Runtime.repository + "/releases/latest", headers: Runtime.userAgent, timeout: 8, followRedirects: false)
            let page = (300..<400).contains(answer.status) ? answer.header("Location") ?? "" : Runtime.repository + "/releases/latest"
            let escaped = NSRegularExpression.escapedPattern(for: Runtime.repository)
            guard let match = Runtime.search("^" + escaped + "/releases/tag/v(\\d+\\.\\d+\\.\\d+)$", page) else { return nil }
            let version = match[1]
            guard let latest = Runtime.versionTuple(.string(version)), let current = Runtime.versionTuple(.string(Runtime.version)) else { return nil }
            if !current.lexicographicallyPrecedes(latest) { return ["latest": .string(version), "page": .string(page), "assets": []] }
            let listing = String(decoding: try download(Runtime.repository + "/releases/expanded_assets/v" + version, limit: 1024 * 1024), as: UTF8.self)
            let folder = (URLComponents(string: Runtime.repository)?.path ?? "") + "/releases/download/v" + version + "/"
            var assets: [JSON] = []
            for row in listing.components(separatedBy: "<li").dropFirst() {
                guard let link = Runtime.search("href=\"" + NSRegularExpression.escapedPattern(for: folder) + "([\\w.+-]+)\"", row),
                      let digest = Runtime.search("sha256:([0-9a-f]{64})\\b", row) else { continue }
                assets.append(["name": .string(link[1]), "url": .string(Runtime.repository + "/releases/download/v\(version)/\(link[1])"), "digest": .string("sha256:" + digest[1])])
            }
            guard assets.contains(where: { $0.object?["name"] == .string(updateAssetName(version)) }) else { return nil }
            return ["latest": .string(version), "page": .string(page), "assets": .array(assets)]
        } catch {
            return nil
        }
    }

    /// The latest GitHub release of this plugin: a plain GET with no identifiers. A draft or pre-release offers nothing.
    public func selfUpdateLoad() -> (JSONObject?, String?) {
        let response: HTTP.Response
        do {
            response = try HTTP.send(Runtime.releasesAPI, headers: Runtime.userAgent.merging(["Accept": "application/vnd.github+json"]) { $1 }, timeout: 8,
                                     limit: 1024 * 1024 + 1)
        } catch { return (nil, "检查失败") }
        if !(200..<300).contains(response.status) {
            if [403, 429].contains(response.status), let release = selfUpdateWeb() { return (release, nil) }
            return response.status == 404 ? (["latest": .null], nil) : (nil, "检查失败")
        }
        guard response.body.count <= 1024 * 1024, let release = (try? JSON.parse(response.body))?.object else { return (nil, "发布信息无法读取") }
        guard let latest = Runtime.versionTuple(release["tag_name"]), release["draft"]?.isTruthy != true, release["prerelease"]?.isTruthy != true else {
            return (["latest": .null], nil)
        }
        var page = Py.str(Py.or(release["html_url"], .string("")))
        if !page.hasPrefix(Runtime.repository + "/releases/") { page = Runtime.repository + "/releases" }
        let assets: [JSON] = (release["assets"]?.array ?? []).compactMap(\.object).map { asset in
            ["name": .string(Py.str(Py.or(asset["name"], .string("")))), "url": .string(Py.str(Py.or(asset["browser_download_url"], .string("")))),
             "digest": .string(Py.str(Py.or(asset["digest"], .string("")))), "size": asset["size"] ?? .null]
        }
        return (["latest": .string(latest.map(String.init).joined(separator: ".")), "page": .string(page), "assets": .array(assets)], nil)
    }

    func selfUpdateCached() { _ = cachedFetch("self_update", ttl: Runtime.externalTTLs["self_update"]!) { selfUpdateLoad() } }

    /// One release from this repository only: its published SHA-256 checked, unpacked privately, and the installer
    /// it ships run. Backups and the user's settings are the installer's job.
    func installRelease(_ version: String, log: String) throws {
        var release = pluginUpdate(["self_update": .object(readCache("self_update")["payload"]?.object ?? JSONObject())])
        if release?["latest"] != .string(version) {
            let (fresh, _) = selfUpdateLoad()
            release = pluginUpdate(["self_update": .object(fresh ?? JSONObject())])
        }
        guard let release, release["latest"] == .string(version) else { throw SelfUpdateError("找不到版本 " + version + " 的发布") }
        let name = "token-monitor-\(version).zip", expected = Runtime.repository + "/releases/download/v\(version)/\(name)"
        let asset = (release["assets"]?.array ?? []).compactMap(\.object).last { $0["name"] == .string(name) }
        guard let asset, asset["url"] == .string(expected) else { throw SelfUpdateError("发布中没有安装包") }
        // GitHub computes each asset's SHA-256 itself; releases publish no separate checksum files.
        let published = Py.str(Py.or(asset["digest"], .string("")))
        guard published.hasPrefix("sha256:") else { throw SelfUpdateError("发布中没有校验值") }
        let digest = String(published.dropFirst(7))
        guard digest.utf8.count == 64, digest.utf8.allSatisfy({ (0x30...0x39).contains($0) || (0x61...0x66).contains($0) }) else { throw SelfUpdateError("校验值格式错误") }
        let archive: Data
        do { archive = try download(expected, limit: 50 * 1024 * 1024) } catch let error as SelfUpdateError { throw error } catch { throw SelfUpdateError("安装失败：下载失败") }
        guard SHA256.hex(Array(archive)) == digest else { throw SelfUpdateError("安装包校验失败") }
        let folder = NSTemporaryDirectory() + "starmeter-update-" + Files.randomName()
        do {
            try Files.makeDirectory(folder)
            defer { try? FileManager.default.removeItem(atPath: folder) }
            let path = folder + "/" + name
            try archive.write(to: URL(fileURLWithPath: path))
            let root = "token-monitor-\(version)/"
            guard let listing = Shell.run(["/usr/bin/zipinfo", "-1", path], timeout: 60) else { throw SelfUpdateError("安装失败：BadZipFile") }
            if listing.split(separator: "\n").contains(where: { !$0.hasPrefix(root) || $0.split(separator: "/", omittingEmptySubsequences: false).contains("..") }) {
                throw SelfUpdateError("安装包结构异常")
            }
            // ditto keeps the panel's executable bits.
            guard Shell.run(["/usr/bin/ditto", "-x", "-k", path, folder], timeout: 120) != nil else { throw SelfUpdateError("安装失败：CalledProcessError") }
            let unpacked = folder + "/" + root
            let installer = FileManager.default.isExecutableFile(atPath: unpacked + "starmeter-install")
                ? [unpacked + "starmeter-install"]
                : [Shell.which("python3", environment: environment) ?? "/usr/bin/python3", unpacked + "install.py"]
            let process = Process()
            process.executableURL = URL(fileURLWithPath: installer[0])
            process.arguments = Array(installer.dropFirst()) + ["--plugin-dir", pluginDir, "--no-refresh"]
            let output = Pipe(), errors = Pipe()
            process.standardOutput = output; process.standardError = errors; process.standardInput = FileHandle.nullDevice
            try process.run()
            var out = Data(), err = Data()
            let group = DispatchGroup()
            group.enter(); DispatchQueue.global().async { out = output.fileHandleForReading.readDataToEndOfFile(); group.leave() }
            group.enter(); DispatchQueue.global().async { err = errors.fileHandleForReading.readDataToEndOfFile(); group.leave() }
            if group.wait(timeout: .now() + 600) == .timedOut { process.terminate(); throw SelfUpdateError("安装失败：TimeoutExpired") }
            process.waitUntilExit()
            try Files.makeDirectory((log as NSString).deletingLastPathComponent)
            let text = "Starmeter \(Runtime.version) → \(version)\nexit \(process.terminationStatus)\n\n" + String(decoding: out, as: UTF8.self) + "\n" + String(decoding: err, as: UTF8.self)
            Platform.remove(log)
            guard case .created = Files.createExclusively(log, contents: Data(text.utf8)) else { throw SelfUpdateError("安装失败：OSError") }
            if process.terminationStatus != 0 { throw SelfUpdateError("安装程序退出码 \(process.terminationStatus)") }
        } catch let error as SelfUpdateError { throw error } catch { throw SelfUpdateError("安装失败：OSError") }
    }

    /// Detached from the menu run: one update at a time, then the panel and SwiftBar reload.
    @discardableResult
    public func selfUpdateWorker(_ version: String) -> Int32 {
        try? Files.makeDirectory(stateDir)
        guard let lock = Platform.lock(stateDir + "/self-update.lock", wait: false) else { return 0 }
        defer { withExtendedLifetime(lock) {} }
        setSelfUpdateState(["status": "running", "version": .string(version)])
        do {
            try installRelease(version, log: stateDir + "/logs/self-update.log")
            setSelfUpdateState(["status": "done", "version": .string(version)]); invalidateCaches(["self_update"])
            // The running panel is the old build; it reopens from the new one on the next click.
            _ = Shell.run(["/usr/bin/osascript", "-e", "if application id \"cc.opencodex.swiftbar.panel\" is running then tell application id \"cc.opencodex.swiftbar.panel\" to quit"], timeout: 8)
            notify("插件已更新到 " + version, subtitle: "Starmeter 插件更新")
        } catch {
            let reason = (error as? SelfUpdateError)?.description ?? "更新失败"
            setSelfUpdateState(["status": "failed", "version": .string(version), "reason": .string(reason.isEmpty ? "更新失败" : reason)])
            notify("插件更新失败：" + (reason.isEmpty ? "未知原因" : reason), subtitle: "详情见 SwiftBarToken/logs/self-update.log")
        }
        refreshSwiftBar()
        return 0
    }

    // MARK: New app versions

    func sendUpdateNotification(_ rows: [JSONObject]) -> Bool {
        let body = rows.map { Py.str($0["name"] ?? .null) + " " + Py.str($0["latest"] ?? .null) }.joined(separator: "；")
        if macApp { return postNotice("应用有新版本", body) }
        if let desktop { desktop.notify(body, subtitle: "应用有新版本"); return true }
        let query = [("plugin", swiftbarName), ("title", "应用有新版本"), ("body", body), ("subtitle", "打开星星 → Harness 更新查看"), ("silent", "true"),
                     ("href", "swiftbar://refreshplugin?name=" + Accounts.quote(swiftbarName))].map { $0.0 + "=" + Accounts.formQuote($0.1) }.joined(separator: "&")
        return Shell.run(["/usr/bin/open", "-g", "swiftbar://notify?" + query], timeout: 5) != nil
    }

    /// Each new version is told once; what was told is kept in updates-notified.json.
    func notifyAppUpdates(_ rows: [JSONObject]) {
        guard moduleEnabled("updates") else { return }
        let selected = Set(selectedUpdateIDs())
        let rows = rows.filter { selected.contains(Py.str($0["id"] ?? .null)) }
        if rows.isEmpty { return }
        let path = stateDir + "/updates-notified.json"
        try? Files.makeDirectory(stateDir)
        guard let lock = Platform.lock(stateDir + "/updates.lock", wait: true) else { return }
        defer { withExtendedLifetime(lock) {} }
        var seen = JSONObject()
        if let data = FileManager.default.contents(atPath: path), let value = (try? JSON.parse(data))?.object {
            for (key, entry) in value {
                if let text = entry.string { seen[key] = [.string(text)] } else if case .array = entry { seen[key] = entry }
            }
        }
        let now = Date().timeIntervalSince1970
        let fresh = rows.filter { row in
            let id = Py.str(row["id"] ?? .null)
            guard id != "opencodex", row["available"]?.isTruthy == true, let latest = row["latest"], latest.isTruthy else { return false }
            guard now - (Py.or(row["checkedAt"], .int(0)).number ?? 0) < Runtime.externalTTLs["app_updates"]! else { return false }
            let version = Py.or(row["build"], latest)
            return !(seen[id]?.array ?? []).contains(version)
        }
        if !fresh.isEmpty && sendUpdateNotification(fresh) {
            for row in fresh {
                let id = Py.str(row["id"] ?? .null)
                seen[id] = .array((seen[id]?.array ?? []) + [Py.or(row["build"], row["latest"] ?? .null)])
            }
            writeJSONCache(path, .object(seen))
        }
    }

    /// What the background worker does once its reads are in: OpenCodex's own update, new-version notices, alerts.
    func afterReads() {
        if (desktop?.localProxy(base) ?? true) && opencodexConfigured() && (moduleEnabled("opencodex") || autoUpdateState()["status"] == .string("running")) {
            autoUpdateOpenCodex()
        }
        if moduleEnabled("updates") { notifyAppUpdates(updateRows(JSONObject())) }
        checkUsageAlerts()
    }
}
