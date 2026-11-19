//! What the Python reference's built-ins make of JSON values, where the port must give the same answer.

use std::fmt;

use jiff::tz::TimeZone;
use sha2::{Digest, Sha256};

use crate::json::{Format, Json, Object, python_repr};
use crate::time::IsoTime;

/// A failure the Python readers treat as "skip this line" (KeyError, TypeError, AttributeError, bad JSON).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineError;

/// A failure that fails a whole log file in the Python readers (ValueError, OSError and the like).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileError {
    pub description: String,
    /// The Python exception this stands for, which is what a failed cache read reports.
    pub kind: &'static str,
}

impl FileError {
    pub fn new(description: impl Into<String>) -> Self {
        FileError { description: description.into(), kind: "ValueError" }
    }

    pub fn of_kind(description: impl Into<String>, kind: &'static str) -> Self {
        FileError { description: description.into(), kind }
    }
}

impl fmt::Display for FileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.description)
    }
}

impl std::error::Error for FileError {}

/// Either failure, for readers that tell them apart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadError {
    Line,
    File(FileError),
}

impl From<LineError> for ReadError {
    fn from(_: LineError) -> Self {
        ReadError::Line
    }
}

impl From<FileError> for ReadError {
    fn from(error: FileError) -> Self {
        ReadError::File(error)
    }
}

/// `str(value)`.
pub fn str(value: Option<&Json>) -> String {
    match value {
        None | Some(Json::Null) => "None".into(),
        Some(Json::Bool(flag)) => if *flag { "True" } else { "False" }.into(),
        Some(Json::Int(number)) => number.to_string(),
        Some(Json::Double(number)) if number.is_nan() => "nan".into(),
        Some(Json::Double(number)) if number.is_infinite() => if *number < 0.0 { "-inf" } else { "inf" }.into(),
        Some(Json::Double(number)) => python_repr(*number),
        Some(Json::String(text)) => text.clone(),
        Some(value) => repr(value),
    }
}

/// `repr(value)` of a list or dict as Python prints them.
pub fn repr(value: &Json) -> String {
    match value {
        Json::String(text) => {
            let quote = if text.contains('\'') && !text.contains('"') { '"' } else { '\'' };
            let mut output = String::from(quote);
            for character in text.chars() {
                match character {
                    '\\' => output.push_str("\\\\"),
                    '\n' => output.push_str("\\n"),
                    '\r' => output.push_str("\\r"),
                    '\t' => output.push_str("\\t"),
                    _ if character == quote => {
                        output.push('\\');
                        output.push(quote);
                    }
                    _ if (character as u32) < 0x20 || character as u32 == 0x7F => {
                        output.push_str(&format!("\\x{:02x}", character as u32));
                    }
                    _ => output.push(character),
                }
            }
            output.push(quote);
            output
        }
        Json::Array(items) => format!("[{}]", items.iter().map(repr).collect::<Vec<_>>().join(", ")),
        Json::Object(object) => {
            let pairs: Vec<String> = object
                .iter()
                .map(|(key, value)| format!("{}: {}", repr(&Json::from(key.as_str())), repr(value)))
                .collect();
            format!("{{{}}}", pairs.join(", "))
        }
        _ => str(Some(value)),
    }
}

/// The engine's `number()`: float(value) when that is finite and not negative, else the integer 0.
pub fn number(value: Option<&Json>) -> Json {
    match value.and_then(Json::python_float) {
        Some(number) if number.is_finite() && number >= 0.0 => Json::Double(number),
        _ => Json::Int(0),
    }
}

/// The value `number()` gives, as a float.
pub fn num(value: Option<&Json>) -> f64 {
    number(value).number().unwrap_or(0.0)
}

/// `value or fallback`, with Python's truthiness.
pub fn or(value: Option<&Json>, fallback: Json) -> Json {
    match value {
        Some(value) if value.is_truthy() => value.clone(),
        _ => fallback,
    }
}

/// sha256 of `json.dumps(values, sort_keys=True)`, in hex: the sources' event and file identities.
pub fn identity(values: Vec<Json>) -> String {
    let text = Json::Array(values).serialized_with(Format { ensure_ascii: true, sort_keys: true, ..Format::default() });
    sha256_hex(text.as_bytes())
}

/// `json.dumps(value)` with its defaults.
pub fn dumps(value: &Json) -> String {
    value.serialized_with(Format { ensure_ascii: true, ..Format::default() })
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

/// `value.get(key)` on something that must be a dict, else Python raises AttributeError.
pub fn get<'a>(value: &'a Json, key: &str) -> Result<Option<&'a Json>, LineError> {
    value.as_object().map(|object| object.get(key)).ok_or(LineError)
}

/// `(value or {})`: a falsy value stands for an empty dict; anything else must be a dict to be read.
pub fn dict(value: Option<&Json>) -> Json {
    or(value, Json::Object(Object::new()))
}

/// `int(value)` as Python's int(): numbers truncate, numeric text parses; anything else fails the file.
pub fn int(value: Option<&Json>) -> Result<i64, ReadError> {
    match value.unwrap_or(&Json::Null) {
        Json::Bool(flag) => Ok(i64::from(*flag)),
        Json::Int(number) => Ok(*number),
        Json::Double(number) if number.is_finite() => Ok(number.trunc() as i64),
        Json::Double(_) => Err(FileError::new("cannot convert float to integer").into()),
        Json::String(text) => {
            let trimmed = text.trim().replace('_', "");
            trimmed.parse().map_err(|_| FileError::new("invalid literal for int()").into())
        }
        _ => Err(LineError.into()),
    }
}

/// A project's folder name, the last part of its working directory; only the name is kept, never the path. A
/// file:// address (the ChatGPT app writes some) is read as the path it stands for, %20 as a space.
pub fn project_name(value: Option<&Json>) -> Json {
    let Some(path) = value.and_then(Json::as_str).map(str::trim).filter(|path| !path.is_empty()) else {
        return Json::Null;
    };
    let path = match path.strip_prefix("file://") {
        Some(rest) => percent_decode(rest).unwrap_or_else(|| rest.to_string()),
        None => path.to_string(),
    };
    let trimmed = path.trim_end_matches(['/', '\\']);
    let name = trimmed.rsplit(['/', '\\']).next().unwrap_or("");
    if name.is_empty() { Json::Null } else { Json::from(name) }
}

/// Foundation's `removingPercentEncoding`: None when an escape is malformed or the bytes are not UTF-8.
pub fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = std::str::from_utf8(bytes.get(index + 1..index + 3)?).ok()?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

/// The log's own time for an event: epoch seconds (milliseconds past 1e11) or ISO text.
pub fn stamp(value: Option<&Json>) -> Result<Json, ReadError> {
    match value {
        Some(Json::Int(number)) => {
            Ok(if *number as f64 > 1e11 { Json::Double(*number as f64 / 1000.0) } else { Json::Int(*number) })
        }
        Some(Json::Double(number)) => Ok(Json::Double(if *number > 1e11 { number / 1000.0 } else { *number })),
        Some(Json::Bool(flag)) => Ok(Json::Bool(*flag)),
        None => Err(LineError.into()),
        Some(other) => IsoTime::parse(&str(Some(other)))
            .map(|time| Json::Double(time.timestamp(&TimeZone::system())))
            .map_err(|_| FileError::new("bad timestamp").into()),
    }
}

/// The calendar date of a day number counted from 1970-01-01.
pub fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}
