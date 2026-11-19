//! JSON the way Python's json module sees it: integers and floats stay apart, objects keep their key order, and
//! comparing follows Python (1 == 1.0 == true). State files written by 5.x read back the same, and files written
//! here read the same in 5.x.

use std::fmt;

use indexmap::IndexMap;

/// Builds an `Object` in the order written: `object! { "a" => 1, "b" => "x" }`.
#[macro_export]
macro_rules! object {
    ($($key:expr => $value:expr),* $(,)?) => {{
        #[allow(unused_mut)]
        let mut object = $crate::json::Object::new();
        $(object.insert($key, $value);)*
        object
    }};
}

#[derive(Clone, Debug, Default)]
pub enum Json {
    #[default]
    Null,
    Bool(bool),
    Int(i64),
    Double(f64),
    String(String),
    Array(Vec<Json>),
    Object(Object),
}

/// A JSON object that keeps its keys in the order they were first set, like a Python dict: setting an existing
/// key keeps its place.
#[derive(Clone, Debug, Default)]
pub struct Object(IndexMap<String, Json>);

impl Object {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, key: &str) -> Option<&Json> {
        self.0.get(key)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Json> {
        self.0.get_mut(key)
    }

    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<Json>) {
        self.0.insert(key.into(), value.into());
    }

    /// Removes a key and keeps the others in order, as `del d[key]` does.
    pub fn remove(&mut self, key: &str) -> Option<Json> {
        self.0.shift_remove(key)
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.0.contains_key(key)
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.0.keys()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &Json)> {
        self.0.iter()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<K: Into<String>, V: Into<Json>> FromIterator<(K, V)> for Object {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(pairs: I) -> Self {
        let mut object = Object::new();
        for (key, value) in pairs {
            object.insert(key, value);
        }
        object
    }
}

impl IntoIterator for Object {
    type Item = (String, Json);
    type IntoIter = indexmap::map::IntoIter<String, Json>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

/// Python compares dicts without regard to order.
impl PartialEq for Object {
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len() && self.iter().all(|(key, value)| other.get(key) == Some(value))
    }
}

impl PartialEq for Json {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Json::Null, Json::Null) => true,
            (Json::String(a), Json::String(b)) => a == b,
            (Json::Array(a), Json::Array(b)) => a == b,
            (Json::Object(a), Json::Object(b)) => a == b,
            // Numbers and bools compare by value across kinds, as in Python.
            _ => match (self.numeric_value(), other.numeric_value()) {
                (Some(a), Some(b)) => a == b,
                _ => false,
            },
        }
    }
}

impl From<bool> for Json {
    fn from(value: bool) -> Self {
        Json::Bool(value)
    }
}
impl From<i64> for Json {
    fn from(value: i64) -> Self {
        Json::Int(value)
    }
}
impl From<i32> for Json {
    fn from(value: i32) -> Self {
        Json::Int(i64::from(value))
    }
}
impl From<f64> for Json {
    fn from(value: f64) -> Self {
        Json::Double(value)
    }
}
impl From<&str> for Json {
    fn from(value: &str) -> Self {
        Json::String(value.to_string())
    }
}
impl From<String> for Json {
    fn from(value: String) -> Self {
        Json::String(value)
    }
}
impl From<Object> for Json {
    fn from(value: Object) -> Self {
        Json::Object(value)
    }
}
impl<T: Into<Json>> From<Vec<T>> for Json {
    fn from(items: Vec<T>) -> Self {
        Json::Array(items.into_iter().map(Into::into).collect())
    }
}
impl<T: Into<Json>> From<Option<T>> for Json {
    fn from(value: Option<T>) -> Self {
        value.map_or(Json::Null, Into::into)
    }
}

