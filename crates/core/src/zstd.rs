//! Zstandard decompression (RFC 8878), for the compressed logs Codex archives and DeepSeek Harness writes:
//! concatenated frames, skippable frames passed over, a damaged frame fails rather than producing wrong text.

use std::fmt;
use std::io::Read;
use std::path::Path;

use ruzstd::decoding::StreamingDecoder;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure(pub String);

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Failure {}

pub fn decompress_file(path: &Path) -> Result<Vec<u8>, Failure> {
    let data = std::fs::read(path).map_err(|_| Failure(format!("cannot read {}", path.display())))?;
    decompress(&data)
}

pub fn decompress(data: &[u8]) -> Result<Vec<u8>, Failure> {
    let mut output = Vec::with_capacity(data.len().saturating_mul(8).max(1 << 16));
    let mut input = data;
    while !input.is_empty() {
        let Some(magic) = input.get(..4).map(|bytes| u32::from_le_bytes(bytes.try_into().expect("four bytes"))) else {
            return Err(Failure("truncated frame".into()));
        };
        if magic & 0xFFFF_FFF0 == 0x184D_2A50 {
            let size = input
                .get(4..8)
                .map(|bytes| u32::from_le_bytes(bytes.try_into().expect("four bytes")) as usize)
                .ok_or_else(|| Failure("truncated skippable frame".into()))?;
            input = input.get(8 + size..).ok_or_else(|| Failure("truncated skippable frame".into()))?;
            continue;
        }
        if magic != 0xFD2F_B528 {
            return Err(Failure("not a zstd frame".into()));
        }
        let mut decoder = StreamingDecoder::new(&mut input).map_err(|error| Failure(error.to_string()))?;
        decoder.read_to_end(&mut output).map_err(|error| Failure(error.to_string()))?;
    }
    Ok(output)
}
