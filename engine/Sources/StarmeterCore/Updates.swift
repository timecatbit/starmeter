import Foundation
#if canImport(FoundationXML)
import FoundationXML
#endif

/// Version checks for the apps and CLIs behind the selected Harness, and OpenCodex's own installer. Only public
/// version metadata is read; no login, no real device ID. Ports of the update parts of swiftbar_plugin.py,
/// swiftbar_update_channels.py and swiftbar_desktop_updates.py.
public enum Updates {
    /// v?1.2.3 (with an optional -suffix) as five numbers, or nil.
    public static func releaseVersion(_ value: JSON?) -> [Int]? {
        var text = Py.str(Py.or(value, .string(""))).trimmingCharacters(in: .whitespacesAndNewlines)
        if text.hasPrefix("v") { text.removeFirst() }
        let core = text.split(whereSeparator: { $0 == "-" || $0 == "+" }).first.map(String.init) ?? ""
        let rest = text.dropFirst(core.count)
        if !rest.isEmpty {
            let tail = rest.dropFirst()
            guard !tail.isEmpty, tail.allSatisfy({ $0.isASCII && ($0.isLetter || $0.isNumber || $0 == "." || $0 == "-") }) else { return nil }
        }
        let parts = core.split(separator: ".", omittingEmptySubsequences: false)
        guard (1...5).contains(parts.count), parts.allSatisfy({ !$0.isEmpty && $0.allSatisfy { $0.isASCII && $0.isNumber } }) else { return nil }
        let numbers = parts.map { Int($0) ?? 0 }
        return numbers + [Int](repeating: 0, count: 5 - numbers.count)
    }

    public static func newer(_ latest: JSON?, _ current: JSON?) -> Bool {
        guard let a = releaseVersion(latest), let b = releaseVersion(current) else { return false }
        return a.lexicographicallyPrecedes(b) == false && a != b
    }

    static func plainVersion(_ value: Any?) -> [Int]? {
        guard let text = value as? String, !text.isEmpty, text.split(separator: ".", omittingEmptySubsequences: false).count <= 5,
              text.split(separator: ".", omittingEmptySubsequences: false).allSatisfy({ !$0.isEmpty && $0.allSatisfy { $0.isASCII && $0.isNumber } }) else { return nil }
        let numbers = text.split(separator: ".").map { Int($0) ?? 0 }
        return numbers + [Int](repeating: 0, count: 5 - numbers.count)
    }

    /// An installed app's version, build and bundle ID, from /Applications or ~/Applications.
    public static func appInfo(_ name: String, home: String) -> JSONObject {
        for base in ["/Applications", home + "/Applications"] {
            guard let data = FileManager.default.contents(atPath: base + "/" + name + ".app/Contents/Info.plist"),
                  let value = try? PropertyListSerialization.propertyList(from: data, format: nil) as? [String: Any] else { continue }
            var info = JSONObject()
            for key in ["CFBundleShortVersionString", "CFBundleVersion", "CFBundleIdentifier"] {
                info[key] = .string(value[key].map { "\($0)" }.flatMap { $0.isEmpty ? nil : $0 } ?? "")
            }
            return info
        }
        return JSONObject()
    }

    public static func installedAppPath(_ name: String, home: String) -> String? {
        for base in ["/Applications", home + "/Applications"] where Harness.isDirectory(base + "/" + name + ".app") { return base + "/" + name + ".app" }
        return nil
    }

    struct CheckError: Error, CustomStringConvertible { let description: String }

    static func connectionProblem(_ message: String) -> String {
        let text = message.lowercased()
        if text.contains("certificate") || text.contains("ssl") { return "证书验证失败（Python 缺少根证书）" }
        if text.contains("timed out") { return "连接超时（检查网络或代理）" }
        if text.contains("hostname could not be found") || text.contains("cannot find host") { return "域名无法解析（检查网络或代理）" }
        if text.contains("refused") || text.contains("could not connect") { return "连接被拒绝（检查代理）" }
        return "无法连接（检查网络或代理）"
    }

