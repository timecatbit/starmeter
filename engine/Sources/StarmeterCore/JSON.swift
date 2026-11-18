import Foundation

/// A JSON value the way Python's json module sees it: integers and floats stay apart, objects keep their key
/// order, and comparing follows Python (1 == 1.0 == true). State files and logs written by the Python engine
/// read back the same, and files written here read the same in Python.
public enum JSON {
    case null
    case bool(Bool)
    case int(Int64)
    case double(Double)
    case string(String)
    case array([JSON])
    case object(JSONObject)
}

/// A JSON object that keeps its keys in the order they were first set, like a Python dict.
public struct JSONObject: Sequence {
    public private(set) var keys: [String] = []
    private var storage: [String: JSON] = [:]

    public init() {}
    public init(_ pairs: [(String, JSON)]) { for (key, value) in pairs { self[key] = value } }

    /// Setting an existing key keeps its place; setting nil removes it.
    public subscript(key: String) -> JSON? {
        get { storage[key] }
        set {
            if let newValue {
                if storage.updateValue(newValue, forKey: key) == nil { keys.append(key) }
            } else if storage.removeValue(forKey: key) != nil {
                keys.removeAll { $0 == key }
            }
        }
    }

    public var count: Int { keys.count }
    public var isEmpty: Bool { keys.isEmpty }

    public func makeIterator() -> AnyIterator<(key: String, value: JSON)> {
        var index = 0
        return AnyIterator {
            guard index < keys.count else { return nil }
            defer { index += 1 }
            return (keys[index], storage[keys[index]]!)
        }
    }
}

extension JSONObject: Equatable {
    /// Python compares dicts without regard to order.
    public static func == (a: JSONObject, b: JSONObject) -> Bool {
        a.count == b.count && a.storage.allSatisfy { key, value in b.storage[key].map { $0 == value } ?? false }
    }
}

extension JSONObject: ExpressibleByDictionaryLiteral {
    public init(dictionaryLiteral elements: (String, JSON)...) { self.init(elements) }
}

extension JSON: Equatable {
    public static func == (a: JSON, b: JSON) -> Bool {
        switch (a, b) {
        case (.null, .null): return true
        case let (.string(x), .string(y)): return x == y
        case let (.array(x), .array(y)): return x == y
        case let (.object(x), .object(y)): return x == y
        default:
            // Numbers and bools compare by value across kinds, as in Python.
            if let x = a.numericValue, let y = b.numericValue { return x == y }
            return false
        }
    }

    private var numericValue: Double? {
        switch self {
        case .bool(let value): return value ? 1 : 0
        case .int(let value): return Double(value)
        case .double(let value): return value
        default: return nil
        }
    }
}

extension JSON: ExpressibleByStringLiteral, ExpressibleByIntegerLiteral, ExpressibleByFloatLiteral,
                ExpressibleByBooleanLiteral, ExpressibleByArrayLiteral, ExpressibleByDictionaryLiteral {
    public init(stringLiteral value: String) { self = .string(value) }
    public init(integerLiteral value: Int64) { self = .int(value) }
    public init(floatLiteral value: Double) { self = .double(value) }
    public init(booleanLiteral value: Bool) { self = .bool(value) }
    public init(arrayLiteral elements: JSON...) { self = .array(elements) }
    public init(dictionaryLiteral elements: (String, JSON)...) { self = .object(JSONObject(elements)) }
}

extension JSON {
    public var string: String? { if case .string(let value) = self { return value }; return nil }
    public var bool: Bool? { if case .bool(let value) = self { return value }; return nil }
    public var array: [JSON]? { if case .array(let value) = self { return value }; return nil }
    public var object: JSONObject? { if case .object(let value) = self { return value }; return nil }
    public var isNull: Bool { if case .null = self { return true }; return false }

    /// An int or float, never a bool: Python's isinstance(x, (int, float)) and not isinstance(x, bool).
    public var number: Double? {
        switch self {
        case .int(let value): return Double(value)
        case .double(let value): return value
        default: return nil
        }
    }

