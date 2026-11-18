import Foundation

/// A time read from a log or an API, as `swiftbar_time.parse_iso` reads it: a trailing Z, one to nine (or
/// more) fraction digits after a dot or comma, and offsets written +HH:MM, +HHMM or +HH. Without an offset the
/// time is naive, like a Python datetime without tzinfo; callers say which zone such a time is in.
public struct ISOTime: Equatable {
    public var year: Int, month: Int, day: Int
    public var hour = 0, minute = 0, second = 0, microsecond = 0
    /// Seconds east of UTC, or nil when the text named no zone.
    public var offset: Int?

    public struct Invalid: Error, CustomStringConvertible {
        public let text: String
        public var description: String { "Invalid isoformat string: '\(text)'" }
    }

    public init(year: Int, month: Int, day: Int, hour: Int = 0, minute: Int = 0, second: Int = 0,
                microsecond: Int = 0, offset: Int? = nil) {
        (self.year, self.month, self.day) = (year, month, day)
        (self.hour, self.minute, self.second, self.microsecond, self.offset) = (hour, minute, second, microsecond, offset)
    }

    public static func parse(_ value: String) throws -> ISOTime {
        let text = value.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let time = scan(Array(text.utf8)) else { throw Invalid(text: text) }
        return time
    }

    /// The moment this time names; a naive time is read in `zone`.
    public func date(assuming zone: TimeZone = .current) -> Date {
        Date(timeIntervalSince1970: timestamp(assuming: zone))
    }

    /// Seconds since 1970 as Python's datetime.timestamp() gives them.
    public func timestamp(assuming zone: TimeZone = .current) -> Double {
        let seconds = Int64(daysFromCivil(year, month, day)) * 86400 + Int64(hour * 3600 + minute * 60 + second)
        // Python divides whole microseconds once (timedelta.total_seconds), so the float, and any hash of its
        // text, comes out exactly the same here.
        if let offset { return Double((seconds - Int64(offset)) * 1_000_000 + Int64(microsecond)) / 1_000_000 }
        return localSeconds(Double(seconds), zone) + Double(microsecond) / 1e6
    }

    /// CPython's datetime._mktime with fold 0: a wall time seen twice when clocks go back is the earlier
    /// moment; one skipped when they go forward is read with the offset from before the change.
    private func localSeconds(_ t: Double, _ zone: TimeZone) -> Double {
        func local(_ u: Double) -> Double { u + Double(zone.secondsFromGMT(for: Date(timeIntervalSince1970: u))) }
        let a = local(t) - t
        let u1 = t - a
        let t1 = local(u1)
        var b: Double
        if t1 == t {
            let u2 = u1 - 86400
            b = local(u2) - u2
            if a == b { return u1 }
        } else {
            b = t1 - u1
        }
        let u2 = t - b
        if local(u2) == t { return u2 }
        if t1 == t { return u1 }
        return Swift.max(u1, u2)
    }

    /// Python's isoformat(): the wall time with microseconds, then the offset if there is one.
    public var isoformat: String {
        var text = String(format: "%04d-%02d-%02dT%02d:%02d:%02d", year, month, day, hour, minute, second)
        if microsecond != 0 { text += String(format: ".%06d", microsecond) }
        if let offset {
            let magnitude = Swift.abs(offset)
            text += String(format: "%@%02d:%02d", offset < 0 ? "-" : "+", magnitude / 3600, magnitude % 3600 / 60)
            if magnitude % 60 != 0 { text += String(format: ":%02d", magnitude % 60) }
        }
        return text
    }

    private static func scan(_ bytes: [UInt8]) -> ISOTime? {
        var index = 0
        func digits(_ count: Int) -> Int? {
            guard index + count <= bytes.count else { return nil }
            var value = 0
            for offset in 0..<count {
                let byte = bytes[index + offset]
                guard byte >= 0x30, byte <= 0x39 else { return nil }
                value = value * 10 + Int(byte - 0x30)
            }
            index += count
            return value
        }
        func accept(_ character: Character) -> Bool {
            guard index < bytes.count, bytes[index] == character.asciiValue! else { return false }
            index += 1
            return true
        }
        guard let year = digits(4), accept("-"), let month = digits(2), accept("-"), let day = digits(2) else { return nil }
        var time = ISOTime(year: year, month: month, day: day)
        if index < bytes.count {
            // Any single character may stand between the date and the time.
            index += utf8Length(bytes[index])
            guard let hour = digits(2) else { return nil }
            time.hour = hour
            if accept(":") {
                guard let minute = digits(2) else { return nil }
                time.minute = minute
                if accept(":") {
                    guard let second = digits(2) else { return nil }
                    time.second = second
                    if accept(".") || accept(",") {
                        var count = 0, micro = 0
                        while index < bytes.count, bytes[index] >= 0x30, bytes[index] <= 0x39 {
                            // Past six digits the rest is dropped, not rounded.
                            if count < 6 { micro = micro * 10 + Int(bytes[index] - 0x30) }
                            count += 1
                            index += 1
                        }
                        guard count > 0 else { return nil }
                        for _ in count..<max(count, 6) { micro *= 10 }
                        time.microsecond = micro
                    }
                }
            }
            if index < bytes.count {
                if accept("Z") || accept("z") {
                    time.offset = 0
                } else {
                    guard index < bytes.count, bytes[index] == 0x2B || bytes[index] == 0x2D else { return nil }
                    let sign = bytes[index] == 0x2D ? -1 : 1
                    index += 1
                    guard let hours = digits(2) else { return nil }
                    var minutes = 0, seconds = 0
                    if accept(":") {
                        guard let value = digits(2) else { return nil }
                        minutes = value
                        if accept(":") {
                            guard let value = digits(2) else { return nil }
                            seconds = value
                        }
                    } else if index < bytes.count {
                        guard let value = digits(2) else { return nil }
                        minutes = value
                    }
                    guard minutes < 60, seconds < 60 else { return nil }
                    let total = hours * 3600 + minutes * 60 + seconds
                    guard total < 86400 else { return nil }
                    time.offset = sign * total
                }
                guard index == bytes.count else { return nil }
            }
        }
        guard (1...12).contains(time.month), time.day >= 1, time.day <= daysIn(time.month, of: time.year),
              time.year >= 1, time.hour < 24, time.minute < 60, time.second < 60 else { return nil }
        return time
    }

    private static func utf8Length(_ lead: UInt8) -> Int {
        switch lead {
        case 0xF0...: return 4
        case 0xE0...: return 3
        case 0xC0...: return 2
        default: return 1
        }
    }
}

/// Days from 1970-01-01 to a date in the proleptic Gregorian calendar.
func daysFromCivil(_ year: Int, _ month: Int, _ day: Int) -> Int {
    let y = month <= 2 ? year - 1 : year
    let era = (y >= 0 ? y : y - 399) / 400
    let yearOfEra = y - era * 400
    let dayOfYear = (153 * (month + (month > 2 ? -3 : 9)) + 2) / 5 + day - 1
    let dayOfEra = yearOfEra * 365 + yearOfEra / 4 - yearOfEra / 100 + dayOfYear
    return era * 146097 + dayOfEra - 719468
}

func daysIn(_ month: Int, of year: Int) -> Int {
    switch month {
    case 2: return year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) ? 29 : 28
    case 4, 6, 9, 11: return 30
    default: return 31
    }
}
