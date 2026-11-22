//! A recoverable, catalog-only export after startup. Errors never enter startup's result.
use std::{
    cell::Cell,
    fs::{File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex},
};

use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{Emitter, Manager, State};

use crate::{
    app::CommandError,
    fs::artifact_trust::{
        ensure_private_directory, read_private_regular_file_bounded, ArtifactTrustError,
    },
    store::{
        legacy_holdings::{LegacyHoldingsCounts, Receipt, Rows},
        SqliteStore, StoreError,
    },
};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum LegacyHoldingsState {
    None,
    Pending,
    Exported,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct LegacyHoldingsStatus {
    pub state: LegacyHoldingsState,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional, type = "number"))]
    pub exported_at_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub counts: Option<LegacyHoldingsCounts>,
    pub dismissed: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Document {
    format: String,
    exported_at: String,
    counts: LegacyHoldingsCounts,
    wallets: Vec<serde_json::Value>,
    snapshots: Vec<serde_json::Value>,
    positions: Vec<serde_json::Value>,
}
impl Document {
    fn new(rows: &Rows, time: String) -> Self {
        Self {
            format: "loupist-legacy-holdings/1".into(),
            exported_at: time,
            counts: rows.counts(),
            wallets: rows.wallets.clone(),
            snapshots: rows.snapshots.clone(),
            positions: rows.positions.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Point {
    BeforeWrite,
    DuringWrite,
    AfterSync,
    AfterRename,
    BeforeCommit,
    AfterCommit,
}

// Faults are available only to unit tests, with no environment or renderer control.
#[derive(Default)]
struct Faults {
    #[cfg(test)]
    at: Option<(Point, i32)>,
    #[cfg(test)]
    hook: Option<Arc<dyn Fn(Point) + Send + Sync>>,
}
impl Faults {
    fn check(&self, point: Point) -> io::Result<()> {
        #[cfg(test)]
        if let Some(hook) = &self.hook {
            hook(point);
        }
        #[cfg(test)]
        if let Some((at, code)) = self.at {
            if point == at {
                return Err(io::Error::from_raw_os_error(code));
            }
        }
        let _ = point;
        Ok(())
    }
}
fn internal(error: impl std::fmt::Display) -> CommandError {
    CommandError::internal(error.to_string())
}
// Diagnostics contain only fixed stage/category tags, never an error's Display text.
#[derive(Clone, Copy)]
enum Stage {
    ReadCatalog,
    Directory,
    InspectFinal,
    Serialize,
    WritePartial,
    SyncFile,
    VerifyPartial,
    Publish,
    SyncDirectory,
    VerifyPublished,
    CommitCatalog,
    AfterCommit,
}
impl Stage {
    fn tag(self) -> &'static str {
        match self {
            Self::ReadCatalog => "read_catalog",
            Self::Directory => "directory",
            Self::InspectFinal => "inspect_final",
            Self::Serialize => "serialize",
            Self::WritePartial => "write_partial",
            Self::SyncFile => "sync_file",
            Self::VerifyPartial => "verify_partial",
            Self::Publish => "publish",
            Self::SyncDirectory => "sync_directory",
            Self::VerifyPublished => "verify_published",
            Self::CommitCatalog => "commit_catalog",
            Self::AfterCommit => "after_commit",
        }
    }
}
#[derive(Debug, thiserror::Error)]
enum ExportError {
    #[error("export I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("export private storage failed: {0}")]
    Private(#[from] ArtifactTrustError),
    #[error("export catalog access failed: {0}")]
    Catalog(#[from] StoreError),
    #[error("export JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("export time is invalid: {0}")]
    Timestamp(#[from] chrono::ParseError),
    #[error("export verification failed")]
    Verification,
    #[error("No free legacy export filename")]
    NameExhausted,
    #[error("export panicked")]
    Panic,
}
impl From<ExportError> for CommandError {
    fn from(error: ExportError) -> Self {
        internal(error)
    }
}
fn failure_message(stage: Stage, error: &ExportError) -> String {
    let kind = match error {
        ExportError::Io(error) | ExportError::Private(ArtifactTrustError::Io(error)) => {
            format!("io:{:?}", error.kind())
        }
        ExportError::Private(_) => "private_storage".into(),
        ExportError::Catalog(_) => "catalog".into(),
        ExportError::Json(_) => "serialization".into(),
        ExportError::Timestamp(_) => "timestamp".into(),
        ExportError::Verification => "verification".into(),
        ExportError::NameExhausted => "name_exhausted".into(),
        ExportError::Panic => "panic".into(),
    };
    format!(
        "Legacy holdings export failed: stage={} kind={kind}",
        stage.tag()
    )
}

fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn private_metadata(path: &Path, directory: bool) -> io::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink()
        || (if directory {
            !metadata.is_dir()
        } else {
            !metadata.is_file()
        })
    {
        return Err(io::Error::other(
            "export path is not a private regular file or directory",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.uid() != unsafe { libc::geteuid() }
            || metadata.permissions().mode() & 0o777 != if directory { 0o700 } else { 0o600 }
            || (!directory && metadata.nlink() != 1)
        {
            return Err(io::Error::other("export path is not private"));
        }
    }
    Ok(())
}
fn directory(root: &Path) -> Result<PathBuf, ExportError> {
    directory_with_sync(root, sync_directory)
}
fn directory_with_sync(
    root: &Path,
    sync: impl FnOnce(&Path) -> Result<(), ExportError>,
) -> Result<PathBuf, ExportError> {
    let path = root.join("legacy-holdings");
    match std::fs::symlink_metadata(&path) {
        Ok(_) => private_metadata(&path, true)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => ensure_private_directory(&path)?,
        Err(error) => return Err(error.into()),
    }
    private_metadata(&path, true)?;
    // Also sync on re-admission, so a failed first parent sync is retried.
    sync(root)?;
    Ok(path)
}
fn file_name(index: usize) -> String {
    if index == 1 {
        "holdings-export.json".into()
    } else {
        format!("holdings-export-{index}.json")
    }
}
fn available_partial(directory: &Path) -> Result<PathBuf, ExportError> {
    for index in 1..=10000 {
        let path = directory.join(format!("{}.partial", file_name(index)));
        match std::fs::symlink_metadata(&path) {
            Ok(_) => {
                // Only an owner-only, singly linked regular partial is ours to
                // rewrite. Preserve every foreign leaf without following it.
                if private_metadata(&path, false).is_err() {
                    continue;
                }
                std::fs::remove_file(&path)?;
                return Ok(path);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(path),
            Err(error) => return Err(error.into()),
        }
    }
    Err(ExportError::NameExhausted)
}
// Use a single exclusive rename: a crash cannot leave a second hard link at the
// partial name, and no existing leaf (even a newly arrived one) is overwritten.
fn publish(partial: &Path, target: &Path) -> io::Result<()> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        use std::os::unix::ffi::OsStrExt;
        let source = std::ffi::CString::new(partial.as_os_str().as_bytes())?;
        let destination = std::ffi::CString::new(target.as_os_str().as_bytes())?;
        #[cfg(target_os = "macos")]
        let result =
            unsafe { libc::renamex_np(source.as_ptr(), destination.as_ptr(), libc::RENAME_EXCL) };
        #[cfg(target_os = "linux")]
        let result = unsafe {
            libc::renameat2(
                libc::AT_FDCWD,
                source.as_ptr(),
                libc::AT_FDCWD,
                destination.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        if result != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
    #[cfg(windows)]
    {
        std::fs::rename(partial, target)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    {
        let _ = (partial, target);
        Err(io::Error::other("exclusive export rename is unavailable"))
    }
}
fn sync_directory(path: &Path) -> Result<(), ExportError> {
    #[cfg(unix)]
    File::open(path).and_then(|file| file.sync_all())?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
fn verified(
    path: &Path,
    expected: &[u8],
    counts: &LegacyHoldingsCounts,
) -> Result<(), ExportError> {
    private_metadata(path, false)?;
    let bytes = read_private_regular_file_bounded(path, expected.len() as u64)?;
    let document: Document = serde_json::from_slice(&bytes)?;
    let actual = LegacyHoldingsCounts {
        wallets: document.wallets.len(),
        snapshots: document.snapshots.len(),
        positions: document.positions.len(),
    };
    if hash(&bytes) != hash(expected) || document.counts != *counts || actual != *counts {
        return Err(ExportError::Verification);
    }
    Ok(())
}

fn export(
    store: &SqliteStore,
    root: &Path,
    faults: &Faults,
    stage: &Cell<Stage>,
) -> Result<(), ExportError> {
    if store.legacy_holdings_receipt()?.is_some() {
        return Ok(());
    }
    let rows = store.legacy_holdings_rows()?;
    if !rows.tables_exist {
        return Ok(());
    }
    stage.set(Stage::Directory);
    let directory = directory(root)?;
    let time = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    for index in 1..=10000 {
        let name = file_name(index);
        let path = directory.join(&name);
        let mut document = Document::new(&rows, time.clone());
        stage.set(Stage::InspectFinal);
        match std::fs::symlink_metadata(&path) {
            Ok(_) => {
                // A crash can leave the completed file without its receipt. Preserve its
                // timestamp only if every byte matches a serialization of the current rows.
                if private_metadata(&path, false).is_err() {
                    continue;
                }
                let bound = serde_json::to_vec(&document)?.len() as u64 + 64;
                let Ok(bytes) = read_private_regular_file_bounded(&path, bound) else {
                    continue;
                };
                let Ok(existing) = serde_json::from_slice::<Document>(&bytes) else {
                    continue;
                };
                if chrono::DateTime::parse_from_rfc3339(&existing.exported_at).is_err() {
                    continue;
                }
                document.exported_at = existing.exported_at;
                stage.set(Stage::Serialize);
                let expected = serde_json::to_vec(&document)?;
                if verified(&path, &expected, &rows.counts()).is_err() {
                    continue;
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                stage.set(Stage::WritePartial);
                faults.check(Point::BeforeWrite)?;
                let partial = available_partial(&directory)?;
                let mut options = OpenOptions::new();
                options.write(true).read(true).create_new(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options
                        .mode(0o600)
                        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
                }
                let mut file = options.open(&partial)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
                }
                stage.set(Stage::Serialize);
                let expected = serde_json::to_vec(&document)?;
                stage.set(Stage::WritePartial);
                let halfway = expected.len() / 2;
                file.write_all(&expected[..halfway])?;
                faults.check(Point::DuringWrite)?;
                file.write_all(&expected[halfway..])?;
                stage.set(Stage::SyncFile);
                file.flush()?;
                file.sync_all()?;
                faults.check(Point::AfterSync)?;
                stage.set(Stage::VerifyPartial);
                verified(&partial, &expected, &rows.counts())?;
                drop(file);
                stage.set(Stage::Publish);
                publish(&partial, &path)?;
            }
            Err(error) => return Err(error.into()),
        }
        stage.set(Stage::SyncDirectory);
        sync_directory(&directory)?;
        stage.set(Stage::VerifyPublished);
        faults.check(Point::AfterRename)?;
        let bytes = serde_json::to_vec(&document)?;
        // Re-verify the synced, published bytes to detect publication/I/O corruption.
        // A same-user process can still rewrite the file before or after commit;
        // keep file I/O outside the catalog transaction.
        verified(&path, &bytes, &rows.counts())?;
        let receipt = Receipt {
            state: "exported".into(),
            file_name: name,
            sha256: hash(&bytes),
            counts: rows.counts(),
            exported_at_ms: chrono::DateTime::parse_from_rfc3339(&document.exported_at)?
                .timestamp_millis(),
            dismissed: false,
        };
        stage.set(Stage::CommitCatalog);
        store.legacy_holdings_commit(&rows, &receipt, |_| {
            faults
                .check(Point::BeforeCommit)
                .map_err(|_| StoreError::Invalid("export interrupted before commit".into()))
        })?;
        stage.set(Stage::AfterCommit);
        faults.check(Point::AfterCommit)?;
        return Ok(());
    }
    Err(ExportError::NameExhausted)
}

#[derive(Default)]
struct Process {
    #[cfg(test)]
    joined: usize,
    running: bool,
    failed: bool,
    dismissed: bool,
}
pub(crate) struct Runtime {
    store: Arc<SqliteStore>,
    root: PathBuf,
    process: Mutex<Process>,
    finished: Condvar,
    changed: Box<dyn Fn(LegacyHoldingsStatus) + Send + Sync>,
}
/// Releases the in-flight slot even if file work unwinds. Construct inside the
/// catch boundary so Drop runs before a panic becomes a failed command result.
struct RunGuard<'a> {
    runtime: &'a Runtime,
    before: LegacyHoldingsStatus,
    armed: bool,
    stage: &'a Cell<Stage>,
}
impl RunGuard<'_> {
    fn finish(&mut self, failed: bool) -> Result<LegacyHoldingsStatus, CommandError> {
        let mut process = self.runtime.process.lock().map_err(internal)?;
        process.running = false;
        process.failed = failed;
        self.armed = false;
        self.runtime.finished.notify_all();
        let status = self.runtime.status_locked(&process);
        drop(process);
        if let Ok(status) = &status {
            if *status != self.before {
                (self.runtime.changed)(status.clone());
            }
        }
        status
    }
}
impl Drop for RunGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            // Event delivery must not cause a second panic while already unwinding.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let status = self.finish(true);
                eprintln!("{}", failure_message(self.stage.get(), &ExportError::Panic));
                status
            }));
        }
    }
}
impl Runtime {
    fn status_locked(&self, process: &Process) -> Result<LegacyHoldingsStatus, CommandError> {
        if let Some(receipt) = self.store.legacy_holdings_receipt().map_err(internal)? {
            return Ok(LegacyHoldingsStatus {
                state: LegacyHoldingsState::Exported,
                exported_at_ms: Some(receipt.exported_at_ms),
                counts: Some(receipt.counts),
                dismissed: receipt.dismissed,
            });
        }
        let state = if self
            .store
            .legacy_holdings_tables_exist()
            .map_err(internal)?
        {
            if process.failed {
                LegacyHoldingsState::Failed
            } else {
                LegacyHoldingsState::Pending
            }
        } else {
            LegacyHoldingsState::None
        };
        Ok(LegacyHoldingsStatus {
            state,
            exported_at_ms: None,
            counts: None,
            dismissed: process.dismissed,
        })
    }
    fn status(&self) -> Result<LegacyHoldingsStatus, CommandError> {
        let process = self.process.lock().map_err(internal)?;
        self.status_locked(&process)
    }
    fn run(&self, faults: &Faults) -> Result<LegacyHoldingsStatus, CommandError> {
        let mut process = self.process.lock().map_err(internal)?;
        if process.running {
            #[cfg(test)]
            {
                process.joined += 1;
                self.finished.notify_all();
            }
            while process.running {
                process = self.finished.wait(process).map_err(internal)?;
            }
            return self.status_locked(&process);
        }
        let before = self.status_locked(&process)?;
        if matches!(
            before.state,
            LegacyHoldingsState::None | LegacyHoldingsState::Exported
        ) {
            return Ok(before);
        }
        process.running = true;
        drop(process);
        let stage = Cell::new(Stage::ReadCatalog);
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut guard = RunGuard {
                runtime: self,
                before,
                armed: true,
                stage: &stage,
            };
            let result = export(&self.store, &self.root, faults, &stage);
            if let Err(error) = &result {
                eprintln!("{}", failure_message(stage.get(), error));
            }
            guard.finish(result.is_err())
        }))
        .unwrap_or_else(|_| self.status())
    }
    fn dismiss(&self) -> Result<LegacyHoldingsStatus, CommandError> {
        let mut process = self.process.lock().map_err(internal)?;
        let before = self.status_locked(&process)?;
        if before.state == LegacyHoldingsState::Exported {
            self.store.legacy_holdings_dismiss().map_err(internal)?;
        } else if before.state == LegacyHoldingsState::Failed {
            process.dismissed = true;
        }
        let status = self.status_locked(&process)?;
        if status != before {
            (self.changed)(status.clone());
        }
        Ok(status)
    }
    fn exported_file(&self) -> Result<(PathBuf, Vec<u8>), CommandError> {
        let receipt = self
            .store
            .legacy_holdings_receipt()
            .map_err(internal)?
            .ok_or_else(|| CommandError::invalid("Legacy holdings have not been exported"))?;
        if !(1..=10000).any(|index| receipt.file_name == file_name(index)) {
            return Err(CommandError::internal("Invalid export receipt"));
        }
        let path = directory(&self.root)?.join(receipt.file_name);
        private_metadata(&path, false).map_err(internal)?;
        let size = std::fs::symlink_metadata(&path).map_err(internal)?.len();
        let bytes = read_private_regular_file_bounded(&path, size).map_err(internal)?;
        if hash(&bytes) != receipt.sha256 {
            return Err(CommandError::internal(
                "Export file no longer matches its receipt",
            ));
        }
        verified(&path, &bytes, &receipt.counts)?;
        Ok((path, bytes))
    }
}