    /// Python's isinstance(x, int), which a bool also passes.
    public var integer: Int64? {
        switch self {
        case .int(let value): return value
        case .bool(let value): return value ? 1 : 0
        default: return nil
        }
    }

    /// What Python's float(x) makes of it: numbers, bools and numeric text, or nil where float() raises.
    public var pythonFloat: Double? {
        switch self {
        case .bool(let value): return value ? 1 : 0
        case .int(let value): return Double(value)
        case .double(let value): return value
        case .string(let text): return parsePythonFloat(text)
        default: return nil
        }
    }

    public subscript(key: String) -> JSON? { object?[key] }
}

/// Python's float() on text: surrounding whitespace, a sign, decimals, exponents, inf and nan.
public func parsePythonFloat(_ text: String) -> Double? {
    let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !trimmed.isEmpty else { return nil }
    let body = trimmed.first == "+" || trimmed.first == "-" ? trimmed.dropFirst() : Substring(trimmed)
    switch body.lowercased() {
    case "inf", "infinity": return trimmed.first == "-" ? -.infinity : .infinity
    case "nan": return .nan
    default:
        // Swift's Double() also takes hex floats and words Python rejects; allow only Python's decimal form.
        guard body.allSatisfy({ $0.isASCII && ($0.isNumber || $0 == "." || $0 == "e" || $0 == "E" || $0 == "+" || $0 == "-") }),
              body.contains(where: { $0.isNumber }) else { return nil }
        return Double(trimmed)
    }
}

public struct JSONError: Error, CustomStringConvertible {
    public let message: String
    public let offset: Int
    /// The bytes were not UTF-8: json.loads raises UnicodeDecodeError then, not JSONDecodeError.
    public var encoding = false
    public var description: String { "\(message) at byte \(offset)" }
}

// MARK: - Reading

extension JSON {
    /// Parses UTF-8 JSON as Python's json.loads does with strict UTF-8 decoding: a byte-order mark, invalid
    /// UTF-8, control characters inside strings and leading zeros are errors; NaN and Infinity are accepted;
    /// a repeated key keeps its first place and its last value.
    public static func parse(_ data: Data) throws -> JSON {
        try data.withUnsafeBytes { raw in
            var parser = JSONParser(bytes: raw.bindMemory(to: UInt8.self))
            return try parser.document()
        }
    }

    public static func parse(_ text: String) throws -> JSON {
        try parse(Data(text.utf8))
    }

    /// Parses only the parts `keep` names and checks the rest without building it, so a large line whose text
    /// nobody reads costs a scan, not a copy. Accepts and rejects exactly what a full parse does.
    /// `surrogates` accepts UTF-8-encoded surrogates, as json.loads does for bytes ('surrogatepass').
    public static func parse(_ bytes: UnsafeBufferPointer<UInt8>, keep: JSONSelection = .all, surrogates: Bool = false) throws -> JSON {
        var parser = JSONParser(bytes: bytes)
        parser.selection = keep
        parser.surrogates = surrogates
        return try parser.document()
    }
}

/// Which parts of a document to build: everything, or only some keys of an object (and what to keep of each).
public indirect enum JSONSelection {
    case all
    case keys([String: JSONSelection])
}

struct JSONParser {
    let bytes: UnsafeBufferPointer<UInt8>
    var index = 0
    var selection: JSONSelection = .all
    var surrogates = false
    static let maximumDepth = 512

    init(bytes: UnsafeBufferPointer<UInt8>) { self.bytes = bytes }

    mutating func document() throws -> JSON {
        skipWhitespace()
        let value = try parseValue(depth: 0, keep: selection)
        skipWhitespace()
        guard index == bytes.count else { throw fail("Extra data") }
        return value
    }

    func fail(_ message: String) -> JSONError { JSONError(message: message, offset: index) }

