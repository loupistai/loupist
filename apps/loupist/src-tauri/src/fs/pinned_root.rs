#[cfg(unix)]
use rustix::{
    fd::{AsFd, BorrowedFd, OwnedFd},
    fs::{fstat, open, FileType, Mode, OFlags},
};
use std::io;
#[cfg(unix)]
use std::path::{Component, Path, PathBuf};
use thiserror::Error;

#[cfg(unix)]
use crate::domain::RootFilesystemIdentity;

#[derive(Debug, Error)]
pub enum PinnedRootError {
    #[error("Loupist root path must be absolute and contain only ordinary components")]
    InvalidPath,
    #[error("Loupist root is not the imported directory")]
    IdentityMismatch,
    #[error("Loupist root is not a directory")]
    NotDirectory,
    #[error("Loupist root I/O failed: {0}")]
    Io(#[from] io::Error),
}

/// An already-open Loupist workspace directory bound to the filesystem identity
/// captured at import time. Security-sensitive work must retain this handle
/// across every phase instead of resolving the stored pathname again.
#[cfg(unix)]
#[derive(Debug)]
pub struct PinnedLoupistRoot {
    descriptor: OwnedFd,
    identity: RootFilesystemIdentity,
    opened_path: PathBuf,
}

#[cfg(unix)]
impl PinnedLoupistRoot {
    /// Opens a selected directory once and captures the identity that may be
    /// stored after initialization and validation complete on this same handle.
    pub fn open_unbound(path: &Path) -> Result<Self, PinnedRootError> {
        if !path.is_absolute()
            || path
                .components()
                .any(|component| !matches!(component, Component::RootDir | Component::Normal(_)))
        {
            return Err(PinnedRootError::InvalidPath);
        }
        let descriptor = open(path, directory_open_flags(), Mode::empty())
            .map_err(|error| PinnedRootError::Io(io::Error::from(error)))?;
        let metadata =
            fstat(&descriptor).map_err(|error| PinnedRootError::Io(io::Error::from(error)))?;
        if FileType::from_raw_mode(metadata.st_mode) != FileType::Directory {
            return Err(PinnedRootError::NotDirectory);
        }
        Ok(Self {
            descriptor,
            identity: RootFilesystemIdentity {
                device: metadata.st_dev as u64,
                inode: metadata.st_ino as u64,
            },
            opened_path: path.to_path_buf(),
        })
    }

    pub fn open(path: &Path, expected: &RootFilesystemIdentity) -> Result<Self, PinnedRootError> {
        let pinned = Self::open_unbound(path)?;
        if pinned.identity() != expected {
            return Err(PinnedRootError::IdentityMismatch);
        }
        Ok(pinned)
    }

    pub fn identity(&self) -> &RootFilesystemIdentity {
        &self.identity
    }

    /// Returns the path used to open this descriptor for display-only values.
    /// Security-sensitive I/O must use this handle instead of resolving it.
    pub fn opened_path(&self) -> &Path {
        &self.opened_path
    }
}

#[cfg(unix)]
impl AsFd for PinnedLoupistRoot {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.descriptor.as_fd()
    }
}

#[cfg(unix)]
fn directory_open_flags() -> OFlags {
    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    use rustix::io::{fcntl_getfd, FdFlags};
    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt;

    #[cfg(unix)]
    fn identity(path: &Path) -> RootFilesystemIdentity {
        let metadata = std::fs::symlink_metadata(path).unwrap();
        RootFilesystemIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
        }
    }

    #[cfg(unix)]
    #[test]
    fn opens_only_the_expected_directory_with_close_on_exec() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("loupist-a");
        let replacement = temporary.path().join("loupist-b");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&replacement).unwrap();
        let expected = identity(&root);

        let discovered = PinnedLoupistRoot::open_unbound(&root).unwrap();
        assert_eq!(discovered.identity(), &expected);
        drop(discovered);

        let pinned = PinnedLoupistRoot::open(&root, &expected).unwrap();
        assert_eq!(pinned.identity(), &expected);
        assert!(fcntl_getfd(&pinned).unwrap().contains(FdFlags::CLOEXEC));

        std::fs::rename(&root, temporary.path().join("loupist-a-original")).unwrap();
        std::fs::rename(&replacement, &root).unwrap();
        assert!(matches!(
            PinnedLoupistRoot::open(&root, &expected),
            Err(PinnedRootError::IdentityMismatch)
        ));
    }
}
