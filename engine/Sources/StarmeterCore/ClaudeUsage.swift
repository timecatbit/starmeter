import Foundation

/// Incremental, usage-only reader of Claude transcripts, the port of swiftbar_claude_usage.py. Response
/// deduplication, advisor records and cache-duration accounting are adapted from ccusage (MIT), commit
/// 615bd802; see lib/CCUSAGE_LICENSE. It never reads credentials and never keeps message text, paths or raw
/// response IDs; of a project it keeps only the folder name. The index file it writes is the same one the
/// Python engine writes, byte for byte, so either engine can pick up where the other left off.
public enum ClaudeUsage {
    /// One counted response, as indexed.
    public struct Record {
        public var key: [UInt8]
        public var message: [UInt8]?
        public var request: [UInt8]?
        public var session: [UInt8]
        public var ts: Double
        public var model: String
        /// input, output, 5-minute cache writes, 1-hour cache writes, cache reads.
        public var metrics: [Int64]
        public var cost: Double?
        public var sidechain: Bool
        public var speed: String?
        public var geo: String
        public var tier: String
        public var slot: Int64 = 0
        public var project: String?
        var file: [UInt8] = []
        var offset: Int64 = 0
    }

    /// A response once deduplicated: what range figures are made of.
    public struct Event: Equatable {
        public var ts: Double
        public var model: String
        public var metrics: [Int64]
        public var cost: Double?
        public var speed: String?
        public var geo: String?
        public var tier: String?
        public var session: [UInt8]?
        public var project: String?
    }

    struct Skip: Error {}

    /// 128-bit hashed identifiers keep the index compact; raw IDs and paths are never stored.
    public static func digest(_ values: JSON...) -> [UInt8] { digest(values) }

    public static func digest(_ values: [JSON]) -> [UInt8] {
        let text = JSON.array(values).serialized(itemSeparator: ",", keySeparator: ":")
        return Array(SHA256.hash(text.utf8).prefix(16))
    }

    static func counter(_ value: JSON?) throws -> Int64 {
        guard let value, !value.isNull else { return 0 }
        guard case .int(let number) = value, number >= 0 else { throw Skip() }
        return number
    }

    public static func normalize(_ usage: JSON?) throws -> [Int64] {
        guard let usage = usage?.object else { throw Skip() }
        let w5: Int64, w1: Int64
        if let breakdown = usage["cache_creation"]?.object {
            w5 = try counter(breakdown["ephemeral_5m_input_tokens"])
            w1 = try counter(breakdown["ephemeral_1h_input_tokens"])
        } else {
            w5 = try counter(usage["cache_creation_input_tokens"])
            w1 = 0
        }
        return [try counter(usage["input_tokens"]), try counter(usage["output_tokens"]), w5, w1, try counter(usage["cache_read_input_tokens"])]
    }

    /// The folder a request ran in, by its last name only.
    public static func projectName(_ value: JSON?) -> String? {
        guard let path = value?.string?.trimmingCharacters(in: .whitespacesAndNewlines), !path.isEmpty else { return nil }
        var trimmed = Substring(path)
        while let last = trimmed.last, last == "/" || last == "\\" { trimmed = trimmed.dropLast() }
        let name = trimmed.replacingOccurrences(of: "\\", with: "/").split(separator: "/", omittingEmptySubsequences: false).last.map(String.init) ?? ""
        return name.isEmpty ? nil : name
    }