impl Json {
    fn numeric_value(&self) -> Option<f64> {
        match self {
            Json::Bool(value) => Some(if *value { 1.0 } else { 0.0 }),
            Json::Int(value) => Some(*value as f64),
            Json::Double(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::String(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&Vec<Json>> {
        match self {
            Json::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&Object> {
        match self {
            Json::Object(object) => Some(object),
            _ => None,
        }
    }

    pub fn as_object_mut(&mut self) -> Option<&mut Object> {
        match self {
            Json::Object(object) => Some(object),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Json::Null)
    }

    /// An int or float, never a bool: Python's `isinstance(x, (int, float)) and not isinstance(x, bool)`.
    pub fn number(&self) -> Option<f64> {
        match self {
            Json::Int(value) => Some(*value as f64),
            Json::Double(value) => Some(*value),
            _ => None,
        }
    }

    /// Python's `isinstance(x, int)`, which a bool also passes.
    pub fn integer(&self) -> Option<i64> {
        match self {
            Json::Int(value) => Some(*value),
            Json::Bool(value) => Some(i64::from(*value)),
            _ => None,
        }
    }

    /// What Python's `float(x)` makes of it: numbers, bools and numeric text, or None where float() raises.
    pub fn python_float(&self) -> Option<f64> {
        match self {
            Json::String(text) => parse_python_float(text),
            _ => self.numeric_value(),
        }
    }

    /// The value at a key of an object, or None for anything else.
    pub fn get(&self, key: &str) -> Option<&Json> {
        self.as_object().and_then(|object| object.get(key))
    }
}

/// Python's `float()` on text: surrounding whitespace, a sign, decimals, exponents, inf and nan.
pub fn parse_python_float(text: &str) -> Option<f64> {
    let trimmed = text.trim();
    let negative = trimmed.starts_with('-');
    let body = trimmed.strip_prefix(['+', '-']).unwrap_or(trimmed);
    match body.to_ascii_lowercase().as_str() {
        "" => None,
        "inf" | "infinity" => Some(if negative { f64::NEG_INFINITY } else { f64::INFINITY }),
        "nan" => Some(f64::NAN),
        _ => {
            // Rust's parser also takes words Python rejects; allow only Python's decimal form.
            let allowed = |c: char| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-');
            if !body.chars().all(allowed) || !body.chars().any(|c| c.is_ascii_digit()) {
                return None;
            }
            trimmed.parse().ok()
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonError {
    pub message: &'static str,
    pub offset: usize,
    /// The bytes were not UTF-8: json.loads raises UnicodeDecodeError then, not JSONDecodeError.
    pub encoding: bool,
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at byte {}", self.message, self.offset)
    }
}

impl std::error::Error for JsonError {}

/// Which parts of a document to build: everything, or only some keys of an object (and what to keep of each).
#[derive(Clone, Debug)]
pub enum Selection {
    All,
    Keys(Vec<(&'static str, Selection)>),
}

impl Json {
    /// Parses UTF-8 JSON as Python's json.loads does with strict UTF-8 decoding: a byte-order mark, invalid UTF-8,
    /// control characters inside strings and leading zeros are errors; NaN and Infinity are accepted; a repeated
    /// key keeps its first place and its last value.
    pub fn parse(bytes: &[u8]) -> Result<Json, JsonError> {
        Parser { bytes, index: 0, surrogates: false }.document(&Selection::All)
    }

    pub fn parse_str(text: &str) -> Result<Json, JsonError> {
        Self::parse(text.as_bytes())
    }

    /// Parses only the parts `keep` names and checks the rest without building it, so a large line whose text
    /// nobody reads costs a scan, not a copy. Accepts and rejects exactly what a full parse does. `surrogates`
    /// accepts UTF-8-encoded surrogates, as json.loads does for bytes ('surrogatepass').
    pub fn parse_selected(bytes: &[u8], keep: &Selection, surrogates: bool) -> Result<Json, JsonError> {
        Parser { bytes, index: 0, surrogates }.document(keep)
    }
}

const MAXIMUM_DEPTH: usize = 512;

struct Parser<'a> {
    bytes: &'a [u8],
    index: usize,
    surrogates: bool,
}

impl Parser<'_> {
    fn document(&mut self, keep: &Selection) -> Result<Json, JsonError> {
        self.skip_whitespace();
        let value = self.value(0, keep)?;
        self.skip_whitespace();
        if self.index != self.bytes.len() {
            return Err(self.fail("Extra data"));
        }
        Ok(value)
    }

    fn fail(&self, message: &'static str) -> JsonError {
        JsonError { message, offset: self.index, encoding: false }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.index).copied()
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.index += 1;
        }
    }

    fn literal(&mut self, word: &[u8]) -> bool {
        if self.bytes[self.index..].starts_with(word) {
            self.index += word.len();
            true
        } else {
            false
        }
    }

    fn value(&mut self, depth: usize, keep: &Selection) -> Result<Json, JsonError> {
        if depth >= MAXIMUM_DEPTH {
            return Err(self.fail("Too deeply nested"));
        }
        match self.peek() {
            Some(b'{') => return self.object(depth, keep),
            Some(b'[') => return self.array(depth, keep),
            Some(b'"') => return self.string().map(Json::String),
            Some(b't') if self.literal(b"true") => return Ok(Json::Bool(true)),
            Some(b'f') if self.literal(b"false") => return Ok(Json::Bool(false)),
            Some(b'n') if self.literal(b"null") => return Ok(Json::Null),
            Some(b'N') if self.literal(b"NaN") => return Ok(Json::Double(f64::NAN)),
            Some(b'I') if self.literal(b"Infinity") => return Ok(Json::Double(f64::INFINITY)),
            Some(b'-') if self.literal(b"-Infinity") => return Ok(Json::Double(f64::NEG_INFINITY)),
            Some(b'-' | b'0'..=b'9') => return self.number(),
            _ => {}
        }
        Err(self.fail("Expecting value"))
    }

    fn object(&mut self, depth: usize, keep: &Selection) -> Result<Json, JsonError> {
        self.index += 1;
        let mut object = Object::new();
        self.skip_whitespace();
        if self.peek() == Some(b'}') {
            self.index += 1;
            return Ok(Json::Object(object));
        }
        loop {
            self.skip_whitespace();
            if self.peek() != Some(b'"') {
                return Err(self.fail("Expecting property name enclosed in double quotes"));
            }
            let key = self.string()?;
            self.skip_whitespace();
            if self.peek() != Some(b':') {
                return Err(self.fail("Expecting ':' delimiter"));
            }
            self.index += 1;
            self.skip_whitespace();
            match keep {
                Selection::All => {
                    let value = self.value(depth + 1, &Selection::All)?;
                    object.insert(key, value);
                }
                Selection::Keys(wanted) => match wanted.iter().find(|(name, _)| *name == key) {
                    Some((_, part)) => {
                        let value = self.value(depth + 1, part)?;
                        object.insert(key, value);
                    }
                    None => self.skip_value(depth + 1)?,
                },
            }
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.index += 1,
                Some(b'}') => {
                    self.index += 1;
                    return Ok(Json::Object(object));
                }
                _ => return Err(self.fail("Expecting ',' delimiter")),
            }
        }
    }

    fn array(&mut self, depth: usize, keep: &Selection) -> Result<Json, JsonError> {
        self.index += 1;
        let mut items = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b']') {
            self.index += 1;
            return Ok(Json::Array(items));
        }
        loop {
            self.skip_whitespace();
            items.push(self.value(depth + 1, keep)?);
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.index += 1,
                Some(b']') => {
                    self.index += 1;
                    return Ok(Json::Array(items));
                }
                _ => return Err(self.fail("Expecting ',' delimiter")),
            }
        }
    }

    /// Checks one value as `value` would, building nothing.
    fn skip_value(&mut self, depth: usize) -> Result<(), JsonError> {
        if depth >= MAXIMUM_DEPTH {
            return Err(self.fail("Too deeply nested"));
        }
        match self.peek() {
            Some(b'{') => {
                self.index += 1;
                self.skip_whitespace();
                if self.peek() == Some(b'}') {
                    self.index += 1;
                    return Ok(());
                }
                loop {
                    self.skip_whitespace();
                    if self.peek() != Some(b'"') {
                        return Err(self.fail("Expecting property name enclosed in double quotes"));
                    }
                    self.skip_string()?;
                    self.skip_whitespace();
                    if self.peek() != Some(b':') {
                        return Err(self.fail("Expecting ':' delimiter"));
                    }
                    self.index += 1;
                    self.skip_whitespace();
                    self.skip_value(depth + 1)?;
                    self.skip_whitespace();
                    match self.peek() {
                        Some(b',') => self.index += 1,
                        Some(b'}') => {
                            self.index += 1;
                            return Ok(());
                        }
                        _ => return Err(self.fail("Expecting ',' delimiter")),
                    }
                }
            }
            Some(b'[') => {
                self.index += 1;
                self.skip_whitespace();
                if self.peek() == Some(b']') {
                    self.index += 1;
                    return Ok(());
                }
                loop {
                    self.skip_whitespace();
                    self.skip_value(depth + 1)?;
                    self.skip_whitespace();
                    match self.peek() {
                        Some(b',') => self.index += 1,
                        Some(b']') => {
                            self.index += 1;
                            return Ok(());
                        }
                        _ => return Err(self.fail("Expecting ',' delimiter")),
                    }
                }
            }
            Some(b'"') => self.skip_string(),
            // Scalars are small; parsing them is as cheap as checking them.
            Some(b'-' | b'0'..=b'9' | b't' | b'f' | b'n' | b'N' | b'I') => {
                self.value(depth, &Selection::All).map(|_| ())
            }
            _ => Err(self.fail("Expecting value")),
        }
    }

    /// Checks a string as `string` would: escapes, no control characters, strict UTF-8.
    fn skip_string(&mut self) -> Result<(), JsonError> {
        self.index += 1;
        let start = self.index;
        let mut ascii_only = true;
        while let Some(byte) = self.peek() {
            match byte {
                b'"' => {
                    // Escapes are ASCII, so checking the raw bytes between the quotes checks every non-ASCII run.
                    if !ascii_only && !is_valid_utf8(&self.bytes[start..self.index], self.surrogates) {
                        return Err(JsonError { message: "Invalid UTF-8", offset: start, encoding: true });
                    }
                    self.index += 1;
                    return Ok(());
                }
                0..0x20 => return Err(self.fail("Invalid control character")),
                b'\\' => {
                    self.index += 1;
                    match self.peek() {
                        None => break,
                        Some(b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't') => self.index += 1,
                        Some(b'u') => {
                            self.index += 1;
                            self.hex4()?;
                        }
                        Some(_) => return Err(self.fail("Invalid \\escape")),
                    }
                }
                _ => {
                    if byte >= 0x80 {
                        ascii_only = false;
                    }
                    self.index += 1;
                }
            }
        }
        Err(self.fail("Unterminated string"))
    }

    fn number(&mut self) -> Result<Json, JsonError> {
        let start = self.index;
        let digit = |parser: &Self, at: usize| parser.bytes.get(at).is_some_and(u8::is_ascii_digit);
        if self.peek() == Some(b'-') {
            self.index += 1;
        }
        if !digit(self, self.index) {
            self.index = start;
            return Err(self.fail("Expecting value"));
        }
        if self.peek() == Some(b'0') {
            self.index += 1;
        } else {
            while digit(self, self.index) {
                self.index += 1;
            }
        }
        let mut integral = true;
        if self.peek() == Some(b'.') && digit(self, self.index + 1) {
            integral = false;
            self.index += 1;
            while digit(self, self.index) {
                self.index += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            let mut probe = self.index + 1;
            if matches!(self.bytes.get(probe), Some(b'+' | b'-')) {
                probe += 1;
            }
            if digit(self, probe) {
                integral = false;
                self.index = probe;
                while digit(self, self.index) {
                    self.index += 1;
                }
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.index]).expect("digits are ASCII");
        // Python keeps integers of any size; past 64 bits the value is kept as a float.
        if integral && let Ok(value) = text.parse::<i64>() {
            return Ok(Json::Int(value));
        }
        text.parse::<f64>().map(Json::Double).map_err(|_| self.fail("Invalid number"))
    }

    fn string(&mut self) -> Result<String, JsonError> {
        self.index += 1;
        let start = self.index;
        // Fast path: no escapes.
        while let Some(byte) = self.peek() {
            match byte {
                b'"' => {
                    let text = self.decode(start, self.index)?;
                    self.index += 1;
                    return Ok(text);
                }
                b'\\' => break,
                0..0x20 => return Err(self.fail("Invalid control character")),
                _ => self.index += 1,
            }
        }
        let mut result = self.decode(start, self.index)?;
        while let Some(byte) = self.peek() {
            match byte {
                b'"' => {
                    self.index += 1;
                    return Ok(result);
                }
                0..0x20 => return Err(self.fail("Invalid control character")),
                b'\\' => {
                    self.index += 1;
                    let Some(escape) = self.peek() else { break };
                    self.index += 1;
                    match escape {
                        b'"' => result.push('"'),
                        b'\\' => result.push('\\'),
                        b'/' => result.push('/'),
                        b'b' => result.push('\u{08}'),
                        b'f' => result.push('\u{0C}'),
                        b'n' => result.push('\n'),
                        b'r' => result.push('\r'),
                        b't' => result.push('\t'),
                        b'u' => {
                            let mut code = self.hex4()?;
                            if (0xD800..0xDC00).contains(&code) && self.bytes[self.index..].starts_with(b"\\u") {
                                let mark = self.index;
                                self.index += 2;
                                let low = self.hex4()?;
                                if (0xDC00..0xE000).contains(&low) {
                                    code = 0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00);
                                } else {
                                    self.index = mark;
                                }
                            }
                            // A lone surrogate, which Python keeps in a str, has no place in a Rust String.
                            result.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
                        }
                        _ => {
                            self.index -= 1;
                            return Err(self.fail("Invalid \\escape"));
                        }
                    }
                }
                _ => {
                    let run = self.index;
                    while self.peek().is_some_and(|byte| byte != b'"' && byte != b'\\' && byte >= 0x20) {
                        self.index += 1;
                    }
                    result += &self.decode(run, self.index)?;
                }
            }
        }
        Err(self.fail("Unterminated string"))
    }

    fn hex4(&mut self) -> Result<u32, JsonError> {
        let Some(digits) = self.bytes.get(self.index..self.index + 4) else {
            return Err(self.fail("Invalid \\uXXXX escape"));
        };
        let mut value = 0;
        for &byte in digits {
            let digit = match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                b'A'..=b'F' => byte - b'A' + 10,
                _ => return Err(self.fail("Invalid \\uXXXX escape")),
            };
            value = value << 4 | u32::from(digit);
            self.index += 1;
        }
        Ok(value)
    }

    fn decode(&self, start: usize, end: usize) -> Result<String, JsonError> {
        let slice = &self.bytes[start..end];
        if !is_valid_utf8(slice, self.surrogates) {
            return Err(JsonError { message: "Invalid UTF-8", offset: start, encoding: true });
        }
        // Encoded surrogates (accepted only with `surrogates`) become U+FFFD, as Swift's decoding did.
        Ok(String::from_utf8_lossy(slice).into_owned())
    }
}

/// Strict UTF-8, as Python's decoder: no overlong forms, no encoded surrogates, nothing past U+10FFFF.
pub fn is_valid_utf8(bytes: &[u8], surrogates: bool) -> bool {
    let mut index = 0;
    while index < bytes.len() {
        let lead = bytes[index];
        if lead < 0x80 {
            index += 1;
            continue;
        }
        let (length, low, high) = match lead {
            0xC2..=0xDF => (2, 0x80, 0xBF),
            0xE0 => (3, 0xA0, 0xBF),
            0xE1..=0xEC | 0xEE..=0xEF => (3, 0x80, 0xBF),
            0xED => (3, 0x80, if surrogates { 0xBF } else { 0x9F }),
            0xF0 => (4, 0x90, 0xBF),
            0xF1..=0xF3 => (4, 0x80, 0xBF),
            0xF4 => (4, 0x80, 0x8F),
            _ => return false,
        };
        let Some(sequence) = bytes.get(index..index + length) else { return false };
        if !(low..=high).contains(&sequence[1]) || sequence[2..].iter().any(|byte| byte & 0xC0 != 0x80) {
            return false;
        }
        index += length;
    }
    true
}

/// How `serialized` lays text out: json.dump's defaults are no ASCII escaping and ", " / ": " separators.
#[derive(Clone, Copy, Debug)]
pub struct Format<'a> {
    pub ensure_ascii: bool,
    pub item: &'a str,
    pub key: &'a str,
    pub sort_keys: bool,
}

impl Default for Format<'_> {
    fn default() -> Self {
        Format { ensure_ascii: false, item: ", ", key: ": ", sort_keys: false }
    }
}

