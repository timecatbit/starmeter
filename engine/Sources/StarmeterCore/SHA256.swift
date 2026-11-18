/// SHA-256 in plain Swift: CryptoKit is Apple-only and the engine also runs on Windows. Used for the short
/// hashed identifiers the indexes keep in place of paths and IDs, so it must match Python's hashlib exactly.
public struct SHA256 {
    private var state: [UInt32] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19]
    private var buffer: [UInt8] = []
    private var length: UInt64 = 0
    private var schedule = [UInt32](repeating: 0, count: 64)

    private static let k: [UInt32] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ]

    public init() {}

    public static func hash<Bytes: Sequence>(_ data: Bytes) -> [UInt8] where Bytes.Element == UInt8 {
        var hasher = SHA256()
        hasher.update(data)
        return hasher.finalize()
    }

    public static func hex<Bytes: Sequence>(_ data: Bytes) -> String where Bytes.Element == UInt8 {
        hash(data).hexString
    }

    public mutating func update<Bytes: Sequence>(_ data: Bytes) where Bytes.Element == UInt8 {
        if let done = data.withContiguousStorageIfAvailable({ update(UnsafeRawBufferPointer($0)) }) { return done }
        update(Array(data))
    }

    public mutating func update(_ data: [UInt8]) {
        data.withUnsafeBytes { update($0) }
    }

    /// Whole blocks go straight from the caller's bytes; only a partial block is buffered.
    public mutating func update(_ data: UnsafeRawBufferPointer) {
        var at = 0
        length += UInt64(data.count)
        if !buffer.isEmpty {
            let take = min(64 - buffer.count, data.count)
            buffer.append(contentsOf: data[0..<take])
            at = take
            if buffer.count == 64 { buffer.withUnsafeBytes { compress($0.baseAddress!) }; buffer.removeAll(keepingCapacity: true) }
        }
        while at + 64 <= data.count {
            compress(data.baseAddress! + at)
            at += 64
        }
        if at < data.count { buffer.append(contentsOf: data[at...]) }
    }

    public mutating func finalize() -> [UInt8] {
        let bits = length &* 8
        var tail = buffer
        tail.append(0x80)
        while tail.count % 64 != 56 { tail.append(0) }
        for shift in stride(from: 56, through: 0, by: -8) { tail.append(UInt8(truncatingIfNeeded: bits >> UInt64(shift))) }
        tail.withUnsafeBytes { raw in for start in stride(from: 0, to: raw.count, by: 64) { compress(raw.baseAddress! + start) } }
        buffer = []
        return state.flatMap { word in (0..<4).map { UInt8(truncatingIfNeeded: word >> UInt32(24 - $0 * 8)) } }
    }

    private mutating func compress(_ block: UnsafeRawPointer) {
        // The schedule is borrowed out of self so the loop below owns it alone.
        var schedule: [UInt32] = []
        swap(&schedule, &self.schedule)
        defer { swap(&schedule, &self.schedule) }
        let k = Self.k
        var h0 = state
        schedule.withUnsafeMutableBufferPointer { w in
            for i in 0..<16 { w[i] = UInt32(bigEndian: block.loadUnaligned(fromByteOffset: i * 4, as: UInt32.self)) }
            for i in 16..<64 {
                let s0 = Self.rotate(w[i - 15], 7) ^ Self.rotate(w[i - 15], 18) ^ (w[i - 15] >> 3)
                let s1 = Self.rotate(w[i - 2], 17) ^ Self.rotate(w[i - 2], 19) ^ (w[i - 2] >> 10)
                w[i] = w[i - 16] &+ s0 &+ w[i - 7] &+ s1
            }
            var (a, b, c, d, e, f, g, h) = (h0[0], h0[1], h0[2], h0[3], h0[4], h0[5], h0[6], h0[7])
            for i in 0..<64 {
                let s1 = Self.rotate(e, 6) ^ Self.rotate(e, 11) ^ Self.rotate(e, 25)
                let choice = (e & f) ^ (~e & g)
                let t1 = h &+ s1 &+ choice &+ k[i] &+ w[i]
                let s0 = Self.rotate(a, 2) ^ Self.rotate(a, 13) ^ Self.rotate(a, 22)
                let majority = (a & b) ^ (a & c) ^ (b & c)
                (h, g, f, e, d, c, b, a) = (g, f, e, d &+ t1, c, b, a, t1 &+ s0 &+ majority)
            }
            h0[0] = h0[0] &+ a; h0[1] = h0[1] &+ b; h0[2] = h0[2] &+ c; h0[3] = h0[3] &+ d
            h0[4] = h0[4] &+ e; h0[5] = h0[5] &+ f; h0[6] = h0[6] &+ g; h0[7] = h0[7] &+ h
        }
        state = h0
    }

    private static func rotate(_ value: UInt32, _ count: UInt32) -> UInt32 { value >> count | value << (32 - count) }
}
extension Array where Element == UInt8 {
    public var hexString: String {
        let digits = Array("0123456789abcdef".utf8)
        var text = [UInt8]()
        text.reserveCapacity(count * 2)
        for byte in self {
            text.append(digits[Int(byte >> 4)])
            text.append(digits[Int(byte & 0xF)])
        }
        return String(decoding: text, as: UTF8.self)
    }
}