    /// The responses one transcript line holds: the reply itself and any advisor calls inside it. Throws for a
    /// line that must be skipped whole.
    static func records(_ row: JSON, sessionFallback: String, location: (String, Int64)) throws -> [Record] {
        guard let row = row.object, row["type"] == .string("assistant"),
              let message = row["message"]?.object, let usage = message["usage"], usage.object != nil else { return [] }
        guard let model = message["model"]?.string, !model.isEmpty, model != "<synthetic>", model != "synthetic" else { return [] }
        let given = row["sessionId"]
        let sessionValue: JSON = given.map { $0.isTruthy } == true ? given! : .string(sessionFallback)
        guard let session = sessionValue.string else { return [] }
        guard let text = row["timestamp"]?.string ?? (row["timestamp"] == nil ? "" : nil) else { throw Skip() }
        let time = try ISOTime.parse(text)
        guard time.offset != nil else { throw Skip() }
        let stamp = time.timestamp()
        let request = row["requestId"], messageID = message["id"]
        if let request, !request.isNull, (request.string ?? "").isEmpty { return [] }
        if let messageID, !messageID.isNull, (messageID.string ?? "").isEmpty { return [] }
        let requestID = request?.string, identifierBase = messageID?.string

        var parts: [(String, JSON, JSON?)] = [(model, usage, row["costUSD"])]
        if let iterations = usage["iterations"]?.array {
            for item in iterations {
                if let advisor = item.object, advisor["type"] == .string("advisor_message"), let name = advisor["model"]?.string, !name.isEmpty {
                    parts.append((name, item, nil))
                }
            }
        }
        var output: [Record] = []
        for (slot, (partModel, counts, cost)) in parts.enumerated() {
            guard let metrics = try? normalize(counts) else { continue }
            var stored: Double?
            if let cost, cost.bool == nil, let number = cost.number, number.isFinite, number >= 0 { stored = number }
            if metrics.reduce(0, +) == 0 && stored == nil { continue }
            let identifier = identifierBase.map { $0 + (slot > 0 ? ":advisor:\(slot)" : "") }
            let key: [UInt8]
            if let requestID, let identifier {
                key = digest(.string(identifier), .string(requestID))
            } else if let identifier, let base = identifierBase {
                // Without a request ID, gateway placeholders are scoped to session + time.
                // msg_/resp_ IDs represent one streamed response across content blocks.
                key = digest(.string(identifier), .string(session), base.hasPrefix("msg_") || base.hasPrefix("resp_") ? .null : .double(stamp))
            } else {
                key = digest("location", [.string(location.0), .int(location.1)], .int(Int64(slot)))
            }
            let fields = counts.object!
            output.append(Record(
                key: key, message: identifier.map { digest(.string($0)) }, request: requestID.map { digest(.string($0)) },
                session: digest(.string(session)), ts: stamp, model: partModel, metrics: metrics, cost: stored,
                sidechain: row["isSidechain"]?.bool == true,
                speed: fields["speed"]?.string, geo: fields["inference_geo"]?.string ?? "global",
                tier: fields["service_tier"]?.string ?? "standard", slot: Int64(slot), project: projectName(row["cwd"])))
        }
        return output
    }

    /// Keeps complete responses; parent entries win over sidechain replays.
    static func deduplicate<S: Sequence>(_ events: S) -> [Record] where S.Element == Record {
        var output: [Record] = []
        var exact: [[UInt8]: Int] = [:]
        var routes: [[UInt8]: [Int]] = [:]
        func routeKey(_ row: Record) -> [UInt8] { (row.message ?? []) + [0xFF] + row.session }
        func outranks(_ a: Record, _ b: Record) -> Bool {
            // Python compares (not sidechain, tokens, has speed, has cost, -ts) as a tuple.
            let left: [Double] = [a.sidechain ? 0 : 1, Double(a.metrics.reduce(0, +)), a.speed != nil ? 1 : 0, a.cost != nil ? 1 : 0, -a.ts]
            let right: [Double] = [b.sidechain ? 0 : 1, Double(b.metrics.reduce(0, +)), b.speed != nil ? 1 : 0, b.cost != nil ? 1 : 0, -b.ts]
            for (x, y) in zip(left, right) where x != y { return x > y }
            return false
        }
        for row in events {
            var found = exact[row.key]
            if found == nil, row.message != nil {
                for index in routes[routeKey(row)] ?? [] {
                    let other = output[index]
                    if (row.sidechain || other.sidechain) && (row.ts == other.ts || (row.request == nil && other.request == nil)) {
                        found = index
                        break
                    }
                }
            }
            if let index = found {
                if outranks(row, output[index]) { output[index] = row }
            } else {
                found = output.count
                output.append(row)
            }
            exact[row.key] = found!
            if row.message != nil {
                let route = routeKey(row)
                if !(routes[route] ?? []).contains(found!) { routes[route, default: []].append(found!) }
            }
        }
        return output
    }

    static let columns = "key,message,request,session,ts,model,i,o,w5,w1,r,cost,sidechain,speed,geo,tier,project"

    static func storedRecord(_ row: [SQLValue]) -> Record {
        Record(key: row[0].blob ?? [], message: row[1].blob, request: row[2].blob, session: row[3].blob ?? [], ts: row[4].double ?? 0,
               model: row[5].text ?? "", metrics: (6...10).map { row[$0].int ?? 0 }, cost: row[11].double,
               sidechain: (row[12].int ?? 0) != 0, speed: row[13].text, geo: row[14].text ?? "", tier: row[15].text ?? "",
               project: row[16].text)
    }

