// Test helper, not shipped: reads {"call": name, "args": [...]} on stdin and writes the Swift engine's answer as
// JSON, so tests/test_engine.py can hold each ported function against the Python one.
import Foundation
import StarmeterCore

struct ProbeError: Error, CustomStringConvertible { let description: String }

func answer(_ call: String, _ args: [JSON]) throws -> JSON {
    func text(_ index: Int) -> String? { index < args.count ? args[index].string : nil }
    switch call {
    case "parse_iso":
        let time = try ISOTime.parse(text(0) ?? "")
        let zone = TimeZone(identifier: text(1) ?? "UTC") ?? TimeZone(identifier: "UTC")!
        return ["wall": .string(time.isoformat.prefix(19) + String(format: ".%06d", time.microsecond)),
                "offset": time.offset.map { .int(Int64($0)) } ?? .null,
                "timestamp": .double(time.timestamp(assuming: zone)),
                "isoformat": .string(time.isoformat)]
    case "keep_unreadable":
        return (try Files.keepUnreadable(text(0)!)).map(JSON.string) ?? .null
    case "preferences.read":
        return .object(Preferences.read(text(0)!))
    case "preferences.update":
        return .object(try Preferences.update(text(0)!, kind: text(1)!, key: text(2), value: text(3)))
    case "json.roundtrip":
        // Parses text(0) and writes it back with the separators given, as json.dumps would.
        let value = try JSON.parse(text(0)!)
        return .string(value.serialized(ensureASCII: args.count > 1 && args[1] == true,
                                        itemSeparator: text(2) ?? ", ", keySeparator: text(3) ?? ": "))
    case "python_repr":
        return .array(args.map { .string(pythonRepr($0.number ?? .nan)) })
    case "catalog":
        return ["manual": .array(Catalog.manualSubscriptions.map(JSON.string)),
                "portal": .array(Catalog.portalAPIs.map(JSON.string)),
                "balance": .array(Catalog.balanceAPIs.map(JSON.string)),
                "keyOnly": .array(Catalog.keyOnlyAPIs.map(JSON.string)),
                "labels": .array(Catalog.services.map { .string($0.label) }),
                "modules": .array(Preferences.modules.map { .array([.string($0.key), .string($0.label), .string($0.category)]) }),
                "credentials": .array(Catalog.services.map { .array(Catalog.credentialNames($0.key).map(JSON.string)) })]
    default:
        throw ProbeError(description: "unknown call \(call)")
    }
}

let input = FileHandle.standardInput.readDataToEndOfFile()
var output: JSON
do {
    guard let request = try JSON.parse(input).object, let call = request["call"]?.string else { throw ProbeError(description: "bad request") }
    output = ["result": try answer(call, request["args"]?.array ?? [])]
} catch {
    output = ["error": .string(String(describing: error))]
}
FileHandle.standardOutput.write(Data((output.serialized() + "\n").utf8))
