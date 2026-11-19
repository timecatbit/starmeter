//! Times read from logs and APIs, as `swiftbar_time.parse_iso` reads them: a trailing Z, one to nine (or more)
//! fraction digits after a dot or comma, and offsets written +HH:MM, +HHMM or +HH. Without an offset the time is
//! naive, like a Python datetime without tzinfo; callers say which zone such a time is in.

use std::fmt;

use jiff::Timestamp;
use jiff::tz::TimeZone;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IsoTime {
    pub year: i64,
    pub month: i64,
    pub day: i64,
    pub hour: i64,
    pub minute: i64,
    pub second: i64,
    pub microsecond: i64,
    /// Seconds east of UTC, or None when the text named no zone.
    pub offset: Option<i64>,
}

/// The text is not a time Python's `parse_iso` would accept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidTime(pub String);

impl fmt::Display for InvalidTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Invalid isoformat string: '{}'", self.0)
    }
}

impl std::error::Error for InvalidTime {}

impl IsoTime {
    pub fn parse(value: &str) -> Result<Self, InvalidTime> {
        let text = value.trim();
        scan(text.as_bytes()).ok_or_else(|| InvalidTime(text.to_string()))
    }

    /// Seconds since 1970 as Python's `datetime.timestamp()` gives them; a naive time is read in `zone`.
    pub fn timestamp(&self, zone: &TimeZone) -> f64 {
        let seconds = days_from_civil(self.year, self.month, self.day) * 86400
            + self.hour * 3600
            + self.minute * 60
            + self.second;
        // Python divides whole microseconds once (timedelta.total_seconds), so the float, and any hash of its
        // text, comes out exactly the same here.
        match self.offset {
            Some(offset) => ((seconds - offset) * 1_000_000 + self.microsecond) as f64 / 1_000_000.0,
            None => local_seconds(seconds, zone) as f64 + self.microsecond as f64 / 1e6,
        }
    }

    /// Python's `isoformat()`: the wall time with microseconds, then the offset if there is one.
    pub fn isoformat(&self) -> String {
        let mut text = self.wall();
        if self.microsecond != 0 {
            text += &format!(".{:06}", self.microsecond);
        }
        if let Some(offset) = self.offset {
            let magnitude = offset.abs();
            let sign = if offset < 0 { '-' } else { '+' };
            text += &format!("{sign}{:02}:{:02}", magnitude / 3600, magnitude % 3600 / 60);
            if magnitude % 60 != 0 {
                text += &format!(":{:02}", magnitude % 60);
            }
        }
        text
    }

    /// The date and time to the second, without fraction or offset.
    pub fn wall(&self) -> String {
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }
}

/// CPython's `datetime._mktime` with fold 0: a wall time seen twice when clocks go back is the earlier moment;
/// one skipped when they go forward is read with the offset from before the change.
fn local_seconds(t: i64, zone: &TimeZone) -> i64 {
    let local = |u: i64| u + offset_at(zone, u);
    let a = local(t) - t;
    let u1 = t - a;
    let t1 = local(u1);
    let b = if t1 == t {
        let u2 = u1 - 86400;
        let b = local(u2) - u2;
        if a == b {
            return u1;
        }
        b
    } else {
        t1 - u1
    };
    let u2 = t - b;
    if local(u2) == t {
        return u2;
    }
    if t1 == t {
        return u1;
    }
    u1.max(u2)
}

fn offset_at(zone: &TimeZone, seconds: i64) -> i64 {
    Timestamp::from_second(seconds).map(|moment| i64::from(zone.to_offset(moment).seconds())).unwrap_or(0)
}