    /// Appended responses sharing no ID or route with indexed ones form their own dedupe groups.
    static func isolated(_ db: Database, _ added: [Record], _ allowed: Set<[UInt8]>?) throws -> Bool {
        let fresh = Set(added.map { FileSlot(file: $0.file, offset: $0.offset, slot: $0.slot) })
        func clash(_ sql: String, _ arguments: [SQLValue]) throws -> Bool {
            for row in try db.rows(sql, arguments) {
                let slot = FileSlot(file: row[0].blob ?? [], offset: row[1].int ?? 0, slot: row[2].int ?? 0)
                if !fresh.contains(slot) && (allowed == nil || allowed!.contains(row[3].blob ?? [])) { return true }
            }
            return false
        }
        for row in added {
            if try clash("SELECT file,offset,slot,session FROM everything WHERE key=?", [.blob(row.key)]) { return false }
            if let message = row.message,
               try clash("SELECT file,offset,slot,session FROM everything WHERE message=? AND session=?", [.blob(message), .blob(row.session)]) {
                return false
            }
        }
        return true
    }

    struct FileSlot: Hashable { var file: [UInt8]; var offset: Int64; var slot: Int64 }

    static func storeCanonical(_ db: Database, _ rows: [Record], first: Int64 = 0) throws {
        let insert = try db.prepare("INSERT INTO canonical VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?)")
        for (index, row) in rows.enumerated() {
            try insert.run([.int(first + Int64(index)), .double(row.ts), .text(row.model)] + row.metrics.map(SQLValue.int)
                           + [row.cost.sql, row.speed.sql, .text(row.geo), .text(row.tier), .blob(row.session), row.project.sql])
        }
    }

    /// The file facts the index compares: inode, size and modification time in nanoseconds, as os.stat gives them.
    struct FileState: Equatable { var inode: Int64; var size: Int64; var mtime: Int64 }

    static func state(of path: String) -> FileState? {
        #if os(Windows)
        guard let attributes = try? FileManager.default.attributesOfItem(atPath: path) else { return nil }
        let inode = (attributes[.systemFileNumber] as? NSNumber)?.int64Value ?? 0
        let size = (attributes[.size] as? NSNumber)?.int64Value ?? 0
        let mtime = Int64(((attributes[.modificationDate] as? Date)?.timeIntervalSince1970 ?? 0) * 1_000_000_000)
        return FileState(inode: inode, size: size, mtime: mtime)
        #else
        var info = stat()
        guard stat(path, &info) == 0 else { return nil }
        #if os(macOS)
        let mtime = Int64(info.st_mtimespec.tv_sec) * 1_000_000_000 + Int64(info.st_mtimespec.tv_nsec)
        #else
        let mtime = Int64(info.st_mtim.tv_sec) * 1_000_000_000 + Int64(info.st_mtim.tv_nsec)
        #endif
        return FileState(inode: Int64(bitPattern: UInt64(info.st_ino)), size: Int64(info.st_size), mtime: mtime)
        #endif
    }

