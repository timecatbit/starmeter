import Foundation

/// Several named keys per API: secrets stay in private files, the index holds names and options only.
/// Port of swiftbar_keys.py.
public enum Keys {
    static func root(_ state: String) -> String { state + "/keys" }

    static func validLabel(_ value: String) -> Bool {
        let count = value.count
        return count >= 1 && count <= 24 && value.unicodeScalars.allSatisfy { !($0.value < 0x20 || $0.value == 0x7F || $0 == "|") }
    }

    static func isID(_ value: String) -> Bool { value.utf8.count == 8 && value.utf8.allSatisfy { (0x30...0x39).contains($0) || (0x61...0x66).contains($0) } }

    public static func load(_ state: String) -> JSONObject {
        let empty: JSONObject = ["schema": 1, "providers": .object(JSONObject()), "labels": .object(JSONObject())]
        guard let data = FileManager.default.contents(atPath: root(state) + "/index.json"), let value = (try? JSON.parse(data))?.object,
              value["schema"] == .int(1) else { return empty }
        var clean = JSONObject()
        for (provider, rows) in value["providers"]?.object ?? JSONObject() {
            clean[provider] = .array((rows.array ?? []).compactMap { row -> JSON? in
                guard let fields = row.object, let id = fields["id"], isID(Py.str(id)), let label = fields["label"]?.string, validLabel(label) else { return nil }
                return ["id": id, "label": .string(label), "options": .object(fields["options"]?.object ?? JSONObject())]
            })
        }
        var labels = JSONObject()
        for (provider, label) in value["labels"]?.object ?? JSONObject() { if let text = label.string, validLabel(text) { labels[provider] = label } }
        return ["schema": 1, "providers": .object(clean), "labels": .object(labels)]
    }

    static func save(_ state: String, _ data: JSONObject) throws {
        // load() reads a damaged index as empty; keep the original names and options before replacing it.
        try Files.keepUnreadable(root(state) + "/index.json")
        try writePrivate(root(state) + "/index.json", JSON.object(data).serialized())
    }

    static func writePrivate(_ path: String, _ text: String) throws {
        let folder = (path as NSString).deletingLastPathComponent
        try Files.makeDirectory(folder)
        #if !os(Windows)
        chmod(folder, 0o700)
        #endif
        try Files.replaceAtomically(path, with: Data(text.utf8), prefix: ".key-")
    }

