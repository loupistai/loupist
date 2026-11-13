//! Private snapshot of the files one Python call may see, and the copy back
//! of changes in authorized workspace subfolders.
//!
//! Every host walk is descriptor-relative and does not follow links. A file
//! with another name, a symbolic link, a special file, a dot directory, git
//! metadata, or a path the native policy would not call workspace content is
//! left out. Writeback replaces host bytes only when the host file is still
//! the same regular file with the same content hash as the snapshot.

use std::{
    cmp::Reverse,
    collections::BTreeMap,
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    os::fd::OwnedFd,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use rustix::fs::{
    fchmod, fstat, mkdirat, openat, renameat, statat, unlinkat, AtFlags, Dir, FileType, Mode,
    OFlags,
};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

use crate::agent_harness::native_policy::{classify, NativeRoots, PathClass};

use super::{
    PythonError, MAX_FILE_BYTES, MAX_SNAPSHOT_BYTES, MAX_SNAPSHOT_FILES, MAX_TREE_ENTRIES,
    MAX_WRITABLE_DIRS, MAX_WRITEBACK_BYTES, MAX_WRITEBACK_FILES,
};

#[derive(Clone, Debug)]
struct FileStamp {
    dev: u64,
    ino: u64,
    sha256: [u8; 32],
}

#[derive(Clone, Copy, Debug)]
struct DirStamp {
    dev: u64,
    ino: u64,
}

#[derive(Debug)]
pub(crate) struct Prepared {
    staging: TempDir,
    workspace: PathBuf,
    roots: NativeRoots,
    root: DirStamp,
    writable: Vec<String>,
    files: BTreeMap<String, FileStamp>,
    dirs: BTreeMap<String, DirStamp>,
    pub(crate) omitted: Vec<String>,
    pub(crate) omitted_count: usize,
    pub(crate) scan_truncated: bool,
    pub(crate) untrusted_inputs: bool,
}

pub(crate) struct WriteReport {
    pub changed: Vec<String>,
    pub conflicts: Vec<Conflict>,
    pub artifacts: Vec<Artifact>,
    pub truncated: bool,
    visited: usize,
}

pub(crate) struct Conflict {
    pub path: String,
    pub reason: &'static str,
}

pub(crate) struct Artifact {
    pub path: String,
    pub kind: &'static str,
    pub media_type: &'static str,
    pub bytes: u64,
}

#[derive(Clone, Copy)]
enum SnapshotRoot {
    Workspace,
    Attachments,
    Scratch,
}

struct Candidate {
    root: SnapshotRoot,
    relative: String,
    identity: DirStamp,
    length: u64,
    modified: (i128, i128),
    writable: bool,
}

impl Candidate {
    fn guest_path(&self) -> String {
        match self.root {
            SnapshotRoot::Workspace => self.relative.clone(),
            SnapshotRoot::Attachments => format!("/attachments/{}", self.relative),
            SnapshotRoot::Scratch => format!("/scratch/{}", self.relative),
        }
    }

    fn priority(&self) -> (bool, Reverse<(i128, i128)>, String) {
        (
            !matches!(self.root, SnapshotRoot::Workspace) || !self.relative.starts_with("outputs/"),
            Reverse(self.modified),
            self.guest_path(),
        )
    }
}

struct Budget {
    files: usize,
    bytes: u64,
    entries: usize,
    candidates: Vec<Candidate>,
}

pub(crate) fn prepare(roots: &NativeRoots) -> Result<Prepared, PythonError> {
    // Private before the first byte is copied in: on a shared temporary
    // folder the default mode would let other local users read the copy.
    let staging = tempfile::Builder::new()
        .permissions(std::os::unix::fs::PermissionsExt::from_mode(0o700))
        .tempdir()
        .map_err(|_| PythonError::failed("workspace snapshot failed"))?;
    let workspace_fd = open_dir(&roots.workspace)?;
    let root_stat =
        fstat(&workspace_fd).map_err(|_| PythonError::failed("workspace snapshot failed"))?;
    let root = DirStamp {
        dev: device(&root_stat),
        ino: root_stat.st_ino,
    };
    let snap_workspace = mkdir(&staging, "workspace")?;
    let snap_attachments = mkdir(&staging, "attachments")?;
    let snap_scratch = mkdir(&staging, "scratch")?;
    mkdir_at(&snap_scratch, "matplotlib", Mode::from_raw_mode(0o755))?;
    let mut prepared = Prepared {
        staging,
        workspace: roots.workspace.clone(),
        roots: roots.clone(),
        root,
        writable: Vec::new(),
        files: BTreeMap::new(),
        dirs: BTreeMap::new(),
        omitted: Vec::new(),
        omitted_count: 0,
        scan_truncated: false,
        untrusted_inputs: false,
    };
    let mut budget = Budget {
        files: 0,
        bytes: 0,
        entries: 0,
        candidates: Vec::new(),
    };
    copy_tree(
        &workspace_fd,
        &snap_workspace,
        &roots.workspace,
        "",
        0,
        true,
        false,
        roots,
        &mut budget,
        &mut prepared,
    )?;
    let attachment_fd = roots
        .attachments
        .is_dir()
        .then(|| open_dir(&roots.attachments).ok())
        .flatten();
    let scratch_fd = roots
        .scratch
        .is_dir()
        .then(|| open_dir(&roots.scratch).ok())
        .flatten();
    for (source, target, logical, writable) in [
        (
            attachment_fd.as_ref(),
            &snap_attachments,
            &roots.attachments,
            false,
        ),
        (scratch_fd.as_ref(), &snap_scratch, &roots.scratch, true),
    ] {
        if let Some(source) = source {
            copy_tree(
                source,
                target,
                logical,
                "",
                0,
                false,
                writable,
                roots,
                &mut budget,
                &mut prepared,
            )?;
        }
    }
    // Reserve outputs even when file selection omits every ordinary file.
    if statat(&workspace_fd, "outputs", AtFlags::SYMLINK_NOFOLLOW)
        .is_ok_and(|meta| FileType::from_raw_mode(meta.st_mode) == FileType::RegularFile)
    {
        return Err(PythonError::failed(
            "workspace outputs must be a directory; rename the outputs file before running Python",
        ));
    }
    ensure_outputs(&mut prepared)?;
    budget.candidates.sort_by_key(Candidate::priority);
    for candidate in std::mem::take(&mut budget.candidates) {
        if budget.files >= MAX_SNAPSHOT_FILES
            || candidate.length > MAX_SNAPSHOT_BYTES.saturating_sub(budget.bytes)
        {
            prepared.omit(candidate.guest_path(), false);
            continue;
        }
        let (source, destination) = match candidate.root {
            SnapshotRoot::Workspace => (&workspace_fd, &snap_workspace),
            SnapshotRoot::Attachments => (
                attachment_fd.as_ref().expect("collected attachment root"),
                &snap_attachments,
            ),
            SnapshotRoot::Scratch => (
                scratch_fd.as_ref().expect("collected scratch root"),
                &snap_scratch,
            ),
        };
        let path = Path::new(&candidate.relative);
        let parent = path.parent().expect("relative file parent");
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("UTF-8 file name");
        let source = open_relative_dir(source, parent)?;
        let destination = open_relative_dir(destination, parent)?;
        let mode = Mode::from_raw_mode(if candidate.writable { 0o644 } else { 0o444 });
        let Some(copied) = copy_file(
            &source,
            &destination,
            name,
            mode,
            candidate.identity,
            MAX_FILE_BYTES.min(MAX_SNAPSHOT_BYTES.saturating_sub(budget.bytes)),
        )?
        else {
            prepared.omit(candidate.guest_path(), false);
            continue;
        };
        prepared.untrusted_inputs |= crate::commands::export_trust::contains_at(
            &roots.app_data,
            &hex::encode(copied.sha256),
        )
        .map_err(|_| PythonError::failed("workspace input trust could not be checked"))?;
        budget.files += 1;
        budget.bytes += copied.len;
        if matches!(candidate.root, SnapshotRoot::Workspace) && candidate.writable {
            prepared.files.insert(
                candidate.relative,
                FileStamp {
                    dev: candidate.identity.dev,
                    ino: candidate.identity.ino,
                    sha256: copied.sha256,
                },
            );
        }
    }
    // WASI preopen rights enforce read-only access. Keep owner-write on host
    // directories so TempDir can remove every private copy on all exit paths.
    Ok(prepared)
}

impl Prepared {
    fn omit(&mut self, path: String, subtree: bool) {
        self.omitted_count = self.omitted_count.saturating_add(1);
        self.scan_truncated |= subtree;
        self.omitted.push(path);
        self.omitted.sort();
        self.omitted.truncate(32);
    }

    pub(crate) fn snapshot_dir(&self) -> &Path {
        self.staging.path()
    }

    pub(crate) fn writable_names(&self) -> &[String] {
        &self.writable
    }

    pub(crate) fn writeback(&self) -> Result<WriteReport, PythonError> {
        let host = open_dir(&self.workspace)?;
        let host_stat =
            fstat(&host).map_err(|_| PythonError::failed("workspace writeback failed"))?;
        let mut report = WriteReport {
            changed: Vec::new(),
            conflicts: Vec::new(),
            artifacts: Vec::new(),
            truncated: false,
            visited: 0,
        };
        if device(&host_stat) != self.root.dev || host_stat.st_ino != self.root.ino {
            report.conflicts.push(Conflict {
                path: ".".into(),
                reason: "replaced",
            });
            return Ok(report);
        }
        let snap = open_dir(&self.staging.path().join("workspace"))?;
        for name in &self.writable {
            if report.truncated || report.changed.len() >= MAX_WRITEBACK_FILES {
                report.truncated = true;
                break;
            }
            sync_writable(name, &host, &snap, self, &mut report)?;
        }
        Ok(report)
    }
}

fn sync_writable(
    name: &str,
    host_root: &OwnedFd,
    snap_root: &OwnedFd,
    prepared: &Prepared,
    report: &mut WriteReport,
) -> Result<(), PythonError> {
    if !writeback_allowed(name, prepared, report) {
        return Ok(());
    }
    let snap_dir = match openat(snap_root, name, dir_flags(), Mode::empty()) {
        Ok(fd) => fd,
        Err(_) => {
            report.conflicts.push(Conflict {
                path: name.into(),
                reason: "directory_removed",
            });
            return Ok(());
        }
    };
    let host_meta = statat(host_root, name, AtFlags::SYMLINK_NOFOLLOW);
    let host_dir = match host_meta {
        Ok(meta) if FileType::from_raw_mode(meta.st_mode) == FileType::Directory => {
            if let Some(stamp) = prepared.dirs.get(name) {
                if device(&meta) != stamp.dev || meta.st_ino != stamp.ino {
                    report.conflicts.push(Conflict {
                        path: name.into(),
                        reason: "replaced",
                    });
                    return Ok(());
                }
            }
            openat(host_root, name, dir_flags(), Mode::empty())
                .map_err(|_| PythonError::failed("workspace writeback failed"))?
        }
        Ok(meta) if FileType::from_raw_mode(meta.st_mode) == FileType::Symlink => {
            report.conflicts.push(Conflict {
                path: name.into(),
                reason: "symlink",
            });
            return Ok(());
        }
        Ok(_) => {
            report.conflicts.push(Conflict {
                path: name.into(),
                reason: "type",
            });
            return Ok(());
        }
        Err(_) => {
            mkdirat(host_root, name, Mode::from_raw_mode(0o755))
                .map_err(|_| PythonError::failed("workspace writeback failed"))?;
            openat(host_root, name, dir_flags(), Mode::empty())
                .map_err(|_| PythonError::failed("workspace writeback failed"))?
        }
    };
    sync_dir(name, &host_dir, &snap_dir, prepared, report)
}

fn sync_dir(
    relative: &str,
    host_dir: &OwnedFd,
    snap_dir: &OwnedFd,
    prepared: &Prepared,
    report: &mut WriteReport,
) -> Result<(), PythonError> {
    if relative.split('/').count() > 32 {
        report.truncated = true;
        return Ok(());
    }
    if !writeback_allowed(relative, prepared, report)
        || !directory_still_named(relative, host_dir, prepared)
    {
        if report
            .conflicts
            .last()
            .is_none_or(|item| item.path != relative)
        {
            report.conflicts.push(Conflict {
                path: relative.into(),
                reason: "replaced",
            });
        }
        return Ok(());
    }
    let mut seen = BTreeMap::<String, ()>::new();
    let entries =
        Dir::read_from(snap_dir).map_err(|_| PythonError::failed("workspace writeback failed"))?;
    for entry in entries {
        let entry = entry.map_err(|_| PythonError::failed("workspace writeback failed"))?;
        report.visited += 1;
        if report.visited > MAX_TREE_ENTRIES {
            report.truncated = true;
            return Ok(());
        }
        let Ok(name) = entry.file_name().to_str() else {
            continue;
        };
        if name == "." || name == ".." || name.starts_with('.') || name.contains(['/', '\\']) {
            continue;
        }
        if report.truncated
            || report.visited >= MAX_TREE_ENTRIES
            || report.changed.len() >= MAX_WRITEBACK_FILES
            || report.artifacts.iter().map(|item| item.bytes).sum::<u64>() >= MAX_WRITEBACK_BYTES
        {
            report.truncated = true;
            return Ok(());
        }
        seen.insert(name.to_owned(), ());
        let path = join_relative(relative, name);
        if !writeback_allowed(&path, prepared, report) {
            continue;
        }
        let meta = match statat(snap_dir, name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(meta) => meta,
            Err(_) => continue,
        };
        let kind = FileType::from_raw_mode(meta.st_mode);
        if kind == FileType::Symlink {
            report.conflicts.push(Conflict {
                path,
                reason: "symlink",
            });
            continue;
        }
        if kind == FileType::Directory {
            sync_child_dir(&path, name, host_dir, snap_dir, prepared, report)?;
            continue;
        }
        if kind != FileType::RegularFile {
            continue;
        }
        write_file(&path, name, host_dir, snap_dir, prepared, report)?;
    }
    for path in prepared.dirs.keys() {
        let Some(rest) = path.strip_prefix(&format!("{relative}/")) else {
            continue;
        };
        if !rest.contains('/') && !seen.contains_key(rest) {
            report.conflicts.push(Conflict {
                path: path.into(),
                reason: "directory_removed",
            });
        }
    }
    for (path, stamp) in &prepared.files {
        if report.truncated || report.changed.len() >= MAX_WRITEBACK_FILES {
            report.truncated |= report.changed.len() >= MAX_WRITEBACK_FILES;
            break;
        }
        let Some(rest) = path.strip_prefix(&format!("{relative}/")) else {
            continue;
        };
        if rest.contains('/') || seen.contains_key(rest) {
            continue;
        }
        if writeback_allowed(path, prepared, report)
            && directory_still_named(relative, host_dir, prepared)
        {
            delete_if_unchanged(path, rest, host_dir, stamp, report)?;
        }
    }
    Ok(())
}

/// Reapply the same policy to guest-created names and to paths whose host
/// parents changed during execution. A private snapshot is not a write grant.
fn writeback_allowed(path: &str, prepared: &Prepared, report: &mut WriteReport) -> bool {
    let target = prepared.workspace.join(path);
    if std::fs::symlink_metadata(&target).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        report.conflicts.push(Conflict {
            path: path.into(),
            reason: "symlink",
        });
        return false;
    }
    let classified = classify(&target, &prepared.roots);
    if classified.class == PathClass::Workspace && classified.path == target {
        true
    } else {
        report.conflicts.push(Conflict {
            path: path.into(),
            reason: "protected",
        });
        false
    }
}

