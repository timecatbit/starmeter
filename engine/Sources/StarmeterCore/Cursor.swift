import Foundation

/// Cursor usage from the account's own usage records on cursor.com, read only after the user turns it on. The
/// login the desktop app or CLI saved is kept in memory and goes to cursor.com only; it is never stored,
/// refreshed or written back. Request and cookie shapes follow CodexBar's Cursor provider (MIT, see
/// lib/CODEXBAR_LICENSE). Port of swiftbar_cursor.py.
public enum Cursor {
    public struct Failure: Error, CustomStringConvertible { public let description: String }

    static let origin = "https://cursor.com"
    static let eventsURL = origin + "/api/dashboard/get-filtered-usage-events", summaryURL = origin + "/api/usage-summary"
    static let pageSize = 1000, maxPages = 50, encrypted = "cursor-safe-storage:"
    /// Read again on every refresh: requests still running and records that land late.
    static let overlap = 3600.0

    public static func appRoot(home: String, environment: [String: String] = ProcessInfo.processInfo.environment) -> String {
        #if os(Windows)
        return (environment["APPDATA"] ?? home + "\\AppData\\Roaming") + "\\Cursor\\User\\globalStorage"
        #else
        return home + "/Library/Application Support/Cursor/User/globalStorage"
        #endif
    }

    static func cliAuth(home: String, environment: [String: String] = ProcessInfo.processInfo.environment) -> String {
        (environment["XDG_CONFIG_HOME"].flatMap { $0.isEmpty ? nil : $0 } ?? home + "/.config") + "/cursor/auth.json"
    }

    /// The desktop app's access token, read only; never recreates Cursor's own files.
    static func appToken(_ root: String) -> String? {
        let path = root + "/state.vscdb"
        guard FileManager.default.fileExists(atPath: path), !Harness.isDirectory(path) else { return nil }
        func read(immutable: Bool) throws -> SQLValue? {
            let db = try Database(readOnlyURI: "file:" + (Paths.resolve(path) ?? path) + (immutable ? "?immutable=1" : "?mode=ro"), timeout: 2)
            return try db.first("SELECT value FROM ItemTable WHERE key='cursorAuth/accessToken' LIMIT 1")?.first
        }
        var value: SQLValue?
        do { value = try read(immutable: false) } catch {
            // An idle database can keep WAL mode with no sidecars; immutable reads it without creating them.
            if ["-wal", "-shm"].contains(where: { FileManager.default.fileExists(atPath: path + $0) }) { return nil }
            guard let found = try? read(immutable: true) else { return nil }
            value = found
        }
        switch value {
        case .text(let text)?: return text.trimmingCharacters(in: .whitespacesAndNewlines)
        case .blob(let bytes)?:
            let text = bytes.count > 1 && bytes[1] == 0 ? String(decoding: stride(from: 0, to: bytes.count - 1, by: 2).map { UInt16(bytes[$0]) | UInt16(bytes[$0 + 1]) << 8 }, as: UTF16.self)
                                                        : String(decoding: bytes, as: UTF8.self)
            return text.trimmingCharacters(in: .whitespacesAndNewlines)
        default: return nil
        }
    }

