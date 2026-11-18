import Foundation
#if os(Windows)
import WinSDK
#endif

/// The few operating-system calls the engine makes, the same on macOS and Windows: file locks, removing and
/// replacing files, process ids, short sleeps and stopping a helper program.
public enum Platform {
    /// An exclusive lock on a file; it is released when this is dropped, and by the system if the process dies.
    public final class Lock {
        #if os(Windows)
        private let handle: HANDLE
        init(_ handle: HANDLE) { self.handle = handle }
        deinit {
            var overlapped = OVERLAPPED()
            UnlockFileEx(handle, 0, 1, 0, &overlapped)
            CloseHandle(handle)
        }
        #else
        private let descriptor: Int32
        init(_ descriptor: Int32) { self.descriptor = descriptor }
        deinit { close(descriptor) }
        #endif
    }

    /// Takes the lock on path (created if missing); nil when another process holds it and `wait` is false.
    public static func lock(_ path: String, wait: Bool) -> Lock? {
        #if os(Windows)
        let handle = path.withCString(encodedAs: UTF16.self) {
            CreateFileW($0, DWORD(GENERIC_READ) | DWORD(GENERIC_WRITE), DWORD(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE), nil,
                        DWORD(OPEN_ALWAYS), DWORD(FILE_ATTRIBUTE_NORMAL), nil)
        }
        guard let handle, handle != INVALID_HANDLE_VALUE else { return nil }
        var overlapped = OVERLAPPED()
        let flags = DWORD(LOCKFILE_EXCLUSIVE_LOCK) | (wait ? 0 : DWORD(LOCKFILE_FAIL_IMMEDIATELY))
        guard LockFileEx(handle, flags, 0, 1, 0, &overlapped) else { CloseHandle(handle); return nil }
        return Lock(handle)
        #else
        let descriptor = open(path, O_CREAT | O_RDWR | O_APPEND, 0o600)
        guard descriptor >= 0 else { return nil }
        guard flock(descriptor, LOCK_EX | (wait ? 0 : LOCK_NB)) == 0 else { close(descriptor); return nil }
        return Lock(descriptor)
        #endif
    }

    /// Removes a file if it is there; a missing file or a failure is not an error, as unlink with the result ignored.
    public static func remove(_ path: String) {
        #if os(Windows)
        _ = path.withCString(encodedAs: UTF16.self) { DeleteFileW($0) }
        #else
        unlink(path)
        #endif
    }

    /// Moves source over destination in one step; false when it couldn't.
    @discardableResult
    public static func replace(_ source: String, _ destination: String) -> Bool { (try? Files.move(source, over: destination)) != nil }

    /// Renames source to a new name only if nothing is there yet (claiming a queued request).
    public static func claim(_ source: String, _ destination: String) -> Bool {
        #if os(Windows)
        return source.withCString(encodedAs: UTF16.self) { from in destination.withCString(encodedAs: UTF16.self) { MoveFileExW(from, $0, 0) } }
        #else
        return rename(source, destination) == 0
        #endif
    }

    /// A second name for an existing file; fails if the target exists. The way to publish a file only once.
    public static func hardLink(_ source: String, _ target: String) -> Bool {
        #if os(Windows)
        return source.withCString(encodedAs: UTF16.self) { from in target.withCString(encodedAs: UTF16.self) { CreateHardLinkW($0, from, nil) } }
        #else
        return link(source, target) == 0
        #endif
    }

    /// Owner-only permissions on POSIX; Windows keeps the folder's own access rules.
    public static func setMode(_ path: String, _ mode: Int) {
        #if !os(Windows)
        chmod(path, mode_t(mode))
        #endif
    }

    public static var processID: Int32 {
        #if os(Windows)
        return Int32(bitPattern: GetCurrentProcessId())
        #else
        return getpid()
        #endif
    }

    /// The user part of the cache folder's name: the uid, or "user" where there is none, as Python's getuid fallback.
    public static var userID: String {
        #if os(Windows)
        return "user"
        #else
        return String(getuid())
        #endif
    }

    public static func sleep(_ seconds: Double) { Thread.sleep(forTimeInterval: seconds) }

    /// Stops a helper program now.
    public static func kill(_ process: Process) {
        #if os(Windows)
        process.terminate()
        #else
        Darwin_or_Glibc_kill(process.processIdentifier, SIGKILL)
        #endif
    }

    /// Whether a process with this id is running.
    public static func alive(_ pid: Int) -> Bool {
        #if os(Windows)
        guard let handle = OpenProcess(DWORD(PROCESS_QUERY_LIMITED_INFORMATION), false, DWORD(pid)) else { return false }
        defer { CloseHandle(handle) }
        var code: DWORD = 0
        return GetExitCodeProcess(handle, &code) && code == 259  // STILL_ACTIVE
        #else
        return Darwin_or_Glibc_kill(pid_t(pid), 0) == 0
        #endif
    }

    /// When a file was last changed, in seconds since 1970, without following a link at path itself.
    public static func modified(_ path: String) -> Double? {
        guard let attributes = try? FileManager.default.attributesOfItem(atPath: path), let date = attributes[.modificationDate] as? Date else { return nil }
        return date.timeIntervalSince1970
    }
}

#if !os(Windows)
/// kill(2), named apart from Platform.kill.
@discardableResult
func Darwin_or_Glibc_kill(_ pid: pid_t, _ signal: Int32) -> Int32 { kill(pid, signal) }
#endif

/// memmem: whether `needle` occurs in `haystack`.
func containsBytes(_ haystack: UnsafeRawPointer, _ length: Int, _ needle: UnsafePointer<UInt8>, _ count: Int) -> Bool {
    #if os(Windows)
    guard count > 0 else { return true }
    guard length >= count else { return false }
    let bytes = haystack.assumingMemoryBound(to: UInt8.self), first = needle[0]
    var index = 0
    while index <= length - count {
        guard let hit = memchr(bytes + index, Int32(first), length - count - index + 1) else { return false }
        let start = bytes.distance(to: hit.assumingMemoryBound(to: UInt8.self))
        if memcmp(bytes + start, needle, count) == 0 { return true }
        index = start + 1
    }
    return false
    #else
    return memmem(haystack, length, needle, count) != nil
    #endif
}
