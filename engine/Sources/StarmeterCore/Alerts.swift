import Foundation

/// Usage alerts, adapted from magpie's quota_alert (MIT, github.com/yetone/magpie). Port of swiftbar_alerts.py.
///
/// A subscription window used past the share the user chose, an API balance at or under the line the user chose,
/// and (if turned on) a window of a day or more renewing soon with little used. Each is said once; what was said is
/// kept in alerts.json. A reading that failed or is no longer current neither alerts nor clears.
public enum Alerts {
    static let sameRun = 600.0
    static let keep = 40.0 * 86400
    static let reminderUnder = 85
    static let reminderSpan = 86400.0
    static let reminderLead = 12 * 3600
    static let atOnce = 5

    static func sameRun(_ mark: JSONObject, _ resetsAt: JSON, _ now: Double) -> Bool {
        let until = mark["until"] ?? .null
        guard let end = until.number, let resets = resetsAt.number else { return until.isNull && resetsAt.isNull }
        return abs(resets - end) <= sameRun || now < end + sameRun
    }

    /// What the readings call for. Marks of what no longer applies are dropped; a returned alert is marked only by told().
    public static func due(windows: [JSONObject], balances: [JSONObject], marks: inout JSONObject, percent: Double, reminder: Bool, now: Double) -> [JSONObject] {
        var out: [JSONObject] = [], seen = Set<String>()
        func alert(_ fields: JSONObject, _ kind: String, _ key: String, _ mark: JSONObject) -> JSONObject {
            var alert = fields
            alert["kind"] = .string(kind); alert["mark"] = [.string(key), .object(mark)]
            return alert
        }
        for window in windows {
            let id = Py.str(window["id"] ?? .null), key = "w|" + id, renewal = "r|" + id
            seen.formUnion([key, renewal])
            guard window["current"]?.isTruthy == true else { continue }
            let resets = window["resetsAt"] ?? .null, used = window["used"]?.number ?? 0
            if percent <= 0 || used < percent { marks[key] = nil }
            else if !(marks[key]?.object.map { sameRun($0, resets, now) } ?? false) {
                out.append(alert(window, "usage", key, ["at": .double(now), "until": resets]))
            }
            if reminder, resets.isTruthy, let end = resets.number, (Py.or(window["span"], .int(0)).number ?? 0) >= reminderSpan, used < Double(reminderUnder),
               now < end, end <= now + Double(reminderLead), !(marks[renewal]?.object.map { sameRun($0, resets, now) } ?? false) {
                out.append(alert(window, "renews", renewal, ["at": .double(now), "until": resets]))
            }
        }
        for balance in balances {
            let key = "b|" + Py.str(balance["id"] ?? .null)
            seen.insert(key)
            guard balance["current"]?.isTruthy == true else { continue }
            if balance["line"]?.isTruthy != true || (balance["amount"]?.number ?? 0) > (balance["line"]?.number ?? 0) { marks[key] = nil }
            else if marks[key] == nil { out.append(alert(balance, "balance", key, ["at": .double(now)])) }
        }
        for (key, mark) in Array(marks) where !seen.contains(key) && now - (mark.object?["at"]?.number ?? 0) > keep { marks[key] = nil }
        return out
    }

    /// Mark the alert as said, so it isn't said again this run.
    public static func told(_ marks: inout JSONObject, _ alert: JSONObject) {
        guard let mark = alert["mark"]?.array, mark.count == 2, let key = mark[0].string else { return }
        marks[key] = mark[1]
    }

    public static func load(_ state: String) -> JSONObject {
        guard let raw = FileManager.default.contents(atPath: state + "/alerts.json"), let value = (try? JSON.parse(raw))?.object,
              value["schema"] == .int(1), let marks = value["marks"]?.object else { return JSONObject() }
        var kept = JSONObject()
        for (key, mark) in marks {
            guard let fields = mark.object, let at = fields["at"] else { continue }
            switch at { case .int, .double, .bool: kept[key] = mark; default: continue }
        }
        return kept
    }

    public static func save(_ state: String, _ marks: JSONObject) throws {
        try Files.makeDirectory(state)
        let document: JSON = ["schema": 1, "marks": .object(marks)]
        try Files.replaceAtomically(state + "/alerts.json", with: Data(document.serialized().utf8), prefix: ".alerts-")
    }
}

extension Runtime {
    static let freshReading = 900.0

    func readingCurrent(_ payload: JSONObject?, _ error: String?, now: Double = Date().timeIntervalSince1970) -> Bool {
        guard let payload, !payload.isEmpty else { return false }
        return !Runtime.refreshFailed(error) && now - (Py.or(payload["_fetchedAt"], .int(0)).number ?? 0) <= Runtime.freshReading
    }