    /// Reindexes only changed files and keeps only normalized counters and hashed IDs. `archive` is a durable
    /// file that keeps the records of transcripts that disappear (Claude Code cleans up after 30 days); they are
    /// deduplicated together with the live ones, so a response that survives in another file still counts once.
    /// Returns the deduplicated responses between start and end, and how many files could not be read.
    public static func readEvents(files: [String], database path: String, allowedSessions: Set<String>? = nil,
                                  start: Double = 0, end: Double = .infinity, archive: String? = nil) throws -> (events: [Event], failures: Int) {
        let archive = archive.flatMap { $0.isEmpty ? nil : $0 }
        try Files.makeDirectory((path as NSString).deletingLastPathComponent)
        let db = try Database(path: path)
        Files.ownerOnly(path)
        var failures = 0, rebuild = false
        var appended: [Record] = []
        do {
            try db.execute("""
                CREATE TABLE IF NOT EXISTS files(path BLOB PRIMARY KEY,inode INTEGER,size INTEGER,mtime INTEGER,offset INTEGER);
                CREATE TABLE IF NOT EXISTS records(file BLOB,offset INTEGER,slot INTEGER,key BLOB,message BLOB,request BLOB,session BLOB,ts REAL,model TEXT,
                  i INTEGER,o INTEGER,w5 INTEGER,w1 INTEGER,r INTEGER,cost REAL,sidechain INTEGER,speed TEXT,geo TEXT,tier TEXT,project TEXT,PRIMARY KEY(file,offset,slot));
                CREATE INDEX IF NOT EXISTS records_key ON records(key);
                CREATE INDEX IF NOT EXISTS records_route ON records(message,session);
                CREATE TABLE IF NOT EXISTS metadata(key TEXT PRIMARY KEY,value TEXT);
                CREATE TABLE IF NOT EXISTS canonical(id INTEGER PRIMARY KEY,ts REAL,model TEXT,i INTEGER,o INTEGER,w5 INTEGER,w1 INTEGER,r INTEGER,cost REAL,speed TEXT,geo TEXT,tier TEXT,session BLOB,project TEXT);
                CREATE INDEX IF NOT EXISTS canonical_ts ON canonical(ts);
                """)
            if let archive {
                try Files.makeDirectory((archive as NSString).deletingLastPathComponent)
                try db.run("ATTACH DATABASE ? AS kept", [.text(archive)])
                try db.execute("""
                    CREATE TABLE IF NOT EXISTS kept.records(file BLOB,offset INTEGER,slot INTEGER,key BLOB,message BLOB,request BLOB,session BLOB,ts REAL,model TEXT,
                      i INTEGER,o INTEGER,w5 INTEGER,w1 INTEGER,r INTEGER,cost REAL,sidechain INTEGER,speed TEXT,geo TEXT,tier TEXT,project TEXT,PRIMARY KEY(file,offset,slot))
                    """)
                // Archives written before records knew their project gain the column, empty for what they hold.
                if !(try db.rows("PRAGMA kept.table_info(records)")).contains(where: { $0[1] == .text("project") }) {
                    try db.execute("ALTER TABLE kept.records ADD COLUMN project TEXT")
                }
                Files.ownerOnly(archive)
                // A transcript that comes back reads as itself again.
                try db.execute("CREATE TEMP VIEW everything AS SELECT * FROM main.records UNION ALL SELECT * FROM kept.records WHERE file NOT IN (SELECT path FROM main.files)")
            } else {
                try db.execute("CREATE TEMP VIEW everything AS SELECT * FROM main.records")
            }
            try db.execute("BEGIN")
            // In pathlib's order: by path components.
            let paths = Set(files.map { Paths.resolve($0) ?? $0 }).sorted { a, b in
                a.split(separator: "/").map(String.init).lexicographicallyPrecedes(b.split(separator: "/").map(String.init), by: pythonLess)
            }
            let active = Set(paths.map { digest(.string($0)) })
            for row in try db.rows("SELECT path FROM files") {
                guard let key = row[0].blob, !active.contains(key) else { continue }
                rebuild = true
                if archive != nil { try db.run("INSERT OR IGNORE INTO kept.records SELECT * FROM main.records WHERE file=?", [.blob(key)]) }
                try db.run("DELETE FROM records WHERE file=?", [.blob(key)])
                try db.run("DELETE FROM files WHERE path=?", [.blob(key)])
            }
            let insert = try db.prepare("INSERT OR REPLACE INTO records VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)")
            for path in paths {
                let key = digest(.string(path))
                guard let state = state(of: path) else { failures += 1; continue }
                let old = try db.first("SELECT inode,size,mtime,offset FROM files WHERE path=?", [.blob(key)])
                // A transcript the archive holds has come back: its records replace the archived ones.
                if archive != nil, old == nil, try db.first("SELECT 1 FROM kept.records WHERE file=? LIMIT 1", [.blob(key)]) != nil { rebuild = true }
                let previous = old.map { FileState(inode: $0[0].int ?? -1, size: $0[1].int ?? -1, mtime: $0[2].int ?? -1) }
                if previous == state { continue }
                let incremental = previous.map { $0.inode == state.inode && state.size > $0.size } ?? false
                let offset = incremental ? (old![3].int ?? 0) : 0
                let parts = path.split(separator: "/").map(String.init)
                let stem = ((path as NSString).lastPathComponent as NSString).deletingPathExtension
                let fallback = allowedSessions.flatMap { allowed in parts.reversed().first(where: allowed.contains) } ?? stem
                var added: [Record] = []
                try db.execute("SAVEPOINT source")
                do {
                    if !incremental { try db.run("DELETE FROM records WHERE file=?", [.blob(key)]) }
                    let end = try LineReader.read(path, from: offset) { position, line in
                        guard contains(line, Self.usageWord), contains(line, Self.assistantWord) else { return }
                        guard let row = try? JSON.parse(withoutByteOrderMark(line), keep: Self.wanted, surrogates: true),
                              let found = try? records(row, sessionFallback: fallback, location: (key.hexString, position)) else { return }
                        for var event in found {
                            try insert.run([.blob(key), .int(position), .int(event.slot), .blob(event.key), event.message.sql, event.request.sql,
                                            .blob(event.session), .double(event.ts), .text(event.model)] + event.metrics.map(SQLValue.int)
                                           + [event.cost.sql, .int(event.sidechain ? 1 : 0), event.speed.sql, .text(event.geo), .text(event.tier), event.project.sql])
                            event.file = key
                            event.offset = position
                            added.append(event)
                        }
                    }
                    try db.run("INSERT OR REPLACE INTO files VALUES (?,?,?,?,?)",
                               [.blob(key), .int(state.inode), .int(state.size), .int(state.mtime), .int(end)])
                    try db.execute("RELEASE source")
                    // A rewritten file replaces earlier rows; a new or grown file only adds rows.
                    if old != nil && !incremental { rebuild = true }
                    appended += added
                } catch is LineReader.Unreadable {
                    try db.execute("ROLLBACK TO source")
                    try db.execute("RELEASE source")
                    failures += 1
                } catch {
                    try? db.execute("ROLLBACK TO source")
                    try? db.execute("RELEASE source")
                    throw error
                }
            }
            let allowed = allowedSessions.map { Set($0.map { digest(.string($0)) }) }
            // Switching the archive on or off changes what is deduplicated, so it starts the dedupe over.
            let scopeSessions: JSON = allowedSessions.map { .array($0.sorted(by: pythonLess).map(JSON.string)) } ?? .null
            let scope = digest(scopeSessions, .bool(archive != nil)).hexString
            let oldScope = try db.first("SELECT value FROM metadata WHERE key='dedupe-scope'")?[0].text
            let added = appended.filter { allowed == nil || allowed!.contains($0.session) }.sorted { a, b in
                if a.file != b.file { return a.file.lexicographicallyPrecedes(b.file) }
                if a.offset != b.offset { return a.offset < b.offset }
                return a.slot < b.slot
            }
            if try rebuild || oldScope == nil || oldScope != scope || (!added.isEmpty && !isolated(db, added, allowed)) {
                // Deduplicate before date filtering, once per file/ownership change.
                var stored: [Record] = []
                try db.each("SELECT \(columns) FROM everything ORDER BY file,offset,slot") { row in
                    if allowed == nil || allowed!.contains(row[3].blob ?? []) { stored.append(storedRecord(row)) }
                }
                let unique = deduplicate(stored)
                try db.run("DELETE FROM canonical")
                try storeCanonical(db, unique)
                try db.run("INSERT OR REPLACE INTO metadata VALUES ('dedupe-scope',?)", [.text(scope)])
            } else if !added.isEmpty {
                let next = try db.first("SELECT COALESCE(MAX(id)+1,0) FROM canonical")?[0].int ?? 0
                try storeCanonical(db, deduplicate(added), first: next)
            }
            try db.execute("COMMIT")
            let events = try db.rows("SELECT ts,model,i,o,w5,w1,r,cost,speed,geo,tier,session,project FROM canonical WHERE ts>=? AND ts<=?",
                                     [.double(start), .double(end)]).map { row in
                Event(ts: row[0].double ?? 0, model: row[1].text ?? "", metrics: (2...6).map { row[$0].int ?? 0 }, cost: row[7].double,
                      speed: row[8].text, geo: row[9].text, tier: row[10].text, session: row[11].blob, project: row[12].text)
            }
            return (events, failures)
        } catch {
            try? db.execute("ROLLBACK")
            throw error
        }
    }