/// Called at the end of setup, after AppState and the window exist. Never awaited.
pub(crate) fn start(app: &tauri::AppHandle) {
    let state = app.state::<crate::app::AppState>();
    let emitter = app.clone();
    let runtime = Arc::new(Runtime {
        store: state.store.clone(),
        root: state.artifacts.app_data_dir.clone(),
        process: Mutex::default(),
        finished: Condvar::new(),
        changed: Box::new(move |status| {
            let _ = emitter.emit(crate::events::LEGACY_HOLDINGS_CHANGED, status);
        }),
    });
    app.manage(runtime.clone());
    tauri::async_runtime::spawn_blocking(move || {
        let _ = runtime.run(&Faults::default());
    });
}

#[tauri::command]
pub async fn legacy_holdings_status(
    runtime: State<'_, Arc<Runtime>>,
) -> Result<LegacyHoldingsStatus, CommandError> {
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move || runtime.status())
        .await
        .map_err(internal)?
}
#[tauri::command]
pub async fn legacy_holdings_retry(
    runtime: State<'_, Arc<Runtime>>,
) -> Result<LegacyHoldingsStatus, CommandError> {
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move || runtime.run(&Faults::default()))
        .await
        .map_err(internal)?
}
#[tauri::command]
pub async fn legacy_holdings_dismiss(
    runtime: State<'_, Arc<Runtime>>,
) -> Result<LegacyHoldingsStatus, CommandError> {
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move || runtime.dismiss())
        .await
        .map_err(internal)?
}
#[tauri::command]
pub async fn legacy_holdings_reveal(runtime: State<'_, Arc<Runtime>>) -> Result<(), CommandError> {
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (target, _) = runtime.exported_file()?;
        #[cfg(target_os = "macos")]
        crate::process_lease::macos_commands::spawn(
            std::process::Command::new("/usr/bin/open")
                .arg("-R")
                .arg(&target),
        )
        .map_err(internal)?;
        #[cfg(windows)]
        std::process::Command::new("explorer.exe")
            .arg(format!("/select,{}", target.display()))
            .spawn()
            .map_err(internal)?;
        #[cfg(not(any(target_os = "macos", windows)))]
        open::that_detached(target.parent().unwrap_or(&target)).map_err(internal)?;
        Ok(())
    })
    .await
    .map_err(internal)?
}
#[tauri::command]
pub async fn legacy_holdings_save_copy(
    runtime: State<'_, Arc<Runtime>>,
) -> Result<bool, CommandError> {
    let runtime = runtime.inner().clone();
    let (_, bytes) = tauri::async_runtime::spawn_blocking(move || runtime.exported_file())
        .await
        .map_err(internal)??;
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_file_name("holdings-export.json")
        .add_filter("JSON", &["json"])
        .save_file()
        .await
    else {
        return Ok(false);
    };
    let path = file.path().to_path_buf();
    tauri::async_runtime::spawn_blocking(move || save_copy(&path, &bytes))
        .await
        .map_err(internal)??;
    Ok(true)
}
fn save_copy(path: &Path, bytes: &[u8]) -> Result<(), CommandError> {
    if !path.is_absolute() {
        return Err(CommandError::invalid(
            "The save panel must select an absolute path",
        ));
    }
    // A temporary sibling and a rename keep both symlinks and hard links untouched.
    let mut file = tempfile::NamedTempFile::new_in(
        path.parent()
            .ok_or_else(|| CommandError::invalid("Missing save directory"))?,
    )
    .map_err(internal)?;
    file.write_all(bytes).map_err(internal)?;
    file.flush().map_err(internal)?;
    file.as_file().sync_all().map_err(internal)?;
    file.persist(path).map_err(|error| internal(error.error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::legacy_holdings::tests::fixture;
    use serde_json::json;

    fn runtime(
        store: Arc<SqliteStore>,
        root: &Path,
    ) -> (Runtime, Arc<Mutex<Vec<LegacyHoldingsStatus>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = events.clone();
        (
            Runtime {
                store,
                root: root.into(),
                process: Mutex::default(),
                finished: Condvar::new(),
                changed: Box::new(move |status| sink.lock().unwrap().push(status)),
            },
            events,
        )
    }
    fn usable(runtime: &Runtime) -> Receipt {
        let receipt = runtime.store.legacy_holdings_receipt().unwrap().unwrap();
        assert!(!runtime.store.legacy_holdings_tables_exist().unwrap());
        let (_, bytes) = runtime.exported_file().unwrap();
        assert_eq!(hash(&bytes), receipt.sha256);
        receipt
    }
    fn files(root: &Path) -> Vec<String> {
        let mut files = std::fs::read_dir(root.join("legacy-holdings"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        files.sort();
        files
    }

    #[test]
    fn baseline_and_store7_export_every_field_independently_and_restart_is_noop() {
        for version7 in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let store = Arc::new(fixture(root.path(), version7));
            let (runtime, events) = runtime(store, root.path());
            assert_eq!(
                runtime.status().unwrap().state,
                LegacyHoldingsState::Pending
            );
            assert_eq!(
                runtime.run(&Faults::default()).unwrap().state,
                LegacyHoldingsState::Exported
            );
            let receipt = usable(&runtime);
            let (_, bytes) = runtime.exported_file().unwrap();
            let doc: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(doc.as_object().unwrap().len(), 6);
            assert_eq!(doc["format"], "loupist-legacy-holdings/1");
            assert_eq!(
                doc["wallets"][0],
                json!({"id":"w1","family":"evm","address":"0xabc","name":"Main","added_at_ms":1,"last_read_at_ms":2,"last_good_read_at_ms":3,"status":"ok","unlisted_count":4})
            );
            assert_eq!(
                doc["snapshots"][0],
                json!({"snapshot_id":"s1","source":"wallet","wallet_id":"w1","imported_at_ms":5,"currency":"USD","note":null})
            );
            assert_eq!(
                doc["snapshots"][1],
                json!({"snapshot_id":"s2","source":"csv","wallet_id":null,"imported_at_ms":6,"currency":"KRW","note":"broker"})
            );
            let ether = json!({"snapshot_id":"s1","symbol":"ETH","market":"CRYPTO","display_name":"Ether","quantity":"2.0000","cost_basis":null,"market_value":"123.45","currency":"USD"});
            if version7 {
                assert_eq!(
                    doc["wallets"][1],
                    json!({"id":"w2","family":"solana","address":"unread","name":null,"added_at_ms":8,"last_read_at_ms":null,"last_good_read_at_ms":null,"status":"unread","unlisted_count":0})
                );
                assert_eq!(
                    doc["wallets"][2],
                    json!({"id":"w3","family":"evm","address":"0xdef","name":"Other","added_at_ms":9,"last_read_at_ms":10,"last_good_read_at_ms":null,"status":"failed","unlisted_count":2})
                );
                assert_eq!(
                    doc["snapshots"][2],
                    json!({"snapshot_id":"s3","source":"kis","wallet_id":null,"imported_at_ms":10,"currency":"EUR","note":null})
                );
                assert_eq!(
                    doc["snapshots"][3],
                    json!({"snapshot_id":"s4","source":"crypto_file","wallet_id":null,"imported_at_ms":11,"currency":"USD","note":"exchange"})
                );
                assert_eq!(
                    doc["positions"],
                    json!([
                        {"snapshot_id":"s1","symbol":"BTC","market":"CRYPTO","display_name":null,"quantity":"3","cost_basis":null,"market_value":null,"currency":"USD"}, ether,
                        {"snapshot_id":"s2","symbol":"ABC","market":"KRX","display_name":null,"quantity":"0.00001","cost_basis":"001.20","market_value":null,"currency":"KRW"}
                    ])
                );
                assert_eq!(
                    doc["counts"],
                    json!({"wallets":3,"snapshots":4,"positions":3})
                );
            } else {
                assert_eq!(doc["wallets"].as_array().unwrap().len(), 1);
                assert_eq!(doc["snapshots"].as_array().unwrap().len(), 2);
                assert_eq!(doc["positions"], json!([ether]));
                assert_eq!(
                    doc["counts"],
                    json!({"wallets":1,"snapshots":2,"positions":1})
                );
            }
            let status = runtime.dismiss().unwrap();
            assert!(status.dismissed);
            assert_eq!(events.lock().unwrap().len(), 2);
            let before =
                std::fs::metadata(root.path().join("legacy-holdings").join(&receipt.file_name))
                    .unwrap()
                    .modified()
                    .unwrap();
            drop(runtime);
            let reopened =
                Arc::new(SqliteStore::open(root.path().join("catalog.sqlite3")).unwrap());
            let (runtime, events) = self::runtime(reopened, root.path());
            assert_eq!(runtime.run(&Faults::default()).unwrap(), status);
            assert_eq!(files(root.path()), ["holdings-export.json"]);
            assert_eq!(
                before,
                std::fs::metadata(root.path().join("legacy-holdings").join(receipt.file_name))
                    .unwrap()
                    .modified()
                    .unwrap()
            );
            assert!(events.lock().unwrap().is_empty());
        }
    }

    #[test]
    fn interruptions_and_full_disk_preserve_rows_or_a_verified_receipt_and_recover() {
        for point in [
            Point::BeforeWrite,
            Point::DuringWrite,
            Point::AfterSync,
            Point::AfterRename,
            Point::BeforeCommit,
            Point::AfterCommit,
        ] {
            for code in [libc::EIO, libc::ENOSPC] {
                let root = tempfile::tempdir().unwrap();
                let store = Arc::new(fixture(root.path(), false));
                let (runtime, events) = runtime(store, root.path());
                let status = runtime
                    .run(&Faults {
                        at: Some((point, code)),
                        ..Faults::default()
                    })
                    .unwrap();
                if point == Point::AfterCommit {
                    assert_eq!(status.state, LegacyHoldingsState::Exported);
                    usable(&runtime);
                } else {
                    assert_eq!(status.state, LegacyHoldingsState::Failed);
                    assert!(runtime.store.legacy_holdings_tables_exist().unwrap());
                    assert!(runtime.store.legacy_holdings_receipt().unwrap().is_none());
                }
                assert_eq!(events.lock().unwrap().len(), 1);
                // A new process has no remembered failed attempt or dismissal.
                let store = runtime.store.clone();
                drop(runtime);
                let (runtime, _) = self::runtime(store, root.path());
                assert_eq!(
                    runtime.run(&Faults::default()).unwrap().state,
                    LegacyHoldingsState::Exported
                );
                usable(&runtime);
                assert_eq!(files(root.path()), ["holdings-export.json"]);
            }
        }
    }

    #[test]
    fn unverified_final_files_are_preserved_and_numbering_is_stable() {
        for kind in ["foreign", "symlink", "directory", "different"] {
            let root = tempfile::tempdir().unwrap();
            let store = Arc::new(fixture(root.path(), false));
            let (runtime, _) = runtime(store, root.path());
            let dir = directory(root.path()).unwrap();
            let path = dir.join("holdings-export.json");
            match kind {
                "foreign" => std::fs::write(&path, b"foreign").unwrap(),
                "symlink" => {
                    #[cfg(unix)]
                    std::os::unix::fs::symlink(root.path().join("absent"), &path).unwrap();
                }
                "directory" => std::fs::create_dir(&path).unwrap(),
                "different" => {
                    assert_eq!(
                        runtime
                            .run(&Faults {
                                at: Some((Point::AfterRename, libc::EIO)),
                                ..Faults::default()
                            })
                            .unwrap()
                            .state,
                        LegacyHoldingsState::Failed
                    );
                    runtime
                        .store
                        .lock()
                        .unwrap()
                        .execute("UPDATE retired_wallet_addresses SET name='changed'", [])
                        .unwrap();
                }
                _ => unreachable!(),
            }
            // Force the allocator past more than one occupied name.
            std::fs::write(dir.join("holdings-export-2.json"), b"also foreign").unwrap();
            let before = std::fs::symlink_metadata(&path).unwrap();
            assert_eq!(
                runtime.run(&Faults::default()).unwrap().state,
                LegacyHoldingsState::Exported
            );
            assert_eq!(usable(&runtime).file_name, "holdings-export-3.json");
            assert_eq!(
                std::fs::symlink_metadata(&path).unwrap().file_type(),
                before.file_type()
            );
            if kind == "foreign" {
                assert_eq!(std::fs::read(&path).unwrap(), b"foreign");
            }
            if kind == "different" {
                let doc: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
                assert_eq!(doc["wallets"][0]["name"], "Main");
                let (_, bytes) = runtime.exported_file().unwrap();
                let doc: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(doc["wallets"][0]["name"], "changed");
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn unwritable_or_foreign_directory_fails_closed_then_recovers() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        for kind in ["permissions", "symlink", "file"] {
            let root = tempfile::tempdir().unwrap();
            let store = Arc::new(fixture(root.path(), false));
            let (runtime, _) = runtime(store, root.path());
            let dir = root.path().join("legacy-holdings");
            match kind {
                "permissions" => {
                    ensure_private_directory(&dir).unwrap();
                    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).unwrap();
                }
                "symlink" => symlink(root.path(), &dir).unwrap(),
                "file" => std::fs::write(&dir, b"foreign").unwrap(),
                _ => unreachable!(),
            }
            assert_eq!(
                runtime.run(&Faults::default()).unwrap().state,
                LegacyHoldingsState::Failed
            );
            assert!(runtime.store.legacy_holdings_tables_exist().unwrap());
            assert!(runtime.store.legacy_holdings_receipt().unwrap().is_none());
            if kind == "permissions" {
                std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
            } else {
                std::fs::remove_file(&dir).unwrap();
            }
            assert_eq!(
                runtime.run(&Faults::default()).unwrap().state,
                LegacyHoldingsState::Exported
            );
            usable(&runtime);
        }
    }

    #[test]
    fn none_and_failed_status_dismissal_events_and_command_guards() {
        let root = tempfile::tempdir().unwrap();
        let (empty, events) = runtime(
            Arc::new(SqliteStore::open_in_memory().unwrap()),
            root.path(),
        );
        assert_eq!(
            empty.run(&Faults::default()).unwrap().state,
            LegacyHoldingsState::None
        );
        assert!(!root.path().join("legacy-holdings").exists());
        assert!(empty.exported_file().is_err());
        assert!(events.lock().unwrap().is_empty());
        let (runtime, events) = runtime(Arc::new(fixture(root.path(), false)), root.path());
        assert!(runtime.exported_file().is_err());
        runtime
            .run(&Faults {
                at: Some((Point::BeforeWrite, libc::ENOSPC)),
                ..Faults::default()
            })
            .unwrap();
        assert!(runtime.exported_file().is_err());
        assert!(runtime.dismiss().unwrap().dismissed);
        let store = runtime.store.clone();
        let root_path = runtime.root.clone();
        drop(runtime);
        let (runtime, _) = self::runtime(store, &root_path);
        let status = runtime.status().unwrap();
        assert_eq!(status.state, LegacyHoldingsState::Pending);
        assert!(!status.dismissed);
        assert_eq!(
            runtime
                .run(&Faults {
                    at: Some((Point::BeforeWrite, libc::ENOSPC)),
                    ..Faults::default()
                })
                .unwrap(),
            LegacyHoldingsStatus {
                state: LegacyHoldingsState::Failed,
                dismissed: false,
                counts: None,
                exported_at_ms: None
            }
        );
        assert_eq!(
            events
                .lock()
                .unwrap()
                .iter()
                .map(|s| (s.state.clone(), s.dismissed))
                .collect::<Vec<_>>(),
            [
                (LegacyHoldingsState::Failed, false),
                (LegacyHoldingsState::Failed, true)
            ]
        );
        runtime.run(&Faults::default()).unwrap();
        let (_, bytes) = runtime.exported_file().unwrap();
        let destination = root.path().join("chosen.json");
        save_copy(&destination, &bytes).unwrap();
        assert_eq!(std::fs::read(&destination).unwrap(), bytes);
        assert!(save_copy(Path::new("relative.json"), &bytes).is_err());
    }

    #[test]
    fn failed_export_is_detached_from_initialized_state_and_watchlist() {
        let root = tempfile::tempdir().unwrap();
        drop(fixture(root.path(), false));
        let state = crate::app::AppState::for_folder_test(root.path().to_path_buf());
        let (runtime, _) = runtime(state.store.clone(), &state.artifacts.app_data_dir);
        let worker = std::thread::spawn(move || {
            runtime
                .run(&Faults {
                    at: Some((Point::BeforeWrite, libc::ENOSPC)),
                    ..Faults::default()
                })
                .unwrap()
        });
        assert!(!state.store.assets_list("", false).unwrap().is_empty());
        assert_eq!(worker.join().unwrap().state, LegacyHoldingsState::Failed);
        assert!(!state.store.assets_list("", false).unwrap().is_empty());
    }
    #[test]
    fn overlapping_retries_join_one_run() {
        let root = tempfile::tempdir().unwrap();
        let (runtime, events) = runtime(Arc::new(fixture(root.path(), false)), root.path());
        let runtime = Arc::new(runtime);
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let (entered, ready) = std::sync::mpsc::channel();
        let gate = barrier.clone();
        let worker = runtime.clone();
        let first = std::thread::spawn(move || {
            worker
                .run(&Faults {
                    hook: Some(Arc::new(move |point| {
                        if point == Point::BeforeWrite {
                            entered.send(()).unwrap();
                            gate.wait();
                        }
                    })),
                    ..Faults::default()
                })
                .unwrap()
        });
        ready.recv().unwrap();
        let worker = runtime.clone();
        let second = std::thread::spawn(move || worker.run(&Faults::default()).unwrap());
        let mut process = runtime.process.lock().unwrap();
        while process.joined == 0 {
            process = runtime.finished.wait(process).unwrap();
        }
        drop(process);
        barrier.wait();
        assert_eq!(first.join().unwrap(), second.join().unwrap());
        usable(&runtime);
        assert_eq!(files(root.path()), ["holdings-export.json"]);
        assert_eq!(events.lock().unwrap().len(), 1);
    }
    #[test]
    fn corrupted_readback_never_authorizes_deletion_and_publish_never_clobbers() {
        let root = tempfile::tempdir().unwrap();
        let (runtime, _) = runtime(Arc::new(fixture(root.path(), false)), root.path());
        let partial = root
            .path()
            .join("legacy-holdings/holdings-export.json.partial");
        let status = runtime
            .run(&Faults {
                hook: Some(Arc::new(move |point| {
                    if point == Point::AfterSync {
                        std::fs::write(&partial, b"corrupt").unwrap();
                    }
                })),
                ..Faults::default()
            })
            .unwrap();
        assert_eq!(status.state, LegacyHoldingsState::Failed);
        assert!(runtime.store.legacy_holdings_tables_exist().unwrap());
        assert!(runtime.store.legacy_holdings_receipt().unwrap().is_none());
        runtime.run(&Faults::default()).unwrap();
        usable(&runtime);
        let source = root.path().join("source");
        let foreign = root.path().join("foreign");
        std::fs::write(&source, b"ours").unwrap();
        std::fs::write(&foreign, b"foreign").unwrap();
        assert!(publish(&source, &foreign).is_err());
        assert_eq!(std::fs::read(source).unwrap(), b"ours");
        assert_eq!(std::fs::read(foreign).unwrap(), b"foreign");
    }
    #[test]
    fn hardening_panic_wakes_joiners_and_allows_retry() {
        for panic_at in [Point::BeforeWrite, Point::BeforeCommit] {
            let root = tempfile::tempdir().unwrap();
            let (runtime, events) = runtime(Arc::new(fixture(root.path(), false)), root.path());
            let runtime = Arc::new(runtime);
            let barrier = Arc::new(std::sync::Barrier::new(2));
            let gate = barrier.clone();
            let (entered, ready) = std::sync::mpsc::channel();
            let worker = runtime.clone();
            let first = std::thread::spawn(move || {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    worker.run(&Faults {
                        hook: Some(Arc::new(move |point| {
                            if point == panic_at {
                                entered.send(()).unwrap();
                                gate.wait();
                                panic!("test export panic");
                            }
                        })),
                        ..Faults::default()
                    })
                }))
            });
            ready
                .recv_timeout(std::time::Duration::from_secs(10))
                .unwrap();
            let (done, completed) = std::sync::mpsc::channel();
            let worker = runtime.clone();
            let waiter =
                std::thread::spawn(move || done.send(worker.run(&Faults::default())).unwrap());
            let mut process = runtime.process.lock().unwrap();
            while process.joined == 0 {
                let waited = runtime
                    .finished
                    .wait_timeout(process, std::time::Duration::from_secs(10))
                    .unwrap();
                process = waited.0;
                assert!(!waited.1.timed_out());
            }
            drop(process);
            barrier.wait();
            let first = first.join().unwrap();
            let running = runtime.process.lock().unwrap().running;
            assert!(!running, "an unwinding export must release its run");
            assert_eq!(first.unwrap().unwrap().state, LegacyHoldingsState::Failed);
            assert_eq!(
                completed
                    .recv_timeout(std::time::Duration::from_secs(10))
                    .unwrap()
                    .unwrap()
                    .state,
                LegacyHoldingsState::Failed
            );
            waiter.join().unwrap();
            assert_eq!(
                events.lock().unwrap().last().unwrap().state,
                LegacyHoldingsState::Failed
            );
            assert_eq!(
                runtime.run(&Faults::default()).unwrap().state,
                LegacyHoldingsState::Exported
            );
            usable(&runtime);
        }
    }
    #[test]
    fn hardening_extra_columns_are_refused_before_write_and_before_commit() {
        for table in [
            "retired_wallet_addresses",
            "retired_holdings_snapshots",
            "retired_holdings_positions",
        ] {
            for generated in [false, true] {
                for after_publish in [false, true] {
                    let root = tempfile::tempdir().unwrap();
                    let store = Arc::new(fixture(root.path(), false));
                    let (runtime, _) = runtime(store.clone(), root.path());
                    let declaration = if generated {
                        "TEXT GENERATED ALWAYS AS ('unexported') VIRTUAL"
                    } else {
                        "TEXT DEFAULT 'unexported'"
                    };
                    let sql = format!("ALTER TABLE {table} ADD COLUMN extra {declaration}");
                    let faults = if after_publish {
                        Faults {
                            hook: Some(Arc::new(move |point| {
                                if point == Point::AfterRename {
                                    store.lock().unwrap().execute_batch(&sql).unwrap();
                                }
                            })),
                            ..Faults::default()
                        }
                    } else {
                        store.lock().unwrap().execute_batch(&sql).unwrap();
                        Faults::default()
                    };
                    assert_eq!(
                        runtime.run(&faults).unwrap().state,
                        LegacyHoldingsState::Failed
                    );
                    assert!(runtime.store.legacy_holdings_tables_exist().unwrap());
                    assert!(runtime.store.legacy_holdings_receipt().unwrap().is_none());
                    let values: Vec<String> = runtime
                        .store
                        .lock()
                        .unwrap()
                        .prepare(&format!("SELECT extra FROM {table}"))
                        .unwrap()
                        .query_map([], |row| row.get(0))
                        .unwrap()
                        .collect::<Result<_, _>>()
                        .unwrap();
                    assert!(!values.is_empty());
                    assert!(values.iter().all(|value| value == "unexported"));
                    if !after_publish {
                        assert!(!root.path().join("legacy-holdings").exists());
                    }
                    runtime
                        .store
                        .lock()
                        .unwrap()
                        .execute_batch(&format!("ALTER TABLE {table} DROP COLUMN extra"))
                        .unwrap();
                    assert_eq!(
                        runtime.run(&Faults::default()).unwrap().state,
                        LegacyHoldingsState::Exported
                    );
                    usable(&runtime);
                }
            }
        }
    }
    #[test]
    fn hardening_failure_messages_contain_only_stage_and_error_kind() {
        let secret = "/private/user/0xabc/quantity-123.456";
        let cases = [
            (
                ExportError::Io(io::Error::new(io::ErrorKind::PermissionDenied, secret)),
                "io:PermissionDenied",
            ),
            (
                ExportError::Io(io::Error::from_raw_os_error(libc::ENOSPC)),
                "io:StorageFull",
            ),
            (
                ExportError::Catalog(StoreError::Invalid(secret.into())),
                "catalog",
            ),
            (
                ExportError::Private(ArtifactTrustError::Untrusted),
                "private_storage",
            ),
            (
                ExportError::Json(
                    serde_json::from_str::<serde_json::Value>("not JSON").unwrap_err(),
                ),
                "serialization",
            ),
            (ExportError::Verification, "verification"),
            (ExportError::Panic, "panic"),
        ];
        for (error, kind) in cases {
            let message = failure_message(Stage::VerifyPublished, &error);
            assert_eq!(
                message,
                format!("Legacy holdings export failed: stage=verify_published kind={kind}")
            );
            assert_eq!(message.lines().count(), 1);
            assert!(!message.contains(secret));
            assert!(!message.contains("0xabc"));
            assert!(!message.contains("123.456"));
        }
    }
    #[test]
    fn hardening_published_corruption_is_refused_and_retry_uses_a_numbered_file() {
        let root = tempfile::tempdir().unwrap();
        let (runtime, _) = runtime(Arc::new(fixture(root.path(), false)), root.path());
        let target = root.path().join("legacy-holdings/holdings-export.json");
        let corrupt = target.clone();
        assert_eq!(
            runtime
                .run(&Faults {
                    hook: Some(Arc::new(move |point| {
                        if point == Point::AfterRename {
                            std::fs::write(&corrupt, b"corrupted after rename").unwrap();
                        }
                    })),
                    ..Faults::default()
                })
                .unwrap()
                .state,
            LegacyHoldingsState::Failed
        );
        assert!(runtime.store.legacy_holdings_tables_exist().unwrap());
        assert!(runtime.store.legacy_holdings_receipt().unwrap().is_none());
        assert_eq!(
            runtime.run(&Faults::default()).unwrap().state,
            LegacyHoldingsState::Exported
        );
        assert_eq!(usable(&runtime).file_name, "holdings-export-2.json");
        assert_eq!(std::fs::read(target).unwrap(), b"corrupted after rename");
    }

    #[cfg(unix)]
    #[test]
    fn hardening_directory_admission_syncs_parent_and_propagates_sync_errors() {
        let root = tempfile::tempdir().unwrap();
        let observed = std::cell::RefCell::new(Vec::new());
        let child = root.path().join("legacy-holdings");
        directory_with_sync(root.path(), |parent| {
            assert_eq!(parent, root.path());
            private_metadata(&child, true).unwrap();
            observed.borrow_mut().push(parent.to_path_buf());
            sync_directory(parent)
        })
        .unwrap();
        assert_eq!(*observed.borrow(), [root.path().to_path_buf()]);
        // Existing private directories also propagate a failed parent sync.
        assert!(matches!(
            directory_with_sync(root.path(), |_| Err(io::Error::from_raw_os_error(
                libc::EIO
            )
            .into())),
            Err(ExportError::Io(_))
        ));
        directory(root.path()).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn hardening_foreign_partial_entries_are_preserved_and_skipped() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        for kind in ["mode", "directory", "symlink"] {
            let root = tempfile::tempdir().unwrap();
            let (runtime, _) = runtime(Arc::new(fixture(root.path(), false)), root.path());
            let dir = directory(root.path()).unwrap();
            let partial = dir.join("holdings-export.json.partial");
            let target = root.path().join("foreign");
            std::fs::write(&target, b"foreign").unwrap();
            match kind {
                "mode" => {
                    std::fs::write(&partial, b"foreign").unwrap();
                    std::fs::set_permissions(&partial, std::fs::Permissions::from_mode(0o644))
                        .unwrap();
                }
                "directory" => std::fs::create_dir(&partial).unwrap(),
                "symlink" => symlink(&target, &partial).unwrap(),
                _ => unreachable!(),
            }
            let before = std::fs::symlink_metadata(&partial).unwrap();
            assert_eq!(
                runtime.run(&Faults::default()).unwrap().state,
                LegacyHoldingsState::Exported
            );
            usable(&runtime);
            assert_eq!(
                std::fs::symlink_metadata(&partial).unwrap().file_type(),
                before.file_type()
            );
            assert_eq!(
                std::fs::symlink_metadata(&partial)
                    .unwrap()
                    .permissions()
                    .mode(),
                before.permissions().mode()
            );
            if kind == "mode" {
                assert_eq!(std::fs::read(&partial).unwrap(), b"foreign");
            }
            assert_eq!(std::fs::read(&target).unwrap(), b"foreign");
            assert_eq!(
                files(root.path()),
                ["holdings-export.json", "holdings-export.json.partial"]
            );
        }
    }
    fn assert_catalog_read_panic_recovers(kind: crate::store::legacy_holdings::ReadKind) {
        use crate::store::legacy_holdings::{with_read_panic, ReadKind};
        for during_export in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let store = Arc::new(fixture(root.path(), false));
            let (runtime, events) = runtime(store.clone(), root.path());
            let rows = store.legacy_holdings_rows().unwrap();
            let catalog = store.assets_list("", false).unwrap();
            // Skip the status probe only when targeting the export's receipt read.
            let skip = usize::from(during_export && kind == ReadKind::Receipt);
            let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                with_read_panic(kind, skip, || {
                    if during_export {
                        Some(runtime.run(&Faults::default()))
                    } else {
                        match kind {
                            ReadKind::Receipt => {
                                store.legacy_holdings_receipt().unwrap();
                            }
                            ReadKind::Rows => {
                                store.legacy_holdings_rows().unwrap();
                            }
                        }
                        None
                    }
                })
            }));
            assert_eq!(store.assets_list("", false).unwrap(), catalog);
            assert!(store.lock().unwrap().is_autocommit());
            assert_eq!(store.legacy_holdings_rows().unwrap(), rows);
            assert!(store.legacy_holdings_receipt().unwrap().is_none());
            if during_export {
                assert_eq!(
                    attempt.unwrap().unwrap().unwrap().state,
                    LegacyHoldingsState::Failed
                );
                assert_eq!(
                    events.lock().unwrap().last().unwrap().state,
                    LegacyHoldingsState::Failed
                );
            } else {
                assert!(
                    attempt.is_err(),
                    "the read must resume the unwind after releasing SQLite"
                );
            }
            assert_eq!(
                runtime.run(&Faults::default()).unwrap().state,
                LegacyHoldingsState::Exported
            );
            usable(&runtime);
        }
    }
    #[test]
    fn catalog_receipt_read_panic_preserves_store_and_retry() {
        assert_catalog_read_panic_recovers(crate::store::legacy_holdings::ReadKind::Receipt);
    }
    #[test]
    fn catalog_rows_read_panic_preserves_store_and_retry() {
        assert_catalog_read_panic_recovers(crate::store::legacy_holdings::ReadKind::Rows);
    }
}
