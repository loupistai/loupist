//! Saving a shared Loupist as a folder or ZIP, and revealing the saved copy.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::Deserialize;
use serde_json::Value;
use tauri::State;

use super::share::{build_pack, record_identity, ScopeRequest, ShareRequest};
use super::{admit, internal, LoupistRequest};
use crate::app::{AppState, CommandError};
use crate::sharing::export::Sections;
use crate::sharing::pack::{self, Files, License};

#[derive(Debug, Clone, Copy, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum Destination {
    Folder,
    Zip,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ShareExportRequest {
    loupist_id: String,
    scope: ScopeRequest,
    sections: Sections,
    excluded_item_keys: Vec<String>,
    acknowledged_finding_ids: Vec<String>,
    #[serde(default)]
    license: Option<License>,
    #[serde(default)]
    byline: Option<String>,
    destination: Destination,
}

impl ShareExportRequest {
    fn share(&self) -> ShareRequest {
        ShareRequest {
            loupist_id: self.loupist_id.clone(),
            scope: self.scope.clone(),
            sections: self.sections,
            excluded_item_keys: self.excluded_item_keys.clone(),
            acknowledged_finding_ids: self.acknowledged_finding_ids.clone(),
            license: self.license,
            byline: self.byline.clone(),
        }
    }
}

/// Where this app session saved each Loupist's copy, for Show in Finder.
#[derive(Clone, Default)]
pub(crate) struct SavedCopies(Arc<Mutex<BTreeMap<String, PathBuf>>>);

impl SavedCopies {
    fn remember(&self, loupist_id: &str, path: PathBuf) {
        if let Ok(mut saved) = self.0.lock() {
            saved.insert(loupist_id.to_owned(), path);
        }
    }

    fn get(&self, loupist_id: &str) -> Option<PathBuf> {
        self.0
            .lock()
            .ok()
            .and_then(|saved| saved.get(loupist_id).cloned())
    }
}

pub(super) fn write_new_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// Writes a pack folder under `parent`; an existing name gets `-2`, `-3`, …
pub(crate) fn write_folder(parent: &Path, slug: &str, files: &Files) -> std::io::Result<PathBuf> {
    let mut target = parent.join(slug);
    let mut suffix = 2;
    loop {
        match std::fs::create_dir(&target) {
            Ok(()) => break,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && suffix < 100 => {
                target = parent.join(format!("{slug}-{suffix}"));
                suffix += 1;
            }
            Err(error) => return Err(error),
        }
    }
    for (path, bytes) in files {
        let destination = target.join(path);
        if let Some(directory) = destination.parent() {
            std::fs::create_dir_all(directory)?;
        }
        write_new_file(&destination, bytes)?;
    }
    Ok(target)
}

/// A deterministic ZIP: sorted entries under one folder, fixed times, 0644.
pub(crate) fn zip_bytes(slug: &str, files: &Files) -> std::io::Result<Vec<u8>> {
    use std::io::Write;
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default())
        .unix_permissions(0o644);
    for (path, bytes) in files {
        writer
            .start_file(format!("{slug}/{path}"), options)
            .map_err(std::io::Error::other)?;
        writer.write_all(bytes)?;
    }
    Ok(writer.finish().map_err(std::io::Error::other)?.into_inner())
}

fn write_zip(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    if std::fs::symlink_metadata(path).is_ok_and(|metadata| !metadata.is_file()) {
        return Err(std::io::Error::other("not a regular file"));
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

#[cfg(feature = "e2e")]
fn e2e_share_dir() -> Option<PathBuf> {
    std::env::var_os("LOUPIST_E2E_SHARE_DIR").map(PathBuf::from)
}
#[cfg(not(feature = "e2e"))]
fn e2e_share_dir() -> Option<PathBuf> {
    None
}

#[tauri::command(rename_all = "snake_case")]
pub async fn share_export(
    request: ShareExportRequest,
    state: State<'_, AppState>,
) -> Result<Option<Value>, CommandError> {
    let state = state.inner();
    let share = request.share();
    let built = build_pack(state, &share, None)?;
    let slug = pack::slug(&built.name, "loupist");
    let path = match request.destination {
        Destination::Folder => {
            let parent = match e2e_share_dir() {
                Some(directory) => directory,
                None => match rfd::AsyncFileDialog::new()
                    .set_title("Choose where to save the shared Loupist")
                    .pick_folder()
                    .await
                {
                    Some(folder) => folder.path().to_owned(),
                    None => return Ok(None),
                },
            };
            let files = built.files.clone();
            tauri::async_runtime::spawn_blocking(move || write_folder(&parent, &slug, &files))
                .await
                .map_err(internal)?
                .map_err(internal)?
        }
        Destination::Zip => {
            let path = match e2e_share_dir() {
                Some(directory) => directory.join(format!("{slug}.zip")),
                None => match rfd::AsyncFileDialog::new()
                    .set_title("Save the shared Loupist")
                    .set_file_name(format!("{slug}.zip"))
                    .add_filter("ZIP", &["zip"])
                    .save_file()
                    .await
                {
                    Some(file) => file.path().to_owned(),
                    None => return Ok(None),
                },
            };
            if path.extension().and_then(|e| e.to_str()) != Some("zip") {
                return Err(CommandError::invalid(
                    "Save the shared Loupist as a .zip file.",
                ));
            }
            let bytes = zip_bytes(&slug, &built.files).map_err(internal)?;
            let target = path.clone();
            tauri::async_runtime::spawn_blocking(move || write_zip(&target, &bytes))
                .await
                .map_err(internal)?
                .map_err(internal)?;
            path
        }
    };
    record_identity(state, &share.loupist_id, &built)?;
    state
        .sharing
        .saved
        .remember(&share.loupist_id, path.clone());
    Ok(Some(
        serde_json::json!({"path": path.display().to_string()}),
    ))
}

#[tauri::command(rename_all = "snake_case")]
pub fn share_reveal(
    request: LoupistRequest,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    admit(state.inner(), &request.loupist_id)?;
    let path = state
        .sharing
        .saved
        .get(&request.loupist_id)
        .ok_or_else(|| CommandError::not_found("saved copy"))?;
    #[cfg(target_os = "macos")]
    std::process::Command::new("/usr/bin/open")
        .arg("-R")
        .arg(&path)
        .spawn()
        .map_err(internal)?;
    #[cfg(windows)]
    std::process::Command::new("explorer.exe")
        .arg(format!("/select,{}", path.display()))
        .spawn()
        .map_err(internal)?;
    #[cfg(not(any(target_os = "macos", windows)))]
    open::that_detached(&path).map_err(internal)?;
    Ok(())
}