    static func releaseBody(_ url: String) throws -> Data {
        let host = URLComponents(string: url)?.host ?? ""
        let response: HTTP.Response
        do {
            response = try HTTP.send(url, headers: ["User-Agent": "Mozilla/5.0 OpenCodex-SwiftBar/4.8", "Accept": "application/json, application/xml, text/plain"],
                                     timeout: 8, limit: 1024 * 1024 + 1)
        } catch let failure as HTTP.Unreachable { throw CheckError(description: host + " " + connectionProblem(failure.description)) }
        guard (200..<300).contains(response.status) else { throw CheckError(description: "\(host) 返回 HTTP \(response.status)") }
        if response.body.count > 1024 * 1024 { throw Py.FileError("release metadata too large") }
        return response.body
    }

    static var machine: String {
        var info = utsname()
        uname(&info)
        return withUnsafeBytes(of: &info.machine) { String(decoding: $0.prefix { $0 != 0 }, as: UTF8.self) }
    }

    static var macVersion: String {
        let version = ProcessInfo.processInfo.operatingSystemVersion
        return version.patchVersion == 0 ? "\(version.majorVersion).\(version.minorVersion)" : "\(version.majorVersion).\(version.minorVersion).\(version.patchVersion)"
    }

    /// The newest stable release in a Sparkle appcast this Mac can run: its version and build.
    public static func sparkleRelease(_ body: Data) -> (String?, String?) {
        final class Reader: NSObject, XMLParserDelegate {
            var items: [[String: String]] = []
            var current: [String: String]?
            var text = ""
            var depth: [String] = []
            func parser(_ parser: XMLParser, didStartElement name: String, namespaceURI: String?, qualifiedName: String?, attributes: [String: String] = [:]) {
                depth.append(name)
                text = ""
                if name == "item" && depth.count == 3 && depth[0] == "rss" && depth[1] == "channel" { current = [:] }
                if name == "enclosure", current != nil, depth.count == 4 {
                    current!["enclosure"] = "1"
                    for (key, value) in attributes where key.hasPrefix("sparkle:") { current!["enclosure:" + key.dropFirst(8)] = value }
                }
            }
            func parser(_ parser: XMLParser, foundCharacters string: String) { text += string }
            func parser(_ parser: XMLParser, didEndElement name: String, namespaceURI: String?, qualifiedName: String?) {
                if current != nil, depth.count == 4, name.hasPrefix("sparkle:"), current![String(name.dropFirst(8))] == nil {
                    current![String(name.dropFirst(8))] = text.trimmingCharacters(in: .whitespacesAndNewlines)
                }
                if name == "item", depth.count == 3, let item = current { items.append(item); current = nil }
                depth.removeLast()
            }
        }
        let parser = XMLParser(data: body)
        let reader = Reader()
        parser.delegate = reader
        guard parser.parse() else { return (nil, nil) }
        var candidates: [(String, String)] = []
        for item in reader.items {
            var version = item["shortVersionString"], build = item["version"]
            if let minimum = item["minimumSystemVersion"], !minimum.isEmpty, let need = releaseVersion(.string(minimum)), let have = releaseVersion(.string(macVersion)),
               have.lexicographicallyPrecedes(need) { continue }
            if let arch = item["hardwareRequirements"], !arch.isEmpty, !arch.split(separator: ",").map(String.init).contains(machine) { continue }
            if let channel = item["channel"], !["", "stable", "release"].contains(channel) { continue }
            if item["enclosure"] != nil {
                if version == nil || version!.isEmpty { version = item["enclosure:shortVersionString"] }
                if build == nil || build!.isEmpty { build = item["enclosure:version"] }
            }
            if let version, let build, releaseVersion(.string(version)) != nil, releaseVersion(.string(build)) != nil { candidates.append((version, build)) }
        }
        var best: (String, String)?
        for candidate in candidates where best == nil || releaseVersion(.string(best!.1))!.lexicographicallyPrecedes(releaseVersion(.string(candidate.1))!) { best = candidate }
        return best.map { ($0.0, $0.1) } ?? (nil, nil)
    }