    /// The only parts of a transcript line the reader looks at; message text and the rest are checked, not kept.
    static let wanted: JSONSelection = .keys([
        "type": .all, "sessionId": .all, "timestamp": .all, "requestId": .all, "costUSD": .all, "isSidechain": .all, "cwd": .all,
        "message": .keys(["model": .all, "id": .all, "usage": .all]),
    ])

    static let usageWord = Array("\"usage\"".utf8)
    static let assistantWord = Array("\"assistant\"".utf8)

    /// Python orders str by code point; Swift's String < compares by Unicode canonical ordering.
    static func pythonLess(_ a: String, _ b: String) -> Bool { a.unicodeScalars.lexicographicallyPrecedes(b.unicodeScalars) }
}

extension JSON {
    /// Python truthiness, for `x or fallback`.
    var isTruthy: Bool {
        switch self {
        case .null: return false
        case .bool(let value): return value
        case .int(let value): return value != 0
        case .double(let value): return value != 0
        case .string(let value): return !value.isEmpty
        case .array(let items): return !items.isEmpty
        case .object(let object): return !object.isEmpty
        }
    }
}

/// Complete lines of a file from an offset: a line Python's readline(32 MiB) would return whole and ending in a
/// newline. Reading stops before a line still being written (no newline yet) or one longer than 32 MiB, and
/// returns where the next read starts. Each line is lent to `body` only for the call.
enum LineReader {
    struct Unreadable: Error {}
    static let limit = 32 * 1024 * 1024

