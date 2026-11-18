import Foundation

/// What the Python engine's built-ins make of JSON values, where the port must give the same answer.
public enum Py {
    /// A failure the Python readers treat as "skip this line" (KeyError, TypeError, AttributeError, bad JSON).
    public struct LineError: Error {}
    /// A failure that fails a whole log file in the Python readers (ValueError, OSError and the like).
    public struct FileError: Error, CustomStringConvertible {
        public let description: String
        public init(_ description: String) { self.description = description }
    }

    /// str(value).
    public static func str(_ value: JSON?) -> String {
        guard let value else { return "None" }
        switch value {
        case .null: return "None"
        case .bool(let flag): return flag ? "True" : "False"
        case .int(let number): return String(number)
        case .double(let number): return number.isNaN ? "nan" : number.isInfinite ? (number < 0 ? "-inf" : "inf") : pythonRepr(number)
        case .string(let text): return text
        default: return repr(value)
        }
    }

    /// repr(value) of a list or dict as Python prints them.
    static func repr(_ value: JSON) -> String {
        switch value {
        case .string(let text):
            let quote = text.contains("'") && !text.contains("\"") ? "\"" : "'"
            var output = quote
            for scalar in text.unicodeScalars {
                switch scalar {
                case "\\": output += "\\\\"
                case "\n": output += "\\n"
                case "\r": output += "\\r"
                case "\t": output += "\\t"
                default:
                    if String(scalar) == quote { output += "\\" + quote }
                    else if scalar.value < 0x20 || scalar.value == 0x7F { output += String(format: "\\x%02x", scalar.value) }
                    else { output.unicodeScalars.append(scalar) }
                }
            }
            return output + quote
        case .array(let items): return "[" + items.map(repr).joined(separator: ", ") + "]"
        case .object(let object): return "{" + object.map { repr(.string($0.key)) + ": " + repr($0.value) }.joined(separator: ", ") + "}"
        default: return str(value)
        }
    }

    /// The engine's number(): float(value) when that is finite and not negative, else the integer 0.
    public static func number(_ value: JSON?) -> JSON {
        guard let value, let number = value.pythonFloat, number.isFinite, number >= 0 else { return .int(0) }
        return .double(number)
    }

    /// The value number() gives, as a Double.
    public static func num(_ value: JSON?) -> Double { number(value).number ?? 0 }

    /// value or fallback, with Python's truthiness.
    public static func or(_ value: JSON?, _ fallback: JSON) -> JSON { value.map { $0.isTruthy ? $0 : fallback } ?? fallback }

    /// sha256 of json.dumps(values, sort_keys=True), in hex: the sources' event and file identities.
    public static func identity(_ values: JSON...) -> String { identity(values) }

    public static func identity(_ values: [JSON]) -> String {
        SHA256.hex(Array(JSON.array(values).serialized(ensureASCII: true, sortKeys: true).utf8))
    }

    /// json.dumps(value) with its defaults.
    public static func dumps(_ value: JSON) -> String { value.serialized(ensureASCII: true) }

    /// value.get(key) on something that must be a dict, else Python raises AttributeError.
    static func get(_ value: JSON, _ key: String) throws -> JSON? {
        guard let object = value.object else { throw LineError() }
        return object[key]
    }

    /// (value or {}): a falsy value stands for an empty dict; anything else must be a dict to be read.
    static func dict(_ value: JSON?) -> JSON { or(value, .object(JSONObject())) }

    /// int(value) as Python's int(): numbers truncate, numeric text parses; anything else fails the file.
    static func int(_ value: JSON?) throws -> Int64 {
        switch value ?? .null {
        case .bool(let flag): return flag ? 1 : 0
        case .int(let number): return number
        case .double(let number):
            guard number.isFinite else { throw FileError("cannot convert float to integer") }
            return Int64(number.rounded(.towardZero))
        case .string(let text):
            let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines).replacingOccurrences(of: "_", with: "")
            guard let number = Int64(trimmed) else { throw FileError("invalid literal for int()") }
            return number
        default: throw LineError()
        }
    }

    /// A project's folder name, the last part of its working directory; only the name is kept, never the path.
    static func projectName(_ value: JSON?) -> JSON {
        guard let path = value?.string?.trimmingCharacters(in: .whitespacesAndNewlines), !path.isEmpty else { return .null }
        var trimmed = Substring(path)
        while let last = trimmed.last, last == "/" || last == "\\" { trimmed = trimmed.dropLast() }
        let name = trimmed.split(omittingEmptySubsequences: false, whereSeparator: { $0 == "/" || $0 == "\\" }).last.map(String.init) ?? ""
        return name.isEmpty ? .null : .string(name)
    }

    /// The log's own time for an event: epoch seconds (milliseconds past 1e11) or ISO text.
    static func stamp(_ value: JSON?) throws -> JSON {
        switch value {
        case .int(let number)?: return Double(number) > 1e11 ? .double(Double(number) / 1000) : .int(number)
        case .double(let number)?: return number > 1e11 ? .double(number / 1000) : .double(number)
        case .bool(let flag)?: return .bool(flag)
        case nil: throw LineError()
        default:
            do { return .double(try ISOTime.parse(str(value)).timestamp()) } catch { throw FileError("bad timestamp") }
        }
    }
}

