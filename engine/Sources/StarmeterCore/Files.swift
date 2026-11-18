import Foundation
#if os(Windows)
import WinSDK
#endif

/// File chores shared by every state file: atomic replacement, keeping an unreadable file aside, and paths
/// the way Python's pathlib spells them.
public enum Files {
    public struct Failure: Error, CustomStringConvertible {
        public let description: String
        init(_ description: String) { self.description = description }
    }

    /// Whether data is a JSON object in this version's schema 1.
    public static func schemaOne(_ data: Data) -> Bool {
        guard let object = (try? JSON.parse(data))?.object else { return false }
        return object["schema"] == .int(1)
    }

    /// Copies path aside when it exists and `readable` says no, so a fresh write loses nothing. Returns the
    /// copy's path. Throws when the copy can't be made, and the caller then doesn't write.
    @discardableResult
    public static func keepUnreadable(_ path: String, readable: (Data) -> Bool = schemaOne) throws -> String? {
        guard let data = FileManager.default.contents(atPath: path) else {
            if FileManager.default.fileExists(atPath: path) { throw Failure("cannot read \(path)") }
            return nil
        }
        if readable(data) { return nil }
        let url = URL(fileURLWithPath: path)
        let suffix = url.pathExtension.isEmpty ? "" : "." + url.pathExtension
        let stem = url.lastPathComponent.dropLast(suffix.count)
        let directory = url.deletingLastPathComponent().path
        let stamp = localStamp(Date())
        for attempt in 0..<100 {
            let name = "\(stem).unreadable-\(stamp)\(attempt > 0 ? "-\(attempt)" : "")\(suffix)"
            let aside = (directory as NSString).appendingPathComponent(name)
            switch createExclusively(aside, contents: data) {
            case .created: return aside
            case .exists: continue
            case .failed(let reason): throw Failure(reason)
            }
        }
        throw Failure("no free name to keep \(url.lastPathComponent) aside")
    }

    /// Writes data next to path under a hidden temporary name, then moves it over path in one step.
    public static func replaceAtomically(_ path: String, with data: Data, prefix: String, before: (() throws -> Void)? = nil) throws {
        let directory = (path as NSString).deletingLastPathComponent
        var temporary: String?
        for _ in 0..<100 {
            let candidate = (directory as NSString).appendingPathComponent(prefix + randomName())
            switch createExclusively(candidate, contents: data) {
            case .created: temporary = candidate
            case .exists: continue
            case .failed(let reason): throw Failure(reason)
            }
            break
        }
        guard let temporary else { throw Failure("no free temporary name in \(directory)") }
        defer { if FileManager.default.fileExists(atPath: temporary) { try? FileManager.default.removeItem(atPath: temporary) } }
        try before?()
        try move(temporary, over: path)
    }

    /// Makes a directory and its parents; new ones get `mode`, as Path.mkdir(parents=True, mode=...).
    public static func makeDirectory(_ path: String, mode: Int = 0o700) throws {
        try FileManager.default.createDirectory(atPath: path, withIntermediateDirectories: true,
                                                attributes: [.posixPermissions: mode])
    }

    /// Readable and writable by the owner only (0600), as os.chmod; Windows keeps its own ACLs.
    public static func ownerOnly(_ path: String) {
        #if !os(Windows)
        chmod(path, 0o600)
        #endif
    }

    enum Creation { case created, exists, failed(String) }

    /// Creates a new file readable only by its owner, failing if the name is taken.
    static func createExclusively(_ path: String, contents: Data) -> Creation {
        #if os(Windows)
        if FileManager.default.fileExists(atPath: path) { return .exists }
        return FileManager.default.createFile(atPath: path, contents: contents) ? .created : .failed("cannot write \(path)")
        #else
        let descriptor = open(path, O_WRONLY | O_CREAT | O_EXCL, 0o600)
        if descriptor < 0 { return errno == EEXIST ? .exists : .failed("cannot create \(path): \(String(cString: strerror(errno)))") }
        defer { close(descriptor) }
        let written = contents.withUnsafeBytes { raw -> Bool in
            var offset = 0
            while offset < raw.count {
                let count = write(descriptor, raw.baseAddress! + offset, raw.count - offset)
                if count <= 0 { return false }
                offset += count
            }
            return true
        }
        if !written { unlink(path); return .failed("cannot write \(path)") }
        return .created
        #endif
    }

    static func move(_ source: String, over destination: String) throws {
        #if os(Windows)
        let moved = source.withCString(encodedAs: UTF16.self) { from in
            destination.withCString(encodedAs: UTF16.self) { to in
                MoveFileExW(from, to, DWORD(MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH))
            }
        }
        if !moved { throw Failure("cannot replace \(destination)") }
        #else
        if rename(source, destination) != 0 { throw Failure("cannot replace \(destination): \(String(cString: strerror(errno)))") }
        #endif
    }

    static func randomName() -> String {
        let letters = Array("abcdefghijklmnopqrstuvwxyz0123456789_")
        return String((0..<8).map { _ in letters.randomElement()! })
    }

    /// time.strftime('%Y%m%d-%H%M%S') in the local zone.
    static func localStamp(_ date: Date) -> String {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = .current
        let c = calendar.dateComponents([.year, .month, .day, .hour, .minute, .second], from: date)
        return String(format: "%04d%02d%02d-%02d%02d%02d", c.year!, c.month!, c.day!, c.hour!, c.minute!, c.second!)
    }
}

/// Where things are, the way the Python engine looks them up.
public enum Paths {
    /// The home directory os.path.expanduser uses: HOME (USERPROFILE on Windows) when set.
    public static var home: String {
        let environment = ProcessInfo.processInfo.environment
        #if os(Windows)
        if let profile = environment["USERPROFILE"], !profile.isEmpty { return profile }
        #else
        if let home = environment["HOME"], !home.isEmpty { return home }
        #endif
        return FileManager.default.homeDirectoryForCurrentUser.path
    }

    /// Path.expanduser(): a leading ~ or ~/ becomes the home directory.
    public static func expandUser(_ path: String) -> String {
        if path == "~" { return home }
        if path.hasPrefix("~/") { return (home as NSString).appendingPathComponent(String(path.dropFirst(2))) }
        #if os(Windows)
        if path.hasPrefix("~\\") { return (home as NSString).appendingPathComponent(String(path.dropFirst(2))) }
        #endif
        return path
    }

    /// Path.resolve() of a path that exists: absolute, with links followed. nil when it doesn't resolve.
    public static func resolve(_ path: String) -> String? {
        #if os(Windows)
        let url = URL(fileURLWithPath: path).standardizedFileURL.resolvingSymlinksInPath()
        return FileManager.default.fileExists(atPath: url.path) ? url.path : nil
        #else
        // realpath, not URL.resolvingSymlinksInPath, which turns /private/tmp back into /tmp.
        guard let resolved = realpath(path, nil) else { return nil }
        defer { free(resolved) }
        return String(cString: resolved)
        #endif
    }

    public static func exists(_ path: String) -> Bool { FileManager.default.fileExists(atPath: path) }
}

/// Nanoseconds since 1970, as time.time_ns().
public func timeNanoseconds() -> Int64 {
    #if os(Windows)
    return Int64(Date().timeIntervalSince1970 * 1_000_000_000)
    #else
    var now = timespec()
    clock_gettime(CLOCK_REALTIME, &now)
    return Int64(now.tv_sec) * 1_000_000_000 + Int64(now.tv_nsec)
    #endif
}
