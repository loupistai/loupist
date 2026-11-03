use std::path::{Path, PathBuf};

#[cfg(windows)]
use std::fs::File;
#[cfg(any(unix, windows))]
use std::sync::Arc;

use crate::{
    app::CommandError,
    domain::{LoupistProfile, RootFilesystemIdentity},
};

#[cfg(unix)]
use crate::fs::pinned_root::{PinnedLoupistRoot, PinnedRootError};
#[cfg(windows)]
use crate::fs::windows::{filesystem_identity, open_directory_no_reparse};

#[cfg(windows)]
#[derive(Debug)]
struct WindowsPinnedLoupistRoot {
    _handle: File,
    identity: RootFilesystemIdentity,
}

#[cfg(windows)]
impl WindowsPinnedLoupistRoot {
    fn open_unbound(path: PathBuf) -> Result<Self, std::io::Error> {
        let handle = open_directory_no_reparse(&path)?;
        let identity = filesystem_identity(&handle)?;
        Ok(Self {
            _handle: handle,
            identity,
        })
    }

    fn open(path: PathBuf, expected: &RootFilesystemIdentity) -> Result<Self, std::io::Error> {
        let pinned = Self::open_unbound(path)?;
        if &pinned.identity != expected {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Loupist root filesystem identity changed",
            ));
        }
        Ok(pinned)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct BoundLoupistRoot {
    #[cfg(unix)]
    pinned: Arc<PinnedLoupistRoot>,
    #[cfg(windows)]
    pinned: Arc<WindowsPinnedLoupistRoot>,
    #[cfg(not(unix))]
    path: PathBuf,
}

#[cfg(unix)]
impl std::os::fd::AsFd for BoundLoupistRoot {
    fn as_fd(&self) -> std::os::fd::BorrowedFd<'_> {
        self.pinned.as_fd()
    }
}

impl BoundLoupistRoot {
    pub(crate) fn open_unbound(path: PathBuf) -> Result<Self, CommandError> {
        if !path.is_absolute() {
            return Err(CommandError::invalid(
                "selected Loupist root is not absolute",
            ));
        }
        #[cfg(unix)]
        {
            let pinned = PinnedLoupistRoot::open_unbound(&path).map_err(map_selected_root_error)?;
            Ok(Self {
                pinned: Arc::new(pinned),
            })
        }
        #[cfg(windows)]
        {
            let pinned =
                WindowsPinnedLoupistRoot::open_unbound(path.clone()).map_err(map_internal)?;
            Ok(Self {
                pinned: Arc::new(pinned),
                path,
            })
        }
        #[cfg(all(not(unix), not(windows)))]
        {
            let metadata = std::fs::symlink_metadata(&path).map_err(map_internal)?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(CommandError::invalid(
                    "selected Loupist root is not a regular directory",
                ));
            }
            Ok(Self { path })
        }
    }

    pub(crate) fn path(&self) -> &Path {
        #[cfg(unix)]
        {
            self.pinned.opened_path()
        }
        #[cfg(not(unix))]
        {
            &self.path
        }
    }

    pub(crate) fn identity(&self) -> Option<RootFilesystemIdentity> {
        #[cfg(unix)]
        {
            Some(self.pinned.identity().clone())
        }
        #[cfg(windows)]
        {
            Some(self.pinned.identity.clone())
        }
        #[cfg(all(not(unix), not(windows)))]
        {
            None
        }
    }
}

pub(crate) fn profile_workspace(
    profile: &LoupistProfile,
) -> Result<BoundLoupistRoot, CommandError> {
    profile.validate().map_err(map_internal)?;
    let path = PathBuf::from(&profile.workspace_root);
    if !path.is_absolute() {
        return Err(CommandError::internal(
            "stored Loupist root is not absolute",
        ));
    }
    #[cfg(unix)]
    {
        let expected = profile.root_filesystem_identity.as_ref().ok_or_else(|| {
            CommandError::conflict("Loupist root identity is missing; reconnect its folder")
        })?;
        let pinned = PinnedLoupistRoot::open(&path, expected).map_err(map_profile_root_error)?;
        Ok(BoundLoupistRoot {
            pinned: Arc::new(pinned),
        })
    }
    #[cfg(windows)]
    {
        let expected = profile.root_filesystem_identity.as_ref().ok_or_else(|| {
            CommandError::conflict("Loupist root identity is missing; reconnect its folder")
        })?;
        let pinned = WindowsPinnedLoupistRoot::open(path.clone(), expected).map_err(|_| {
            CommandError::conflict("Loupist root moved or was rebound; reconnect its folder")
        })?;
        Ok(BoundLoupistRoot {
            pinned: Arc::new(pinned),
            path,
        })
    }
    #[cfg(all(not(unix), not(windows)))]
    {
        let canonical = path
            .canonicalize()
            .map_err(|_| CommandError::conflict("Loupist root moved; reconnect its folder"))?;
        if canonical != path {
            return Err(CommandError::conflict(
                "Loupist root was rebound; reconnect its folder",
            ));
        }
        let metadata = std::fs::symlink_metadata(&path).map_err(map_internal)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(CommandError::conflict(
                "Loupist root is not the selected directory",
            ));
        }
        Ok(BoundLoupistRoot { path: canonical })
    }
}

fn map_internal(error: impl std::fmt::Display) -> CommandError {
    CommandError::internal(error.to_string())
}

#[cfg(unix)]
fn map_selected_root_error(error: PinnedRootError) -> CommandError {
    match error {
        PinnedRootError::InvalidPath | PinnedRootError::NotDirectory => {
            CommandError::invalid("selected Loupist root is not a regular directory")
        }
        PinnedRootError::IdentityMismatch => {
            CommandError::conflict("Loupist root identity changed; reconnect its folder")
        }
        PinnedRootError::Io(error) => CommandError::invalid(error.to_string()),
    }
}

#[cfg(unix)]
fn map_profile_root_error(error: PinnedRootError) -> CommandError {
    match error {
        PinnedRootError::IdentityMismatch => {
            CommandError::conflict("Loupist root identity changed; reconnect its folder")
        }
        PinnedRootError::InvalidPath | PinnedRootError::NotDirectory | PinnedRootError::Io(_) => {
            CommandError::conflict("Loupist root moved or was rebound; reconnect its folder")
        }
    }
}
