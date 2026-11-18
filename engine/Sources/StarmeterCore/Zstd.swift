import Foundation

/// Zstandard decompression (RFC 8878), for the compressed logs Codex archives and DeepSeek Harness writes. Apple's
/// Compression framework has no zstd and the engine also runs on Windows, so this is a plain-Swift decoder:
/// frames without dictionaries, raw/RLE/compressed blocks, Huffman literals and FSE sequences. Checksums are
/// skipped; a damaged frame fails rather than producing wrong text.
public enum Zstd {
    public struct Failure: Error, CustomStringConvertible {
        public let description: String
        init(_ description: String) { self.description = description }
    }

    public static func decompress(file path: String) throws -> Data {
        guard let data = FileManager.default.contents(atPath: path) else { throw Failure("cannot read \(path)") }
        return try decompress(data)
    }

    public static func decompress(_ data: Data) throws -> Data {
        let input = [UInt8](data)
        let output = Output(capacity: max(1 << 16, input.count * 8))
        var at = 0
        while at < input.count {
            guard at + 4 <= input.count else { throw Failure("truncated frame") }
            let magic = UInt32(input[at]) | UInt32(input[at + 1]) << 8 | UInt32(input[at + 2]) << 16 | UInt32(input[at + 3]) << 24
            if magic & 0xFFFF_FFF0 == 0x184D_2A50 {
                // A skippable frame.
                guard at + 8 <= input.count else { throw Failure("truncated skippable frame") }
                let size = Int(UInt32(input[at + 4]) | UInt32(input[at + 5]) << 8 | UInt32(input[at + 6]) << 16 | UInt32(input[at + 7]) << 24)
                at += 8 + size
                continue
            }
            guard magic == 0xFD2F_B528 else { throw Failure("not a zstd frame") }
            var frame = Frame(input: input, at: at + 4)
            try frame.decode(into: output)
            at = frame.at
        }
        return output.data()
    }

    /// The decompressed bytes so far, grown by doubling; matches copy within it with memcpy.
    final class Output {
        var bytes: UnsafeMutablePointer<UInt8>
        var count = 0
        var capacity: Int

        init(capacity: Int) {
            self.capacity = capacity
            bytes = .allocate(capacity: capacity)
        }

        deinit { bytes.deallocate() }

        func reserve(_ more: Int) {
            guard count + more > capacity else { return }
            var grown = capacity * 2
            while grown < count + more { grown *= 2 }
            let larger = UnsafeMutablePointer<UInt8>.allocate(capacity: grown)
            larger.update(from: bytes, count: count)
            bytes.deallocate()
            bytes = larger
            capacity = grown
        }

        func append<C: Collection>(_ source: C) where C.Element == UInt8 {
            reserve(source.count)
            if source.withContiguousStorageIfAvailable({ (bytes + count).update(from: $0.baseAddress!, count: $0.count) }) == nil {
                var at = count
                for byte in source { bytes[at] = byte; at += 1 }
            }
            count += source.count
        }

        func append(repeating byte: UInt8, count length: Int) {
            reserve(length)
            (bytes + count).initialize(repeating: byte, count: length)
            count += length
        }

        /// Copies `length` bytes from `offset` back, overlapping when the offset is shorter than the length.
        func copyMatch(offset: Int, length: Int) {
            reserve(length)
            let from = bytes + (count - offset), to = bytes + count
            if offset >= length {
                to.update(from: from, count: length)
            } else {
                for step in 0..<length { to[step] = from[step] }
            }
            count += length
        }

        func data() -> Data { Data(bytes: bytes, count: count) }
    }

    struct Frame {
        let input: [UInt8]
        var at: Int
        var huffman: Huffman?
        var tables: [FSETable?] = [nil, nil, nil]  // literal lengths, offsets, match lengths
        var repeats = [1, 4, 8]

        init(input: [UInt8], at: Int) { self.input = input; self.at = at }

        mutating func byte() throws -> UInt8 {
            guard at < input.count else { throw Failure("truncated header") }
            defer { at += 1 }
            return input[at]
        }

        mutating func little(_ count: Int) throws -> Int {
            var value = 0
            for shift in 0..<count { value |= Int(try byte()) << (8 * shift) }
            return value
        }