impl Json {
    /// Text as Python's `json.dump(value, f, ensure_ascii=False)` writes it.
    pub fn serialized(&self) -> String {
        self.serialized_with(Format::default())
    }

    pub fn serialized_with(&self, format: Format) -> String {
        let mut output = String::new();
        self.write(&mut output, format);
        output
    }

    /// `json.dumps(value, indent=n)`: one item per line, "," then a newline between items, empty containers kept
    /// short.
    pub fn serialized_indented(&self, ensure_ascii: bool, indent: usize) -> String {
        self.indented(ensure_ascii, indent, 0)
    }

    fn indented(&self, ensure_ascii: bool, indent: usize, level: usize) -> String {
        let inner = " ".repeat(indent * (level + 1));
        let outer = " ".repeat(indent * level);
        let flat = Format { ensure_ascii, ..Format::default() };
        match self {
            Json::Array(items) if !items.is_empty() => {
                let lines: Vec<String> =
                    items.iter().map(|item| inner.clone() + &item.indented(ensure_ascii, indent, level + 1)).collect();
                format!("[\n{}\n{outer}]", lines.join(",\n"))
            }
            Json::Object(object) if !object.is_empty() => {
                let lines: Vec<String> = object
                    .iter()
                    .map(|(name, value)| {
                        let mut key = String::new();
                        write_string(name, &mut key, ensure_ascii);
                        format!("{inner}{key}: {}", value.indented(ensure_ascii, indent, level + 1))
                    })
                    .collect();
                format!("{{\n{}\n{outer}}}", lines.join(",\n"))
            }
            _ => self.serialized_with(flat),
        }
    }

