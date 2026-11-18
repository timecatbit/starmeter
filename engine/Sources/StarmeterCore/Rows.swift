import Foundation

/// The menu rows SwiftBar shows and the panel parses back into entries: one line each, with "| key=value"
/// options. Ports of the row helpers and the per-module rows in swiftbar_plugin.py.
public typealias Options = JSONObject

extension Runtime {
    static let rankColor = "#24272D,#F2F3F5", mutedColor = "#6A707A,#ABAFB7", komariOnline = "#2F7D50,#8AC6A4", warnColor = "#B54708,#FEC84B", badColor = "#B42318,#FDA29B"
    static let menuIconSize = 14
    static let fallbackSeparator = "\u{1F}"

    /// SwiftBar splits options on bars and lines, so dynamic text holds neither; leading hyphens can't fake nesting.
    public static func sanitize(_ value: String) -> String {
        var out = ""
        for scalar in value.unicodeScalars {
            if scalar == "|" { out += "｜" }
            else if scalar.value < 32 { out += " " }
            else { out.unicodeScalars.append(scalar) }
        }
        let text = out.trimmingCharacters(in: .whitespacesAndNewlines)
        let stripped = text.drop { $0 == "-" }
        return String(repeating: "‑", count: text.count - stripped.count) + stripped
    }