/// Model prices from scripts/update_pricing.py (ccusage's snapshot): the newest valid copy among the one that
/// ships with the engine and the one downloaded later.
public enum Pricing {
    /// Folders holding codex-pricing.json and model-pricing.json, in the order the Python engine looks.
    public static var folders: [String] = []
    private static var loaded: [String: JSONObject] = [:]
    private static let lock = NSLock()

    public static func reset() { lock.lock(); loaded = [:]; lock.unlock() }

    static func rate(_ value: JSON?) -> Bool {
        guard let value, value.bool == nil, let number = value.number else { return false }
        return number.isFinite && number >= 0 && number <= 10000
    }

    static func validRates(_ price: JSON?) -> Bool {
        guard let price = price?.object, ["input", "output", "cacheRead", "cacheWrite"].allSatisfy({ rate(price[$0]) }) else { return false }
        guard let tier = price["longContext"], !tier.isNull else { return true }
        guard let fields = tier.object, ["input", "output", "cacheRead", "cacheWrite"].allSatisfy({ rate(fields[$0]) }),
              case .int(let threshold)? = fields["threshold"], threshold > 0 else { return false }
        return true
    }

    /// Downloaded tables are data only: every field is checked before a single price is used.
    public static func valid(_ name: String, _ data: JSON) -> Bool {
        guard let table = data.object, let source = table["source"]?.object else { return false }
        let commit = Py.str(source["commit"] ?? .string(""))
        guard commit.utf8.count == 40, commit.utf8.allSatisfy({ (0x30...0x39).contains($0) || (0x61...0x66).contains($0) }) else { return false }
        guard let models = table["models"]?.object, models.count > 0, models.count <= 5000,
              models.allSatisfy({ !$0.key.isEmpty && $0.key.count <= 200 && validRates($0.value) }) else { return false }
        if name == "codex-pricing.json" {
            guard let aliases = table["aliases"]?.object, aliases.allSatisfy({ $0.value.string != nil }) else { return false }
            guard let fallbacks = table["autoReviewFallbacks"]?.array, fallbacks.allSatisfy({ row in
                guard let row = row.object, row["model"]?.string != nil else { return false }
                return isDay(Py.str(row["releasedOn"] ?? .string("")))
            }) else { return false }
            guard let multipliers = table["multipliers"]?.object, multipliers.allSatisfy({ tier in
                tier.value.object?.allSatisfy { rate($0.value) && ($0.value.number ?? 0) > 0 && ($0.value.number ?? 0) <= 100 } ?? false
            }) else { return false }
        }
        return true
    }

    static func isDay(_ text: String) -> Bool {
        let bytes = Array(text.utf8)
        guard bytes.count == 10, bytes[4] == 0x2D, bytes[7] == 0x2D else { return false }
        return bytes.enumerated().allSatisfy { $0.offset == 4 || $0.offset == 7 || (0x30...0x39).contains($0.element) }
    }

    /// A table's stamp: ccusage's commit time (or a second past the last); 0 when unknown.
    static func committed(_ table: JSONObject) -> Double {
        guard let text = table["source"]?.object?["committedAt"], text.isTruthy,
              let time = try? ISOTime.parse(Py.str(text)) else { return 0 }
        return time.timestamp()
    }

    public static func table(_ name: String) -> JSONObject {
        lock.lock(); defer { lock.unlock() }
        if let table = loaded[name] { return table }
        var best: JSONObject?
        for folder in folders {
            guard let data = FileManager.default.contents(atPath: (folder as NSString).appendingPathComponent(name)),
                  let value = try? JSON.parse(data), valid(name, value), let table = value.object else { continue }
            // max() keeps the first of equal stamps.
            if best == nil || committed(table) > committed(best!) { best = table }
        }
        loaded[name] = best ?? JSONObject()
        return loaded[name]!
    }

