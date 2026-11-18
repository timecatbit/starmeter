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
}