fn directory_still_named(path: &str, fd: &OwnedFd, prepared: &Prepared) -> bool {
    let Ok(named) = open_dir(&prepared.workspace.join(path)) else {
        return false;
    };
    let (Ok(actual), Ok(current)) = (fstat(fd), fstat(&named)) else {
        return false;
    };
    actual.st_dev == current.st_dev
        && actual.st_ino == current.st_ino
        && prepared
            .dirs
            .get(path)
            .is_none_or(|stamp| device(&current) == stamp.dev && current.st_ino == stamp.ino)
}

fn sync_child_dir(
    path: &str,
    name: &str,
    host_dir: &OwnedFd,
    snap_dir: &OwnedFd,
    prepared: &Prepared,
    report: &mut WriteReport,
) -> Result<(), PythonError> {
    let snap_child = openat(snap_dir, name, dir_flags(), Mode::empty())
        .map_err(|_| PythonError::failed("workspace writeback failed"))?;
    let host_child = match statat(host_dir, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(meta) if FileType::from_raw_mode(meta.st_mode) == FileType::Directory => {
            if let Some(stamp) = prepared.dirs.get(path) {
                if device(&meta) != stamp.dev || meta.st_ino != stamp.ino {
                    report.conflicts.push(Conflict {
                        path: path.into(),
                        reason: "replaced",
                    });
                    return Ok(());
                }
            }
            openat(host_dir, name, dir_flags(), Mode::empty())
                .map_err(|_| PythonError::failed("workspace writeback failed"))?
        }
        Ok(meta) if FileType::from_raw_mode(meta.st_mode) == FileType::Symlink => {
            report.conflicts.push(Conflict {
                path: path.into(),
                reason: "symlink",
            });
            return Ok(());
        }
        Ok(_) => {
            report.conflicts.push(Conflict {
                path: path.into(),
                reason: "type",
            });
            return Ok(());
        }
        Err(_) => {
            mkdirat(host_dir, name, Mode::from_raw_mode(0o755)).ok();
            match openat(host_dir, name, dir_flags(), Mode::empty()) {
                Ok(fd) => fd,
                Err(_) => {
                    report.conflicts.push(Conflict {
                        path: path.into(),
                        reason: "type",
                    });
                    return Ok(());
                }
            }
        }
    };
    sync_dir(path, &host_child, &snap_child, prepared, report)
}