    static func priced(_ price: JSONObject, _ event: JSONObject, _ multiplier: Double = 1) -> Double {
        let inputs = Py.num(event["input"]), cached = Py.num(event["cached"]), output = Py.num(event["output"]), writes = Py.num(event["writes"])
        var price = price
        // Each event is one request, so its own context size selects the long-context tier.
        if let tier = price["longContext"]?.object, tier.isTruthy, inputs + cached > (tier["threshold"]?.number ?? .infinity) { price = tier }
        func p(_ key: String) -> Double { price[key]?.number ?? 0 }
        return (inputs * p("input") + cached * p("cacheRead") + output * p("output") + writes * p("cacheWrite")) * multiplier / 1e6
    }

    /// The model name without a trailing date: -2026-01-31 or -20260131.
    static func undated(_ model: String) -> String {
        let bytes = Array(model.utf8)
        func digits(_ range: Range<Int>) -> Bool { range.allSatisfy { (0x30...0x39).contains(bytes[$0]) } }
        let n = bytes.count
        if n >= 11, bytes[n - 11] == 0x2D, digits(n - 10..<n - 6), bytes[n - 6] == 0x2D, digits(n - 5..<n - 3), bytes[n - 3] == 0x2D, digits(n - 2..<n) {
            return String(decoding: bytes[..<(n - 11)], as: UTF8.self)
        }
        if n >= 9, bytes[n - 9] == 0x2D, digits(n - 8..<n) { return String(decoding: bytes[..<(n - 9)], as: UTF8.self) }
        return model
    }

    /// API-equivalent estimate with ccusage's Codex rules; unknown models stay unpriced.
    public static func codexCost(_ event: JSONObject) -> Double? {
        let table = Pricing.table("codex-pricing.json")
        var model = Py.str(Py.or(event["model"], .string("")))
        if model == "codex-auto-review" {
            // The reviewer bills as the newest model released by the request's (UTC) day; a request read live is today's.
            let when = event["timestamp"]?.number ?? Date().timeIntervalSince1970
            let day = utcDay(when)
            model = table["autoReviewFallbacks"]?.array?.first(where: { day >= ($0.object?["releasedOn"]?.string ?? "~") })?.object?["model"]?.string ?? "gpt-5"
        }
        let base = undated(model)
        let resolved = table["aliases"]?.object?[base]?.string ?? base
        let models = table["models"]?.object ?? JSONObject()
        guard let price = (models[resolved]?.object).flatMap({ $0.isTruthy ? $0 : nil }) ?? models[base]?.object, price.isTruthy else { return nil }
        let tier = Py.or(event["tier"], .string("standard"))
        let rates = (tier.string.flatMap { table["multipliers"]?.object?[$0]?.object }) ?? JSONObject()
        let multiplier = rates[resolved]?.number ?? rates[base]?.number ?? 1
        return priced(price, event, multiplier)
    }

    /// Other vendors' models (DeepSeek, GLM, Kimi, Gemini…) from the same snapshot, when the client logged no cost.
    public static func modelCost(_ event: JSONObject) -> Double? {
        let models = Pricing.table("model-pricing.json")["models"]?.object ?? JSONObject()
        let name = Py.str(Py.or(event["model"], .string(""))).lowercased()
        let model = name.split(separator: "/", omittingEmptySubsequences: false).last.map(String.init) ?? name
        guard let price = (models[model]?.object).flatMap({ $0.isTruthy ? $0 : nil }) ?? models[undated(model)]?.object, price.isTruthy else { return nil }
        return priced(price, event)
    }

    static func utcDay(_ seconds: Double) -> String {
        let days = Int((seconds / 86400).rounded(.down))
        let (y, m, d) = civilFromDays(days)
        return String(format: "%04d-%02d-%02d", y, m, d)
    }
}

extension JSONObject {
    var isTruthy: Bool { !isEmpty }
}

/// The calendar date of a day number counted from 1970-01-01.
func civilFromDays(_ days: Int) -> (Int, Int, Int) {
    let z = days + 719468
    let era = (z >= 0 ? z : z - 146096) / 146097
    let doe = z - era * 146097
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100)
    let mp = (5 * doy + 2) / 153
    let d = doy - (153 * mp + 2) / 5 + 1
    let m = mp < 10 ? mp + 3 : mp - 9
    return (yoe + era * 400 + (m <= 2 ? 1 : 0), m, d)
}