        mutating func decode(into output: Output) throws {
            let descriptor = try byte()
            let sizeFlag = Int(descriptor >> 6), single = descriptor & 0x20 != 0, checksum = descriptor & 0x04 != 0
            let dictionaryBytes = [0, 1, 2, 4][Int(descriptor & 3)]
            guard descriptor & 0x08 == 0 else { throw Failure("reserved bit set") }
            if !single { _ = try byte() }  // window descriptor
            if dictionaryBytes > 0 {
                guard try little(dictionaryBytes) == 0 else { throw Failure("dictionaries are not supported") }
            }
            let sizeBytes = [single ? 1 : 0, 2, 4, 8][sizeFlag]
            _ = try little(sizeBytes)
            let frameStart = output.count
            while true {
                let header = try little(3)
                let last = header & 1 != 0, kind = (header >> 1) & 3, size = header >> 3
                switch kind {
                case 0:
                    guard at + size <= input.count else { throw Failure("truncated raw block") }
                    output.append(input[at..<at + size])
                    at += size
                case 1:
                    output.append(repeating: try byte(), count: size)
                case 2:
                    guard at + size <= input.count, size <= 128 * 1024 else { throw Failure("bad compressed block") }
                    try block(input[at..<at + size], frameStart: frameStart, into: output)
                    at += size
                default:
                    throw Failure("reserved block type")
                }
                if last { break }
            }
            if checksum { at += 4 }
        }

        mutating func block(_ block: ArraySlice<UInt8>, frameStart: Int, into output: Output) throws {
            var at = block.startIndex
            func need(_ count: Int) throws { guard at + count <= block.endIndex else { throw Failure("truncated block") } }
            // Literals section.
            try need(1)
            let kind = Int(block[at] & 3), format = Int(block[at] >> 2) & 3
            var literals: [UInt8]
            if kind < 2 {
                let regenerated: Int
                switch format {
                case 1: try need(2); regenerated = Int(block[at] >> 4) | Int(block[at + 1]) << 4; at += 2
                case 3: try need(3); regenerated = Int(block[at] >> 4) | Int(block[at + 1]) << 4 | Int(block[at + 2]) << 12; at += 3
                default: regenerated = Int(block[at] >> 3); at += 1
                }
                if kind == 0 {
                    try need(regenerated)
                    literals = Array(block[at..<at + regenerated])
                    at += regenerated
                } else {
                    try need(1)
                    literals = [UInt8](repeating: block[at], count: regenerated)
                    at += 1
                }
            } else {
                let headerBytes = [3, 3, 4, 5][format], streams = format == 0 ? 1 : 4
                try need(headerBytes)
                var header = 0
                for shift in 0..<headerBytes { header |= Int(block[at + shift]) << (8 * shift) }
                let bits = [10, 10, 14, 18][format], mask = (1 << bits) - 1
                let regenerated = (header >> 4) & mask, compressed = (header >> (4 + bits)) & mask
                at += headerBytes
                try need(compressed)
                var payload = block[at..<at + compressed]
                at += compressed
                if kind == 2 {
                    let (table, used) = try Huffman.read(payload)
                    huffman = table
                    payload = payload.dropFirst(used)
                }
                guard let huffman else { throw Failure("treeless literals without a table") }
                literals = try huffman.decode(payload, streams: streams, count: regenerated)
            }
            // Sequences section.
            try need(1)
            var count = Int(block[at]); at += 1
            if count >= 128 {
                if count == 255 { try need(2); count = Int(block[at]) | Int(block[at + 1]) << 8 | 0x7F00; at += 2 }
                else { try need(1); count = (count - 128) << 8 | Int(block[at]); at += 1 }
            }
            var literal = 0
            if count > 0 {
                try need(1)
                let modes = block[at]; at += 1
                for (index, shift) in [(0, 6), (1, 4), (2, 2)] {
                    let mode = Int(modes >> UInt8(shift)) & 3
                    switch mode {
                    case 0: tables[index] = FSETable.predefined[index]
                    case 1:
                        try need(1)
                        tables[index] = FSETable(single: Int(block[at])); at += 1
                    case 2:
                        let (counts, accuracy, used) = try FSETable.readCounts(block[at...], maxAccuracy: [9, 8, 9][index])
                        tables[index] = try FSETable(counts: counts, accuracy: accuracy); at += used
                    default:
                        guard tables[index] != nil else { throw Failure("repeat mode without a table") }
                    }
                }
                var bits = try BackwardBits(block[at..<block.endIndex])
                let ll = tables[0]!, of = tables[1]!, ml = tables[2]!
                var llState = bits.read(ll.accuracy), ofState = bits.read(of.accuracy), mlState = bits.read(ml.accuracy)
                for index in 0..<count {
                    let ofCode = Int(of.symbols[ofState]), mlCode = Int(ml.symbols[mlState]), llCode = Int(ll.symbols[llState])
                    guard ofCode < 32, mlCode < Self.matchBase.count, llCode < Self.literalBase.count else { throw Failure("bad sequence code") }
                    let offsetValue = (1 << ofCode) + bits.read(ofCode)
                    let matchLength = Self.matchBase[mlCode] + bits.read(Self.matchBits[mlCode])
                    let literalLength = Self.literalBase[llCode] + bits.read(Self.literalBits[llCode])
                    var offset: Int
                    if offsetValue > 3 {
                        offset = offsetValue - 3
                        repeats = [offset, repeats[0], repeats[1]]
                    } else {
                        let choice = offsetValue + (literalLength == 0 ? 1 : 0)
                        switch choice {
                        case 1: offset = repeats[0]
                        case 2: offset = repeats[1]; repeats = [offset, repeats[0], repeats[2]]
                        case 3: offset = repeats[2]; repeats = [offset, repeats[0], repeats[1]]
                        default: offset = repeats[0] - 1; repeats = [offset, repeats[0], repeats[1]]
                        }
                    }
                    guard literal + literalLength <= literals.count else { throw Failure("literals overrun") }
                    output.append(literals[literal..<literal + literalLength])
                    literal += literalLength
                    guard offset > 0, offset <= output.count - frameStart else { throw Failure("offset out of range") }
                    output.copyMatch(offset: offset, length: matchLength)
                    if index < count - 1 {
                        llState = ll.next(llState, &bits)
                        mlState = ml.next(mlState, &bits)
                        ofState = of.next(ofState, &bits)
                    }
                }
                guard bits.position == 0 else { throw Failure("sequence bits left over") }
            } else {
                guard at == block.endIndex else { throw Failure("bytes after an empty sequence section") }
            }
            output.append(literals[literal...])
        }