fn write_file(
    path: &str,
    name: &str,
    host_dir: &OwnedFd,
    snap_dir: &OwnedFd,
    prepared: &Prepared,
    report: &mut WriteReport,
) -> Result<(), PythonError> {
    let snap = openat(
        snap_dir,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| PythonError::failed("workspace writeback failed"))?;
    let snap_file = File::from(snap);
    let snap_meta = snap_file
        .metadata()
        .map_err(|_| PythonError::failed("workspace writeback failed"))?;
    if !snap_meta.is_file() {
        return Ok(());
    }
    if snap_meta.len() > MAX_FILE_BYTES {
        report.conflicts.push(Conflict {
            path: path.into(),
            reason: "too_large",
        });
        return Ok(());
    }
    let bytes = read_limited(&snap_file, MAX_FILE_BYTES)?;
    let snap_hash = sha256(&bytes);
    if prepared.untrusted_inputs {
        // Tag exact derived bytes before publishing them; the tag stays valid
        // when a later turn reads a renamed or copied result.
        crate::commands::export_trust::record_digest(
            &prepared.roots.app_data,
            &hex::encode(snap_hash),
        )
        .map_err(|_| PythonError::failed("workspace output trust could not be retained"))?;
    }
    match statat(host_dir, name, AtFlags::SYMLINK_NOFOLLOW) {
        Err(_) => {
            if prepared.files.contains_key(path) {
                report.conflicts.push(Conflict {
                    path: path.into(),
                    reason: "replaced",
                });
                return Ok(());
            }
            if !write_fits(bytes.len() as u64, report) {
                return Ok(());
            }
            if create_exclusive(host_dir, name, &bytes)? {
                record_written(path, bytes.len() as u64, report);
            } else {
                report.conflicts.push(Conflict {
                    path: path.into(),
                    reason: "host_changed",
                });
            }
        }
        Ok(meta) if FileType::from_raw_mode(meta.st_mode) == FileType::Symlink => {
            report.conflicts.push(Conflict {
                path: path.into(),
                reason: "symlink",
            });
        }
        Ok(meta) if FileType::from_raw_mode(meta.st_mode) != FileType::RegularFile => {
            report.conflicts.push(Conflict {
                path: path.into(),
                reason: "type",
            });
        }
        Ok(meta) if meta.st_nlink != 1 => {
            report.conflicts.push(Conflict {
                path: path.into(),
                reason: "hard_link",
            });
        }
        Ok(meta) => {
            let Some(stamp) = prepared.files.get(path) else {
                report.conflicts.push(Conflict {
                    path: path.into(),
                    reason: "host_changed",
                });
                return Ok(());
            };
            if device(&meta) != stamp.dev || meta.st_ino != stamp.ino {
                report.conflicts.push(Conflict {
                    path: path.into(),
                    reason: "replaced",
                });
                return Ok(());
            }
            let host = openat(
                host_dir,
                name,
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|_| PythonError::failed("workspace writeback failed"))?;
            let host_file = File::from(host);
            let host_bytes = match read_limited(&host_file, MAX_FILE_BYTES) {
                Ok(bytes) => bytes,
                Err(_) => {
                    report.conflicts.push(Conflict {
                        path: path.into(),
                        reason: "host_changed",
                    });
                    return Ok(());
                }
            };
            let host_hash = sha256(&host_bytes);
            if host_hash != stamp.sha256 {
                report.conflicts.push(Conflict {
                    path: path.into(),
                    reason: "host_changed",
                });
                return Ok(());
            }
            if host_hash == snap_hash {
                return Ok(());
            }
            if !write_fits(bytes.len() as u64, report) {
                return Ok(());
            }
            match replace_same_inode(host_dir, name, &meta, &host_file, &bytes, stamp)? {
                ReplaceOutcome::Wrote => record_written(path, bytes.len() as u64, report),
                ReplaceOutcome::Conflict(reason) => {
                    report.conflicts.push(Conflict {
                        path: path.into(),
                        reason,
                    });
                }
            }
        }
    }
    Ok(())
}