    mutating func skipWhitespace() {
        while index < bytes.count {
            switch bytes[index] {
            case 0x20, 0x09, 0x0A, 0x0D: index += 1
            default: return
            }
        }
    }

    mutating func literal(_ word: StaticString) -> Bool {
        let count = word.utf8CodeUnitCount
        guard index + count <= bytes.count else { return false }
        let start = word.utf8Start
        for offset in 0..<count where bytes[index + offset] != start[offset] { return false }
        index += count
        return true
    }

    mutating func parseValue(depth: Int, keep: JSONSelection = .all) throws -> JSON {
        guard depth < Self.maximumDepth else { throw fail("Too deeply nested") }
        guard index < bytes.count else { throw fail("Expecting value") }
        switch bytes[index] {
        case UInt8(ascii: "{"): return try parseObject(depth: depth, keep: keep)
        case UInt8(ascii: "["): return try parseArray(depth: depth, keep: keep)
        case UInt8(ascii: "\""): return .string(try parseString())
        case UInt8(ascii: "t"): if literal("true") { return .bool(true) }
        case UInt8(ascii: "f"): if literal("false") { return .bool(false) }
        case UInt8(ascii: "n"): if literal("null") { return .null }
        case UInt8(ascii: "N"): if literal("NaN") { return .double(.nan) }
        case UInt8(ascii: "I"): if literal("Infinity") { return .double(.infinity) }
        case UInt8(ascii: "-"):
            if literal("-Infinity") { return .double(-.infinity) }
            return try parseNumber()
        case UInt8(ascii: "0")...UInt8(ascii: "9"): return try parseNumber()
        default: break
        }
        throw fail("Expecting value")
    }

    mutating func parseObject(depth: Int, keep: JSONSelection = .all) throws -> JSON {
        index += 1
        var object = JSONObject()
        skipWhitespace()
        if index < bytes.count, bytes[index] == UInt8(ascii: "}") { index += 1; return .object(object) }
        while true {
            skipWhitespace()
            guard index < bytes.count, bytes[index] == UInt8(ascii: "\"") else {
                throw fail("Expecting property name enclosed in double quotes")
            }
            let key = try parseString()
            skipWhitespace()
            guard index < bytes.count, bytes[index] == UInt8(ascii: ":") else { throw fail("Expecting ':' delimiter") }
            index += 1
            skipWhitespace()
            switch keep {
            case .all: object[key] = try parseValue(depth: depth + 1)
            case .keys(let wanted):
                if let part = wanted[key] { object[key] = try parseValue(depth: depth + 1, keep: part) } else { try skipValue(depth: depth + 1) }
            }
            skipWhitespace()
            guard index < bytes.count else { throw fail("Expecting ',' delimiter") }
            if bytes[index] == UInt8(ascii: ",") { index += 1; continue }
            if bytes[index] == UInt8(ascii: "}") { index += 1; return .object(object) }
            throw fail("Expecting ',' delimiter")
        }
    }

    mutating func parseArray(depth: Int, keep: JSONSelection = .all) throws -> JSON {
        index += 1
        var items: [JSON] = []
        skipWhitespace()
        if index < bytes.count, bytes[index] == UInt8(ascii: "]") { index += 1; return .array(items) }
        while true {
            skipWhitespace()
            items.append(try parseValue(depth: depth + 1, keep: keep))
            skipWhitespace()
            guard index < bytes.count else { throw fail("Expecting ',' delimiter") }
            if bytes[index] == UInt8(ascii: ",") { index += 1; continue }
            if bytes[index] == UInt8(ascii: "]") { index += 1; return .array(items) }
            throw fail("Expecting ',' delimiter")
        }
    }

