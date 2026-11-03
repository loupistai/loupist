use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::ffi::CString;
#[cfg(unix)]
use std::os::fd::{AsFd, OwnedFd};

use rusqlite::{params, OptionalExtension, Transaction};
#[cfg(unix)]
use rustix::fs::{self, AtFlags, Mode, OFlags};
#[cfg(unix)]
use rustix::io::Errno;

use super::canonical::digest_bytes;
use super::error::{KnowledgeError, KnowledgeResult};
use super::model::{validate_digest, validate_media_type, validate_time, MAX_BLOB_BYTES};
use crate::fs::artifact_trust::ensure_private_directory;

/// Content-addressed bytes for retained representations.
///
/// The file and its containing directory are synced before the caller inserts
/// the database row. Parents are opened from a pinned directory descriptor, so
/// a swapped symlink cannot redirect the write.
#[derive(Debug)]
pub struct BlobStore {
    root: PathBuf,
}

impl BlobStore {
    pub fn open(root: impl AsRef<Path>) -> KnowledgeResult<Self> {
        let root = root.as_ref().to_path_buf();
        ensure_private_directory(&root)?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    #[cfg(any(windows, test))]
    pub fn path_for(&self, digest: &str) -> KnowledgeResult<PathBuf> {
        validate_digest(digest)?;
        Ok(self.root.join("sha256").join(&digest[..2]).join(digest))
    }

    pub(super) fn store(
        &self,
        tx: &Transaction<'_>,
        bytes: &[u8],
        media_type: &str,
        at_ms: i64,
    ) -> KnowledgeResult<String> {
        validate_media_type(media_type)?;
        validate_time(at_ms)?;
        if bytes.len() > MAX_BLOB_BYTES {
            return Err(KnowledgeError::Invalid("blob is too large"));
        }
        let digest = digest_bytes(bytes);
        self.write_file(&digest, bytes)?;
        let stored = self.read_file(&digest)?;
        if digest_bytes(&stored) != digest {
            return Err(KnowledgeError::DigestMismatch);
        }
        let existing: Option<i64> = tx
            .query_row(
                "SELECT byte_length FROM knowledge_blobs WHERE digest = ?1",
                [&digest],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(length) = existing {
            if length as usize != bytes.len() {
                return Err(KnowledgeError::DigestMismatch);
            }
            return Ok(digest);
        }
        tx.execute(
            "INSERT INTO knowledge_blobs (digest, byte_length, media_type, created_at_ms)
             VALUES (?1, ?2, ?3, ?4)",
            params![digest, bytes.len() as i64, media_type, at_ms],
        )?;
        Ok(digest)
    }

    pub(super) fn read(&self, digest: &str) -> KnowledgeResult<Vec<u8>> {
        self.read_file(digest)
    }

    pub(super) fn write_file(&self, digest: &str, bytes: &[u8]) -> KnowledgeResult<()> {
        #[cfg(unix)]
        {
            self.write_file_unix(digest, bytes)
        }
        #[cfg(windows)]
        {
            return self.write_file_windows(digest, bytes);
        }
    }

    #[cfg(unix)]
    fn write_file_unix(&self, digest: &str, bytes: &[u8]) -> KnowledgeResult<()> {
        validate_digest(digest)?;
        if bytes.len() > MAX_BLOB_BYTES {
            return Err(KnowledgeError::Invalid("blob is too large"));
        }
        let root = open_root(&self.root)?;
        let sha = open_or_create_child(&root, "sha256")?;
        let shard = open_or_create_child(&sha, &digest[..2])?;
        sync_dir(&sha)?;
        if let Some(existing) = read_named(&shard, digest)? {
            if digest_bytes(&existing) != digest {
                return Err(KnowledgeError::DigestMismatch);
            }
            return Ok(());
        }
        let temp_name = format!(".{digest}.{}.partial", uuid::Uuid::new_v4().simple());
        let temp = c_name(&temp_name)?;
        let final_name = c_name(digest)?;
        let mut cleanup = TempFile {
            directory: &shard,
            name: temp.clone(),
            armed: false,
        };
        let file_fd = match fs::openat(
            &shard,
            &temp,
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::RUSR | Mode::WUSR,
        ) {
            Ok(fd) => fd,
            Err(Errno::LOOP) | Err(Errno::NOTDIR) => {
                return Err(KnowledgeError::Invalid("blob path is untrusted"));
            }
            Err(error) => return Err(io_error(error)),
        };
        cleanup.armed = true;
        fs::fchmod(&file_fd, Mode::RUSR | Mode::WUSR).map_err(io_error)?;
        {
            let mut file = File::from(file_fd);
            file.write_all(bytes)?;
            file.sync_all()?;
        }
        match fs::linkat(&shard, &temp, &shard, &final_name, AtFlags::empty()) {
            Ok(()) => {
                cleanup.armed = false;
                let _ = fs::unlinkat(&shard, &temp, AtFlags::empty());
            }
            Err(Errno::EXIST) => {
                let existing = read_named(&shard, digest)?.ok_or(KnowledgeError::DigestMismatch)?;
                if digest_bytes(&existing) != digest {
                    return Err(KnowledgeError::DigestMismatch);
                }
                return sync_dir(&shard);
            }
            Err(Errno::LOOP) | Err(Errno::NOTDIR) => {
                return Err(KnowledgeError::Invalid("blob path is untrusted"));
            }
            Err(error) => return Err(io_error(error)),
        }
        let stored = read_named(&shard, digest)?.ok_or(KnowledgeError::MissingBytes)?;
        if digest_bytes(&stored) != digest {
            let _ = fs::unlinkat(&shard, &final_name, AtFlags::empty());
            return Err(KnowledgeError::DigestMismatch);
        }
        sync_dir(&shard)
    }

    fn read_file(&self, digest: &str) -> KnowledgeResult<Vec<u8>> {
        #[cfg(unix)]
        {
            self.read_file_unix(digest)
        }
        #[cfg(windows)]
        {
            return self.read_file_windows(digest);
        }
    }

    #[cfg(unix)]
    fn read_file_unix(&self, digest: &str) -> KnowledgeResult<Vec<u8>> {
        validate_digest(digest)?;
        let root = open_root(&self.root)?;
        let sha = open_existing_child(&root, "sha256")?;
        let shard = open_existing_child(&sha, &digest[..2])?;
        let bytes = read_named(&shard, digest)?.ok_or(KnowledgeError::MissingBytes)?;
        if digest_bytes(&bytes) != digest {
            return Err(KnowledgeError::DigestMismatch);
        }
        Ok(bytes)
    }

    #[cfg(windows)]
    fn write_file_windows(&self, digest: &str, bytes: &[u8]) -> KnowledgeResult<()> {
        validate_digest(digest)?;
        if bytes.len() > MAX_BLOB_BYTES {
            return Err(KnowledgeError::Invalid("blob is too large"));
        }
        let path = self.path_for(digest)?;
        let pinned = pin_windows_blob_directories(&self.root, digest)?;
        if let Ok(existing) = read_regular(&path) {
            if digest_bytes(&existing) != digest {
                return Err(KnowledgeError::DigestMismatch);
            }
            return pinned.confirm();
        }
        let temp = pinned.shard_path.join(format!(
            ".{digest}.{}.partial",
            uuid::Uuid::new_v4().simple()
        ));
        let mut cleanup = WindowsTempFile {
            path: temp.clone(),
            armed: false,
        };
        {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            crate::fs::windows::add_open_reparse_point_flag(&mut options);
            let mut file = options.open(&temp)?;
            cleanup.armed = true;
            if let Err(error) = file.write_all(bytes).and_then(|()| file.sync_all()) {
                return Err(error.into());
            }
        }
        if let Err(error) = crate::fs::windows::move_file_no_replace(&temp, &path) {
            cleanup.remove();
            let existing = read_regular(&path).map_err(|_| {
                KnowledgeError::Io(std::io::Error::new(
                    error.kind(),
                    "blob replace did not land",
                ))
            })?;
            if digest_bytes(&existing) != digest {
                return Err(KnowledgeError::DigestMismatch);
            }
            return pinned.confirm();
        }
        cleanup.armed = false;
        let stored = read_regular(&path)?;
        if digest_bytes(&stored) != digest {
            let _ = std::fs::remove_file(&path);
            return Err(KnowledgeError::DigestMismatch);
        }
        pinned.confirm()
    }

    #[cfg(windows)]
    fn read_file_windows(&self, digest: &str) -> KnowledgeResult<Vec<u8>> {
        validate_digest(digest)?;
        let path = self.path_for(digest)?;
        let parent = path
            .parent()
            .ok_or(KnowledgeError::Invalid("blob path is invalid"))?;
        let ancestors = crate::fs::windows::open_parent_directories_no_reparse(&path)
            .map_err(|_| KnowledgeError::Invalid("blob path is untrusted"))?;
        let root = crate::fs::windows::open_directory_no_reparse(&self.root)
            .map_err(|_| KnowledgeError::Invalid("blob path is untrusted"))?;
        let root_id = crate::fs::windows::filesystem_identity(&root)
            .map_err(|_| KnowledgeError::Invalid("blob path is untrusted"))?;
        let shard = crate::fs::windows::open_directory_no_reparse(parent)
            .map_err(|_| KnowledgeError::Invalid("blob path is untrusted"))?;
        let shard_id = crate::fs::windows::filesystem_identity(&shard)
            .map_err(|_| KnowledgeError::Invalid("blob path is untrusted"))?;
        let bytes = read_regular(&path)?;
        if digest_bytes(&bytes) != digest {
            return Err(KnowledgeError::DigestMismatch);
        }
        confirm_windows_identity(&self.root, &root_id, parent, &shard_id)?;
        drop(ancestors);
        drop(root);
        drop(shard);
        Ok(bytes)
    }
}

#[cfg(windows)]
struct PinnedWindowsBlobDirs {
    _ancestors: Vec<File>,
    _root: File,
    root_id: crate::domain::RootFilesystemIdentity,
    _sha: File,
    _shard: File,
    shard_id: crate::domain::RootFilesystemIdentity,
    root_path: PathBuf,
    shard_path: PathBuf,
}

#[cfg(windows)]
impl PinnedWindowsBlobDirs {
    fn confirm(&self) -> KnowledgeResult<()> {
        confirm_windows_identity(
            &self.root_path,
            &self.root_id,
            &self.shard_path,
            &self.shard_id,
        )
    }
}

#[cfg(windows)]
fn pin_windows_blob_directories(
    root: &Path,
    digest: &str,
) -> KnowledgeResult<PinnedWindowsBlobDirs> {
    let ancestors = crate::fs::windows::open_parent_directories_no_reparse(root)
        .map_err(|_| KnowledgeError::Invalid("blob path is untrusted"))?;
    let root_file = crate::fs::windows::open_directory_no_reparse(root)
        .map_err(|_| KnowledgeError::Invalid("blob path is untrusted"))?;
    let root_id = crate::fs::windows::filesystem_identity(&root_file)
        .map_err(|_| KnowledgeError::Invalid("blob path is untrusted"))?;
    let sha_path = root.join("sha256");
    let sha = open_or_create_no_reparse_dir(&sha_path)?;
    let shard_path = sha_path.join(&digest[..2]);
    let shard = open_or_create_no_reparse_dir(&shard_path)?;
    let shard_id = crate::fs::windows::filesystem_identity(&shard)
        .map_err(|_| KnowledgeError::Invalid("blob path is untrusted"))?;
    let observed_root = crate::fs::windows::filesystem_identity(&root_file)
        .map_err(|_| KnowledgeError::Invalid("blob path is untrusted"))?;
    if observed_root != root_id {
        return Err(KnowledgeError::Invalid("blob path is untrusted"));
    }
    Ok(PinnedWindowsBlobDirs {
        _ancestors: ancestors,
        _root: root_file,
        root_id,
        _sha: sha,
        _shard: shard,
        shard_id,
        root_path: root.to_path_buf(),
        shard_path,
    })
}

#[cfg(windows)]
fn open_or_create_no_reparse_dir(path: &Path) -> KnowledgeResult<File> {
    match crate::fs::windows::open_directory_no_reparse(path) {
        Ok(file) => Ok(file),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match std::fs::create_dir(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
            crate::fs::windows::open_directory_no_reparse(path)
                .map_err(|_| KnowledgeError::Invalid("blob path is untrusted"))
        }
        Err(_) => Err(KnowledgeError::Invalid("blob path is untrusted")),
    }
}

#[cfg(windows)]
struct WindowsTempFile {
    path: PathBuf,
    armed: bool,
}

#[cfg(windows)]
impl WindowsTempFile {
    fn remove(&mut self) {
        if self.armed {
            let _ = std::fs::remove_file(&self.path);
            self.armed = false;
        }
    }
}

#[cfg(windows)]
impl Drop for WindowsTempFile {
    fn drop(&mut self) {
        self.remove();
    }
}

#[cfg(windows)]
fn confirm_windows_identity(
    root: &Path,
    root_id: &crate::domain::RootFilesystemIdentity,
    shard: &Path,
    shard_id: &crate::domain::RootFilesystemIdentity,
) -> KnowledgeResult<()> {
    let root = crate::fs::windows::open_directory_no_reparse(root)
        .map_err(|_| KnowledgeError::Invalid("blob path is untrusted"))?;
    let shard = crate::fs::windows::open_directory_no_reparse(shard)
        .map_err(|_| KnowledgeError::Invalid("blob path is untrusted"))?;
    let observed_root = crate::fs::windows::filesystem_identity(&root)
        .map_err(|_| KnowledgeError::Invalid("blob path is untrusted"))?;
    let observed_shard = crate::fs::windows::filesystem_identity(&shard)
        .map_err(|_| KnowledgeError::Invalid("blob path is untrusted"))?;
    if &observed_root != root_id || &observed_shard != shard_id {
        return Err(KnowledgeError::Invalid("blob path is untrusted"));
    }
    Ok(())
}

#[cfg(windows)]
fn read_regular(path: &Path) -> KnowledgeResult<Vec<u8>> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    crate::fs::windows::add_open_reparse_point_flag(&mut options);
    let mut file = options
        .open(path)
        .map_err(|_| KnowledgeError::MissingBytes)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || crate::fs::windows::metadata_is_reparse(&metadata) {
        return Err(KnowledgeError::Invalid("blob path is untrusted"));
    }
    if metadata.len() > MAX_BLOB_BYTES as u64 {
        return Err(KnowledgeError::Invalid("blob is too large"));
    }
    let mut bytes = Vec::new();
    Read::take(&mut file, MAX_BLOB_BYTES as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > MAX_BLOB_BYTES {
        return Err(KnowledgeError::Invalid("blob is too large"));
    }
    Ok(bytes)
}

#[cfg(unix)]
struct TempFile<'a> {
    directory: &'a OwnedFd,
    name: CString,
    armed: bool,
}

#[cfg(unix)]
impl Drop for TempFile<'_> {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::unlinkat(self.directory, &self.name, AtFlags::empty());
        }
    }
}