fn write_fits(bytes: u64, report: &mut WriteReport) -> bool {
    let used = report.artifacts.iter().map(|item| item.bytes).sum::<u64>();
    if used.saturating_add(bytes) > MAX_WRITEBACK_BYTES {
        report.truncated = true;
        false
    } else {
        true
    }
}

enum ReplaceOutcome {
    Wrote,
    Conflict(&'static str),
}

fn replace_same_inode(
    host_dir: &OwnedFd,
    name: &str,
    meta: &rustix::fs::Stat,
    host_file: &File,
    bytes: &[u8],
    stamp: &FileStamp,
) -> Result<ReplaceOutcome, PythonError> {
    static TEMPS: AtomicU64 = AtomicU64::new(1);
    let temp = format!(
        "pywrite-{}-{}",
        std::process::id(),
        TEMPS.fetch_add(1, Ordering::Relaxed)
    );
    let flags = OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let fd = openat(host_dir, temp.as_str(), flags, Mode::from_raw_mode(0o600))
        .map_err(|_| PythonError::failed("workspace writeback failed"))?;
    let result = (|| {
        let mut file = File::from(fd);
        file.write_all(bytes)
            .map_err(|_| PythonError::failed("workspace writeback failed"))?;
        fchmod(&file, Mode::from_raw_mode(meta.st_mode & 0o777))
            .map_err(|_| PythonError::failed("workspace writeback failed"))?;
        file.sync_all().ok();
        let mut conflict = None;
        let current = statat(host_dir, name, AtFlags::SYMLINK_NOFOLLOW).ok();
        let kind = current
            .as_ref()
            .map(|meta| FileType::from_raw_mode(meta.st_mode));
        if current.is_none() {
            conflict = Some("replaced");
        } else if kind == Some(FileType::Symlink) {
            conflict = Some("symlink");
        } else if kind != Some(FileType::RegularFile) {
            conflict = Some("type");
        } else if current.as_ref().is_some_and(|meta| meta.st_nlink != 1) {
            conflict = Some("hard_link");
        } else if current
            .as_ref()
            .is_some_and(|item| item.st_dev != meta.st_dev || item.st_ino != meta.st_ino)
        {
            conflict = Some("replaced");
        }
        if conflict.is_none() {
            let mut readable = host_file;
            if readable.seek(SeekFrom::Start(0)).is_err() {
                return Err(PythonError::failed("workspace writeback failed"));
            }
            if read_fd_hash(readable).ok() != Some(stamp.sha256) {
                conflict = Some("host_changed");
            }
        }
        if let Some(reason) = conflict {
            return Ok(ReplaceOutcome::Conflict(reason));
        }
        renameat(host_dir, temp.as_str(), host_dir, name)
            .map_err(|_| PythonError::failed("workspace writeback failed"))?;
        Ok(ReplaceOutcome::Wrote)
    })();
    if !matches!(&result, Ok(ReplaceOutcome::Wrote)) {
        let _ = unlinkat(host_dir, temp.as_str(), AtFlags::empty());
    }
    result
}

fn create_exclusive(host_dir: &OwnedFd, name: &str, bytes: &[u8]) -> Result<bool, PythonError> {
    let flags = OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let fd = match openat(host_dir, name, flags, Mode::from_raw_mode(0o644)) {
        Ok(fd) => fd,
        Err(rustix::io::Errno::EXIST) => return Ok(false),
        Err(_) => return Err(PythonError::failed("workspace writeback failed")),
    };
    let mut file = File::from(fd);
    if file.write_all(bytes).is_err() {
        let _ = unlinkat(host_dir, name, AtFlags::empty());
        return Err(PythonError::failed("workspace writeback failed"));
    }
    file.sync_all().ok();
    Ok(true)
}

fn delete_if_unchanged(
    path: &str,
    name: &str,
    host_dir: &OwnedFd,
    stamp: &FileStamp,
    report: &mut WriteReport,
) -> Result<(), PythonError> {
    let meta = match statat(host_dir, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(meta) => meta,
        Err(_) => return Ok(()),
    };
    let kind = FileType::from_raw_mode(meta.st_mode);
    if kind == FileType::Symlink {
        report.conflicts.push(Conflict {
            path: path.into(),
            reason: "symlink",
        });
        return Ok(());
    }
    if kind != FileType::RegularFile {
        report.conflicts.push(Conflict {
            path: path.into(),
            reason: "type",
        });
        return Ok(());
    }
    if meta.st_nlink != 1 {
        report.conflicts.push(Conflict {
            path: path.into(),
            reason: "hard_link",
        });
        return Ok(());
    }
    if device(&meta) != stamp.dev || meta.st_ino != stamp.ino {
        report.conflicts.push(Conflict {
            path: path.into(),
            reason: "replaced",
        });
        return Ok(());
    }
    let host = match openat(
        host_dir,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    ) {
        Ok(fd) => fd,
        Err(_) => return Ok(()),
    };
    if read_fd_hash(&File::from(host)).ok() != Some(stamp.sha256) {
        report.conflicts.push(Conflict {
            path: path.into(),
            reason: "host_changed",
        });
        return Ok(());
    }
    unlinkat(host_dir, name, AtFlags::empty()).ok();
    report.changed.push(path.to_owned());
    Ok(())
}

fn record_written(path: &str, bytes: u64, report: &mut WriteReport) {
    report.changed.push(path.to_owned());
    let (kind, media_type) = if path.ends_with(".png") {
        ("chart", "image/png")
    } else {
        ("file", media_type_of(path))
    };
    report.artifacts.push(Artifact {
        path: path.to_owned(),
        kind,
        media_type,
        bytes,
    });
}

fn media_type_of(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("json") => "application/json",
        Some("csv") => "text/csv",
        Some("svg") => "image/svg+xml",
        Some("txt") | Some("md") => "text/plain",
        _ => "application/octet-stream",
    }
}