    static func fileToken(_ path: String) -> String? {
        guard let data = FileManager.default.contents(atPath: path), let value = (try? JSON.parse(data))?.object?["accessToken"]?.string else { return nil }
        return value.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    static func claims(_ token: String) -> JSONObject {
        let parts = token.split(separator: ".", omittingEmptySubsequences: false)
        guard parts.count > 1 else { return JSONObject() }
        var part = String(parts[1]).replacingOccurrences(of: "-", with: "+").replacingOccurrences(of: "_", with: "/")
        while part.count % 4 != 0 { part += "=" }
        guard let data = Data(base64Encoded: part), let object = (try? JSON.parse(data))?.object else { return JSONObject() }
        return object
    }

    /// The user ID, token and where it came from, for the first usable login: the desktop app first, then the CLI.
    static func session(home: String, now: Double = Date().timeIntervalSince1970) throws -> (String, String, String) {
        var problems = Set<String>()
        for (place, token) in [("Cursor 桌面版", appToken(appRoot(home: home))), ("Cursor CLI", fileToken(cliAuth(home: home)))] {
            guard let token, !token.isEmpty else { continue }
            if token.hasPrefix(encrypted) { problems.insert("encrypted"); continue }
            let info = claims(token)
            let subject = Py.str(Py.or(info["sub"], .string(""))).split(separator: "|", omittingEmptySubsequences: false).last.map(String.init) ?? ""
            if subject.isEmpty { problems.insert("format"); continue }
            guard let expiry = info["exp"], let seconds = Accounts.numeric(expiry), seconds > now + 60 else { problems.insert("expired"); continue }
            return (subject, token, place)
        }
        if problems.contains("expired") { throw Failure(description: "Cursor 登录已过期，请打开 Cursor 重新登录") }
        if problems.contains("encrypted") { throw Failure(description: "Cursor 登录已加密保存，暂时无法读取") }
        if problems.contains("format") { throw Failure(description: "无法识别 Cursor 登录信息") }
        throw Failure(description: "未找到 Cursor 登录，请先登录 Cursor 桌面版或 Cursor CLI")
    }

    /// POSTs the body to a cursor.com endpoint (GET when there is none) with the session cookie; no redirects.
    static func post(_ body: JSON?, user: String, token: String, url: String = eventsURL) throws -> JSONObject {
        var headers = ["Accept": "application/json", "Origin": origin, "Cookie": "WorkosCursorSessionToken=\(user)%3A%3A\(token)"]
        if body != nil { headers["Content-Type"] = "application/json" }
        let response: HTTP.Response
        do {
            response = try HTTP.send(url, method: body == nil ? "GET" : "POST", headers: headers, body: body.map { Data($0.serialized(ensureASCII: true).utf8) },
                                     timeout: 20, followRedirects: false, limit: 32 * 1024 * 1024 + 1)
        } catch { throw Failure(description: "无法连接 cursor.com") }
        switch response.status {
        case 200..<300: break
        case 301, 302, 303, 307, 308, 401: throw Failure(description: "Cursor 登录已失效，请打开 Cursor 重新登录")
        case 403: throw Failure(description: "cursor.com 拒绝读取用量记录")
        case 429: throw Failure(description: "cursor.com 请求过于频繁，稍后重试")
        default: throw Failure(description: "cursor.com 暂时不可用（HTTP \(response.status)）")
        }
        if response.body.count > 32 * 1024 * 1024 { throw Failure(description: "用量记录过大") }
        guard let value = (try? JSON.parse(response.body))?.object else { throw Failure(description: "用量记录格式异常") }
        return value
    }

    /// Every usage event between start and end, page by page; an event ID seen before (at a page seam) is skipped.
    static func events(user: String, token: String, start: Double, end: Double) throws -> ([JSONObject], Bool) {
        var rows: [JSONObject] = [], seen = Set<String>()
        for page in 1...maxPages {
            let value = try post(["page": .int(Int64(page)), "pageSize": .int(Int64(pageSize)),
                                  "startDate": .string(String(Int64(Swift.max(0, start) * 1000))), "endDate": .string(String(Int64(end * 1000)))],
                                 user: user, token: token)
            let batchValue = Py.or(value["usageEventsDisplay"], .array([]))
            guard let batch = batchValue.array else { throw Failure(description: "用量记录格式异常") }
            for row in batch {
                guard let fields = row.object else { continue }
                if let key = fields["usageEventId"], key.isTruthy {
                    if !seen.insert(Py.dumps(key)).inserted { continue }
                }
                rows.append(fields)
            }
            let total = value["totalUsageEventsCount"]
            if batch.count < pageSize || (total?.bool == nil && total?.integer.map { Int64(rows.count) >= $0 } == true) { return (rows, true) }
        }
        return (rows, false)
    }

    /// One event in the shape the shared tally reads. Cursor's input excludes cache reads and writes, and
    /// totalCents is the request at the model's list price.
    static func normalize(_ row: JSONObject) -> JSONObject? {
        let usage = row["tokenUsage"]?.object ?? JSONObject()
        guard let stampText = row["timestamp"].map(Py.str) ?? Optional("None"), let whole = Int64(stampText.trimmingCharacters(in: .whitespaces)) else { return nil }
        let cents = usage["totalCents"]
        var cost: JSON = .null
        if let cents, cents.bool == nil, let value = cents.number, value >= 0 { cost = .double((Py.number(cents).number ?? 0) / 100) }
        return ["timestamp": .double(Double(whole) / 1000), "model": .string(Py.str(Py.or(row["model"], .string("default")))),
                "session": Py.or(row["conversationId"], .null), "input": Py.number(usage["inputTokens"]), "output": Py.number(usage["outputTokens"]),
                "cached": Py.number(usage["cacheReadTokens"]), "writes": Py.number(usage["cacheWriteTokens"]), "cost": cost]
    }

    /// This billing cycle's included plan usage, as Cursor's dashboard shows it: used ÷ limit, its own percent
    /// fields only when there is no limit. Amounts are in cents.
    public static func plan(home: String, now: Double = Date().timeIntervalSince1970) throws -> JSONObject {
        let (user, token, place) = try session(home: home, now: now)
        let value = try post(nil, user: user, token: token, url: summaryURL)
        let usage = value["individualUsage"]?.object ?? JSONObject()
        func part(_ name: String) -> JSONObject { usage[name]?.object ?? JSONObject() }
        let planPart = part("plan"), overall = part("overall"), extra = part("onDemand")
        var used: JSON = .null, limit: JSON = .null, percent: JSON = .null
        for block in [planPart, overall] where Py.num(block["limit"]) > 0 {
            used = Py.number(block["used"]); limit = Py.number(block["limit"])
            percent = .double(100 * Py.num(block["used"]) / Py.num(block["limit"]))
            break
        }
        if percent.isNull, let total = planPart["totalPercentUsed"], total.bool == nil, total.number != nil { percent = Py.number(total) }
        func stamp(_ key: String) -> JSON {
            guard let text = value[key]?.string, let time = try? ISOTime.parse(text) else { return .null }
            return .double(time.timestamp())
        }
        return ["membership": .string(Py.str(Py.or(value["membershipType"], .string("")))), "unlimited": .bool(value["isUnlimited"]?.bool == true),
                "percent": percent, "usedCents": used, "limitCents": limit, "cycleStart": stamp("billingCycleStart"), "cycleEnd": stamp("billingCycleEnd"),
                "login": .string(place),
                "onDemand": ["enabled": .bool(extra["enabled"]?.bool == true), "usedCents": Py.number(extra["used"]),
                             "limitCents": Py.num(extra["limit"]) > 0 ? Py.number(extra["limit"]) : .null]]
    }

    /// Which account a kept history belongs to, without keeping the account ID itself.
    static func account(_ user: String) -> String { String(SHA256.hex(Array(("cursor:" + user).utf8)).prefix(16)) }

    /// The account's events from start to until, in one store every range shares. Only what the store lacks is
    /// asked for; a store for another account, or one a read could not finish, starts over.
    public static func refresh(_ store: JSONObject?, home: String, start: Double, until: Double, now: Double = Date().timeIntervalSince1970) throws -> JSONObject {
        let (user, token, place) = try session(home: home, now: now)
        let owner = account(user), held = store ?? JSONObject()
        var low = held["from"], high = held["through"]
        var kept = held["events"]?.array?.compactMap(\.object) ?? []
        func plain(_ value: JSON?) -> Double? { value.flatMap { $0.bool == nil ? $0.number : nil } }
        if held["account"] != .string(owner) || held["complete"]?.bool != true || held["events"]?.array == nil
            || plain(low) == nil || plain(high) == nil || plain(low)! > plain(high)! {
            low = nil; high = nil; kept = []
        }
        var complete = true
        func read(_ begin: Double, _ end: Double) throws -> [JSONObject] {
            let (rows, done) = try events(user: user, token: token, start: begin, end: end)
            complete = complete && done
            return rows.compactMap(normalize)
        }
        func when(_ event: JSONObject) -> Double { event["timestamp"]?.number ?? 0 }
        if low == nil {
            kept = try read(start, until); low = .double(start)
        } else {
            // Each read runs a second past its seam and each side keeps its own events, so whether Cursor counts
            // the ends of a window in or out never loses or doubles one.
            if start < plain(low)! {
                let edge = plain(low)!
                kept = try read(start, edge + 1).filter { when($0) < edge } + kept
                low = .double(start)
            }
            let seam = Swift.max(plain(low)!, plain(high)! - overlap)
            if until > seam { kept = kept.filter { when($0) < seam } + (try read(seam - 1, until).filter { when($0) >= seam }) }
        }
        let sorted = kept.enumerated().sorted { when($0.element) != when($1.element) ? when($0.element) < when($1.element) : $0.offset < $1.offset }.map(\.element)
        let through: JSON = high.flatMap { plain($0) }.map { until >= $0 ? JSON.double(until) : high! } ?? .double(until)
        return ["account": .string(owner), "events": .array(sorted.map(JSON.object)), "from": low ?? .double(start), "through": through,
                "complete": .bool(complete), "login": .string(place)]
    }

    /// The card's figures from the kept events; a call Cursor also reported to Magpie counts once, from Magpie.
    public static func tally(events: [JSONObject], complete: Bool, login: JSON, start: Double, end: Double, zone: TimeZone, carriers: [Carrier]?) -> JSONObject {
        var result = Harness.aggregate(events, start: start, end: end, zone: zone, price: Pricing.modelCost, carriers: carriers)
        result["source"] = "Cursor 账户用量记录"
        result["login"] = login
        if !complete { result["partial"] = true; result["warning"] = "用量记录过多，只读取了一部分" }
        return result
    }
}

extension Database {
    /// Opens a database by URI (mode=ro or immutable=1), as sqlite3.connect(uri, uri=True).
    public convenience init(readOnlyURI uri: String, timeout: Double = 2) throws {
        try self.init(path: uri, timeout: timeout, flags: .readOnlyURI)
    }
}

extension Runtime {
    /// Cursor's records for a range: every range shares one store of the account's events, so switching ranges
    /// reads nothing new unless a range reaches further back, and a refresh asks only for the time since.
    public func cursorRangeUsageLive(_ start: Double, _ end: Double, _ now: Double, _ carriers: [Carrier]) -> JSONObject {
        guard settings()["cursorUsage"]?.isTruthy == true else { return ["error": "未开启用量读取"] }
        let name = "cursor_usage_events"
        let held = readCache(name)["payload"]?.object ?? JSONObject()
        func plain(_ value: JSON?) -> Double? { value.flatMap { $0.bool == nil ? $0.number : nil } }
        let covered = plain(held["from"]).map { $0 <= start } ?? false
        // Due when the store ends more than five minutes before this range does; yesterday's range never is.
        let fresh = covered && plain(held["through"]).map { end - $0 < refreshTTL(300) } == true
        let (payload, error) = cachedFetch(name, ttl: fresh ? refreshTTL(300) : 0) {
            do { return (try Cursor.refresh(held, home: home, start: start, until: now), nil) }
            catch let failure as Cursor.Failure { return (nil, failure.description) }
        }
        // A failed read reports the reason; the range bundle keeps the last good figures with their own time.
        guard error == nil, let payload, let events = payload["events"]?.array, plain(payload["from"]).map({ $0 <= start }) == true else {
            return ["error": .string(error ?? "用量记录暂不可用")]
        }
        return Cursor.tally(events: events.compactMap(\.object), complete: payload["complete"]?.isTruthy ?? true, login: payload["login"] ?? .null,
                            start: start, end: end, zone: localZone(), carriers: carriers)
    }
}