    /// The package channel Codex came from, from its executable alone.
    public static func codexChannel(_ binary: String) -> JSONObject {
        let path = Paths.resolve(binary) ?? binary
        func object(_ file: String) -> JSONObject { ClaudeApp.readObject(file) }
        var parent = (path as NSString).deletingLastPathComponent
        while parent != "/" && !parent.isEmpty {
            let name = (parent as NSString).lastPathComponent, above = ((parent as NSString).deletingLastPathComponent as NSString).lastPathComponent
            if name == "codex" && above == "Caskroom" {
                let receipt = object(parent + "/.metadata/INSTALL_RECEIPT.json")
                if Py.dict(receipt["source"]).object?["tap"] != .string("homebrew/cask") { return ["channel": "Homebrew 自定义 Cask"] }
                return ["channel": "Homebrew Cask", "channelId": "brew-cask", "releaseURL": "https://formulae.brew.sh/api/cask/codex.json"]
            }
            if name == "codex" && above == "Cellar" {
                let receipt = object(((path as NSString).deletingLastPathComponent as NSString).deletingLastPathComponent + "/INSTALL_RECEIPT.json")
                if Py.dict(receipt["source"]).object?["tap"] != .string("homebrew/core") { return ["channel": "Homebrew 自定义 Formula"] }
                return ["channel": "Homebrew Formula", "channelId": "brew-formula", "releaseURL": "https://formulae.brew.sh/api/formula/codex.json"]
            }
            if name == "codex" && above == "@openai" && object(parent + "/package.json")["name"] == .string("@openai/codex") {
                return ["channel": "npm", "channelId": "npm", "releaseURL": "https://registry.npmjs.org/@openai/codex/latest"]
            }
            parent = (parent as NSString).deletingLastPathComponent
        }
        return ["channel": "安装渠道待确认"]
    }

    static func latestVersion(_ channel: JSONObject, _ body: Data) throws -> String {
        guard let data = (try? JSON.parse(body))?.object else { throw Py.FileError("bad metadata") }
        if channel["channelId"] == .string("brew-formula") {
            guard let stable = data["versions"]?.object?["stable"] else { throw Py.FileError("KeyError") }
            return Py.str(stable)
        }
        guard let version = data["version"] else { throw Py.FileError("KeyError") }
        return Py.str(version)
    }

    /// ChatGPT's own Sparkle metadata: a download it already staged, or the official appcast it cached.
    static func chatgptRelease(home: String, bundle: JSON?, now: Double = Date().timeIntervalSince1970) -> JSONObject? {
        guard bundle == .string("com.openai.codex") else { return nil }
        let root = home + "/Library/Caches/com.openai.codex"
        var candidates: [JSONObject] = []
        let installation = root + "/org.sparkle-project.Sparkle/Installation"
        var seen = 0
        outer: for a in ((try? FileManager.default.contentsOfDirectory(atPath: installation)) ?? []).sorted() {
            for b in ((try? FileManager.default.contentsOfDirectory(atPath: installation + "/" + a)) ?? []).sorted() {
                for app in ((try? FileManager.default.contentsOfDirectory(atPath: installation + "/" + a + "/" + b)) ?? []).sorted() where app.hasSuffix(".app") {
                    seen += 1
                    if seen > 32 { break outer }
                    guard let data = FileManager.default.contents(atPath: [installation, a, b, app, "Contents/Info.plist"].joined(separator: "/")), data.count <= 1024 * 1024,
                          let info = try? PropertyListSerialization.propertyList(from: data, format: nil) as? [String: Any] else { continue }
                    if info["CFBundleIdentifier"] as? String == "com.openai.codex", plainVersion(info["CFBundleShortVersionString"]) != nil, plainVersion(info["CFBundleVersion"]) != nil {
                        candidates.append(["latest": .string(info["CFBundleShortVersionString"] as! String), "build": .string(info["CFBundleVersion"] as! String),
                                           "releaseSource": "client-download", "observedAt": .double(now)])
                    }
                }
            }
        }
        if let feed = cachedFeed(root, now: now) { candidates.append(feed) }
        var best: JSONObject?
        for row in candidates where best == nil || plainVersion(best!["build"]?.string)!.lexicographicallyPrecedes(plainVersion(row["build"]?.string)!) { best = row }
        return best
    }