fn scan(bytes: &[u8]) -> Option<IsoTime> {
    let mut index = 0;
    let digits = |index: &mut usize, count: usize| -> Option<i64> {
        let part = bytes.get(*index..*index + count)?;
        let mut value = 0;
        for &byte in part {
            if !byte.is_ascii_digit() {
                return None;
            }
            value = value * 10 + i64::from(byte - b'0');
        }
        *index += count;
        Some(value)
    };
    let accept = |index: &mut usize, character: u8| -> bool {
        if bytes.get(*index) == Some(&character) {
            *index += 1;
            true
        } else {
            false
        }
    };
    let year = digits(&mut index, 4)?;
    if !accept(&mut index, b'-') {
        return None;
    }
    let month = digits(&mut index, 2)?;
    if !accept(&mut index, b'-') {
        return None;
    }
    let day = digits(&mut index, 2)?;
    let mut time = IsoTime { year, month, day, hour: 0, minute: 0, second: 0, microsecond: 0, offset: None };
    if index < bytes.len() {
        // Any single character may stand between the date and the time.
        index += utf8_length(bytes[index]);
        time.hour = digits(&mut index, 2)?;
        if accept(&mut index, b':') {
            time.minute = digits(&mut index, 2)?;
            if accept(&mut index, b':') {
                time.second = digits(&mut index, 2)?;
                if accept(&mut index, b'.') || accept(&mut index, b',') {
                    let (mut count, mut micro) = (0, 0);
                    while let Some(byte) = bytes.get(index).filter(|byte| byte.is_ascii_digit()) {
                        // Past six digits the rest is dropped, not rounded.
                        if count < 6 {
                            micro = micro * 10 + i64::from(byte - b'0');
                        }
                        count += 1;
                        index += 1;
                    }
                    if count == 0 {
                        return None;
                    }
                    for _ in count..count.max(6) {
                        micro *= 10;
                    }
                    time.microsecond = micro;
                }
            }
        }
        if index < bytes.len() {
            if accept(&mut index, b'Z') || accept(&mut index, b'z') {
                time.offset = Some(0);
            } else {
                let sign = match bytes.get(index) {
                    Some(b'+') => 1,
                    Some(b'-') => -1,
                    _ => return None,
                };
                index += 1;
                let hours = digits(&mut index, 2)?;
                let (mut minutes, mut seconds) = (0, 0);
                if accept(&mut index, b':') {
                    minutes = digits(&mut index, 2)?;
                    if accept(&mut index, b':') {
                        seconds = digits(&mut index, 2)?;
                    }
                } else if index < bytes.len() {
                    minutes = digits(&mut index, 2)?;
                }
                if minutes >= 60 || seconds >= 60 {
                    return None;
                }
                let total = hours * 3600 + minutes * 60 + seconds;
                if total >= 86400 {
                    return None;
                }
                time.offset = Some(sign * total);
            }
            if index != bytes.len() {
                return None;
            }
        }
    }
    let valid = (1..=12).contains(&time.month)
        && time.day >= 1
        && time.day <= days_in(time.month, time.year)
        && time.year >= 1
        && time.hour < 24
        && time.minute < 60
        && time.second < 60;
    valid.then_some(time)
}

fn utf8_length(lead: u8) -> usize {
    match lead {
        0xF0.. => 4,
        0xE0.. => 3,
        0xC0.. => 2,
        _ => 1,
    }
}

/// Days from 1970-01-01 to a date in the proleptic Gregorian calendar.
pub fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let year_of_era = y - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146097 + day_of_era - 719468
}

pub fn days_in(month: i64, year: i64) -> i64 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_what_logs_write() {
        let time = IsoTime::parse("2026-11-18T10:00:00.1234567Z").unwrap();
        assert_eq!(time.microsecond, 123456);
        assert_eq!(time.isoformat(), "2026-11-18T10:00:00.123456+00:00");
        assert_eq!(IsoTime::parse("2026-11-18 10:00:00-0530").unwrap().offset, Some(-19800));
        assert!(IsoTime::parse("2026-02-29T00:00:00Z").is_err());
        assert!(IsoTime::parse("2026-11-18T10:00:00.").is_err());
    }

    #[test]
    fn naive_times_follow_python_in_gaps_and_folds() {
        let new_york = TimeZone::get("America/New_York").unwrap();
        // Skipped at 02:00 on 8 March: read with the offset from before the change, so 07:30 UTC.
        let gap = IsoTime::parse("2026-03-08T02:30:00").unwrap();
        assert_eq!(gap.timestamp(&new_york), 1772955000.0);
        // Seen twice on 1 November: the earlier moment, 05:30 UTC.
        let fold = IsoTime::parse("2026-11-01T01:30:00").unwrap();
        assert_eq!(fold.timestamp(&new_york), 1793511000.0);
    }
}