#[allow(clippy::too_many_arguments)] // Descriptor walk carries separate host, guest and policy roots.
fn copy_tree(
    host_dir: &OwnedFd,
    snap_dir: &OwnedFd,
    logical_dir: &Path,
    relative: &str,
    depth: u32,
    workspace: bool,
    guest_writable: bool,
    roots: &NativeRoots,
    budget: &mut Budget,
    prepared: &mut Prepared,
) -> Result<(), PythonError> {
    // Bound metadata memory while choosing entries independently of directory
    // enumeration order. A skipped directory represents an unscanned subtree.
    let root = if workspace {
        SnapshotRoot::Workspace
    } else if guest_writable {
        SnapshotRoot::Scratch
    } else {
        SnapshotRoot::Attachments
    };
    let remaining = MAX_TREE_ENTRIES.saturating_sub(budget.entries);
    let mut ordered = BTreeMap::new();
    let entries =
        Dir::read_from(host_dir).map_err(|_| PythonError::failed("workspace snapshot failed"))?;
    for entry in entries {
        let entry = entry.map_err(|_| PythonError::failed("workspace snapshot failed"))?;
        let Ok(name) = entry.file_name().to_str() else {
            continue;
        };
        if name == "." || name == ".." || name.starts_with('.') || name.contains(['/', '\\', '\0'])
        {
            continue;
        }
        let Ok(meta) = statat(host_dir, name, AtFlags::SYMLINK_NOFOLLOW) else {
            continue;
        };
        let kind = FileType::from_raw_mode(meta.st_mode);
        if !matches!(kind, FileType::Directory | FileType::RegularFile) {
            continue;
        }
        let logical = logical_dir.join(name);
        let class = classify(&logical, roots).class;
        let allowed = if workspace {
            class == PathClass::Workspace
        } else {
            matches!(
                class,
                PathClass::Attachments | PathClass::Scratch | PathClass::Workspace
            )
        };
        if !allowed || (kind == FileType::RegularFile && meta.st_nlink != 1) {
            continue;
        }
        let key = (
            !(workspace && depth == 0 && name == "outputs"),
            Reverse((i128::from(meta.st_mtime), i128::from(meta.st_mtime_nsec))),
            name.to_owned(),
        );
        ordered.insert(key, (name.to_owned(), meta));
        if ordered.len() > remaining {
            let (_, (name, meta)) = ordered.pop_last().expect("entry inserted");
            let directory = FileType::from_raw_mode(meta.st_mode) == FileType::Directory;
            let path = join_relative(relative, &name);
            prepared.omit(omission_path(root, &path, directory), directory);
        }
    }
    for (_, (name, meta)) in ordered {
        let kind = FileType::from_raw_mode(meta.st_mode);
        let path = join_relative(relative, &name);
        let directory = kind == FileType::Directory;
        if budget.entries >= MAX_TREE_ENTRIES || (directory && depth >= 32) {
            prepared.omit(omission_path(root, &path, directory), directory);
            continue;
        }
        budget.entries += 1;
        if directory {
            if workspace && depth == 0 && protected_dir(device(&meta), meta.st_ino, roots) {
                continue;
            }
            if workspace
                && depth == 0
                && name != "outputs"
                && prepared
                    .writable
                    .iter()
                    .filter(|name| name.as_str() != "outputs")
                    .count()
                    >= MAX_WRITABLE_DIRS - 1
            {
                prepared.omit(omission_path(root, &path, true), true);
                continue;
            }
            let host_child = openat(host_dir, name.as_str(), dir_flags(), Mode::empty())
                .map_err(|_| PythonError::failed("workspace snapshot failed"))?;
            let opened =
                fstat(&host_child).map_err(|_| PythonError::failed("workspace snapshot failed"))?;
            if opened.st_dev != meta.st_dev || opened.st_ino != meta.st_ino {
                return Err(PythonError::failed(
                    "workspace changed while creating the snapshot; try again",
                ));
            }
            mkdir_at(snap_dir, &name, Mode::from_raw_mode(0o755))?;
            let snap_child = openat(snap_dir, name.as_str(), dir_flags(), Mode::empty())
                .map_err(|_| PythonError::failed("workspace snapshot failed"))?;
            if workspace && depth == 0 {
                prepared.writable.push(name.clone());
            }
            if workspace && writable_ancestor(&path, prepared) {
                prepared.dirs.insert(
                    path.clone(),
                    DirStamp {
                        dev: device(&meta),
                        ino: meta.st_ino,
                    },
                );
            }
            copy_tree(
                &host_child,
                &snap_child,
                &logical_dir.join(&name),
                &path,
                depth + 1,
                workspace,
                guest_writable,
                roots,
                budget,
                prepared,
            )?;
        } else if meta.st_size < 0 || meta.st_size as u64 > MAX_FILE_BYTES {
            prepared.omit(omission_path(root, &path, false), false);
        } else {
            budget.candidates.push(Candidate {
                root,
                relative: path.clone(),
                identity: DirStamp {
                    dev: device(&meta),
                    ino: meta.st_ino,
                },
                length: meta.st_size as u64,
                modified: (i128::from(meta.st_mtime), i128::from(meta.st_mtime_nsec)),
                writable: guest_writable || (workspace && writable_ancestor(&path, prepared)),
            });
        }
    }
    Ok(())
}