    /// Every subscription window the homepage shows, as the usage alerts read it.
    public func alertWindows(_ data: JSONObject, now: Double = Date().timeIntervalSince1970) -> [JSONObject] {
        var out: [JSONObject] = []
        func add(_ source: String, _ name: String, _ window: String, _ used: Double, _ resetsAt: JSON, _ span: JSON, _ current: Bool) {
            out.append(["id": .string(source + "|" + window), "name": .string(name), "window": .string(window), "used": .double(used),
                        "resetsAt": resetsAt, "span": span, "current": .bool(current)])
        }
        func difference(_ end: JSON?, _ start: JSON?) -> JSON {
            guard let end, let start else { return .null }
            if case .int(let a) = end, case .int(let b) = start { return .int(a - b) }
            return .double((end.number ?? 0) - (start.number ?? 0))
        }
        if moduleEnabled("claude-subscription") {
            let payload = object(data, "claude_subscription")
            let current = payload["fetchError"]?.isTruthy != true && !Runtime.refreshFailed(failure(data, "claude_subscription")) && !Runtime.claudeUsageStale(payload)
            for row in payload["windows"]?.array ?? [] {
                let window = row.object ?? JSONObject()
                var resets: JSON = .null
                if let raw = window["resetsAt"], raw.isTruthy, let text = raw.string, let time = try? ISOTime.parse(text) { resets = .double(time.timestamp(assuming: localZone())) }
                var span: JSON = .null
                if let value = window["span"], value.isTruthy { span = value }
                else if let seconds = Runtime.claudeSpan(window) { span = .int(Int64(seconds)) }
                add("claude-subscription", "Claude 订阅", Py.str(window["label"] ?? .null), window["used"]?.pythonFloat ?? 0, resets, span, current)
            }
        }
        if moduleEnabled("chatgpt") && chatgptSource() == "proxy" {
            let current = readingCurrent(data["quota"]?.object, proxyQuotaError(data), now: now)
            for (name, used, _) in accountUsage(data) { if let used { add("chatgpt-account", "ChatGPT 订阅", name, used, .null, .null, current) } }
        } else if moduleEnabled("chatgpt") {
            let payload = object(data, "chatgpt"), current = readingCurrent(payload, failure(data, "chatgpt"), now: now)
            for row in payload["windows"]?.array ?? [] {
                let window = row.object ?? JSONObject()
                let label = Runtime.chatgptWindowLabel(window)
                var span: JSON = .null
                switch Py.or(window["minutes"], .int(0)) {
                case .int(let minutes) where minutes != 0: span = .int(minutes * 60)
                case .double(let minutes) where minutes != 0: span = .double(minutes * 60)
                default: break
                }
                add("chatgpt", "ChatGPT 订阅", label.hasPrefix("ChatGPT · ") ? String(label.dropFirst("ChatGPT · ".count)) : label,
                    100 - (window["remaining"]?.number ?? 0), window["resetsAt"] ?? .null, span, current)
            }
        }
        if moduleEnabled("minimax") {
            let payload = object(data, "minimax"), current = readingCurrent(payload, failure(data, "minimax"), now: now)
            for row in payload["windows"]?.array ?? [] {
                let window = row.object ?? JSONObject()
                var span: JSON = .null
                if let starts = window["startsAt"], starts.isTruthy, let ends = window["resetsAt"], ends.isTruthy { span = difference(ends, starts) }
                else if Py.str(window["name"] ?? .string("")).hasSuffix("本周额度") { span = .int(7 * 86400) }
                add("minimax", "MiniMax 订阅", Py.str(window["name"] ?? .null), 100 - (window["remaining"]?.number ?? 0), window["resetsAt"] ?? .null, span, current)
            }
        }
        if moduleEnabled("qoder-subscription") {
            // The App writes its summary only when the user opens Credits usage; past 10 minutes it isn't current.
            let payload = object(data, "qoder_subscription")
            let current = now - (Py.or(payload["observedAt"], .int(0)).number ?? 0) <= 600
            for (field, window) in [("plan", "套餐 Credits"), ("addOn", "加购 Credits")] {
                guard let part = payload[field]?.object, let total = part["total"], total.isTruthy else { continue }
                add("qoder-subscription", "Qoder CN 订阅", window, 100 * (part["used"]?.number ?? 0) / (total.number ?? 1),
                    field == "plan" ? payload["resetsAt"] ?? .null : .null, .null, current)
            }
        }
        if moduleEnabled("cursor-subscription") && settings()["cursorUsage"]?.isTruthy == true {
            let payload = object(data, "cursor_subscription")
            if let percent = payload["percent"], !percent.isNull {
                let end = payload["cycleEnd"], start = payload["cycleStart"]
                let span = end?.isTruthy == true && start?.isTruthy == true ? difference(end, start) : .null
                add("cursor-subscription", "Cursor 订阅", "本周期", percent.pythonFloat ?? 0, end ?? .null, span,
                    readingCurrent(payload, failure(data, "cursor_subscription"), now: now))
            }
        }
        return out
    }