    fn write(&self, output: &mut String, format: Format) {
        match self {
            Json::Null => output.push_str("null"),
            Json::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
            Json::Int(value) => output.push_str(&value.to_string()),
            Json::Double(value) => output.push_str(&python_repr(*value)),
            Json::String(value) => write_string(value, output, format.ensure_ascii),
            Json::Array(items) => {
                output.push('[');
                for (offset, item) in items.iter().enumerate() {
                    if offset > 0 {
                        output.push_str(format.item);
                    }
                    item.write(output, format);
                }
                output.push(']');
            }
            Json::Object(object) => {
                output.push('{');
                let mut names: Vec<&String> = object.keys().collect();
                if format.sort_keys {
                    // Python sorts str by code point, which is byte order in UTF-8.
                    names.sort();
                }
                for (offset, name) in names.into_iter().enumerate() {
                    if offset > 0 {
                        output.push_str(format.item);
                    }
                    write_string(name, output, format.ensure_ascii);
                    output.push_str(format.key);
                    object.get(name).expect("listed key").write(output, format);
                }
                output.push('}');
            }
        }
    }
}

fn write_string(value: &str, output: &mut String, ensure_ascii: bool) {
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            '\u{08}' => output.push_str("\\b"),
            '\u{0C}' => output.push_str("\\f"),
            _ if (character as u32) < 0x20 || (ensure_ascii && (character as u32) > 0x7E) => {
                let mut units = [0u16; 2];
                for unit in character.encode_utf16(&mut units) {
                    output.push_str(&format!("\\u{unit:04x}"));
                }
            }
            _ => output.push(character),
        }
    }
    output.push('"');
}