    static func cachedFeed(_ root: String, now: Double) -> JSONObject? {
        let database = root + "/Cache.db"
        guard FileManager.default.fileExists(atPath: database), let db = try? Database(readOnlyURI: "file:" + database + "?mode=ro", timeout: 0.2),
              let rows = try? db.rows("""
                SELECT r.request_key,r.time_stamp,d.isDataOnFS,d.receiver_data FROM cfurl_cache_response r JOIN cfurl_cache_receiver_data d USING(entry_ID)
                WHERE r.request_key LIKE 'https://persistent.oaistatic.com/codex-app-prod/appcast%' AND length(d.receiver_data)<=? ORDER BY r.time_stamp DESC LIMIT 12
                """, [.int(1024 * 1024)]) else { return nil }
        for row in rows {
            guard let url = row[0].text, let parts = URLComponents(string: url), parts.host == "persistent.oaistatic.com", parts.port == nil || parts.port == 443,
                  parts.user == nil, parts.password == nil else { continue }
            let path = parts.path
            guard path.hasPrefix("/codex-app-prod/appcast"), path.hasSuffix(".xml") else { continue }
            guard let stamp = row[1].text, let time = try? ISOTime.parse(stamp) else { return nil }
            var utc = time
            utc.offset = 0
            let checked = utc.timestamp()
            guard -60 <= now - checked, now - checked <= 7 * 86400 else { return nil }
            var body: Data
            if (row[2].int ?? 0) != 0 {
                let name = row[3].blob.map { String(decoding: $0, as: UTF8.self) } ?? row[3].text ?? ""
                guard (1...80).contains(name.count), name.allSatisfy({ $0.isASCII && ($0.isLetter || $0.isNumber || $0 == "-") }) else { return nil }
                let file = root + "/fsCachedData/" + name
                if (try? FileManager.default.destinationOfSymbolicLink(atPath: file)) != nil { return nil }
                guard let data = FileManager.default.contents(atPath: file), data.count <= 1024 * 1024 else { return nil }
                body = data
            } else {
                guard let blob = row[3].blob else { return nil }
                body = Data(blob)
            }
            let (latest, build) = sparkleRelease(body)
            if let latest, let build, plainVersion(latest) != nil, plainVersion(build) != nil {
                return ["latest": .string(latest), "build": .string(build), "releaseSource": "client-feed", "observedAt": .double(checked)]
            }
            return nil
        }
        return nil
    }
}

extension Runtime {
    static let updateLinks: [String: String] = ["opencodex": "https://github.com/lidge-jun/opencodex/releases/latest", "claude": "https://claude.ai/download",
        "chatgpt": "https://chatgpt.com/download", "claude-code": "https://github.com/anthropics/claude-code/releases", "codex-cli": "https://github.com/openai/codex/releases",
        "magpie": "https://github.com/yetone/magpie/releases/latest", "opencode": "https://opencode.ai", "deepseek-harness": "https://github.com/deepseek-ai/deepseek-harness/releases",
        "workbuddy": "https://cloud.tencent.com/product/workbuddy", "qoder": "https://qoder.cn/download", "gemini": "https://github.com/google-gemini/gemini-cli/releases",
        "cursor": "https://cursor.com/downloads"]
    static let updateNames: [String: String] = ["opencodex": "OpenCodex", "claude": "Claude", "chatgpt": "ChatGPT", "claude-code": "Claude Code", "codex-cli": "Codex",
        "magpie": "Magpie", "opencode": "OpenCode", "deepseek-harness": "DeepSeek Harness", "workbuddy": "WorkBuddy", "qoder": "Qoder CN", "gemini": "Gemini CLI", "cursor": "Cursor"]
    static let manualUpdates: Set<String> = ["magpie", "opencode", "deepseek-harness", "workbuddy", "qoder", "gemini", "cursor"]
    static let updateApps: [String: [String]] = ["magpie": ["magpie", "Magpie"], "claude": ["Claude"], "chatgpt": ["ChatGPT"], "workbuddy": ["WorkBuddy"],
                                                 "qoder": ["Qoder CN", "QoderCN"], "cursor": ["Cursor"]]
    static let harnessUpdates = ["opencodex": "opencodex", "magpie": "magpie", "claude": "claude", "codex": "chatgpt", "opencode": "opencode",
                                 "deepseek-harness": "deepseek-harness", "workbuddy": "workbuddy", "qoder": "qoder", "gemini": "gemini", "cursor": "cursor"]

    public func appInfo(_ name: String) -> JSONObject { memoized("app:" + name) { Updates.appInfo(name, home: home) } }

    /// The update checks the selected Harness call for; a configured OpenCodex adds ChatGPT and Codex.
    public func selectedUpdateIDs() -> [String] {
        var visible = Set(settings()["visible"]?.array?.compactMap(\.string) ?? [])
        if opencodexConfigured() || visible.contains("opencodex") { visible.insert("codex") }
        var result: [String] = []
        for key in Preferences.usage where visible.contains(key) {
            result.append(Runtime.harnessUpdates[key]!)
            if key == "claude" { result.append("claude-code") }
            if key == "codex" { result.append("codex-cli") }
        }
        return result
    }

