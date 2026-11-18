import Foundation

/// Running a helper program the engine relies on (pmset, security, the codex CLI…), with a time limit.
public enum Shell {
    /// The program's standard output, or nil when it could not run, failed, or ran past the timeout.
    public static func run(_ arguments: [String], timeout: Double, environment: [String: String]? = nil, input: Data? = nil) -> String? {
        guard let data = runData(arguments, timeout: timeout, environment: environment, input: input) else { return nil }
        return String(decoding: data, as: UTF8.self)
    }

    public static func runData(_ arguments: [String], timeout: Double, environment: [String: String]? = nil, input: Data? = nil) -> Data? {
        guard let executable = arguments.first else { return nil }
        let process = Process()
        process.executableURL = URL(fileURLWithPath: executable)
        process.arguments = Array(arguments.dropFirst())
        if let environment { process.environment = environment }
        let output = Pipe()
        process.standardOutput = output
        process.standardError = FileHandle.nullDevice
        let feed = input.map { _ in Pipe() }
        process.standardInput = feed ?? FileHandle.nullDevice
        do { try process.run() } catch { return nil }
        if let feed, let input { feed.fileHandleForWriting.write(input); try? feed.fileHandleForWriting.close() }
        let deadline = DispatchTime.now() + timeout
        let finished = DispatchSemaphore(value: 0)
        var data = Data()
        DispatchQueue.global().async {
            data = output.fileHandleForReading.readDataToEndOfFile()
            finished.signal()
        }
        if finished.wait(timeout: deadline) == .timedOut {
            process.terminate()
            return nil
        }
        process.waitUntilExit()
        return process.terminationStatus == 0 ? data : nil
    }

    /// Starts a program in its own session with no terminal attached, as Popen(start_new_session=True) does;
    /// it outlives this run.
    public static func detach(_ arguments: [String], environment: [String: String]? = nil) {
        guard let executable = arguments.first else { return }
        #if os(macOS)
        var attributes: posix_spawnattr_t?
        posix_spawnattr_init(&attributes)
        defer { posix_spawnattr_destroy(&attributes) }
        posix_spawnattr_setflags(&attributes, Int16(POSIX_SPAWN_SETSID))
        var actions: posix_spawn_file_actions_t?
        posix_spawn_file_actions_init(&actions)
        defer { posix_spawn_file_actions_destroy(&actions) }
        posix_spawn_file_actions_addopen(&actions, 0, "/dev/null", O_RDONLY, 0)
        posix_spawn_file_actions_addopen(&actions, 1, "/dev/null", O_WRONLY, 0)
        posix_spawn_file_actions_addopen(&actions, 2, "/dev/null", O_WRONLY, 0)
        let argv = arguments.map { strdup($0) } + [nil]
        defer { argv.forEach { free($0) } }
        let pairs = (environment ?? ProcessInfo.processInfo.environment).map { strdup($0.key + "=" + $0.value) } + [nil]
        defer { pairs.forEach { free($0) } }
        var pid: pid_t = 0
        _ = posix_spawn(&pid, executable, &actions, &attributes, argv, pairs)
        #else
        let process = Process()
        process.executableURL = URL(fileURLWithPath: executable)
        process.arguments = Array(arguments.dropFirst())
        if let environment { process.environment = environment }
        process.standardInput = FileHandle.nullDevice; process.standardOutput = FileHandle.nullDevice; process.standardError = FileHandle.nullDevice
        try? process.run()
        #endif
    }
}
