//! Filesystem safety: pinned Loupist roots, link-refusing workspace reads, bundled
//! artifact trust and private deletion below the app-data root.

pub(crate) mod artifact_trust;
pub(crate) mod loupist_root;
pub(crate) mod pinned_root;
pub(crate) mod secure_delete;
#[cfg(windows)]
pub(crate) mod windows;
pub(crate) mod workspace_files;
