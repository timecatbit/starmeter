import Foundation
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif

/// Plain HTTP for the engine's few requests: one call at a time, a time limit, an optional ban on redirects (an
/// Authorization header must never follow one to another host), a cap on the body, and no cookies.
public enum HTTP {
    public struct Response {
        public let status: Int
        public let body: Data
        public let headers: [String: String]
        public func header(_ name: String) -> String? { headers.first { $0.key.lowercased() == name.lowercased() }?.value }
    }

    /// The request never got an answer: no connection, a timeout, a bad address.
    public struct Unreachable: Error, CustomStringConvertible {
        public let description: String
    }

    /// Tests point real hosts at a local server: "https://api.deepseek.com" → "http://127.0.0.1:8123".
    public static var routes: [String: String] = [:]

    final class Delegate: NSObject, URLSessionTaskDelegate {
        let follow: Bool
        init(follow: Bool) { self.follow = follow }
        func urlSession(_ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse,
                        newRequest request: URLRequest, completionHandler: @escaping (URLRequest?) -> Void) {
            completionHandler(follow ? request : nil)
        }
    }

    public static func send(_ address: String, method: String = "GET", headers: [String: String] = [:], body: Data? = nil, timeout: Double,
                            followRedirects: Bool = true, direct: Bool = false, limit: Int = 64 << 20) throws -> Response {
        var target = address
        for (from, to) in routes where address.hasPrefix(from) { target = to + address.dropFirst(from.count); break }
        guard let url = URL(string: target), let scheme = url.scheme, ["http", "https"].contains(scheme) else { throw Unreachable(description: "bad address") }
        var request = URLRequest(url: url, cachePolicy: .reloadIgnoringLocalAndRemoteCacheData, timeoutInterval: timeout)
        request.httpMethod = method
        request.httpBody = body
        request.httpShouldHandleCookies = false
        for (name, value) in headers { request.setValue(value, forHTTPHeaderField: name) }
        let configuration = URLSessionConfiguration.ephemeral
        configuration.timeoutIntervalForRequest = timeout
        configuration.timeoutIntervalForResource = timeout
        configuration.httpCookieStorage = nil
        configuration.urlCache = nil
        // Local addresses go straight there, never through a proxy.
        if direct { configuration.connectionProxyDictionary = [:] }
        let session = URLSession(configuration: configuration, delegate: Delegate(follow: followRedirects), delegateQueue: nil)
        defer { session.finishTasksAndInvalidate() }
        let done = DispatchSemaphore(value: 0)
        var result: Result<Response, Error> = .failure(Unreachable(description: "no answer"))
        let task = session.dataTask(with: request) { data, response, error in
            if let response = response as? HTTPURLResponse {
                var fields: [String: String] = [:]
                for (key, value) in response.allHeaderFields { fields[String(describing: key)] = String(describing: value) }
                result = .success(Response(status: response.statusCode, body: (data ?? Data()).prefix(limit), headers: fields))
            } else {
                result = .failure(Unreachable(description: error.map { ($0 as NSError).localizedDescription } ?? "no answer"))
            }
            done.signal()
        }
        task.resume()
        if done.wait(timeout: .now() + timeout + 5) == .timedOut {
            task.cancel()
            throw Unreachable(description: "timed out")
        }
        return try result.get()
    }

    /// An HTTP error status, as urllib raises HTTPError: the status and the start of the body.
    public struct Status: Error {
        public let code: Int
        public let body: Data
    }

    /// GET a JSON object with a bearer key, no redirects, and the first 2 MiB of the answer, as get_json does.
    static func getJSON(_ url: String, key: String) throws -> JSONObject {
        let response = try send(url, headers: ["Authorization": "Bearer " + key, "Accept": "application/json"], timeout: 8, followRedirects: false)
        guard (200..<300).contains(response.status) else { throw Status(code: response.status, body: response.body) }
        guard let object = (try? JSON.parse(response.body.prefix(2 * 1024 * 1024)))?.object else { throw Py.FileError("invalid account response") }
        return object
    }
}

/// Exact decimal arithmetic for money, as the Python engine does it with Decimal before turning it into a float.
struct Money {
    var value: Decimal

    /// Decimal(str(value)): numbers by their Python text, numeric strings as written. nil where Decimal raises.
    init?(_ json: JSON?) {
        guard let json else { return nil }
        let text: String
        switch json {
        case .int, .double: text = Py.str(json)
        case .string(let raw): text = raw.trimmingCharacters(in: .whitespaces)
        default: return nil
        }
        guard !text.isEmpty, text.allSatisfy({ $0.isASCII && ($0.isNumber || "+-.eE".contains($0)) }),
              let parsed = Decimal(string: text, locale: Locale(identifier: "en_US_POSIX")), !parsed.isNaN else { return nil }
        value = parsed
    }

    init(_ value: Decimal) { self.value = value }

    /// float(Decimal): the nearest double to the exact decimal.
    var double: Double { Double((value as NSDecimalNumber).description(withLocale: Locale(identifier: "en_US_POSIX"))) ?? 0 }
}