        static let literalBase = Array(0...15) + [16, 18, 20, 22, 24, 28, 32, 40, 48, 64, 128, 256, 512, 1024, 2048, 4096, 8192, 16384, 32768, 65536]
        static let literalBits = [Int](repeating: 0, count: 16) + [1, 1, 1, 1, 2, 2, 3, 3, 4, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]
        static let matchBase = Array(3...34) + [35, 37, 39, 41, 43, 47, 51, 59, 67, 83, 99, 131, 259, 515, 1027, 2051, 4099, 8195, 16387, 32771, 65539]
        static let matchBits = [Int](repeating: 0, count: 32) + [1, 1, 1, 1, 2, 2, 3, 3, 4, 4, 5, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]
    }

    /// A bitstream read from its end: the last byte's highest set bit marks where the bits begin. Reading past the
    /// start yields zeros, as the reference decoder's final state updates allow.
    struct BackwardBits {
        /// The stream's bytes plus eight zero bytes, so every read can load a whole word.
        private let storage: Storage
        var position: Int

        final class Storage {
            let bytes: UnsafeMutableRawPointer
            init(_ slice: ArraySlice<UInt8>) {
                bytes = .allocate(byteCount: slice.count + 8, alignment: 8)
                bytes.initializeMemory(as: UInt8.self, repeating: 0, count: slice.count + 8)
                slice.withUnsafeBytes { bytes.copyMemory(from: $0.baseAddress!, byteCount: $0.count) }
            }
            deinit { bytes.deallocate() }
        }

        init(_ slice: ArraySlice<UInt8>) throws {
            guard let last = slice.last, last != 0 else { throw Failure("bitstream without an end mark") }
            position = (slice.count - 1) * 8 + (7 - last.leadingZeroBitCount)
            storage = Storage(slice)
        }

        /// The `count` bits from bit `start` up; bits before the stream are zeros.
        @inline(__always) func peek(_ start: Int, _ count: Int) -> Int {
            guard count > 0 else { return 0 }
            if start < 0 {
                let available = count + start
                return available > 0 ? peek(0, available) << (-start) : 0
            }
            let word = UInt64(littleEndian: storage.bytes.loadUnaligned(fromByteOffset: start >> 3, as: UInt64.self))
            return Int((word >> UInt64(start & 7)) & ((1 << UInt64(count)) - 1))
        }