    static func asOption(_ key: String, _ value: JSON) -> String {
        if let flag = value.bool { return key + "=" + (flag ? "true" : "false") }
        // An option can't span lines, or a tooltip would leak out as a grey row.
        let text = Py.str(value).replacingOccurrences(of: "\r", with: "\n").split(separator: "\n", omittingEmptySubsequences: false)
            .map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }.joined(separator: " · ")
        if !text.isEmpty && !text.contains(where: { $0 == " " || $0 == "\t" || $0 == "\"" }) { return key + "=" + text }
        return key + "=\"" + text.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(of: "\"", with: "\\\"") + "\""
    }

    /// One menu row; its level is how deeply it nests (SwiftBar's -- prefixes).
    static func item(_ text: String, level: Int = 0, _ given: Options = [:]) -> String {
        var options = given
        if options["color"] == nil { options["color"] = .null }
        if options["size"] == nil { options["size"] = 12 }
        if !text.isEmpty && level == 0 { options["tooltip"] = nil }
        if !text.isEmpty, let symbol = options["sfimage"]?.string, let icon = menuIcons[symbol] {
            options["sfimage"] = nil
            options["templateimage"] = .string(icon)
            options["sfsize"] = nil; options["width"] = nil; options["height"] = nil
        }
        if !text.isEmpty, options["sfimage"]?.isTruthy == true, options["sfsize"] == nil { options["sfsize"] = 13 }
        // None and False drop out; 0 and "" stay, as `is not None and is not False` keeps them.
        let attributes = options.filter { !$0.value.isNull && $0.value.bool != false }.map { asOption($0.key, $0.value) }
        var line = String(repeating: "--", count: level) + sanitize(text)
        if !attributes.isEmpty { line += " | " + attributes.joined(separator: " ") }
        return line
    }

    /// The options that make a row run one of this plugin's actions through SwiftBar.
    func act(_ name: String, _ args: String...) -> Options { act(name, args) }

    func act(_ name: String, _ args: [String]) -> Options {
        var options: Options = ["bash": "/bin/bash", "terminal": "false", "refresh": "true", "param1": .string(plugin), "param2": "--action", "param3": .string(name)]
        for (index, value) in args.enumerated() { options["param\(index + 4)"] = .string(value) }
        return options
    }

    static func merged(_ parts: Options...) -> Options {
        var result = Options()
        for part in parts { for (key, value) in part { result[key] = value } }
        return result
    }

    public static func fmtTokens(_ value: JSON?) -> String {
        guard let number = value?.pythonFloat else { return "—" }
        if number >= 1e9 { return String(format: "%.2fB", number / 1e9) }
        if number >= 1e6 { return String(format: "%.2fM", number / 1e6) }
        if number >= 1e3 { return String(format: "%.1fk", number / 1e3) }
        return String(Int64(number.rounded(.towardZero)))
    }

    static func shorten(_ value: String, _ limit: Int = 42) -> String {
        let text = sanitize(value)
        return text.count <= limit ? text : String(text.prefix(limit - 1)) + "…"
    }

    /// Wide and fullwidth characters take two columns, as unicodedata's W, F and A classes do in a terminal.
    static func displayWidth(_ scalar: Unicode.Scalar) -> Int {
        let v = scalar.value
        let wide: [ClosedRange<UInt32>] = [0x1100...0x115F, 0x2E80...0x303E, 0x3041...0x33FF, 0x3400...0x4DBF, 0x4E00...0x9FFF, 0xA000...0xA4CF,
                                           0xAC00...0xD7A3, 0xF900...0xFAFF, 0xFE30...0xFE4F, 0xFF00...0xFF60, 0xFFE0...0xFFE6, 0x1F300...0x1F64F,
                                           0x1F900...0x1F9FF, 0x20000...0x3FFFD, 0x00A1...0x00A1, 0x00B0...0x00B1, 0x00D7...0x00D7, 0x00F7...0x00F7,
                                           0x2018...0x2019, 0x201C...0x201D, 0x2026...0x2026, 0x2030...0x2030, 0x25A0...0x25FF, 0x2600...0x26FF]
        return wide.contains { $0.contains(v) } ? 2 : 1
    }

    static func padDisplay(_ value: String, _ width: Int) -> String {
        let text = sanitize(value)
        let used = text.unicodeScalars.reduce(0) { $0 + displayWidth($1) }
        return text + String(repeating: " ", count: Swift.max(0, width - used))
    }

    /// Python's '%g'.
    static func g(_ value: Double) -> String {
        if value == 0 { return value.sign == .minus ? "-0" : "0" }
        let exponent = Int((Swift.abs(value)).log10Floor)
        if exponent < -4 || exponent >= 6 {
            var text = String(format: "%.5e", value)
            if let e = text.firstIndex(of: "e") {
                var mantissa = String(text[..<e]), power = String(text[text.index(after: e)...])
                if mantissa.contains(".") { while mantissa.hasSuffix("0") { mantissa.removeLast() }; if mantissa.hasSuffix(".") { mantissa.removeLast() } }
                let sign = power.hasPrefix("-") ? "-" : "+"
                power = String(power.drop { $0 == "+" || $0 == "-" })
                while power.count > 2 && power.hasPrefix("0") { power.removeFirst() }
                text = mantissa + "e" + sign + power
            }
            return text
        }
        var text = String(format: "%.\(Swift.max(0, 5 - exponent))f", value)
        if text.contains(".") { while text.hasSuffix("0") { text.removeLast() }; if text.hasSuffix(".") { text.removeLast() } }
        return text
    }

    /// Python's round(value, digits): the exact value to that many decimals.
    static func round(_ value: Double, _ digits: Int) -> Double { Double(String(format: "%.\(digits)f", value)) ?? value }

    static func usageLevel(_ percent: Double?, _ stale: Bool = false) -> JSON {
        guard let percent, !stale else { return .null }
        return .string(percent >= 90 ? "critical" : percent >= 70 ? "warning" : "normal")
    }

    /// Every subscription reports the share already used, worded like the API rows' 本月已用.
    static func usedText(_ percent: Double) -> String { "已用 " + g(round(Swift.min(100, Swift.max(0, percent)), 1)) + "%" }

    /// Pace through a window: the share used less the share of the window gone by.
    static func paceText(_ used: Double, _ resetsAt: Double?, _ span: Double?, now: Double) -> String? {
        guard let span, span != 0, let resetsAt, resetsAt != 0 else { return nil }
        let start = resetsAt - span
        guard start <= now, now < resetsAt else { return nil }
        let ahead = used - 100 * (now - start) / span
        if Swift.abs(ahead) < 3 { return "与匀速持平" }
        return "比匀速" + (ahead > 0 ? "多用" : "少用") + " \(Int(Swift.abs(ahead).rounded(.toNearestOrEven)))%"
    }

    /// A window row's second line: when it resets, and its pace while the reading is current.
    func windowDetail(_ used: Double, _ resetsAt: Double?, _ span: Double?, _ readAt: Double?, failed: Bool = false) -> JSON {
        guard let resetsAt, resetsAt != 0 else { return .null }
        // The service often returns xx:59:59.999 for an xx:00 reset.
        var parts = ["重置于 " + Runtime.format(resetsAt + 30, localZone(), "%m/%d %H:%M")]
        if let readAt, readAt != 0, !failed, Date().timeIntervalSince1970 - readAt <= 600, let pace = Runtime.paceText(used, resetsAt, span, now: readAt) { parts.append(pace) }
        return .string(parts.joined(separator: " · "))
    }

    static func claudeSpan(_ window: JSONObject) -> Double? {
        // Statusline and older cached readings carry no length; their labels still say it.
        if let span = window["span"], span.isTruthy { return span.number }
        let label = Py.str(window["label"] ?? .string(""))
        return label == "5 小时" ? 5 * 3600 : label.hasSuffix("本周") ? 7 * 86400 : nil
    }

    static func minimaxSpan(_ window: JSONObject) -> Double? {
        if let starts = window["startsAt"], starts.isTruthy, let ends = window["resetsAt"], ends.isTruthy { return (ends.number ?? 0) - (starts.number ?? 0) }
        return Py.str(window["name"] ?? .string("")).hasSuffix("本周额度") ? 7 * 86400 : nil
    }

    func freshness(_ fetchedAt: JSON?) -> String {
        guard let at = fetchedAt?.pythonFloat, at != 0 else { return "待同步" }
        let elapsed = Swift.max(0, Int(Date().timeIntervalSince1970 - at))
        if elapsed < 60 { return "刚刚更新" }
        if elapsed < 3600 { return "\(elapsed / 60) 分钟前更新" }
        if elapsed < 86400 { return "\(elapsed / 3600) 小时前更新" }
        return "\(elapsed / 86400) 天前更新"
    }

    static func withFailure(_ badge: String, _ hasData: Bool, _ failed: Bool) -> String {
        // Values stay put while refreshing; only a real failure is marked, and plainly.
        failed ? (hasData ? badge + " · 刷新失败" : "刷新失败") : badge
    }

    func refreshRow(_ payload: JSONObject, _ error: String?, level: Int = 1) -> String {
        let failed = Runtime.refreshFailed(error)
        let detail: JSON = failed ? .string(error! + (payload["_fetchedAt"]?.isTruthy == true ? "\n保留最近一次成功的数据 · " + freshness(payload["_fetchedAt"]) : "")) : .null
        return Runtime.item("更新时间", level: level, ["badge": .string(failed ? "刷新失败" : freshness(payload["_fetchedAt"])), "paneldetail": detail,
                                                     "color": .string(failed ? Runtime.warnColor : Runtime.mutedColor), "size": 10])
    }

    func snapshotTime(_ stamp: Double) -> String {
        Runtime.format(stamp, localZone(), Runtime.dayNumber(stamp, localZone()) == Runtime.dayNumber(Date().timeIntervalSince1970, localZone()) ? "%H:%M" : "%m/%d %H:%M")
    }

    static func creditsAmount(_ value: Double) -> String {
        if value < 1000 { return g(round(value, 1)) }
        let formatter = NumberFormatter()
        formatter.numberStyle = .decimal; formatter.maximumFractionDigits = 0; formatter.groupingSeparator = ","; formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.usesGroupingSeparator = true; formatter.groupingSize = 3; formatter.roundingMode = .halfEven
        return formatter.string(from: NSNumber(value: value)) ?? String(Int(value))
    }

    static func moneyText(_ amount: Double, _ currency: String) -> String {
        if currency == "Credits" { return creditsAmount(amount) + " Credits" }
        return (["CNY": "¥", "USD": "$"][currency] ?? currency + " ") + String(format: "%.2f", amount)
    }

    func logoMark(_ size: Int) -> Options {
        if let raw = logo("starmeter-mark.svg") { return ["templateimage": .string(raw), "width": .int(Int64(size)), "height": .int(Int64(size))] }
        return ["sfimage": "sparkles", "sfsize": .int(Int64(Swift.min(size, 18)))]
    }

    func brandIcon(_ key: String) -> Options {
        let service = Catalog.byKey[key]!
        var raw: String?, template = false
        if service.brand == "DeepSeek" { raw = logo("deepseek-logo.svg") }
        else if let (file, mono) = Runtime.brandLogos[service.brand] { raw = logo("logos/" + file); template = mono }
        if let raw { return [template ? "templateimage" : "image": .string(raw), "width": .int(Int64(Runtime.menuIconSize)), "height": .int(Int64(Runtime.menuIconSize))] }
        return ["sfimage": .string(service.category == "订阅" ? "person.crop.circle" : "creditcard")]
    }

    func fileHref(_ path: String) -> String {
        "file://" + (path.addingPercentEncoding(withAllowedCharacters: CharacterSet(charactersIn: "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_.-~/")) ?? path)
    }
}

extension Double {
    var log10Floor: Double { Foundation.floor(Foundation.log10(self)) }
}
