//! Claude usage from the transcripts Claude Code and the Claude app write: every response's tokens, deduplicated
//! across copies of the same message, kept in a compact index of hashed identifiers.

use sha2::{Digest, Sha256};

use crate::json::{Format, Json};

/// 128-bit hashed identifiers keep the index compact; raw IDs and paths are never stored.
pub fn digest(values: Vec<Json>) -> [u8; 16] {
    let text = Json::Array(values).serialized_with(Format { item: ",", key: ":", ..Format::default() });
    let hash = Sha256::digest(text.as_bytes());
    let mut digest = [0; 16];
    digest.copy_from_slice(&hash[..16]);
    digest
}
