//! Small shared helpers that hold no app state.

pub(crate) mod credential_text;
pub(crate) mod json_pointer;

use sha2::{Digest, Sha256};

/// Lowercase hex SHA-256 digest of raw bytes, shared by every content-digest surface.
pub fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