    public func liveAppUpdateIDs() -> [String] {
        selectedUpdateIDs().filter { $0 != "opencodex" && !Runtime.manualUpdates.contains($0) }
    }

    /// A client-specific rollout or download wins over anonymous metadata.
    func applyChatGPTRelease(_ given: JSONObject, _ info: JSONObject) -> JSONObject {
        var row = given
        guard info["CFBundleIdentifier"] == .string("com.openai.codex") else { return row }
        let installed = info["CFBundleVersion"] ?? .null
        row["current"] = info["CFBundleShortVersionString"] ?? .null
        row["currentBuild"] = installed
        if let local = Updates.chatgptRelease(home: home, bundle: info["CFBundleIdentifier"]), Updates.newer(local["build"], installed) {
            if !(row["build"]?.isTruthy ?? false) || !Updates.newer(row["build"], local["build"]) {
                for (key, value) in local { row[key] = value }
                row["available"] = true; row["status"] = "有新版本"; row["checkedAt"] = .double(Date().timeIntervalSince1970)
                row["channel"] = local["releaseSource"] == .string("client-download") ? "已下载" : "客户端更新源"
            }
        } else if let source = row["releaseSource"], source == .string("client-feed") || source == .string("client-download") {
            // The app was updated or its staged download disappeared: no old alert.
            row["available"] = false; row["status"] = "等待检查"
            for key in ["latest", "build", "releaseSource", "channel"] { row[key] = nil }
        }
        if row["releaseSource"] == .string("public") {
            row["available"] = .bool(Updates.newer(row["build"], installed))
            if !Py.str(row["status"] ?? .string("")).contains("失败") { row["status"] = .string(row["available"] == .bool(true) ? "有新版本" : "公开渠道暂无更新") }
        }
        return row
    }

    func appUpdateProbe(_ key: String) -> JSONObject {
        var row: JSONObject = ["id": .string(key), "name": .string(Runtime.updateNames[key]!), "href": .string(Runtime.updateLinks[key]!),
                               "checkedAt": .double(Date().timeIntervalSince1970)]
        do {
            if key == "claude-code" || key == "codex-cli" {
                guard let binary = Shell.which(key == "claude-code" ? "claude" : "codex", environment: environment) else { row["status"] = "未安装"; return row }
                guard let output = Shell.run([binary, "--version"], timeout: 4, environment: environment) else { throw Py.FileError("SubprocessError", kind: "SubprocessError") }
                row["current"] = .string(Runtime.firstVersion(output) ?? "")
                if key == "claude-code" {
                    row["channel"] = "Claude 官方"
                    row["latest"] = .string(String(decoding: try Updates.releaseBody("https://downloads.claude.ai/claude-code-releases/latest"), as: UTF8.self)
                        .trimmingCharacters(in: .whitespacesAndNewlines))
                } else {
                    let channel = Updates.codexChannel(binary)
                    row["channel"] = channel["channel"]
                    guard let url = channel["releaseURL"]?.string else { row["status"] = "更新渠道待确认"; row["available"] = false; return row }
                    row["latest"] = .string(try Updates.latestVersion(channel, try Updates.releaseBody(url)))
                }
                guard Updates.releaseVersion(row["current"]) != nil, Updates.releaseVersion(row["latest"]) != nil else { throw Py.FileError("invalid version") }
                row["available"] = .bool(Updates.newer(row["latest"], row["current"]))
                row["status"] = .string(row["available"] == .bool(true) ? "有新版本" : "已是最新")
                return row
            }
            let info = appInfo(key == "claude" ? "Claude" : "ChatGPT")
            if info.isEmpty { row["status"] = "未安装"; return row }
            row["current"] = info["CFBundleShortVersionString"]
            // A constant anonymous checker ID meets the public feed schema without sending a device ID.
            let ident = Runtime.uuid5URL("opencodex-swiftbar-update-check")
            if key == "claude" {
                let query = "device_id=\(Accounts.formQuote(ident))&version=\(Accounts.formQuote(Py.str(row["current"]!)))&os_version=\(Accounts.formQuote(Updates.macVersion))"
                let raw = try Updates.releaseBody("https://api.anthropic.com/api/desktop/darwin/universal/squirrel/update?" + query)
                if raw.isEmpty { row["latest"] = row["current"] }
                else {
                    guard let release = (try? JSON.parse(raw))?.object?["currentRelease"] else { throw Py.FileError("KeyError") }
                    row["latest"] = release
                }
            } else {
                guard info["CFBundleIdentifier"] == .string("com.openai.codex") else { row["status"] = "更新渠道待确认"; return row }
                let query = [("installation_id", ident), ("arch", Updates.machine), ("app_version", Py.str(row["current"]!)), ("beta", "false"),
                             ("os-version", Updates.macVersion), ("plan_type", "unknown")].map { $0.0 + "=" + Accounts.formQuote($0.1) }.joined(separator: "&")
                let (latest, build) = Updates.sparkleRelease(try Updates.releaseBody("https://updates.oaistatic.com/codex/app/appcast?" + query))
                guard let latest, let build else { throw Py.FileError("no compatible release") }
                row["latest"] = .string(latest); row["build"] = .string(build); row["currentBuild"] = info["CFBundleVersion"]; row["releaseSource"] = "public"
            }
            guard Updates.releaseVersion(row["latest"]) != nil, Updates.releaseVersion(row["current"]) != nil else { throw Py.FileError("invalid version") }
            row["available"] = .bool(Updates.newer(row["latest"], row["current"]))
            if row["build"]?.isTruthy == true { row["available"] = .bool(Updates.newer(row["build"], row["currentBuild"])) }
            row["status"] = .string(row["available"] == .bool(true) ? "有新版本" : "已是最新")
        } catch {
            // The reason is shown under the row: it tells a missing certificate from a blocked network.
            let reason = (error as? Updates.CheckError)?.description ?? ((error as? Py.FileError)?.kind == "SubprocessError" ? "读取本机版本失败" : "版本信息无法识别")
            row["status"] = "检查失败，稍后重试"; row["available"] = false; row["reason"] = .string(reason)
        }
        if key == "chatgpt" { row = applyChatGPTRelease(row, appInfo("ChatGPT")) }
        return row
    }

