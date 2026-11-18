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
    case "harness.gateway":
        let log = try Harness.gatewayRequests(path: text(0)!, cache: text(1)!, start: args[2].number ?? 0, end: args[3].number ?? .infinity, kind: text(4)!)
        return .array(log.items.map { agent, requests in
            .array([.string(agent), .array(requests.sorted { [$0.key.input, $0.key.cached, $0.key.writes, $0.key.output].lexicographicallyPrecedes([$1.key.input, $1.key.cached, $1.key.writes, $1.key.output]) }.map { split, times in
                .array([.int(split.input), .int(split.cached), .int(split.writes), .int(split.output), .array(times.map(JSON.double))])
            })])
        })
    case "harness.read":
        // [source, root, cache, start, end, zone, excludeProxy, archive, carriers]; carriers: [[name, log path or null, kind, agents or null]]
        let source = text(0)!, zone = TimeZone(identifier: text(5) ?? "UTC")!
        let start = args[3].number ?? 0, end = args[4].number ?? .infinity
        var carriers: [Carrier] = []
        for spec in args[8].array ?? [] {
            let fields = spec.array ?? []
            guard let name = fields[0].string else { continue }
            if let path = fields[1].string {
                let log = try Harness.gatewayRequests(path: path, cache: text(2)!, start: start, end: end, kind: fields[2].string ?? "opencodex")
                carriers.append((name, Harness.requestsFrom(log, agents: fields[3].array.map { Set($0.compactMap(\.string)) })))
            } else {
                carriers.append((name, nil))
            }
        }
        let excludeProxy = args[6] == .bool(true)
        let carried = carriers.isEmpty ? nil : carriers
        switch source {
        case "gemini":
            return .object(try Harness.geminiUsage(root: text(1)!, cache: text(2)!, start: start, end: end, zone: zone, excludeProxy: excludeProxy, carriers: carried, archive: text(7)))
        case "opencode":
            return .object(try Harness.opencodeUsage(root: text(1)!, start: start, end: end, zone: zone, excludeProxy: excludeProxy, carriers: carried))
        default:
            return .object(try Harness.jsonlUsage(source: source, root: text(1)!, cache: text(2)!, start: start, end: end, zone: zone, excludeProxy: excludeProxy,
                                                  carriers: carried, price: source == "magpie" ? Harness.magpiePrice : nil, archive: text(7)))
        }
    case "range.usage":
        let runtime = Runtime()
        let (payload, error) = runtime.render { runtime.rangeUsage(text(0), ttl: args.count > 1 ? args[1].number : nil) }
        return ["payload": payload.map(JSON.object) ?? .null, "error": error.map(JSON.string) ?? .null, "cache": .string(runtime.rangeCacheName(text(0)))]
    case "range.bounds":
        let runtime = Runtime()
        let zone = TimeZone(identifier: text(2) ?? "UTC")!
        let (start, end) = runtime.rangeBounds(text(0)!, now: args[1].number!, zone: zone)
        return [.double(start), .double(end)]
    case "dashboard":
        // [data, now, range error]
        let runtime = Runtime()
        return .object(runtime.render { runtime.dashboardPayload(args[0].object ?? JSONObject(), rangeError: text(2), now: args[1].number ?? Date().timeIntervalSince1970) })
    case "accounts.account":
        // [source, key or null, options, environment, routes]
        for (from, to) in args[4].object ?? JSONObject() { HTTP.routes[from] = to.string }
        var environment = ProcessInfo.processInfo.environment
        for (name, value) in args[3].object ?? JSONObject() { environment[name] = value.string }
        return .object(Accounts.account(text(0)!, key: text(1), options: args[2].object ?? JSONObject(), environment: environment))
    case "accounts.check":
        for (from, to) in args[2].object ?? JSONObject() { HTTP.routes[from] = to.string }
        return .object(Accounts.checkKey(text(0)!, key: text(1)!))
    case "runtime":
        // [what, routes, ...]: a Runtime call against the environment this probe runs in.
        for (from, to) in args[1].object ?? JSONObject() { HTTP.routes[from] = to.string }
        let runtime = Runtime()
        return runtime.render { () -> JSON in
            switch text(0)! {
            case "provider_quotas": let (payload, error) = runtime.providerQuotas(); return ["payload": payload.map(JSON.object) ?? .null, "error": error.map(JSON.string) ?? .null]
            case "komari": let (payload, error) = runtime.komariSnapshot(); return ["payload": payload.map(JSON.object) ?? .null, "error": error.map(JSON.string) ?? .null]
            case "opencodex_range": return .object(runtime.opencodexRangeUsageLive(text(2)!, now: args[3].number!))
            case "loaders": return .array(runtime.externalLoaders().map { .string($0.0) })
            case "ready": return .object(JSONObject(Preferences.modules.map { ($0.key, .bool(runtime.defaultReady($0.key))) }))
            case "status": return .object(ServiceStatus.readAll(args[2].array?.compactMap(\.string) ?? [], version: "test"))
            case "fx": let (payload, error) = runtime.exchangeRateLoad(); return ["payload": payload.map(JSON.object) ?? .null, "error": error.map(JSON.string) ?? .null]
            case "opencodex_refresh":
                runtime.opencodexRefresh()
                return .object(JSONObject(Runtime.opencodexEndpoints.map { ($0.0, .object(runtime.readCache($0.0))) }))
            default: return "unknown"
            }
        }
    case "claude.parse":
        return .object(try ClaudeSubscription.parseUsage(args[0], now: args[1].number ?? 0))
    case "claude.desktop":
        return .object(ClaudeSubscription.desktop(root: text(0)!, now: args[1].number!))
    case "claude.local":
        return ClaudeSubscription.localSnapshot(text(0)!, now: args[1].number!).map(JSON.object) ?? .null
    case "claude.oauth":
        // [state, force, requestedAt, routes]
        for (from, to) in args[3].object ?? JSONObject() { HTTP.routes[from] = to.string }
        let (payload, error) = ClaudeSubscription.oauthFetch(state: text(0)!, consent: true, force: args[1] == .bool(true), requestedAt: args[2].number ?? 0)
        return ["payload": .object(payload), "error": error.map(JSON.string) ?? .null]
    case "qoder.snapshot":
        return Qoder.snapshot(home: text(0)!, now: args[1].number!).map(JSON.object) ?? .null
    case "chatgpt.account":
        return .object(ChatGPT.account(binary: text(0), home: text(1)))
    case "cursor.refresh":
        // [store or null, home, start, until, now, routes]
        for (from, to) in args[5].object ?? JSONObject() { HTTP.routes[from] = to.string }
        do { return .object(try Cursor.refresh(args[0].object, home: text(1)!, start: args[2].number!, until: args[3].number!, now: args[4].number!)) }
        catch let failure as Cursor.Failure { return ["failure": .string(failure.description)] }
    case "cursor.plan":
        for (from, to) in args[2].object ?? JSONObject() { HTTP.routes[from] = to.string }
        do { return .object(try Cursor.plan(home: text(0)!, now: args[1].number!)) }
        catch let failure as Cursor.Failure { return ["failure": .string(failure.description)] }
    case "sessions.active":
        // [claude roots, codex root, now, with prices, claude app map]
        let runtime = Runtime()
        var app: [String: (title: String?, effort: String?)] = [:]
        for (key, value) in args[4].object ?? JSONObject() { app[key] = (value["title"]?.string, value["effort"]?.string) }
        return .array(Sessions.active(claudeRoots: args[0].array?.compactMap(\.string) ?? [], codexRoot: text(1), now: args[2].number!,
                                      prices: args[3] == .bool(true) ? runtime.sessionPrices() : nil, claudeApp: app))
    case "updates.pieces":
        // Small pure pieces: versions, CLI output, the checker ID, appcasts and Codex's channel.
        return ["versions": .array(args[0].array!.map { Updates.releaseVersion($0).map { .array($0.map { .int(Int64($0)) }) } ?? .null }),
                "first": .array(args[1].array!.map { Runtime.firstVersion($0.string!).map(JSON.string) ?? .null }),
                "uuid": .string(Runtime.uuid5URL("opencodex-swiftbar-update-check")),
                "sparkle": .array(args[2].array!.map { let (a, b) = Updates.sparkleRelease(Data($0.string!.utf8)); return [a.map(JSON.string) ?? .null, b.map(JSON.string) ?? .null] }),
                "channels": .array(args[3].array!.map { .object(Updates.codexChannel($0.string!)) })]
    case "zstd":
        // The SHA-256 and size of each file's decompressed bytes.
        return .array(args.map { path in
            do {
                let data = try Zstd.decompress(file: path.string ?? "")
                return ["sha256": .string(SHA256.hex(data)), "size": .int(Int64(data.count))]
            } catch {
                return ["error": .string(String(describing: error))]
            }
        })
    case "sha256":
        return .string(SHA256.hex(Array((text(0) ?? "").utf8)))
    case "claude.digest":
        return .string(ClaudeUsage.digest(args).hexString)
    case "claude.price":
        let metrics = args[1].array?.compactMap { $0.integer } ?? []
        return ClaudePricing.cost(model: text(0) ?? "", metrics: metrics, speed: text(2) ?? "standard", geo: text(3) ?? "global",
                                  tier: text(4) ?? "standard").map(JSON.double) ?? .null
    case "claude.read_events":
        let files = args[0].array?.compactMap(\.string) ?? []
        let allowed = args[2].array.map { Set($0.compactMap(\.string)) }
        let (events, failures) = try ClaudeUsage.readEvents(files: files, database: text(1)!, allowedSessions: allowed,
                                                            start: args[3].number ?? 0, end: args[4].number ?? .infinity, archive: text(5))
        return ["failures": .int(Int64(failures)), "events": .array(events.map { event in
            ["ts": .double(event.ts), "model": .string(event.model), "metrics": .array(event.metrics.map(JSON.int)),
             "cost": event.cost.map(JSON.double) ?? .null, "speed": event.speed.map(JSON.string) ?? .null,
             "geo": event.geo.map(JSON.string) ?? .null, "tier": event.tier.map(JSON.string) ?? .null,
             "session": event.session.map { .string($0.hexString) } ?? .null, "project": event.project.map(JSON.string) ?? .null]
        })]
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

if let folder = ProcessInfo.processInfo.environment["STARMETER_PRICING"] { Pricing.folders = [folder] }
let input = FileHandle.standardInput.readDataToEndOfFile()
var output: JSON
do {
    guard let request = try JSON.parse(input).object, let call = request["call"]?.string else { throw ProbeError(description: "bad request") }
    output = ["result": try answer(call, request["args"]?.array ?? [])]
} catch {
    output = ["error": .string(String(describing: error))]
}
FileHandle.standardOutput.write(Data((output.serialized() + "\n").utf8))
