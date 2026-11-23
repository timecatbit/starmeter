//! One SQLite connection, used the way the Python reference's sqlite3 uses it: bytes are blobs, str is text, and
//! opening waits for another process's lock.

use std::fmt;
use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags, params_from_iter};

pub use rusqlite::types::Value;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SqlError(pub String);

impl fmt::Display for SqlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SQLite error: {}", self.0)
    }
}

impl std::error::Error for SqlError {}

impl From<rusqlite::Error> for SqlError {
    fn from(error: rusqlite::Error) -> Self {
        SqlError(error.to_string())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    ReadWrite,
    ReadOnly,
    /// Read-only, with a `file:` URI as the path (for `?immutable=1` and the like).
    ReadOnlyUri,
}

pub struct Database {
    connection: Connection,
}

/// A `file:` URI for opening `path` with `query` (`mode=ro`, `immutable=1`): SQLite reads `?` as where the query
/// begins, `#` as where the fragment does and `%XX` as an escape, so those are escaped in the path, and on Windows its
/// backslashes become slashes and a drive letter gets the slash SQLite drops again.
pub fn file_uri(path: &str, query: &str) -> String {
    file_uri_on(path, query, cfg!(windows))
}

fn file_uri_on(path: &str, query: &str, windows: bool) -> String {
    let mut text = if windows { path.replace('\\', "/") } else { path.to_string() };
    if windows && text.as_bytes().get(1) == Some(&b':') {
        text.insert(0, '/');
    }
    let escaped = text.replace('%', "%25").replace('?', "%3F").replace('#', "%23");
    format!("file:{escaped}?{query}")
}

impl Database {
    /// Opens (creating, when writable) the file, waiting up to `timeout` seconds for another process's lock, as
    /// `sqlite3.connect` does.
    pub fn open(path: &Path, timeout: f64, mode: Mode) -> Result<Self, SqlError> {
        let flags = match mode {
            Mode::ReadWrite => OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
            Mode::ReadOnly => OpenFlags::SQLITE_OPEN_READ_ONLY,
            Mode::ReadOnlyUri => OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
        } | OpenFlags::SQLITE_OPEN_FULL_MUTEX;
        let connection = Connection::open_with_flags(path, flags)?;
        connection.busy_timeout(Duration::from_secs_f64(timeout))?;
        Ok(Database { connection })
    }

    /// Runs one or more statements that take no arguments, like executescript.
    pub fn execute(&self, sql: &str) -> Result<(), SqlError> {
        Ok(self.connection.execute_batch(sql)?)
    }

    /// Runs one statement and ignores any rows.
    pub fn run(&self, sql: &str, arguments: &[Value]) -> Result<(), SqlError> {
        let mut statement = self.connection.prepare_cached(sql)?;
        let mut rows = statement.query(params_from_iter(arguments.iter()))?;
        while rows.next()?.is_some() {}
        Ok(())
    }

    pub fn rows(&self, sql: &str, arguments: &[Value]) -> Result<Vec<Vec<Value>>, SqlError> {
        let mut result = Vec::new();
        self.each(sql, arguments, |row| {
            result.push(row);
            Ok(())
        })?;
        Ok(result)
    }

    pub fn first(&self, sql: &str, arguments: &[Value]) -> Result<Option<Vec<Value>>, SqlError> {
        let mut statement = self.connection.prepare_cached(sql)?;
        let columns = statement.column_count();
        let mut rows = statement.query(params_from_iter(arguments.iter()))?;
        match rows.next()? {
            Some(row) => Ok(Some(read_row(row, columns)?)),
            None => Ok(None),
        }
    }

    /// Streams rows; the statement is finished when this returns or fails.
    pub fn each(
        &self,
        sql: &str,
        arguments: &[Value],
        mut body: impl FnMut(Vec<Value>) -> Result<(), SqlError>,
    ) -> Result<(), SqlError> {
        let mut statement = self.connection.prepare_cached(sql)?;
        let columns = statement.column_count();
        let mut rows = statement.query(params_from_iter(arguments.iter()))?;
        while let Some(row) = rows.next()? {
            body(read_row(row, columns)?)?;
        }
        Ok(())
    }

    /// Rows changed by the last statement.
    pub fn changes(&self) -> u64 {
        self.connection.changes()
    }
}

fn read_row(row: &rusqlite::Row, columns: usize) -> Result<Vec<Value>, SqlError> {
    (0..columns).map(|column| row.get::<_, Value>(column).map_err(SqlError::from)).collect()
}

/// The integer in a column, if it holds one.
pub fn int(value: &Value) -> Option<i64> {
    match value {
        Value::Integer(number) => Some(*number),
        _ => None,
    }
}

/// A number in a column, integer or real.
pub fn double(value: &Value) -> Option<f64> {
    match value {
        Value::Integer(number) => Some(*number as f64),
        Value::Real(number) => Some(*number),
        _ => None,
    }
}

pub fn text(value: &Value) -> Option<&str> {
    match value {
        Value::Text(text) => Some(text),
        _ => None,
    }
}

pub fn blob(value: &Value) -> Option<&[u8]> {
    match value {
        Value::Blob(bytes) => Some(bytes),
        _ => None,
    }
}

#[cfg(test)]
mod uri_tests {
    use super::*;

    #[test]
    fn a_path_with_the_characters_a_uri_reads_specially_still_opens() {
        let folder = std::env::temp_dir().join(format!("starmeter-uri a?b#c%41 {}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("state.vscdb");
        let db = Database::open(&path, 1.0, Mode::ReadWrite).unwrap();
        db.execute("CREATE TABLE t (v TEXT); INSERT INTO t VALUES ('here');").unwrap();
        drop(db);
        let uri = file_uri(&path.to_string_lossy(), "mode=ro");
        let db = Database::open(Path::new(&uri), 1.0, Mode::ReadOnlyUri).expect("opened read-only");
        let row = db.first("SELECT v FROM t", &[]).unwrap();
        assert_eq!(row.and_then(|row| row.into_iter().next()), Some(Value::Text("here".into())));
        // Read-only indeed.
        assert!(db.execute("INSERT INTO t VALUES ('no')").is_err());
        drop(db);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn on_windows_the_path_is_written_with_slashes_and_a_drive_kept() {
        assert_eq!(
            file_uri_on(r"C:\Users\a#b\state.vscdb", "mode=ro", true),
            "file:/C:/Users/a%23b/state.vscdb?mode=ro"
        );
        assert_eq!(file_uri_on("/Users/a?b/x.db", "immutable=1", false), "file:/Users/a%3Fb/x.db?immutable=1");
    }
}
