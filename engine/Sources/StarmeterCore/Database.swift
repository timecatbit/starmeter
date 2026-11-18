import CSQLite
import Foundation

/// A value as SQLite stores it, bound and read the way Python's sqlite3 does: bytes are blobs, str is text.
public enum SQLValue: Equatable {
    case null
    case int(Int64)
    case double(Double)
    case text(String)
    case blob([UInt8])

    public var int: Int64? { if case .int(let value) = self { return value }; return nil }
    public var double: Double? {
        switch self {
        case .double(let value): return value
        case .int(let value): return Double(value)
        default: return nil
        }
    }
    public var text: String? { if case .text(let value) = self { return value }; return nil }
    public var blob: [UInt8]? { if case .blob(let value) = self { return value }; return nil }
    public var isNull: Bool { self == .null }
}

extension SQLValue: ExpressibleByIntegerLiteral, ExpressibleByFloatLiteral, ExpressibleByStringLiteral, ExpressibleByNilLiteral {
    public init(integerLiteral value: Int64) { self = .int(value) }
    public init(floatLiteral value: Double) { self = .double(value) }
    public init(stringLiteral value: String) { self = .text(value) }
    public init(nilLiteral: ()) { self = .null }
}

extension Optional where Wrapped == String {
    public var sql: SQLValue { map(SQLValue.text) ?? .null }
}
extension Optional where Wrapped == [UInt8] {
    public var sql: SQLValue { map(SQLValue.blob) ?? .null }
}
extension Optional where Wrapped == Double {
    public var sql: SQLValue { map(SQLValue.double) ?? .null }
}

public enum SQLiteFlags { case readWrite, readOnly, readOnlyURI }

public struct SQLiteError: Error, CustomStringConvertible {
    public let code: Int32
    public let message: String
    public var description: String { "SQLite error \(code): \(message)" }
}

/// One SQLite connection. Statements are prepared per call; `Statement` serves repeated inserts.
public final class Database {
    let handle: OpaquePointer

    /// Opens (creating) the file, waiting up to `timeout` seconds for another process's lock, as sqlite3.connect does.
    public init(path: String, timeout: Double = 10, flags: SQLiteFlags = .readWrite) throws {
        var connection: OpaquePointer?
        let mode: Int32
        switch flags {
        case .readWrite: mode = SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_FULLMUTEX
        case .readOnly: mode = SQLITE_OPEN_READONLY | SQLITE_OPEN_FULLMUTEX
        case .readOnlyURI: mode = SQLITE_OPEN_READONLY | SQLITE_OPEN_URI | SQLITE_OPEN_FULLMUTEX
        }
        let status = sqlite3_open_v2(path, &connection, mode, nil)
        guard status == SQLITE_OK, let connection else {
            let message = connection.map { String(cString: sqlite3_errmsg($0)) } ?? "cannot open"
            if let connection { sqlite3_close_v2(connection) }
            throw SQLiteError(code: status, message: message)
        }
        handle = connection
        sqlite3_busy_timeout(handle, Int32(timeout * 1000))
    }

    deinit { sqlite3_close_v2(handle) }

    func failure(_ code: Int32) -> SQLiteError { SQLiteError(code: code, message: String(cString: sqlite3_errmsg(handle))) }

    /// Runs one or more statements that take no arguments, like executescript.
    public func execute(_ sql: String) throws {
        let status = sqlite3_exec(handle, sql, nil, nil, nil)
        if status != SQLITE_OK { throw failure(status) }
    }

    public func prepare(_ sql: String) throws -> Statement { try Statement(self, sql) }

    /// Runs one statement and ignores any rows.
    public func run(_ sql: String, _ arguments: [SQLValue] = []) throws {
        let statement = try prepare(sql)
        try statement.bind(arguments)
        while try statement.step() {}
    }

    public func rows(_ sql: String, _ arguments: [SQLValue] = []) throws -> [[SQLValue]] {
        var result: [[SQLValue]] = []
        try each(sql, arguments) { result.append($0) }
        return result
    }

    public func first(_ sql: String, _ arguments: [SQLValue] = []) throws -> [SQLValue]? {
        let statement = try prepare(sql)
        try statement.bind(arguments)
        return try statement.step() ? statement.row() : nil
    }

    /// Streams rows; the statement is finalized when this returns or throws.
    public func each(_ sql: String, _ arguments: [SQLValue] = [], _ body: ([SQLValue]) throws -> Void) throws {
        let statement = try prepare(sql)
        try statement.bind(arguments)
        while try statement.step() { try body(statement.row()) }
    }

    public var changes: Int { Int(sqlite3_changes(handle)) }
}

public final class Statement {
    let database: Database
    let handle: OpaquePointer
    private static let transient = unsafeBitCast(-1, to: sqlite3_destructor_type.self)

    init(_ database: Database, _ sql: String) throws {
        self.database = database
        var statement: OpaquePointer?
        let status = sqlite3_prepare_v2(database.handle, sql, -1, &statement, nil)
        guard status == SQLITE_OK, let statement else { throw database.failure(status) }
        handle = statement
    }

    deinit { sqlite3_finalize(handle) }

    public func bind(_ arguments: [SQLValue]) throws {
        sqlite3_reset(handle)
        sqlite3_clear_bindings(handle)
        for (offset, value) in arguments.enumerated() {
            let index = Int32(offset + 1)
            let status: Int32
            switch value {
            case .null: status = sqlite3_bind_null(handle, index)
            case .int(let number): status = sqlite3_bind_int64(handle, index, number)
            case .double(let number): status = sqlite3_bind_double(handle, index, number)
            case .text(let text): status = sqlite3_bind_text(handle, index, text, -1, Self.transient)
            case .blob(let bytes):
                status = bytes.withUnsafeBytes { raw in
                    sqlite3_bind_blob(handle, index, raw.baseAddress ?? UnsafeRawPointer(bitPattern: 1), Int32(raw.count), Self.transient)
                }
            }
            if status != SQLITE_OK { throw database.failure(status) }
        }
    }

    /// Advances to the next row; false once the statement is done.
    public func step() throws -> Bool {
        let status = sqlite3_step(handle)
        if status == SQLITE_ROW { return true }
        if status == SQLITE_DONE { return false }
        throw database.failure(status)
    }

    /// Binds, runs to completion and resets, for one of many inserts.
    public func run(_ arguments: [SQLValue]) throws {
        try bind(arguments)
        while try step() {}
    }

    public func row() -> [SQLValue] {
        (0..<sqlite3_column_count(handle)).map { column in
            switch sqlite3_column_type(handle, column) {
            case SQLITE_INTEGER: return .int(sqlite3_column_int64(handle, column))
            case SQLITE_FLOAT: return .double(sqlite3_column_double(handle, column))
            case SQLITE_TEXT: return .text(String(cString: sqlite3_column_text(handle, column)))
            case SQLITE_BLOB:
                let count = Int(sqlite3_column_bytes(handle, column))
                guard count > 0, let bytes = sqlite3_column_blob(handle, column) else { return .blob([]) }
                return .blob(Array(UnsafeRawBufferPointer(start: bytes, count: count)))
            default: return .null
            }
        }
    }
}