    /// The first version number in a CLI's --version output, by the plugin's own pattern.
    public static func firstVersion(_ text: String) -> String? {
        let pattern = try! NSRegularExpression(pattern: #"\b(\d+(?:\.\d+){1,4}(?:[-+][a-zA-Z0-9.-]+)?)\b"#)
        guard let match = pattern.firstMatch(in: text, range: NSRange(text.startIndex..., in: text)), let range = Range(match.range(at: 1), in: text) else { return nil }
        return String(text[range])
    }

    /// uuid.uuid5(uuid.NAMESPACE_URL, name), the constant anonymous checker ID.
    public static func uuid5URL(_ name: String) -> String {
        let namespace: [UInt8] = [0x6b, 0xa7, 0xb8, 0x11, 0x9d, 0xad, 0x11, 0xd1, 0x80, 0xb4, 0x00, 0xc0, 0x4f, 0xd4, 0x30, 0xc8]
        var digest = Array(SHA1.hash(namespace + Array(name.utf8)).prefix(16))
        digest[6] = digest[6] & 0x0F | 0x50
        digest[8] = digest[8] & 0x3F | 0x80
        let hex = digest.hexString
        return [hex.prefix(8), hex.dropFirst(8).prefix(4), hex.dropFirst(12).prefix(4), hex.dropFirst(16).prefix(4), hex.dropFirst(20)].map(String.init).joined(separator: "-")
    }

    public func appUpdatesCached() -> (JSONObject?, String?) {
        guard moduleEnabled("updates") else { return (["apps": []], nil) }
        let selected = liveAppUpdateIDs()
        guard !selected.isEmpty else { return (["apps": []], nil) }
        let (result, error) = cachedFetch("app_updates", ttl: 21600) {
            var rows = [JSONObject?](repeating: nil, count: selected.count)
            let lock = NSLock()
            DispatchQueue.concurrentPerform(iterations: selected.count) { index in
                // Preferences can change while another check is still in flight.
                guard self.moduleEnabled("updates"), self.liveAppUpdateIDs().contains(selected[index]) else { return }
                let row = self.appUpdateProbe(selected[index])
                lock.lock(); rows[index] = row; lock.unlock()
            }
            return (["apps": .array(rows.compactMap { $0 }.map(JSON.object))], nil)
        }
        if (result?["apps"]?.array ?? []).contains(where: { Py.str($0["status"] ?? .string("")).contains("失败") }) {
            var record = readCache("app_updates")
            record["force"] = true
            record["retryAt"] = .double(Date().timeIntervalSince1970 + 600)
            writeJSONCache(cachePath("app_updates"), .object(record))
        }
        return (result, error)
    }

    // MARK: OpenCodex's own installer

    var autoUpdatePath: String { stateDir + "/auto-update.json" }

    public func autoUpdateState() -> JSONObject {
        let state = ClaudeSubscription.readState(autoUpdatePath)
        return state["schema"] == .int(1) ? state : JSONObject()
    }

    /// A notification titled Starmeter through AppleScript, as the plugin's other notices.
    public func notify(_ message: String, subtitle: String = "") {
        func literal(_ text: String) -> String { "\"" + text.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(of: "\"", with: "\\\"") + "\"" }
        var script = "display notification " + literal(message) + " with title " + literal(Runtime.product)
        if !subtitle.isEmpty { script += " subtitle " + literal(subtitle) }
        _ = Shell.run(["/usr/bin/osascript", "-e", script], timeout: 5)
    }

    /// OpenCodex's stable-channel installer only, never the other apps. The intent is saved before the request;
    /// an unclear answer is settled from the job and health endpoints, never by asking a second time.
    public func autoUpdateOpenCodex() {
        let path = autoUpdatePath
        try? Files.makeDirectory(stateDir)
        let descriptor = open(path + ".lock", O_CREAT | O_RDWR | O_APPEND, 0o600)
        guard descriptor >= 0 else { return }
        defer { close(descriptor) }
        guard flock(descriptor, LOCK_EX | LOCK_NB) == 0 else { return }
        let now = Date().timeIntervalSince1970
        var state = autoUpdateState()
        func save(_ values: JSONObject) -> Bool {
            state["schema"] = 1; state["checkedAt"] = .double(now)
            for (key, value) in values { state[key] = value }
            writeJSONCache(path, .object(state))
            return autoUpdateState() == state
        }
        func finish(_ ok: Bool, _ message: String) {
            guard save(["status": .string(ok ? "succeeded" : "failed"), "message": .string(message), "retryAt": ok ? .int(0) : .double(now + 3600)]) else { return }
            let noticeKey = Py.str(state["target"] ?? .null) + (ok ? "::success" : "::failure")
            // Every notice is titled Starmeter, so the body names the program it updated.
            if moduleEnabled("opencodex") && state["noticeKey"] != .string(noticeKey) {
                notify("OpenCodex " + message, subtitle: "OpenCodex 自动更新")
                _ = save(["noticeKey": .string(noticeKey)])
            }
            invalidateCaches(["badge"])
        }
        let active = state["status"] == .string("running")
        let record = readCache("badge"), badge = record["payload"]?.object ?? JSONObject()
        func number(_ value: JSON?) -> Double { value?.pythonFloat ?? 0 }
        if !active {
            guard moduleEnabled("opencodex") else { return }
            if now - number(state["checkedAt"]) < 30 { return }
            if now - number(record["fetchedAt"]) > 3600 || badge["updateAvailable"]?.isTruthy != true { return }
            if !Updates.newer(badge["latestVersion"], badge["currentVersion"]) { return }
            if state["target"] == badge["latestVersion"] && now < number(state["retryAt"]) { return }
        }
        let (health, error) = opencodexRequest("/healthz", timeout: 3, auth: false)
        let current = health?["version"]
        if active {
            let target = state["target"]
            let route = "/api/update/status" + (state["jobId"].flatMap { $0.isTruthy ? "?jobId=" + Accounts.quote(Py.str($0)) : nil } ?? "")
            let (result, _) = opencodexRequest(route, timeout: 5)
            let job = Py.dict(result?["job"]).object ?? JSONObject()
            let matched = job["latestVersion"] == target
            let installed = current?.isTruthy == true && target?.isTruthy == true && !Updates.newer(target, current)
            if installed {
                let healthySince = state["healthySince"].flatMap { $0.isTruthy ? $0.pythonFloat : nil } ?? now
                if (matched && job["status"] == .string("succeeded")) || now - healthySince >= 15 { finish(true, "已自动更新至 " + Py.str(current!)); return }
                _ = save(["healthySince": .double(healthySince), "message": "新版本已启动，正在确认运行状态"]); return
            }
            if matched && job["status"] == .string("failed") { finish(false, "自动更新未完成，1 小时后重试"); return }
            if now - (state["startedAt"]?.pythonFloat ?? now) > 1800 { finish(false, "更新结果暂未确认，1 小时后重新检查"); return }
            _ = save(["healthySince": 0, "message": .string(job["status"] == .string("restarting") ? "正在重启并确认版本" : "正在自动更新")])
            return
        }
        guard error == nil, let current, current.isTruthy else { return }
        let target = badge["latestVersion"]
        guard Updates.newer(target, current) else { return }
        if badge["canUpdate"] != .bool(true) {
            _ = save(["target": target ?? .null, "status": "blocked", "message": "此安装方式需手动更新", "retryAt": .double(now + 3600)]); return
        }
        let (status, statusError) = opencodexRequest("/api/update/status", timeout: 5)
        let job = Py.dict(status?["job"]).object ?? JSONObject()
        if let statusError, statusError.code != 404 { return }
        if job["status"] == .string("running") || job["status"] == .string("restarting") {
            // Track an existing installer, including one started outside this plugin.
            _ = save(["target": Py.or(job["latestVersion"], target ?? .null), "status": "running", "jobId": job["id"] ?? .null, "startedAt": .double(now),
                      "healthySince": 0, "message": "正在自动更新"])
            return
        }
        guard moduleEnabled("opencodex") else { return }
        guard save(["target": target ?? .null, "previous": current, "status": "running", "startedAt": .double(now), "healthySince": 0, "jobId": .null,
                    "message": "正在自动更新"]) else { return }
        let (run, runError) = opencodexRequest("/api/update/run", method: "POST", body: ["tag": "latest", "restart": true], timeout: 30)
        let started = Py.dict(run?["job"]).object ?? JSONObject()
        if let id = started["id"], id.isTruthy { _ = save(["jobId": id, "target": Py.or(started["latestVersion"], target ?? .null)]) }
        else if let runError {
            // A timeout may mean the proxy is already restarting: keep the intent and look again next time.
            if (400..<500).contains(runError.code) { finish(false, "自动更新暂未启动，1 小时后重试") }
            else { _ = save(["message": "正在确认更新任务"]) }
        }
    }
}

/// SHA-1, only for uuid5's fixed checker ID.
enum SHA1 {
    static func hash(_ message: [UInt8]) -> [UInt8] {
        var h: [UInt32] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0]
        var data = message
        let length = UInt64(message.count) * 8
        data.append(0x80)
        while data.count % 64 != 56 { data.append(0) }
        for shift in stride(from: 56, through: 0, by: -8) { data.append(UInt8(truncatingIfNeeded: length >> UInt64(shift))) }
        for chunk in stride(from: 0, to: data.count, by: 64) {
            var w = [UInt32](repeating: 0, count: 80)
            for i in 0..<16 { w[i] = UInt32(data[chunk + i * 4]) << 24 | UInt32(data[chunk + i * 4 + 1]) << 16 | UInt32(data[chunk + i * 4 + 2]) << 8 | UInt32(data[chunk + i * 4 + 3]) }
            for i in 16..<80 { let x = w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]; w[i] = x << 1 | x >> 31 }
            var (a, b, c, d, e) = (h[0], h[1], h[2], h[3], h[4])
            for i in 0..<80 {
                let (f, k): (UInt32, UInt32) = i < 20 ? ((b & c) | (~b & d), 0x5A827999) : i < 40 ? (b ^ c ^ d, 0x6ED9EBA1) : i < 60 ? ((b & c) | (b & d) | (c & d), 0x8F1BBCDC) : (b ^ c ^ d, 0xCA62C1D6)
                let temp = (a << 5 | a >> 27) &+ f &+ e &+ k &+ w[i]
                (e, d, c, b, a) = (d, c, b << 30 | b >> 2, a, temp)
            }
            h[0] = h[0] &+ a; h[1] = h[1] &+ b; h[2] = h[2] &+ c; h[3] = h[3] &+ d; h[4] = h[4] &+ e
        }
        return h.flatMap { word in (0..<4).map { UInt8(truncatingIfNeeded: word >> UInt32(24 - $0 * 8)) } }
    }
}