fn omission_path(root: SnapshotRoot, path: &str, directory: bool) -> String {
    let mut path = match root {
        SnapshotRoot::Workspace => path.to_owned(),
        SnapshotRoot::Attachments => format!("/attachments/{path}"),
        SnapshotRoot::Scratch => format!("/scratch/{path}"),
    };
    if directory {
        path.push('/');
    }
    path
}

fn open_relative_dir(root: &OwnedFd, path: &Path) -> Result<OwnedFd, PythonError> {
    let mut current = openat(root, ".", dir_flags(), Mode::empty())
        .map_err(|_| PythonError::failed("workspace snapshot failed"))?;
    for component in path.components() {
        let std::path::Component::Normal(name) = component else {
            return Err(PythonError::failed("workspace snapshot path is invalid"));
        };
        current = openat(&current, name, dir_flags(), Mode::empty())
            .map_err(|_| PythonError::failed("workspace snapshot failed"))?;
    }
    Ok(current)
}

struct Copied {
    sha256: [u8; 32],
    len: u64,
}

fn copy_file(
    host_dir: &OwnedFd,
    snap_dir: &OwnedFd,
    name: &str,
    mode: Mode,
    expected: DirStamp,
    max_bytes: u64,
) -> Result<Option<Copied>, PythonError> {
    let source = openat(
        host_dir,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| PythonError::failed("workspace snapshot failed"))?;
    let meta = fstat(&source).map_err(|_| PythonError::failed("workspace snapshot failed"))?;
    if FileType::from_raw_mode(meta.st_mode) != FileType::RegularFile
        || meta.st_nlink != 1
        || device(&meta) != expected.dev
        || meta.st_ino != expected.ino
    {
        return Err(PythonError::failed("workspace snapshot failed"));
    }
    let flags = OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let dest = openat(snap_dir, name, flags, mode)
        .map_err(|_| PythonError::failed("workspace snapshot failed"))?;
    let mut input = File::from(source);
    let mut output = File::from(dest);
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut len = 0u64;
    loop {
        let count = input
            .read(&mut buffer)
            .map_err(|_| PythonError::failed("workspace snapshot failed"))?;
        if count == 0 {
            break;
        }
        len += count as u64;
        if len > max_bytes {
            drop(output);
            unlinkat(snap_dir, name, AtFlags::empty())
                .map_err(|_| PythonError::failed("workspace snapshot failed"))?;
            return Ok(None);
        }
        hasher.update(&buffer[..count]);
        output
            .write_all(&buffer[..count])
            .map_err(|_| PythonError::failed("workspace snapshot failed"))?;
    }
    Ok(Some(Copied {
        sha256: hasher.finalize().into(),
        len,
    }))
}