    public static func label(_ value: String?) throws -> String {
        let text = (value ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        guard validLabel(text) else { throw Preferences.Invalid("invalid key name") }
        return text
    }

    public static func entries(_ state: String, _ provider: String) -> [JSONObject] {
        load(state)["providers"]?.object?[provider]?.array?.compactMap(\.object) ?? []
    }

    static func secretPath(_ state: String, _ provider: String, _ id: String) throws -> String {
        let validProvider = (2...32).contains(provider.utf8.count) && provider.utf8.allSatisfy { (0x61...0x7A).contains($0) || (0x30...0x39).contains($0) || $0 == 0x2D }
        guard validProvider, isID(id) else { throw Preferences.Invalid("invalid key id") }
        return root(state) + "/" + provider + "/" + id + ".key"
    }

    public static func secret(_ state: String, _ provider: String, _ id: String) -> String {
        guard let path = try? secretPath(state, provider, id), let data = FileManager.default.contents(atPath: path),
              let text = String(data: data, encoding: .utf8) else { return "" }
        return text.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    /// An API key as typed: 8 to 1024 printable ASCII characters.
    public static func validKey(_ value: String) -> Bool {
        (8...1024).contains(value.utf8.count) && value.utf8.allSatisfy { (0x21...0x7E).contains($0) }
    }

    @discardableResult
    public static func add(_ state: String, _ provider: String, name: String, value: String, options: JSONObject = JSONObject()) throws -> String {
        guard validKey(value) else { throw Preferences.Invalid("invalid API key") }
        var data = load(state)
        let id = (0..<4).map { _ in String(format: "%02x", UInt8.random(in: 0...255)) }.joined()
        try writePrivate(try secretPath(state, provider, id), value + "\n")
        var providers = data["providers"]?.object ?? JSONObject()
        var rows = providers[provider]?.array ?? []
        rows.append(["id": .string(id), "label": .string(try label(name)), "options": .object(options)])
        providers[provider] = .array(rows)
        data["providers"] = .object(providers)
        try save(state, data)
        return id
    }

    /// The classic single connection is "default"; its name lives in the index too.
    public static func rename(_ state: String, _ provider: String, id: String, name: String) throws {
        var data = load(state)
        if id == "default" {
            var labels = data["labels"]?.object ?? JSONObject()
            labels[provider] = .string(try label(name))
            data["labels"] = .object(labels)
        } else {
            var providers = data["providers"]?.object ?? JSONObject()
            var rows = providers[provider]?.array ?? []
            guard let index = rows.firstIndex(where: { $0["id"] == .string(id) }), var row = rows[index].object else { throw Preferences.Invalid("unknown key") }
            row["label"] = .string(try label(name))
            rows[index] = .object(row)
            providers[provider] = .array(rows)
            data["providers"] = .object(providers)
        }
        try save(state, data)
    }

    public static func remove(_ state: String, _ provider: String, id: String) throws {
        var data = load(state)
        var providers = data["providers"]?.object ?? JSONObject()
        let rows = providers[provider]?.array ?? []
        guard rows.contains(where: { $0["id"] == .string(id) }) else { throw Preferences.Invalid("unknown key") }
        providers[provider] = .array(rows.filter { $0["id"] != .string(id) })
        data["providers"] = .object(providers)
        try save(state, data)
        if let path = try? secretPath(state, provider, id) { try? FileManager.default.removeItem(atPath: path) }
    }

    public static func defaultLabel(_ state: String, _ provider: String) -> String {
        load(state)["labels"]?.object?[provider]?.string ?? "默认"
    }
}

/// An answer that was not the data: the HTTP status (0 when there was no answer), what went wrong, and the body.
public struct RequestError: Error {
    public var code: Int
    public var message: String
    public var body: JSON?
}

extension Runtime {
    public static let opencodexEndpoints: [(String, String, Bool)] = [("healthz", "/healthz", false), ("ready", "/readyz", false),
        ("quota", "/api/codex-auth/quota", true), ("accounts", "/api/codex-auth/accounts", true), ("active", "/api/codex-auth/active", true)]
    static let reasons = [400: "Bad Request", 401: "Unauthorized", 403: "Forbidden", 404: "Not Found", 405: "Method Not Allowed", 408: "Request Timeout",
                          409: "Conflict", 429: "Too Many Requests", 500: "Internal Server Error", 501: "Not Implemented", 502: "Bad Gateway",
                          503: "Service Unavailable", 504: "Gateway Timeout"]

    public func opencodexConfigured() -> Bool {
        env("OCX_BASE_URL") != nil || FileManager.default.fileExists(atPath: tokenFile) || FileManager.default.fileExists(atPath: ocxHome + "/runtime-port.json")
    }

    func adminToken() -> String? {
        for key in ["OPENCODEX_ADMIN_AUTH_TOKEN", "OCX_ADMIN_TOKEN"] {
            if let value = environment[key]?.trimmingCharacters(in: .whitespacesAndNewlines), !value.isEmpty { return value }
        }
        guard let data = FileManager.default.contents(atPath: tokenFile), let text = String(data: data, encoding: .utf8) else { return nil }
        let token = text.trimmingCharacters(in: .whitespacesAndNewlines)
        return token.isEmpty ? nil : token
    }

    var baseIsLocal: Bool { ["127.0.0.1", "localhost", "::1"].contains(URLComponents(string: base)?.host ?? "") }

    /// OpenCodex's management API: the data, or what went wrong. The admin token only travels to this Mac or over HTTPS.
    public func opencodexRequest(_ path: String, method: String = "GET", body: JSON? = nil, timeout: Double = 4, auth: Bool = true) -> (JSONObject?, RequestError?) {
        var headers = ["Accept": "application/json"]
        let payload = body.map { Data($0.serialized(ensureASCII: true).utf8) }
        if body != nil { headers["Content-Type"] = "application/json" }
        if auth {
            if !baseIsLocal && URLComponents(string: base)?.scheme != "https" { return (nil, RequestError(code: 0, message: "管理令牌只发送到本机或 HTTPS 地址", body: nil)) }
            if let token = adminToken() { headers["x-opencodex-api-key"] = token }
        }
        do {
            let response = try HTTP.send(base + path, method: method, headers: headers, body: payload, timeout: timeout, direct: baseIsLocal)
            if !(200..<300).contains(response.status) {
                var parsed: JSON?, message: String?
                if let value = try? JSON.parse(response.body) {
                    parsed = value
                    if let object = value.object {
                        let found = Py.or(object["error"], object["message"] ?? .null)
                        if let inner = found.object { message = Py.str(Py.or(inner["message"], .string(JSON.object(inner).serialized()))) }
                        else if found.isTruthy { message = Py.str(found) }
                    }
                }
                return (nil, RequestError(code: response.status, message: message ?? Runtime.reasons[response.status] ?? "HTTP \(response.status)", body: parsed))
            }
            guard !response.body.isEmpty else { return (JSONObject(), nil) }
            guard let parsed = try? JSON.parse(response.body) else { return (nil, RequestError(code: 0, message: "JSONDecodeError", body: nil)) }
            if let object = parsed.object, let error = object["error"], error.isTruthy {
                return (nil, RequestError(code: response.status, message: Py.str(error), body: parsed))
            }
            // A list or a bare value is no error, just not data: the cache calls it 数据格式异常.
            guard let object = parsed.object else { return (nil, nil) }
            return (object, nil)
        } catch let failure as HTTP.Unreachable {
            return (nil, RequestError(code: 0, message: failure.description, body: nil))
        } catch {
            return (nil, RequestError(code: 0, message: String(describing: type(of: error)), body: nil))
        }
    }

    func opencodexEndpoint(_ name: String) -> (JSONObject?, String?) {
        guard let (_, path, auth) = Runtime.opencodexEndpoints.first(where: { $0.0 == name }) else { return (nil, "unknown endpoint") }
        let (value, error) = opencodexRequest(path, timeout: name == "healthz" ? 1.5 : 3, auth: auth)
        // /readyz answers 503 while starting, with the real state in its body.
        if name == "ready", let error, let body = error.body?.object { return (body, nil) }
        return (value, error?.message)
    }

    /// The health check first; while it fails, the other endpoints wait for the next one.
    public func opencodexRefresh() {
        let (health, error) = cachedFetch("healthz", ttl: refreshTTL(30)) { opencodexEndpoint("healthz") }
        guard error == nil, health != nil else { return }
        let names = Runtime.opencodexEndpoints.map(\.0).filter { $0 != "healthz" }
        DispatchQueue.concurrentPerform(iterations: names.count) { index in
            _ = cachedFetch(names[index], ttl: refreshTTL(30)) { opencodexEndpoint(names[index]) }
        }
    }

    func usageWindow(_ start: Double, _ end: Double) throws -> JSONObject {
        let query = "range=all&since=\(Int64((start * 1000).rounded(.towardZero)))&until=\(Int64((end * 1000).rounded(.towardZero)))"
        let (value, error) = opencodexRequest("/api/usage?" + query, timeout: 12)
        guard error == nil, let value, value["error"]?.isTruthy != true else { throw Py.FileError("OpenCodex 用量读取失败") }
        return value
    }

    /// OpenCodex's own figures for a range, with its timeline for the hourly charts and heatmap.
    public func opencodexRangeUsageLive(_ key: String, now: Double) -> JSONObject {
        guard opencodexConfigured() else { return ["error": "未配置 OpenCodex"] }
        do {
            let zone = localZone()
            let (start, end) = rangeBounds(key, now: now, zone: zone)
            var payload = try usageWindow(start, end)
            func timeline(_ hours: Int, _ minutes: Int) -> JSONObject? {
                let (value, error) = opencodexRequest("/api/usage/timeline?hours=\(hours)&bucketMinutes=\(minutes)&metric=total&aggregation=sum", timeout: 12)
                guard error == nil, let value, value["truncated"]?.isTruthy != true else { return nil }
                return value
            }
            func points(_ line: JSONObject, _ body: (Double, Double) -> Void) {
                let begin = line["start"]?.number ?? 0, step = line["bucketSeconds"]?.number ?? 0
                for series in line["series"]?.array ?? [] {
                    for (index, value) in (series["points"]?.array ?? []).enumerated() {
                        body(begin + Double(index) * step, Py.or(value, .int(0)).pythonFloat ?? 0)
                    }
                }
            }
            if Runtime.shortRanges.contains(key) {
                // The timeline counts back from now, so 48 hours always reach yesterday's midnight.
                guard let line = timeline(key == "yesterday" ? 48 : 24, key == "today" ? 1 : 60) else { payload["chartUnavailable"] = true; return payload }
                let begin = Int64((start / 3600).rounded(.down)) * 3600, last = Int64((end / 3600).rounded(.down)) * 3600
                var buckets = Ordered<Int64, JSON>()
                for stamp in stride(from: begin, through: last, by: 3600) { buckets[stamp] = .int(0) }
                var activity: [Double] = []
                points(line) { stamp, value in
                    let bucket = Int64((stamp / 3600).rounded(.down)) * 3600
                    if start <= stamp && stamp <= end && value > 0 { activity.append(stamp) }
                    if let current = buckets[bucket] { buckets[bucket] = Harness.add(current, .double(value)) }
                }
                // Timeline is hour-aligned. Query both partial edges with the exact same bounds as the totals.
                for stamp in Set([begin, last]).sorted() {
                    let low = Swift.max(start, Double(stamp)), high = Swift.min(end, Double(stamp) + 3600 - 0.001)
                    if low != Double(stamp) || stamp == last || Double(stamp) < (line["start"]?.number ?? 0) {
                        buckets[stamp] = try usageWindow(low, high)["summary"]?.object?["totalTokens"] ?? .null
                    }
                }
                payload["hours"] = .array(buckets.items.sorted { $0.0 < $1.0 }.map { ["timestamp": .int($0.0), "totalTokens": $0.1] })
                if key == "today" && buckets.items.contains(where: { $0.1.isTruthy }) {
                    let minute = Int64((end / 60).rounded(.down)) * 60
                    payload["firstActivityAt"] = activity.min().map(JSON.double) ?? (start >= Double(minute) ? .double(start) : .int(minute))
                }
                payload["chartIncomplete"] = .bool(line["missingMeasurements"]?.isTruthy ?? false)
            } else if Runtime.hourlyRanges.contains(key), let line = timeline(168, 60) {
                // The chart goes by day here; these hours only fill the heatmap.
                var buckets = [Int64: Double]()
                points(line) { stamp, value in
                    if start <= stamp && stamp <= end { buckets[Int64((stamp / 3600).rounded(.down)) * 3600, default: 0] += value }
                }
                payload["hours"] = .array(buckets.keys.sorted().map { ["timestamp": .int($0), "totalTokens": .double(buckets[$0]!)] })
            }
            return payload
        } catch {
            return ["error": "记录暂不可用"]
        }
    }

    // MARK: Komari

    public var komariBaseURL: String { Runtime.monitorURL(environment["KOMARI_BASE_URL"] ?? "") }

    /// A Komari address the user set, or "" for anything that isn't a plain http(s) address (examples included).
    public static func monitorURL(_ value: String) -> String {
        var text = value.trimmingCharacters(in: .whitespacesAndNewlines)
        while text.hasSuffix("/") { text.removeLast() }
        guard let parts = URLComponents(string: text), let scheme = parts.scheme, ["http", "https"].contains(scheme),
              let host = parts.host?.lowercased(), !host.isEmpty, parts.user == nil, parts.password == nil, parts.query == nil, parts.fragment == nil,
              !host.hasSuffix(".example"), !["example.com", "example.org", "your-komari.example"].contains(host) else { return "" }
        return text
    }

    /// A public Komari endpoint through the system curl, which keeps the system's TLS and proxy settings.
    func komariPublicJSON(_ path: String, body: JSON? = nil) -> (JSONObject?, String?) {
        #if os(Windows)
        var command = ["curl.exe"]
        #else
        var command = ["/usr/bin/curl"]
        #endif
        command += ["--disable", "--silent", "--show-error", "--fail", "--connect-timeout", "2", "--max-time", "5", "--max-filesize", "4194304",
                    "--header", "Accept: application/json", komariBaseURL + path]
        if let body { command += ["--header", "Content-Type: application/json", "--data-binary", body.serialized(ensureASCII: true)] }
        guard let output = Shell.runData(command, timeout: 6) else { return (nil, "Komari 连接失败") }
        guard let payload = (try? JSON.parse(output))?.object else { return (nil, "Komari 请求超时或数据异常") }
        if payload["error"]?.isTruthy == true || payload["status"] == .string("error") { return (nil, "Komari 接口暂不可用") }
        return (payload, nil)
    }

    func komariSnapshotLive() throws -> JSONObject {
        var nodes: (JSONObject?, String?) = (nil, nil), latest: (JSONObject?, String?) = (nil, nil)
        DispatchQueue.concurrentPerform(iterations: 2) { index in
            if index == 0 { nodes = komariPublicJSON("/api/nodes") }
            else { latest = komariPublicJSON("/api/rpc2", body: ["jsonrpc": "2.0", "method": "common:getNodesLatestStatus", "params": .object(JSONObject()), "id": 1]) }
        }
        if nodes.1 != nil { throw Py.FileError("Komari 节点读取失败") }
        guard let rows = nodes.0?["data"]?.array else { throw Py.FileError("Komari 节点格式异常") }
        let states = latest.0?["result"]?.object
        let statusValid = latest.1 == nil && states != nil
        var servers: [JSON] = []
        for row in rows {
            guard let fields = row.object, fields["uuid"]?.isTruthy == true, fields["hidden"]?.isTruthy != true else { continue }
            let state = (statusValid ? fields["uuid"]?.string.flatMap { states?[$0] } : nil)?.object ?? JSONObject()
            var server = JSONObject()
            for key in ["uuid", "name", "weight", "expired_at"] { server[key] = fields[key] ?? .null }
            for key in ["online", "cpu", "ram", "ram_total", "time"] { server[key] = state[key] ?? .null }
            // An omitted online flag is unknown, never silently offline.
            if server["online"]?.bool == nil { server["online"] = .null }
            servers.append(.object(server))
        }
        return ["servers": .array(servers), "statusAvailable": .bool(statusValid)]
    }

    public func komariSnapshot() -> (JSONObject?, String?) {
        cachedFetch("komari", ttl: 30) {
            if komariBaseURL.isEmpty { return (["servers": [], "statusAvailable": false, "configured": false], nil) }
            return (try komariSnapshotLive(), nil)
        }
    }

    // MARK: DeepSeek through OpenCodex

    /// OpenCodex drops negative balances; when it has none to show, DeepSeek is asked directly and a negative kept.
    func deepseekBalanceDirect() -> JSONObject? {
        var provider: JSONObject
        var key = (environment["DEEPSEEK_API_KEY"] ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        if key.isEmpty, let file = env("DEEPSEEK_API_KEY_FILE") {
            guard let data = FileManager.default.contents(atPath: Paths.expandUser(file)), let text = String(data: data, encoding: .utf8) else { return nil }
            key = text.trimmingCharacters(in: .whitespacesAndNewlines)
        }
        if !key.isEmpty { provider = ["baseUrl": "https://api.deepseek.com", "apiKey": .string(key)] }
        else {
            guard let data = FileManager.default.contents(atPath: ocxHome + "/config.json"), let config = (try? JSON.parse(data))?.object else { return nil }
            provider = (Py.or(config["providers"], .object(JSONObject())).object?["deepseek"]).flatMap { $0.isTruthy ? $0.object : nil } ?? JSONObject()
        }
        var base = Py.str(Py.or(provider["baseUrl"], .string("")))
        while base.hasSuffix("/") { base.removeLast() }
        guard ["https://api.deepseek.com", "https://api.deepseek.com/v1"].contains(base) else { return nil }
        let secret = Py.str(Py.or(provider["apiKey"], .string(""))).trimmingCharacters(in: .whitespacesAndNewlines)
        guard !secret.isEmpty, let response = try? HTTP.send("https://api.deepseek.com/user/balance",
                                                              headers: ["Accept": "application/json", "Authorization": "Bearer " + secret], timeout: 4),
              (200..<300).contains(response.status), let body = (try? JSON.parse(response.body))?.object, let infos = body["balance_infos"]?.array else { return nil }
        let rows = infos.compactMap(\.object)
        func balance(_ row: JSONObject) -> Double? {
            var values = Ordered<String, Double>()
            for key in ["total_balance", "granted_balance", "topped_up_balance"] {
                if let value = row[key]?.pythonFloat, value >= 0 { values[key] = value }
            }
            // total_balance first; when only the split fields are filled, the largest one actually available.
            if (values["total_balance"] ?? 0) > 0 { return values["total_balance"] }
            if let largest = values.items.map(\.1).filter({ $0 > 0 }).max() { return largest }
            return values["total_balance"]
        }
        let funded = rows.indices.filter { (balance(rows[$0]) ?? 0) > 0 }
        let usd = rows.indices.first { Py.str(rows[$0]["currency"] ?? .null).uppercased() == "USD" }
        let cny = rows.indices.first { Py.str(rows[$0]["currency"] ?? .null).uppercased() == "CNY" }
        // USD with money in it first; a CNY account skips an empty USD row.
        guard let preferred = (usd.flatMap { funded.contains($0) ? $0 : nil }) ?? funded.first ?? usd ?? cny ?? rows.indices.first,
              let amount = balance(rows[preferred]) else { return nil }
        let currency = Py.str(Py.or(rows[preferred]["currency"], .string(""))).uppercased()
        let symbol = ["CNY": "¥", "USD": "$", "EUR": "€", "GBP": "£"][currency] ?? currency + " "
        let now = Int64(Date().timeIntervalSince1970 * 1000)
        return ["generatedAt": .int(now), "reports": [["provider": "deepseek", "label": "DeepSeek", "source": "swiftbar:direct-balance",
            "quota": ["customWindows": [["label": .string(String(format: "API balance (%@%.2f)", symbol, amount)), "percent": 0]], "updatedAt": .int(now)]]]]
    }

    /// The balance in a report's label: the first currency sign followed by an amount, as the Python regex finds it.
    static func reportBalanceValue(_ report: JSONObject?) -> Double? {
        let quota = Py.dict(report?["quota"]).object ?? JSONObject()
        for window in Py.or(quota["customWindows"], .array([])).array ?? [] {
            let label = Array(Py.str(Py.or(Py.dict(window).object?["label"], .string(""))))
            for (index, character) in label.enumerated() where "¥$€£".contains(character) {
                var at = index + 1
                while at < label.count, label[at].isWhitespace { at += 1 }
                var text = ""
                if at < label.count, label[at] == "-" { text.append("-"); at += 1 }
                guard at < label.count, label[at].isASCII, label[at].isNumber else { continue }
                while at < label.count, (label[at].isASCII && label[at].isNumber) || label[at] == "," { if label[at] != "," { text.append(label[at]) }; at += 1 }
                if at + 1 < label.count, label[at] == ".", label[at + 1].isASCII, label[at + 1].isNumber {
                    text.append("."); at += 1
                    while at < label.count, label[at].isASCII, label[at].isNumber { text.append(label[at]); at += 1 }
                }
                if let value = Double(text) { return value }
            }
        }
        return nil
    }

    public func providerQuotas() -> (JSONObject?, String?) {
        cachedFetch("provider_quotas", ttl: 300) {
            if env("DEEPSEEK_API_KEY") != nil || env("DEEPSEEK_API_KEY_FILE") != nil || !opencodexConfigured() {
                if let direct = deepseekBalanceDirect() { return (direct, nil) }
                return (nil, "未配置 DeepSeek API 凭据")
            }
            let (payload, error) = opencodexRequest("/api/provider-quotas", timeout: 4)
            var reports = (payload?["reports"]?.array ?? []).compactMap(\.object)
            var failure = error?.message
            let existing = reports.first { Py.str($0["provider"] ?? .string("")).lowercased() == "deepseek" }
            let value = Runtime.reportBalanceValue(existing)
            // Only call DeepSeek when the proxy has no usable balance, including a negative one.
            if value == nil || value == 0, let direct = deepseekBalanceDirect() {
                reports = reports.filter { Py.str($0["provider"] ?? .string("")).lowercased() != "deepseek" } + (direct["reports"]?.array ?? []).compactMap(\.object)
                failure = nil
            }
            if let failure { return (nil, failure) }
            if payload == nil && reports.isEmpty { return (nil, "余额暂不可用") }
            // Keep only display fields; no API credentials or provider configuration are cached.
            return (["reports": .array(reports.map { row in
                var kept = JSONObject()
                for key in ["provider", "label", "source", "quota"] where row[key] != nil { kept[key] = row[key] }
                return .object(kept)
            })], nil)
        }
    }
}
