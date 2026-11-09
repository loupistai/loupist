//! Private host/worker protocol. No field is accepted from a model except code
//! and the bounded timeout; Rust constructs every path and writable grant.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const MEMORY_BYTES: usize = 512 * 1024 * 1024;
pub const OUTPUT_BYTES: usize = 64 * 1024;
pub const CODE_BYTES: usize = 64 * 1024;
pub const DEFAULT_TIMEOUT_SECONDS: u64 = 60;
pub const MAX_TIMEOUT_SECONDS: u64 = 120;
pub const MAX_REQUEST_BYTES: usize = CODE_BYTES * 6 + 64 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerRequest {
    pub code: String,
    pub timeout_seconds: u64,
    pub runtime: PathBuf,
    pub cache: PathBuf,
    pub snapshot: PathBuf,
    pub writable: Vec<String>,
}