#[cfg(unix)]
fn open_root(path: &Path) -> KnowledgeResult<OwnedFd> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(KnowledgeError::Invalid("blob path is untrusted"));
    }
    match fs::open(
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    ) {
        Ok(fd) => Ok(fd),
        Err(Errno::LOOP) | Err(Errno::NOTDIR) => {
            Err(KnowledgeError::Invalid("blob path is untrusted"))
        }
        Err(error) => Err(io_error(error)),
    }
}

#[cfg(unix)]
fn open_or_create_child(parent: &OwnedFd, name: &str) -> KnowledgeResult<OwnedFd> {
    let child = c_name(name)?;
    match open_directory(parent, &child) {
        Ok(fd) => Ok(fd),
        Err(KnowledgeError::MissingBytes) => {
            match fs::mkdirat(parent, &child, Mode::RUSR | Mode::WUSR | Mode::XUSR) {
                Ok(()) => {}
                Err(Errno::EXIST) => {}
                Err(Errno::LOOP) | Err(Errno::NOTDIR) => {
                    return Err(KnowledgeError::Invalid("blob path is untrusted"));
                }
                Err(error) => return Err(io_error(error)),
            }
            sync_dir(parent)?;
            open_directory(parent, &child)
        }
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
fn open_existing_child(parent: &OwnedFd, name: &str) -> KnowledgeResult<OwnedFd> {
    open_directory(parent, &c_name(name)?)
}

#[cfg(unix)]
fn open_directory(parent: &OwnedFd, name: &CString) -> KnowledgeResult<OwnedFd> {
    match fs::openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    ) {
        Ok(fd) => Ok(fd),
        Err(Errno::NOENT) => Err(KnowledgeError::MissingBytes),
        Err(Errno::LOOP) | Err(Errno::NOTDIR) => {
            Err(KnowledgeError::Invalid("blob path is untrusted"))
        }
        Err(error) => Err(io_error(error)),
    }
}

#[cfg(unix)]
fn read_named(directory: &OwnedFd, name: &str) -> KnowledgeResult<Option<Vec<u8>>> {
    let child = c_name(name)?;
    let fd = match fs::openat(
        directory,
        &child,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    ) {
        Ok(fd) => fd,
        Err(Errno::NOENT) => return Ok(None),
        Err(Errno::LOOP) | Err(Errno::NOTDIR) => {
            return Err(KnowledgeError::Invalid("blob path is untrusted"));
        }
        Err(error) => return Err(io_error(error)),
    };
    let mut file = File::from(fd);
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(KnowledgeError::Invalid("blob path is untrusted"));
    }
    if metadata.len() > MAX_BLOB_BYTES as u64 {
        return Err(KnowledgeError::Invalid("blob is too large"));
    }
    let mut bytes = Vec::new();
    Read::take(&mut file, MAX_BLOB_BYTES as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > MAX_BLOB_BYTES {
        return Err(KnowledgeError::Invalid("blob is too large"));
    }
    Ok(Some(bytes))
}

#[cfg(unix)]
fn sync_dir(directory: &impl AsFd) -> KnowledgeResult<()> {
    fs::fsync(directory).map_err(io_error)
}

#[cfg(unix)]
fn c_name(name: &str) -> KnowledgeResult<CString> {
    CString::new(name).map_err(|_| KnowledgeError::Invalid("blob path is invalid"))
}

#[cfg(unix)]
fn io_error(error: Errno) -> KnowledgeError {
    KnowledgeError::Io(std::io::Error::from(error))
}

#[cfg(all(windows, test))]
mod windows_tests {
    use super::*;

    fn sample() -> (Vec<u8>, String) {
        let bytes = b"secret".to_vec();
        let digest = digest_bytes(&bytes);
        (bytes, digest)
    }

    #[test]
    fn mismatched_write_leaves_no_partial() {
        let temporary = tempfile::tempdir().unwrap();
        let store = BlobStore::open(temporary.path().join("blobs")).unwrap();
        let digest = "ab".repeat(32);
        let error = store.write_file(&digest, b"not-the-digest").unwrap_err();
        assert!(matches!(error, KnowledgeError::DigestMismatch));
        let shard = temporary.path().join("blobs").join("sha256").join("ab");
        let leftovers = std::fs::read_dir(&shard)
            .map(|entries| entries.count())
            .unwrap_or(0);
        assert_eq!(leftovers, 0);
    }

    #[test]
    fn sha256_junction_does_not_create_a_shard_outside_the_root() {
        use std::os::windows::fs::symlink_dir;

        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("blobs");
        let outside = temporary.path().join("outside");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&outside).unwrap();
        if let Err(error) = symlink_dir(&outside, root.join("sha256")) {
            if error.kind() == std::io::ErrorKind::PermissionDenied {
                return;
            }
            panic!("failed to create test reparse point: {error}");
        }
        let store = BlobStore::open(&root).unwrap();
        let (bytes, digest) = sample();
        assert!(store.write_file(&digest, &bytes).is_err());
        assert!(std::fs::read_dir(&outside).unwrap().next().is_none());
        assert!(store.read_file(&digest).is_err());
    }

    #[test]
    fn read_rejects_bytes_reached_through_a_sha256_junction() {
        use std::os::windows::fs::symlink_dir;

        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("blobs");
        let outside = temporary.path().join("outside");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&outside).unwrap();
        let (bytes, digest) = sample();
        let shard = outside.join(&digest[..2]);
        std::fs::create_dir(&shard).unwrap();
        std::fs::write(shard.join(&digest), &bytes).unwrap();
        if let Err(error) = symlink_dir(&outside, root.join("sha256")) {
            if error.kind() == std::io::ErrorKind::PermissionDenied {
                return;
            }
            panic!("failed to create test reparse point: {error}");
        }
        let store = BlobStore::open(&root).unwrap();
        assert!(store.read_file(&digest).is_err());
        assert_eq!(std::fs::read(shard.join(&digest)).unwrap(), bytes);
    }
}
