//! Reading logs line by line as the Python reference's readline(32 MiB) does, and why a read stopped.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::Error;
use crate::database;
use crate::py::FileError;

/// Why reading one log stopped.
#[derive(Debug)]
pub enum ReadFailure {
    /// The file could not be opened or read; it counts as a failure and the rest go on.
    Unreadable,
    /// A line made the whole file unusable, as a Python ValueError does; also counted, and the rest go on.
    File(FileError),
    Other(Error),
}

impl From<FileError> for ReadFailure {
    fn from(error: FileError) -> Self {
        ReadFailure::File(error)
    }
}

impl From<database::SqlError> for ReadFailure {
    fn from(error: database::SqlError) -> Self {
        ReadFailure::Other(error.into())
    }
}

impl From<Error> for ReadFailure {
    fn from(error: Error) -> Self {
        ReadFailure::Other(error)
    }
}

/// Lines longer than this (Python's readline(32 MiB)) stop a read.
pub const LINE_LIMIT: usize = 32 * 1024 * 1024;

/// Complete lines of a file from an offset: a line Python's readline(32 MiB) would return whole and ending in a
/// newline. Reading stops before a line still being written (no newline yet) or one longer than 32 MiB, and
/// returns where the next read starts. Each line is lent to `body` with its offset.
pub fn read_lines(
    path: &Path,
    offset: i64,
    mut body: impl FnMut(i64, &[u8]) -> Result<(), ReadFailure>,
) -> Result<i64, ReadFailure> {
    let mut file = File::open(path).map_err(|_| ReadFailure::Unreadable)?;
    file.seek(SeekFrom::Start(offset.max(0) as u64)).map_err(|_| ReadFailure::Unreadable)?;
    let mut buffer: Vec<u8> = Vec::new();
    let mut buffer_start = offset;
    let mut done = false;
    let mut chunk = vec![0u8; 4 << 20];
    loop {
        let mut line_start = 0;
        let mut stop = false;
        while line_start < buffer.len() {
            let Some(found) = memchr::memchr(b'\n', &buffer[line_start..]) else { break };
            let newline = line_start + found;
            if newline - line_start + 1 > LINE_LIMIT {
                stop = true;
                break;
            }
            body(buffer_start + line_start as i64, &buffer[line_start..=newline])?;
            line_start = newline + 1;
        }
        if line_start > 0 {
            buffer.drain(..line_start);
            buffer_start += line_start as i64;
        }
        if stop || buffer.len() >= LINE_LIMIT || done {
            return Ok(buffer_start);
        }
        let count = file.read(&mut chunk).map_err(|_| ReadFailure::Unreadable)?;
        if count == 0 {
            done = true;
        } else {
            buffer.extend_from_slice(&chunk[..count]);
        }
    }
}

/// The same lines from bytes already in memory (a decompressed log), starting at `offset`.
pub fn lines_in(
    bytes: &[u8],
    offset: usize,
    mut body: impl FnMut(i64, &[u8]) -> Result<(), ReadFailure>,
) -> Result<(), ReadFailure> {
    let mut line_start = offset;
    while line_start < bytes.len() {
        let Some(found) = memchr::memchr(b'\n', &bytes[line_start..]) else { break };
        let newline = line_start + found;
        if newline - line_start + 1 > LINE_LIMIT {
            return Ok(());
        }
        body(line_start as i64, &bytes[line_start..=newline])?;
        line_start = newline + 1;
    }
    Ok(())
}

/// The line without a leading UTF-8 byte-order mark, which json.loads on bytes also drops.
pub fn without_byte_order_mark(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes)
}
