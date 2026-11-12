//! Host-owned tags for exact bytes exported by the app. They are deliberately
//! outside exported sidecars, which the model can edit, and survive restarts.
//! Content addressing also recognizes unchanged copies and renamed exports.
use crate::app::{AppState, CommandError};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub(crate) const MAX_HASH_BYTES: u64 = 32 * 1024 * 1024;
const HASH_DEADLINE: Duration = Duration::from_millis(500);

struct HashBudget<'a> {
    remaining: u64,
    deadline: Instant,
    cancel: &'a crate::web::SearchCancel,
}

impl HashBudget<'_> {
    fn stopped(&self) -> bool {
        self.cancel.is_cancelled() || Instant::now() >= self.deadline
    }
}

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

/// Check all admitted candidates under one cumulative budget. An incomplete
/// check is untrusted, never evidence that the file is ordinary user text.
pub(crate) fn tagged_files(
    state: &AppState,
    paths: &[PathBuf],
    cancel: &crate::web::SearchCancel,
) -> Result<bool, CommandError> {
    tagged_files_with_budget(
        state,
        paths,
        &mut HashBudget {
            remaining: MAX_HASH_BYTES,
            deadline: Instant::now() + HASH_DEADLINE,
            cancel,
        },
    )
}

fn tagged_files_with_budget(
    state: &AppState,
    paths: &[PathBuf],
    budget: &mut HashBudget<'_>,
) -> Result<bool, CommandError> {
    if budget.stopped() {
        return Ok(true);
    }
    if !state.artifacts.app_data_dir.join("export-trust").exists() {
        return Ok(false);
    }
    for path in paths {
        if budget.stopped() {
            return Ok(true);
        }
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NONBLOCK);
        }
        // Native tools still own missing/unreadable-file errors.
        let Ok(mut file) = options.open(path) else {
            continue;
        };
        let metadata = file
            .metadata()
            .map_err(|e| CommandError::internal(e.to_string()))?;
        if !metadata.is_file() {
            continue;
        }
        if metadata.len() > budget.remaining {
            return Ok(true);
        }
        let Some(digest) = hash_reader(&mut file, budget)? else {
            return Ok(true);
        };
        if contains(state, &digest)? {
            return Ok(true);
        }
    }
    Ok(budget.stopped())
}

/// Check cancellation/deadline between bounded reads, including a file growing
/// after metadata. None means the scan could not establish clean bytes.
fn hash_reader(
    reader: &mut impl Read,
    budget: &mut HashBudget<'_>,
) -> Result<Option<String>, CommandError> {
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        if budget.stopped() {
            return Ok(None);
        }
        let limit = buffer
            .len()
            .min(budget.remaining.saturating_add(1) as usize);
        let count = reader
            .read(&mut buffer[..limit])
            .map_err(|e| CommandError::internal(e.to_string()))?;
        if count == 0 {
            return Ok(Some(hex::encode(hash.finalize())));
        }
        if count as u64 > budget.remaining {
            return Ok(None);
        }
        budget.remaining -= count as u64;
        hash.update(&buffer[..count]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web::SearchCancel;

    #[test]
    fn file_trust_has_a_cumulative_byte_budget_and_overflow_is_untrusted() {
        let temp = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temp.path().join("app"));
        record(&state, b"an unrelated export").unwrap();
        let paths = [temp.path().join("a"), temp.path().join("b")];
        std::fs::write(&paths[0], b"abcdef").unwrap();
        std::fs::write(&paths[1], b"ghijkl").unwrap();
        let cancel = SearchCancel::never();
        let mut budget = HashBudget {
            remaining: 10,
            deadline: Instant::now() + Duration::from_secs(60),
            cancel: &cancel,
        };
        assert!(tagged_files_with_budget(&state, &paths, &mut budget).unwrap());
        assert_eq!(
            budget.remaining, 4,
            "the second candidate cannot reset the budget"
        );
        budget.remaining = 12;
        assert!(!tagged_files_with_budget(&state, &paths, &mut budget).unwrap());
        assert_eq!(budget.remaining, 0);
        budget.remaining = 4;
        assert!(tagged_files_with_budget(&state, &paths, &mut budget).unwrap());
        assert_eq!(budget.remaining, 4, "oversized files are not read at all");
    }

    #[test]
    fn an_expired_deadline_or_cancellation_never_establishes_trusted_files() {
        let temp = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temp.path().join("app"));
        let path = temp.path().join("not-opened");
        let cancel = SearchCancel::never();
        let mut budget = HashBudget {
            remaining: MAX_HASH_BYTES,
            deadline: Instant::now() - Duration::from_secs(1),
            cancel: &cancel,
        };
        assert!(
            tagged_files_with_budget(&state, std::slice::from_ref(&path), &mut budget).unwrap()
        );
        let (sender, receiver) = tokio::sync::watch::channel(false);
        let cancel = SearchCancel::from_watch(receiver);
        sender.send(true).unwrap();
        assert!(tagged_files(&state, &[path], &cancel).unwrap());
    }

    #[test]
    fn cancellation_is_checked_between_hash_reads_without_waiting_for_eof() {
        struct CancelOnRead(tokio::sync::watch::Sender<bool>);
        impl Read for CancelOnRead {
            fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
                assert!(
                    !*self.0.borrow(),
                    "the cancelled reader must not be called again"
                );
                bytes[0] = b'x';
                self.0.send(true).unwrap();
                Ok(1)
            }
        }
        let (sender, receiver) = tokio::sync::watch::channel(false);
        let cancel = SearchCancel::from_watch(receiver);
        let mut budget = HashBudget {
            remaining: MAX_HASH_BYTES,
            deadline: Instant::now() + Duration::from_secs(60),
            cancel: &cancel,
        };
        assert!(hash_reader(&mut CancelOnRead(sender), &mut budget)
            .unwrap()
            .is_none());
        assert_eq!(budget.remaining, MAX_HASH_BYTES - 1);
    }

    #[test]
    fn a_file_growing_after_metadata_cannot_escape_the_byte_budget() {
        let cancel = SearchCancel::never();
        let mut budget = HashBudget {
            remaining: 4,
            deadline: Instant::now() + Duration::from_secs(60),
            cancel: &cancel,
        };
        let mut reader = std::io::Cursor::new(b"123456789");
        assert!(hash_reader(&mut reader, &mut budget).unwrap().is_none());
        assert_eq!(
            reader.position(),
            5,
            "read at most one extra byte to find overflow"
        );
    }
}