    /// Checks one value as parseValue would, building nothing.
    mutating func skipValue(depth: Int) throws {
        guard depth < Self.maximumDepth else { throw fail("Too deeply nested") }
        guard index < bytes.count else { throw fail("Expecting value") }
        switch bytes[index] {
        case UInt8(ascii: "{"):
            index += 1
            skipWhitespace()
            if index < bytes.count, bytes[index] == UInt8(ascii: "}") { index += 1; return }
            while true {
                skipWhitespace()
                guard index < bytes.count, bytes[index] == UInt8(ascii: "\"") else { throw fail("Expecting property name enclosed in double quotes") }
                try skipString()
                skipWhitespace()
                guard index < bytes.count, bytes[index] == UInt8(ascii: ":") else { throw fail("Expecting ':' delimiter") }
                index += 1
                skipWhitespace()
                try skipValue(depth: depth + 1)
                skipWhitespace()
                guard index < bytes.count else { throw fail("Expecting ',' delimiter") }
                if bytes[index] == UInt8(ascii: ",") { index += 1; continue }
                if bytes[index] == UInt8(ascii: "}") { index += 1; return }
                throw fail("Expecting ',' delimiter")
            }
        case UInt8(ascii: "["):
            index += 1
            skipWhitespace()
            if index < bytes.count, bytes[index] == UInt8(ascii: "]") { index += 1; return }
            while true {
                skipWhitespace()
                try skipValue(depth: depth + 1)
                skipWhitespace()
                guard index < bytes.count else { throw fail("Expecting ',' delimiter") }
                if bytes[index] == UInt8(ascii: ",") { index += 1; continue }
                if bytes[index] == UInt8(ascii: "]") { index += 1; return }
                throw fail("Expecting ',' delimiter")
            }
        case UInt8(ascii: "\""): try skipString()
        case UInt8(ascii: "-"), UInt8(ascii: "0")...UInt8(ascii: "9"), UInt8(ascii: "t"), UInt8(ascii: "f"), UInt8(ascii: "n"),
             UInt8(ascii: "N"), UInt8(ascii: "I"):
            // Scalars are small; parsing them is as cheap as checking them.
            _ = try parseValue(depth: depth)
        default: throw fail("Expecting value")
        }
    }

    /// Checks a string as parseString would: escapes, no control characters, strict UTF-8.
    mutating func skipString() throws {
        index += 1
        var asciiOnly = true
        let start = index
        while index < bytes.count {
            let byte = bytes[index]
            if byte == UInt8(ascii: "\"") {
                if !asciiOnly {
                    // Escapes are ASCII, so checking the raw bytes between the quotes checks every non-ASCII run.
                    guard isValidUTF8(UnsafeBufferPointer(rebasing: bytes[start..<index]), surrogates: surrogates) else {
                        throw JSONError(message: "Invalid UTF-8", offset: start, encoding: true)
                    }
                }
                index += 1
                return
            }
            if byte < 0x20 { throw fail("Invalid control character") }
            if byte >= 0x80 { asciiOnly = false }
            if byte == UInt8(ascii: "\\") {
                index += 1
                guard index < bytes.count else { break }
                switch bytes[index] {
                case UInt8(ascii: "\""), UInt8(ascii: "\\"), UInt8(ascii: "/"), UInt8(ascii: "b"), UInt8(ascii: "f"),
                     UInt8(ascii: "n"), UInt8(ascii: "r"), UInt8(ascii: "t"):
                    index += 1
                case UInt8(ascii: "u"):
                    index += 1
                    _ = try hex4()
                default: throw fail("Invalid \\escape")
                }
                continue
            }
            index += 1
        }
        throw fail("Unterminated string")
    }