        @inline(__always) mutating func read(_ count: Int) -> Int {
            guard count > 0 else { return 0 }
            position -= count
            return peek(position, count)
        }
    }

    struct FSETable {
        var accuracy: Int
        var symbols: [UInt8]
        var bits: [UInt8]
        var baselines: [Int]

        init(single symbol: Int) {
            accuracy = 0; symbols = [UInt8(symbol)]; bits = [0]; baselines = [0]
        }

        init(counts: [Int], accuracy: Int) throws {
            let size = 1 << accuracy
            self.accuracy = accuracy
            symbols = [UInt8](repeating: 0, count: size)
            bits = [UInt8](repeating: 0, count: size)
            baselines = [Int](repeating: 0, count: size)
            var high = size - 1
            var next = [Int](repeating: 0, count: counts.count)
            for (symbol, count) in counts.enumerated() where count == -1 {
                symbols[high] = UInt8(symbol); high -= 1; next[symbol] = 1
            }
            let step = (size >> 1) + (size >> 3) + 3, mask = size - 1
            var position = 0
            for (symbol, count) in counts.enumerated() where count > 0 {
                next[symbol] = count
                for _ in 0..<count {
                    symbols[position] = UInt8(symbol)
                    repeat { position = (position + step) & mask } while position > high
                }
            }
            guard position == 0 else { throw Failure("bad FSE distribution") }
            for cell in 0..<size {
                let symbol = Int(symbols[cell])
                let state = next[symbol]; next[symbol] += 1
                let width = accuracy - (Int.bitWidth - 1 - state.leadingZeroBitCount)
                bits[cell] = UInt8(width)
                baselines[cell] = (state << width) - size
            }
        }

        func next(_ state: Int, _ stream: inout BackwardBits) -> Int { baselines[state] + stream.read(Int(bits[state])) }

        /// Normalized counts as a table description encodes them; returns them, the accuracy and the bytes used.
        static func readCounts(_ slice: ArraySlice<UInt8>, maxAccuracy: Int) throws -> ([Int], Int, Int) {
            let bytes = Array(slice)
            var bit = 0
            func peek(_ count: Int) -> Int {
                var value = 0
                for offset in 0..<count {
                    let index = (bit + offset) >> 3
                    if index < bytes.count, bytes[index] >> UInt8((bit + offset) & 7) & 1 == 1 { value |= 1 << offset }
                }
                return value
            }
            let accuracy = peek(4) + 5; bit += 4
            guard accuracy <= maxAccuracy else { throw Failure("FSE accuracy too large") }
            var remaining = (1 << accuracy) + 1, threshold = 1 << accuracy, width = accuracy + 1, counts: [Int] = []
            while remaining > 1 {
                let most = (2 * threshold - 1) - remaining
                var value = peek(width - 1)
                if value < most {
                    bit += width - 1
                } else {
                    value = peek(width)
                    if value >= threshold { value -= most }
                    bit += width
                }
                let probability = value - 1
                remaining -= probability < 0 ? -probability : probability
                counts.append(probability)
                if probability == 0 {
                    while true {
                        let repeatCount = peek(2); bit += 2
                        counts.append(contentsOf: repeatElement(0, count: repeatCount))
                        if repeatCount != 3 { break }
                    }
                }
                while remaining < threshold { width -= 1; threshold >>= 1 }
                guard bit <= bytes.count * 8, counts.count <= 256 else { throw Failure("truncated FSE description") }
            }
            guard remaining == 1, counts.count <= 256 else { throw Failure("bad FSE description") }
            return (counts, accuracy, (bit + 7) / 8)
        }

        static let predefined: [FSETable] = [
            try! FSETable(counts: [4, 3, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 3, 2, 1, 1, 1, 1, 1, -1, -1, -1, -1], accuracy: 6),
            try! FSETable(counts: [1, 1, 1, 1, 1, 1, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, -1, -1, -1, -1, -1], accuracy: 5),
            try! FSETable(counts: [1, 4, 3, 2, 2, 2, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, -1, -1, -1, -1, -1, -1, -1], accuracy: 6),
        ]
    }

    struct Huffman {
        var maxBits: Int
        var symbols: [UInt8]
        var lengths: [UInt8]