    /// One balance per account of each balance API, with the line set for its currency.
    public func alertBalances(_ data: JSONObject, now: Double = Date().timeIntervalSince1970) -> [JSONObject] {
        var out: [JSONObject] = []
        let lines = settings()["balanceAlerts"]?.object ?? JSONObject()
        for provider in Catalog.balanceAPIs where moduleEnabled(provider) {
            let counted = Runtime.countedValues(apiValues(provider, data).1.filter { $0["kind"] == .string("balance") })
            for value in counted {
                let account = (value["account"]?.array ?? []).map { Py.str($0) }.joined(separator: ":")
                let currency = Py.str(value["currency"] ?? .null)
                out.append(["id": .string(provider + "|" + account),
                            "name": .string(Catalog.byKey[provider]!.label + (counted.count > 1 ? " · " + Py.str(value["label"] ?? .null) : "")),
                            "amount": value["amount"] ?? .null, "currency": value["currency"] ?? .null, "line": lines[provider + "|" + currency] ?? .null,
                            "current": .bool(value["failed"]?.isTruthy != true && now - (Py.or(value["at"], .int(0)).number ?? 0) <= Runtime.freshReading)])
            }
        }
        return out
    }

    func alertClock(_ at: Double, now: Double = Date().timeIntervalSince1970) -> String {
        // The service often returns xx:59:59.999 for an xx:00 reset.
        let zone = localZone(), moment = at + 30
        let days = Runtime.dayNumber(moment, zone) - Runtime.dayNumber(now, zone)
        let clock = Runtime.format(moment, zone, "%H:%M")
        if days <= 0 { return clock }
        if days == 1 { return "明天 " + clock }
        if days < 7 {
            let weekday = ((Runtime.dayNumber(moment, zone) + 3) % 7 + 7) % 7
            return "周" + String(Array("一二三四五六日")[weekday]) + " " + clock
        }
        return "\(Int(Runtime.format(moment, zone, "%m"))!)月\(Int(Runtime.format(moment, zone, "%d"))!)日 " + clock
    }

    /// (subtitle, body) of one alert's notification; the title is always Starmeter.
    public func alertNotice(_ alert: JSONObject, now: Double = Date().timeIntervalSince1970) -> (String, String) {
        let name = Py.str(alert["name"] ?? .null)
        if alert["kind"] == .string("balance") {
            let currency = Py.str(alert["currency"] ?? .null)
            return ("余额提醒", "\(name) 余额 \(Runtime.moneyText(alert["amount"]?.number ?? 0, currency))，已低于提醒线 \(Runtime.moneyText(alert["line"]?.number ?? 0, currency))")
        }
        let reset = alert["resetsAt"]?.isTruthy == true ? "，\(alertClock(alert["resetsAt"]?.number ?? 0, now: now)) 重置" : ""
        let used = Runtime.g((alert["used"]?.number ?? 0).rounded(.toNearestOrEven)) + "%"
        let window = Py.str(alert["window"] ?? .null)
        if alert["kind"] == .string("renews") { return ("重置提醒", "\(name) · \(window) 只用了 \(used)\(reset)，记得用掉") }
        return ("用量提醒", "\(name) · \(window) 已用 \(used)\(reset)")
    }

    /// Run by the background worker after its reads; only cached readings are looked at.
    public func checkUsageAlerts() {
        let settings = self.settings()
        let percent = settings["usageAlert"]?.number ?? 0, reminder = settings["resetReminder"]?.isTruthy == true
        guard percent != 0 || reminder || settings["balanceAlerts"]?.isTruthy == true else { return }
        let data = cachedData()
        var marks = Alerts.load(stateDir)
        let due = Alerts.due(windows: alertWindows(data), balances: alertBalances(data), marks: &marks, percent: percent, reminder: reminder,
                             now: Date().timeIntervalSince1970)
        // Only a notice that went out is marked; one that failed, or past the few sent at once, is due again next run.
        for alert in due.prefix(Alerts.atOnce) {
            let (subtitle, body) = alertNotice(alert)
            if postNotice(subtitle, body) { Alerts.told(&marks, alert) }
        }
        try? Alerts.save(stateDir, marks)
    }
}