    mutating func parseNumber() throws -> JSON {
        let start = index
        if bytes[index] == UInt8(ascii: "-") { index += 1 }
        guard index < bytes.count, isDigit(bytes[index]) else { index = start; throw fail("Expecting value") }
        if bytes[index] == UInt8(ascii: "0") {
            index += 1
        } else {
            while index < bytes.count, isDigit(bytes[index]) { index += 1 }
        }
        var integral = true
        if index + 1 < bytes.count, bytes[index] == UInt8(ascii: "."), isDigit(bytes[index + 1]) {
            integral = false
            index += 1
            while index < bytes.count, isDigit(bytes[index]) { index += 1 }
        }
        if index < bytes.count, bytes[index] == UInt8(ascii: "e") || bytes[index] == UInt8(ascii: "E") {
            var probe = index + 1
            if probe < bytes.count, bytes[probe] == UInt8(ascii: "+") || bytes[probe] == UInt8(ascii: "-") { probe += 1 }
            if probe < bytes.count, isDigit(bytes[probe]) {
                integral = false
                index = probe
                while index < bytes.count, isDigit(bytes[index]) { index += 1 }
            }
        }
        let text = String(decoding: UnsafeBufferPointer(rebasing: bytes[start..<index]), as: UTF8.self)
        // Python keeps integers of any size; past 64 bits the value is kept as a float.
        if integral, let value = Int64(text) { return .int(value) }
        guard let value = Double(text) else { throw fail("Invalid number") }
        return .double(value)
    }

    func isDigit(_ byte: UInt8) -> Bool { byte >= UInt8(ascii: "0") && byte <= UInt8(ascii: "9") }

    mutating func parseString() throws -> String {
        index += 1
        let start = index
        // Fast path: no escapes.
        while index < bytes.count {
            let byte = bytes[index]
            if byte == UInt8(ascii: "\"") {
                let slice = UnsafeBufferPointer(rebasing: bytes[start..<index])
                index += 1
                return try decodeUTF8(slice, at: start)
            }
            if byte == UInt8(ascii: "\\") { break }
            if byte < 0x20 { throw fail("Invalid control character") }
            index += 1
        }
        var result = try decodeUTF8(UnsafeBufferPointer(rebasing: bytes[start..<index]), at: start)
        while index < bytes.count {
            let byte = bytes[index]
            if byte == UInt8(ascii: "\"") { index += 1; return result }
            if byte < 0x20 { throw fail("Invalid control character") }
            if byte == UInt8(ascii: "\\") {
                index += 1
                guard index < bytes.count else { break }
                let escape = bytes[index]
                index += 1
                switch escape {
                case UInt8(ascii: "\""): result.append("\"")
                case UInt8(ascii: "\\"): result.append("\\")
                case UInt8(ascii: "/"): result.append("/")
                case UInt8(ascii: "b"): result.append("\u{08}")
                case UInt8(ascii: "f"): result.append("\u{0C}")
                case UInt8(ascii: "n"): result.append("\n")
                case UInt8(ascii: "r"): result.append("\r")
                case UInt8(ascii: "t"): result.append("\t")
                case UInt8(ascii: "u"):
                    var code = try hex4()
                    if (0xD800..<0xDC00).contains(code), index + 1 < bytes.count,
                       bytes[index] == UInt8(ascii: "\\"), bytes[index + 1] == UInt8(ascii: "u") {
                        let mark = index
                        index += 2
                        let low = try hex4()
                        if (0xDC00..<0xE000).contains(low) {
                            code = 0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00)
                        } else {
                            index = mark
                        }
                    }
                    // A lone surrogate, which Python keeps in a str, has no place in a Swift String.
                    result.unicodeScalars.append(Unicode.Scalar(code) ?? "\u{FFFD}")
                default:
                    index -= 1
                    throw fail("Invalid \\escape")
                }
                continue
            }
            let runStart = index
            while index < bytes.count, bytes[index] != UInt8(ascii: "\""), bytes[index] != UInt8(ascii: "\\"), bytes[index] >= 0x20 {
                index += 1
            }
            result += try decodeUTF8(UnsafeBufferPointer(rebasing: bytes[runStart..<index]), at: runStart)
        }
        throw fail("Unterminated string")
    }

    mutating func hex4() throws -> UInt32 {
        guard index + 4 <= bytes.count else { throw fail("Invalid \\uXXXX escape") }
        var value: UInt32 = 0
        for _ in 0..<4 {
            let byte = bytes[index]
            let digit: UInt8
            switch byte {
            case UInt8(ascii: "0")...UInt8(ascii: "9"): digit = byte - UInt8(ascii: "0")
            case UInt8(ascii: "a")...UInt8(ascii: "f"): digit = byte - UInt8(ascii: "a") + 10
            case UInt8(ascii: "A")...UInt8(ascii: "F"): digit = byte - UInt8(ascii: "A") + 10
            default: throw fail("Invalid \\uXXXX escape")
            }
            value = value << 4 | UInt32(digit)
            index += 1
        }
        return value
    }

    func decodeUTF8(_ slice: UnsafeBufferPointer<UInt8>, at offset: Int) throws -> String {
        guard isValidUTF8(slice, surrogates: surrogates) else { throw JSONError(message: "Invalid UTF-8", offset: offset, encoding: true) }
        return String(decoding: slice, as: UTF8.self)
    }
}