fn ensure_outputs(prepared: &mut Prepared) -> Result<(), PythonError> {
    if prepared.writable.iter().any(|name| name == "outputs") {
        return Ok(());
    }
    let snap = open_dir(&prepared.staging.path().join("workspace"))?;
    if statat(&snap, "outputs", AtFlags::SYMLINK_NOFOLLOW)
        .is_ok_and(|meta| FileType::from_raw_mode(meta.st_mode) != FileType::Directory)
    {
        return Err(PythonError::failed(
            "workspace outputs must be a directory; rename the outputs file before running Python",
        ));
    }
    mkdir_at(&snap, "outputs", Mode::from_raw_mode(0o755))?;
    prepared.writable.push("outputs".into());
    prepared.writable.sort();
    Ok(())
}

fn writable_ancestor(path: &str, prepared: &Prepared) -> bool {
    let top = path.split('/').next().unwrap_or("");
    prepared.writable.iter().any(|name| name == top)
}

fn protected_dir(dev: u64, ino: u64, roots: &NativeRoots) -> bool {
    [
        Some(roots.app_data.as_path()),
        Some(roots.loupist_data.as_path()),
        Some(roots.attachments.as_path()),
        Some(roots.scratch.as_path()),
        roots.home.as_deref(),
    ]
    .into_iter()
    .flatten()
    .any(|path| same_dir(path, dev, ino))
}

