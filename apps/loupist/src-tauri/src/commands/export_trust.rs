//! Host-owned tags for exact bytes exported by the app. They are deliberately
//! outside exported sidecars, which the model can edit, and survive restarts.
//! Content addressing also recognizes unchanged copies and renamed exports.
use crate::app::{AppState, CommandError};
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path};

fn marker(app_data: &Path, digest: &str) -> std::path::PathBuf {
    app_data.join("export-trust").join(digest)
}

/// Record before publishing bytes so a failed tag cannot leave an unmarked export.
pub(crate) fn record(state: &AppState, bytes: &[u8]) -> Result<(), CommandError> {
    record_digest(
        &state.artifacts.app_data_dir,
        &hex::encode(Sha256::digest(bytes)),
    )
}

pub(crate) fn record_digest(app_data: &Path, digest: &str) -> Result<(), CommandError> {
    validate_digest(digest)?;
    let path = marker(app_data, digest);
    std::fs::create_dir_all(path.parent().expect("export trust directory"))
        .and_then(|()| std::fs::write(path, b"third_party_text\n"))
        .map_err(|e| CommandError::internal(e.to_string()))
}

pub(crate) fn contains(state: &AppState, digest: &str) -> Result<bool, CommandError> {
    contains_at(&state.artifacts.app_data_dir, digest)
}

fn validate_digest(digest: &str) -> Result<(), CommandError> {
    if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(CommandError::invalid("Invalid export digest"));
    }
    Ok(())
}

pub(crate) fn contains_at(app_data: &Path, digest: &str) -> Result<bool, CommandError> {
    validate_digest(digest)?;
    match std::fs::metadata(marker(app_data, digest)) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(CommandError::internal(error.to_string())),
    }
}

/// Hash through one handle without retaining potentially large file contents.
/// Native tools still own missing/unreadable-file errors.
pub(crate) fn tagged_file(state: &AppState, path: &Path) -> Result<bool, CommandError> {
    if !state.artifacts.app_data_dir.join("export-trust").exists() {
        return Ok(false);
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let Ok(mut file) = options.open(path) else {
        return Ok(false);
    };
    let metadata = file
        .metadata()
        .map_err(|e| CommandError::internal(e.to_string()))?;
    if !metadata.is_file() {
        return Ok(false);
    }
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|e| CommandError::internal(e.to_string()))?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    contains(state, &hex::encode(hash.finalize()))
}