/// Strict UTF-8, as Python's decoder: no overlong forms, no encoded surrogates, nothing past U+10FFFF.
func isValidUTF8(_ bytes: UnsafeBufferPointer<UInt8>, surrogates: Bool = false) -> Bool {
    var index = 0
    while index < bytes.count {
        let lead = bytes[index]
        if lead < 0x80 { index += 1; continue }
        let length: Int
        var low: UInt8 = 0x80, high: UInt8 = 0xBF
        switch lead {
        case 0xC2...0xDF: length = 2
        case 0xE0: length = 3; low = 0xA0
        case 0xE1...0xEC, 0xEE...0xEF: length = 3
        case 0xED: length = 3; high = surrogates ? 0xBF : 0x9F
        case 0xF0: length = 4; low = 0x90
        case 0xF1...0xF3: length = 4
        case 0xF4: length = 4; high = 0x8F
        default: return false
        }
        guard index + length <= bytes.count else { return false }
        let second = bytes[index + 1]
        guard second >= low, second <= high else { return false }
        for offset in 2..<max(2, length) where bytes[index + offset] & 0xC0 != 0x80 { return false }
        index += length
    }
    return true
}

// MARK: - Writing

extension JSON {
    /// Text as Python's json.dumps writes it. The defaults match json.dump(value, f, ensure_ascii=False);
    /// pass ensureASCII for json.dumps' own default and (",", ":") for its compact separators.
    public func serialized(ensureASCII: Bool = false, itemSeparator: String = ", ", keySeparator: String = ": ",
                           sortKeys: Bool = false) -> String {
        var output = ""
        write(to: &output, ensureASCII: ensureASCII, item: itemSeparator, key: keySeparator, sortKeys: sortKeys)
        return output
    }

    /// json.dumps(value, indent=n): one item per line, "," then a newline between items, empty containers kept short.
    public func serialized(ensureASCII: Bool = false, indent: Int, level: Int = 0) -> String {
        let inner = String(repeating: " ", count: indent * (level + 1)), outer = String(repeating: " ", count: indent * level)
        switch self {
        case .array(let items) where !items.isEmpty:
            return "[\n" + items.map { inner + $0.serialized(ensureASCII: ensureASCII, indent: indent, level: level + 1) }.joined(separator: ",\n") + "\n" + outer + "]"
        case .object(let object) where !object.isEmpty:
            return "{\n" + object.keys.map { name in
                inner + JSON.string(name).serialized(ensureASCII: ensureASCII) + ": " + object[name]!.serialized(ensureASCII: ensureASCII, indent: indent, level: level + 1)
            }.joined(separator: ",\n") + "\n" + outer + "}"
        default:
            return serialized(ensureASCII: ensureASCII)
        }
    }