    static func read(_ path: String, from offset: Int64, _ body: (Int64, UnsafeBufferPointer<UInt8>) throws -> Void) throws -> Int64 {
        guard let handle = FileHandle(forReadingAtPath: path) else { throw Unreadable() }
        defer { try? handle.close() }
        do { try handle.seek(toOffset: UInt64(offset)) } catch { throw Unreadable() }
        var buffer: [UInt8] = []
        var bufferStart = offset
        var done = false
        while true {
            // Hand out every complete line in the buffer.
            let (used, stop) = try buffer.withUnsafeBufferPointer { all -> (Int, Bool) in
                var lineStart = 0
                while lineStart < all.count,
                      let hit = memchr(all.baseAddress! + lineStart, 0x0A, all.count - lineStart) {
                    let newline = all.baseAddress!.distance(to: hit.assumingMemoryBound(to: UInt8.self))
                    if newline - lineStart + 1 > limit { return (lineStart, true) }
                    try body(bufferStart + Int64(lineStart), UnsafeBufferPointer(rebasing: all[lineStart...newline]))
                    lineStart = newline + 1
                }
                return (lineStart, false)
            }
            if used > 0 {
                buffer.removeFirst(used)
                bufferStart += Int64(used)
            }
            if stop || buffer.count >= limit || done { return bufferStart }
            let chunk: Data
            do { chunk = try handle.read(upToCount: 4 << 20) ?? Data() } catch { throw Unreadable() }
            if chunk.isEmpty { done = true } else { buffer.append(contentsOf: chunk) }
        }
    }
}

extension LineReader {
    /// The same lines from bytes already in memory (a decompressed log), starting at `offset`.
    static func lines(_ bytes: UnsafeBufferPointer<UInt8>, from offset: Int, _ body: (Int64, UnsafeBufferPointer<UInt8>) throws -> Void) throws {
        var lineStart = offset
        guard let base = bytes.baseAddress else { return }
        while lineStart < bytes.count, let hit = memchr(base + lineStart, 0x0A, bytes.count - lineStart) {
            let newline = base.distance(to: hit.assumingMemoryBound(to: UInt8.self))
            if newline - lineStart + 1 > limit { return }
            try body(Int64(lineStart), UnsafeBufferPointer(rebasing: bytes[lineStart...newline]))
            lineStart = newline + 1
        }
    }
}

/// Whether the bytes contain `word`.
func contains(_ bytes: UnsafeBufferPointer<UInt8>, _ word: [UInt8]) -> Bool {
    guard let first = word.first, bytes.count >= word.count, let base = bytes.baseAddress else { return false }
    #if !os(Windows)
    return word.withUnsafeBufferPointer { memmem(base, bytes.count, $0.baseAddress, word.count) != nil }
    #else
    var index = 0
    let last = bytes.count - word.count
    while index <= last, let hit = memchr(base + index, Int32(first), last - index + 1) {
        let at = base.distance(to: hit.assumingMemoryBound(to: UInt8.self))
        if memcmp(base + at, word, word.count) == 0 { return true }
        index = at + 1
    }
    return false
    #endif
}

/// The line without a leading UTF-8 byte-order mark, which json.loads on bytes also drops.
func withoutByteOrderMark(_ bytes: UnsafeBufferPointer<UInt8>) -> UnsafeBufferPointer<UInt8> {
    if bytes.count >= 3, bytes[0] == 0xEF, bytes[1] == 0xBB, bytes[2] == 0xBF { return UnsafeBufferPointer(rebasing: bytes[3...]) }
    return bytes
}