impl fmt::Display for Json {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.serialized())
    }
}

/// Python's `repr()` of a float as json writes it: the shortest text that reads back the same, positional from
/// 1e-4 up to 1e16 and in exponent form outside it; NaN and Infinity as json spells them.
pub fn python_repr(value: f64) -> String {
    if value.is_nan() {
        return "NaN".into();
    }
    if value.is_infinite() {
        return if value < 0.0 { "-Infinity" } else { "Infinity" }.into();
    }
    if value == 0.0 {
        return if value.is_sign_negative() { "-0.0" } else { "0.0" }.into();
    }
    // Rust's exponent form is the shortest round-trip text too: take its digits and exponent and lay them out by
    // Python's rules.
    let text = format!("{:e}", value.abs());
    let (mantissa, exponent) = text.split_once('e').expect("exponent form");
    let exponent: i32 = exponent.parse().expect("exponent digits");
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    let sign = if value < 0.0 { "-" } else { "" };
    if !(-4..16).contains(&exponent) {
        let mantissa = if digits.len() > 1 { format!("{}.{}", &digits[..1], &digits[1..]) } else { digits.to_string() };
        let power = exponent.unsigned_abs();
        return format!("{sign}{mantissa}e{}{power:02}", if exponent < 0 { '-' } else { '+' });
    }
    if exponent < 0 {
        return format!("{sign}0.{}{digits}", "0".repeat((-exponent - 1) as usize));
    }
    let whole = exponent as usize + 1;
    if digits.len() <= whole {
        return format!("{sign}{digits}{}.0", "0".repeat(whole - digits.len()));
    }
    format!("{sign}{}.{}", &digits[..whole], &digits[whole..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_print_as_python_repr() {
        let cases = [
            (0.1, "0.1"),
            (1e-5, "1e-05"),
            (1e16, "1e+16"),
            (1e15, "1000000000000000.0"),
            (9999999999999998.0, "9999999999999998.0"),
            (5e-324, "5e-324"),
            (-0.0, "-0.0"),
            (3.0, "3.0"),
            (-123.456, "-123.456"),
            (1e300, "1e+300"),
            (0.0001, "0.0001"),
        ];
        for (value, text) in cases {
            assert_eq!(python_repr(value), text, "{value}");
        }
    }

    #[test]
    fn objects_keep_the_first_place_and_the_last_value() {
        let value = Json::parse_str(r#"{"b": 1, "a": [true, false, null], "b": 2.5}"#).unwrap();
        assert_eq!(value.serialized(), r#"{"b": 2.5, "a": [true, false, null]}"#);
        assert_eq!(Json::Int(1), Json::Double(1.0));
        assert_eq!(Json::Bool(true), Json::Int(1));
    }

    #[test]
    fn strings_escape_as_json_dumps() {
        let value = Json::parse_str(r#""caf\u00e9 \ud83d\ude00 \u0001""#).unwrap();
        assert_eq!(value.serialized(), "\"café 😀 \\u0001\"");
        let ascii = Format { ensure_ascii: true, ..Format::default() };
        assert_eq!(value.serialized_with(ascii), r#""caf\u00e9 \ud83d\ude00 \u0001""#);
    }

    #[test]
    fn rejects_what_json_loads_rejects() {
        for broken in ["\u{feff}{}", "01", "[1,]", "{\"a\" 1}", "[1 2]", "\"tab\there\"", "nul", "[", "", "1 2"] {
            assert!(Json::parse_str(broken).is_err(), "{broken:?}");
        }
    }
}