    private func write(to output: inout String, ensureASCII: Bool, item: String, key: String, sortKeys: Bool) {
        switch self {
        case .null: output += "null"
        case .bool(let value): output += value ? "true" : "false"
        case .int(let value): output += String(value)
        case .double(let value): output += pythonRepr(value)
        case .string(let value): writeString(value, to: &output, ensureASCII: ensureASCII)
        case .array(let items):
            output += "["
            for (offset, element) in items.enumerated() {
                if offset > 0 { output += item }
                element.write(to: &output, ensureASCII: ensureASCII, item: item, key: key, sortKeys: sortKeys)
            }
            output += "]"
        case .object(let object):
            output += "{"
            let names = sortKeys ? object.keys.sorted { $0.unicodeScalars.lexicographicallyPrecedes($1.unicodeScalars) } : object.keys
            for (offset, name) in names.enumerated() {
                if offset > 0 { output += item }
                writeString(name, to: &output, ensureASCII: ensureASCII)
                output += key
                object[name]!.write(to: &output, ensureASCII: ensureASCII, item: item, key: key, sortKeys: sortKeys)
            }
            output += "}"
        }
    }

    private func writeString(_ value: String, to output: inout String, ensureASCII: Bool) {
        output += "\""
        for scalar in value.unicodeScalars {
            switch scalar {
            case "\"": output += "\\\""
            case "\\": output += "\\\\"
            case "\n": output += "\\n"
            case "\r": output += "\\r"
            case "\t": output += "\\t"
            case "\u{08}": output += "\\b"
            case "\u{0C}": output += "\\f"
            default:
                if scalar.value < 0x20 || (ensureASCII && scalar.value > 0x7E) {
                    if scalar.value > 0xFFFF {
                        let offset = scalar.value - 0x10000
                        output += hexEscape(0xD800 + (offset >> 10)) + hexEscape(0xDC00 + (offset & 0x3FF))
                    } else {
                        output += hexEscape(scalar.value)
                    }
                } else {
                    output.unicodeScalars.append(scalar)
                }
            }
        }
        output += "\""
    }

    private func hexEscape(_ value: UInt32) -> String {
        let digits = String(value, radix: 16)
        return "\\u" + String(repeating: "0", count: 4 - digits.count) + digits
    }
}

/// Python's repr() of a float: the shortest text that reads back the same, positional from 1e-4 up to 1e16
/// and in exponent form outside it.
public func pythonRepr(_ value: Double) -> String {
    if value.isNaN { return "NaN" }
    if value.isInfinite { return value < 0 ? "-Infinity" : "Infinity" }
    if value == 0 { return value.sign == .minus ? "-0.0" : "0.0" }
    // Swift's description is also the shortest round-trip form; take its digits and exponent and lay them out
    // by Python's rules.
    var text = Swift.abs(value).description
    var exponent = 0
    if let mark = text.firstIndex(where: { $0 == "e" || $0 == "E" }) {
        exponent = Int(text[text.index(after: mark)...])!
        text = String(text[..<mark])
    }
    var digits: String
    if let dot = text.firstIndex(of: ".") {
        let whole = text[..<dot], fraction = text[text.index(after: dot)...]
        exponent += whole.count - 1
        digits = String(whole + fraction)
    } else {
        exponent += text.count - 1
        digits = text
    }
    // Leading zeros (0.001 → "0001") move the exponent down.
    while digits.count > 1, digits.first == "0" { digits.removeFirst(); exponent -= 1 }
    while digits.count > 1, digits.last == "0" { digits.removeLast() }
    let sign = value < 0 ? "-" : ""
    if exponent < -4 || exponent >= 16 {
        let mantissa = digits.count > 1 ? String(digits.first!) + "." + digits.dropFirst() : digits
        let power = String(Swift.abs(exponent))
        return sign + mantissa + "e" + (exponent < 0 ? "-" : "+") + (power.count < 2 ? "0" + power : power)
    }
    if exponent < 0 {
        return sign + "0." + String(repeating: "0", count: -exponent - 1) + digits
    }
    if digits.count <= exponent + 1 {
        return sign + digits + String(repeating: "0", count: exponent + 1 - digits.count) + ".0"
    }
    let split = digits.index(digits.startIndex, offsetBy: exponent + 1)
    return sign + digits[..<split] + "." + digits[split...]
}