        /// Reads a Huffman tree description; returns the table and the bytes it took.
        static func read(_ slice: ArraySlice<UInt8>) throws -> (Huffman, Int) {
            guard let header = slice.first else { throw Failure("missing Huffman description") }
            var weights: [Int] = []
            var used: Int
            if header >= 128 {
                let count = Int(header) - 127
                used = 1 + (count + 1) / 2
                guard slice.count >= used else { throw Failure("truncated Huffman weights") }
                for index in 0..<count {
                    let byte = slice[slice.startIndex + 1 + index / 2]
                    weights.append(Int(index % 2 == 0 ? byte >> 4 : byte & 0xF))
                }
            } else {
                used = 1 + Int(header)
                guard slice.count >= used else { throw Failure("truncated Huffman weights") }
                let body = slice[(slice.startIndex + 1)..<(slice.startIndex + used)]
                let (counts, accuracy, described) = try FSETable.readCounts(body, maxAccuracy: 6)
                let table = try FSETable(counts: counts, accuracy: accuracy)
                var bits = try BackwardBits(body.dropFirst(described))
                var one = bits.read(accuracy), two = bits.read(accuracy)
                while weights.count < 255 {
                    weights.append(Int(table.symbols[one]))
                    one = table.next(one, &bits)
                    if bits.position < 0 { weights.append(Int(table.symbols[two])); break }
                    weights.append(Int(table.symbols[two]))
                    two = table.next(two, &bits)
                    if bits.position < 0 { weights.append(Int(table.symbols[one])); break }
                }
            }
            // The last weight is implied: it completes the total to a power of two.
            let total = weights.reduce(0) { $0 + ($1 > 0 ? 1 << ($1 - 1) : 0) }
            guard total > 0 else { throw Failure("empty Huffman tree") }
            let maxBits = Int.bitWidth - total.leadingZeroBitCount
            let rest = (1 << maxBits) - total
            guard rest > 0, rest & (rest - 1) == 0, maxBits <= 11 else { throw Failure("bad Huffman weights") }
            weights.append(rest.trailingZeroBitCount + 1)
            var table = Huffman(maxBits: maxBits, symbols: [UInt8](repeating: 0, count: 1 << maxBits), lengths: [UInt8](repeating: 0, count: 1 << maxBits))
            // Codes are handed out from the lightest weight up, in symbol order.
            var start = 0
            for weight in 1...maxBits {
                for (symbol, w) in weights.enumerated() where w == weight {
                    let length = maxBits + 1 - weight, span = 1 << (weight - 1)
                    for cell in start..<start + span { table.symbols[cell] = UInt8(symbol); table.lengths[cell] = UInt8(length) }
                    start += span
                }
            }
            return (table, used)
        }

        func decode(_ payload: ArraySlice<UInt8>, streams: Int, count: Int) throws -> [UInt8] {
            var output = [UInt8](repeating: 0, count: count)
            try output.withUnsafeMutableBufferPointer { out in
                if streams == 1 {
                    try decodeStream(payload, into: out, from: 0, count: count)
                    return
                }
                guard payload.count >= 6 else { throw Failure("truncated jump table") }
                let s = payload.startIndex
                let sizes = [Int(payload[s]) | Int(payload[s + 1]) << 8, Int(payload[s + 2]) | Int(payload[s + 3]) << 8, Int(payload[s + 4]) | Int(payload[s + 5]) << 8]
                let fourth = payload.count - 6 - sizes.reduce(0, +)
                guard fourth > 0 else { throw Failure("bad jump table") }
                let each = (count + 3) / 4
                guard count - 3 * each >= 0 else { throw Failure("bad literal count") }
                var at = s + 6, written = 0
                for (index, size) in (sizes + [fourth]).enumerated() {
                    let produce = index < 3 ? each : count - 3 * each
                    try decodeStream(payload[at..<at + size], into: out, from: written, count: produce)
                    at += size
                    written += produce
                }
            }
            return output
        }

        func decodeStream(_ slice: ArraySlice<UInt8>, into output: UnsafeMutableBufferPointer<UInt8>, from start: Int, count: Int) throws {
            var bits = try BackwardBits(slice)
            let width = maxBits
            try symbols.withUnsafeBufferPointer { symbols in
                try lengths.withUnsafeBufferPointer { lengths in
                    for index in start..<start + count {
                        let cell = bits.peek(bits.position - width, width)
                        output[index] = symbols[cell]
                        bits.position -= Int(lengths[cell])
                    }
                    guard bits.position == 0 else { throw Failure("Huffman stream not used up") }
                }
            }
        }
    }
}