#[allow(clippy::unnecessary_cast)] // rustix dev_t differs between macOS and Linux.
fn device(meta: &rustix::fs::Stat) -> u64 {
    meta.st_dev as u64
}

fn same_dir(path: &Path, dev: u64, ino: u64) -> bool {
    let Ok(fd) = open_dir(path) else { return false };
    fstat(&fd).is_ok_and(|meta| device(&meta) == dev && meta.st_ino == ino)
}

fn join_relative(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_owned()
    } else {
        format!("{prefix}/{name}")
    }
}

fn open_dir(path: &Path) -> Result<OwnedFd, PythonError> {
    openat(
        rustix::fs::CWD,
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| PythonError::failed("workspace snapshot failed"))
}

fn dir_flags() -> OFlags {
    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC
}

fn mkdir(staging: &TempDir, name: &str) -> Result<OwnedFd, PythonError> {
    let root = open_dir(staging.path())?;
    mkdir_at(&root, name, Mode::from_raw_mode(0o755))?;
    openat(&root, name, dir_flags(), Mode::empty())
        .map_err(|_| PythonError::failed("workspace snapshot failed"))
}

fn mkdir_at(dir: &OwnedFd, name: &str, mode: Mode) -> Result<(), PythonError> {
    match mkdirat(dir, name, mode) {
        Ok(()) => Ok(()),
        Err(error) if error == rustix::io::Errno::EXIST => Ok(()),
        Err(_) => Err(PythonError::failed("workspace snapshot failed")),
    }
}

fn read_limited(file: &File, max: u64) -> Result<Vec<u8>, PythonError> {
    let mut reader = file;
    let mut output = Vec::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|_| PythonError::failed("workspace writeback failed"))?;
        if count == 0 {
            break;
        }
        if output.len() as u64 + count as u64 > max {
            return Err(PythonError::failed("workspace writeback failed"));
        }
        output.extend_from_slice(&buffer[..count]);
    }
    Ok(output)
}

fn read_fd_hash(file: &File) -> Result<[u8; 32], PythonError> {
    Ok(sha256(&read_limited(file, MAX_FILE_BYTES)?))
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

#[cfg(test)]
mod selection_tests {
    use super::*;

    fn candidate(root: SnapshotRoot, path: &str, modified: i128) -> Candidate {
        Candidate {
            root,
            relative: path.into(),
            identity: DirStamp { dev: 1, ino: 1 },
            length: 1,
            modified: (modified, 0),
            writable: false,
        }
    }

    #[test]
    fn outputs_priority_is_exact_and_other_entries_use_recency_then_path() {
        let mut files = vec![candidate(SnapshotRoot::Workspace, "outputs/old.txt", 0)];
        let negatives = [
            (SnapshotRoot::Workspace, "outputs-old/new.txt"),
            (SnapshotRoot::Workspace, "outputs_backup/new.txt"),
            (SnapshotRoot::Workspace, "notes/outputs/new.txt"),
            (SnapshotRoot::Workspace, "Outputs/new.txt"),
            (SnapshotRoot::Attachments, "outputs/new.txt"),
            (SnapshotRoot::Scratch, "outputs/new.txt"),
        ];
        for (index, (root, path)) in negatives.into_iter().enumerate() {
            files.push(candidate(root, path, 10 + index as i128));
        }
        files.push(candidate(SnapshotRoot::Workspace, "notes/z.txt", 1));
        files.push(candidate(SnapshotRoot::Workspace, "notes/a.txt", 1));
        files.sort_by_key(Candidate::priority);
        assert_eq!(files[0].guest_path(), "outputs/old.txt");
        assert_eq!(files[1].guest_path(), "/scratch/outputs/new.txt");
        assert_eq!(files[7].guest_path(), "notes/a.txt");
        assert_eq!(files[8].guest_path(), "notes/z.txt");
    }
}
