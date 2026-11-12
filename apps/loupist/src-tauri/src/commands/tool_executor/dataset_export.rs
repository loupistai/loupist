use super::*;
use std::{fs::File, io::Write};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExportRequest {
    dataset_id: String,
    revision: u32,
    path: String,
    format: ExportFormat,
    #[serde(default)]
    pointer: Option<String>,
    #[serde(default)]
    columns: Vec<String>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ExportFormat {
    Json,
    Csv,
}

struct ExportContent {
    bytes: Vec<u8>,
    row_count: Option<usize>,
    columns: Vec<String>,
}

fn export_error(error: impl std::fmt::Display) -> BrokerError {
    BrokerError::Execution(format!("Dataset export: {error}"))
}

impl AppToolExecutor {
    pub(super) fn dataset_export(
        &self,
        policy: &ToolPolicy,
        params: Value,
    ) -> Result<Value, BrokerError> {
        let request: ExportRequest = serde_json::from_value(params.clone()).map_err(|_| {
            let (field, expected) = if !params.get("dataset_id").is_some_and(Value::is_string) {
                ("dataset_export.dataset_id", "expected a string")
            } else if !params
                .get("revision")
                .and_then(Value::as_u64)
                .is_some_and(|value| value <= u32::MAX as u64)
            {
                ("dataset_export.revision", "expected a positive integer")
            } else if !params.get("path").is_some_and(Value::is_string) {
                ("dataset_export.path", "expected a string")
            } else if !params
                .get("format")
                .and_then(Value::as_str)
                .is_some_and(|value| matches!(value, "json" | "csv"))
            {
                ("dataset_export.format", "expected json or csv")
            } else if params
                .get("pointer")
                .is_some_and(|value| !value.is_null() && !value.is_string())
            {
                ("dataset_export.pointer", "expected a string or null")
            } else if params
                .get("columns")
                .is_some_and(|value| serde_json::from_value::<Vec<String>>(value.clone()).is_err())
            {
                ("dataset_export.columns", "expected an array of strings")
            } else {
                (
                    "dataset_export",
                    "expected only dataset_id, revision, path, format, pointer, and columns fields",
                )
            };
            BrokerError::invalid(field, expected)
        })?;
        if request.dataset_id.is_empty()
            || request.revision == 0
            || request.columns.len() > 80
            || request
                .columns
                .iter()
                .any(|column| column.is_empty() || column.len() > 256 || column.contains('\0'))
            || request.columns.iter().collect::<BTreeSet<_>>().len() != request.columns.len()
            || request
                .pointer
                .as_ref()
                .is_some_and(|pointer| pointer.len() > 2048)
            || (matches!(request.format, ExportFormat::Json) && !request.columns.is_empty())
        {
            return Err(BrokerError::Malformed);
        }
        let path =
            crate::fs::workspace_files::relative_path(&request.path).map_err(export_error)?;
        if path == "." {
            return Err(export_error("choose a file path inside the workspace"));
        }
        self.host_file_gate(&path, super::workspace::HostFile::Export)?;
        let (reference, payload, metadata) = self
            .state
            .store
            .dataset(&self.loupist_id, &request.dataset_id, request.revision)
            .map_err(command_error)?;
        super::datasets::ensure_within_cutoff(
            &request.dataset_id,
            request.revision,
            &metadata,
            parse_as_of_date(policy.as_of.as_deref())?,
        )?;
        let selected = request
            .pointer
            .as_deref()
            .map_or(Some(&payload), |pointer| payload.pointer(pointer))
            .ok_or_else(|| {
                export_error("dataset pointer is absent; inspect dataset_describe before exporting")
            })?;
        let ExportContent {
            bytes,
            row_count,
            columns,
        } = match request.format {
            ExportFormat::Json => ExportContent {
                bytes: serde_json::to_vec_pretty(selected).map_err(export_error)?,
                row_count: selected.as_array().map(Vec::len),
                columns: vec![],
            },
            ExportFormat::Csv => csv_bytes(selected, &request.columns)?,
        };
        if bytes.len() > crate::datasets::MAX_DATASET_BYTES * 2 {
            return Err(export_error("export exceeds its size limit"));
        }
        let digest = sha256(&bytes);
        let metadata_path = format!("{path}.provenance.json");
        let provenance = json!({
            "schema":"loupist.dataset-export/1", "dataset_ref":reference,
            "pointer":request.pointer, "columns":columns, "row_count":row_count,
            "file":path, "content_sha256":digest, "size_bytes":bytes.len(),
            "exported_at":Utc::now().to_rfc3339(), "source_metadata":metadata,
            "interpretation":"A copy of a pinned dataset revision. Source metadata and execution do not verify financial conclusions."
        });
        let metadata_bytes = serde_json::to_vec_pretty(&provenance).map_err(export_error)?;
        let profile = self
            .state
            .store
            .get_loupist(&self.loupist_id)
            .map_err(store_error)?
            .ok_or_else(|| export_error("Loupist does not exist"))?;
        let root = crate::fs::loupist_root::profile_workspace(&profile).map_err(command_error)?;
        if !crate::custom_connections::taint::retained_trusted(&metadata) {
            crate::commands::export_trust::record(&self.state, &bytes).map_err(command_error)?;
            crate::commands::export_trust::record(&self.state, &metadata_bytes)
                .map_err(command_error)?;
        }
        write_export(&root, &path, &bytes, &metadata_bytes)?;
        Ok(
            json!({"path":path, "metadata_path":metadata_path, "dataset_ref":reference,
            "size_bytes":bytes.len(), "content_sha256":digest, "row_count":row_count, "columns":columns,
            "status":"written", "lifecycle":"Local workspace files are immediate and remain if this reply is interrupted."}),
        )
    }
}

fn csv_bytes(selected: &Value, requested_columns: &[String]) -> Result<ExportContent, BrokerError> {
    let rows = selected.as_array().ok_or_else(|| {
        export_error("CSV requires an array of row objects; select it with pointer")
    })?;
    if rows.iter().any(|row| !row.is_object()) {
        return Err(export_error(
            "CSV requires row objects; use JSON for scalar or nested arrays",
        ));
    }
    let columns = if requested_columns.is_empty() {
        rows.iter()
            .filter_map(Value::as_object)
            .flat_map(|row| row.keys().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
    } else {
        requested_columns.to_vec()
    };
    if columns.is_empty() || columns.len() > 80 {
        return Err(export_error(
            "CSV needs 1–80 columns; specify columns for an empty array",
        ));
    }
    if !rows.is_empty()
        && columns
            .iter()
            .any(|column| rows.iter().all(|row| row.get(column).is_none()))
    {
        return Err(export_error(
            "a selected CSV column does not exist in any row",
        ));
    }
    let mut output = String::new();
    csv_row(&mut output, columns.iter().map(String::as_str));
    for row in rows {
        let values = columns
            .iter()
            .map(|column| match row.get(column) {
                None | Some(Value::Null) => String::new(),
                Some(Value::String(value)) => value.clone(),
                Some(value) => value.to_string(),
            })
            .collect::<Vec<_>>();
        csv_row(&mut output, values.iter().map(String::as_str));
    }
    Ok(ExportContent {
        bytes: output.into_bytes(),
        row_count: Some(rows.len()),
        columns,
    })
}

fn csv_row<'a>(output: &mut String, cells: impl Iterator<Item = &'a str>) {
    for (index, cell) in cells.enumerate() {
        if index > 0 {
            output.push(',');
        }
        if cell.contains([',', '"', '\r', '\n']) {
            output.push('"');
            output.push_str(&cell.replace('"', "\"\""));
            output.push('"');
        } else {
            output.push_str(cell);
        }
    }
    output.push_str("\r\n");
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExportStep {
    WritingData,
    WritingMetadata,
    PublishedMetadata,
    PublishedData,
}

#[cfg(unix)]
fn write_export(
    root: &crate::fs::loupist_root::BoundLoupistRoot,
    path: &str,
    bytes: &[u8],
    metadata: &[u8],
) -> Result<(), BrokerError> {
    write_export_with_hook(root, path, bytes, metadata, |_| Ok(()))
}

#[cfg(unix)]
fn write_export_with_hook(
    root: &crate::fs::loupist_root::BoundLoupistRoot,
    path: &str,
    bytes: &[u8],
    metadata: &[u8],
    mut hook: impl FnMut(ExportStep) -> Result<(), BrokerError>,
) -> Result<(), BrokerError> {
    use rustix::{
        fs::{linkat, mkdirat, openat, statat, unlinkat, AtFlags, Mode, OFlags},
        io::fcntl_dupfd_cloexec,
    };
    let mut directory = fcntl_dupfd_cloexec(root, 3).map_err(export_error)?;
    let parts = path.split('/').collect::<Vec<_>>();
    for part in &parts[..parts.len() - 1] {
        match mkdirat(&directory, *part, Mode::from_raw_mode(0o755)) {
            Ok(()) => {}
            Err(error) if error == rustix::io::Errno::EXIST => {}
            Err(error) => return Err(export_error(error)),
        }
        directory = openat(
            &directory,
            *part,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(export_error)?;
    }
    let name = parts.last().ok_or(BrokerError::Malformed)?;
    let metadata_name = format!("{name}.provenance.json");
    for target in [*name, metadata_name.as_str()] {
        match statat(&directory, target, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(_) => {
                return Err(export_error(format!(
                    "{target} already exists; choose a new path"
                )))
            }
            Err(error) if error == rustix::io::Errno::NOENT => {}
            Err(error) => return Err(export_error(error)),
        }
    }
    // Stage both complete files in a private, pinned transaction directory.
    let transaction_name = format!(".loupist-export-{}", uuid::Uuid::new_v4().simple());
    mkdirat(
        &directory,
        transaction_name.as_str(),
        Mode::from_raw_mode(0o700),
    )
    .map_err(export_error)?;
    let transaction = openat(
        &directory,
        transaction_name.as_str(),
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(export_error)?;
    let mut staged = Vec::new();
    let mut published = Vec::new();
    let result = (|| {
        for (temporary_name, content, step) in [
            ("data", bytes, ExportStep::WritingData),
            ("metadata", metadata, ExportStep::WritingMetadata),
        ] {
            let mut file = File::from(
                openat(
                    &transaction,
                    temporary_name,
                    OFlags::WRONLY
                        | OFlags::CREATE
                        | OFlags::EXCL
                        | OFlags::NOFOLLOW
                        | OFlags::CLOEXEC,
                    Mode::from_raw_mode(0o644),
                )
                .map_err(export_error)?,
            );
            staged.push((temporary_name, file.try_clone().map_err(export_error)?));
            hook(step)?;
            file.write_all(content).map_err(export_error)?;
            file.sync_all().map_err(export_error)?;
        }
        // Exclusive publication exposes only complete files. If the second link
        // fails, rollback checks filesystem identity through these pinned dirs.
        for (temporary_name, target, step) in [
            (
                "metadata",
                metadata_name.as_str(),
                ExportStep::PublishedMetadata,
            ),
            ("data", *name, ExportStep::PublishedData),
        ] {
            linkat(
                &transaction,
                temporary_name,
                &directory,
                target,
                AtFlags::empty(),
            )
            .map_err(export_error)?;
            published.push((temporary_name, target));
            hook(step)?;
        }
        Ok(())
    })();
    let mut rollback_failure = None;
    if result.is_err() {
        for (temporary_name, target) in published.into_iter().rev() {
            let file = &staged
                .iter()
                .find(|(name, _)| *name == temporary_name)
                .expect("published staged file")
                .1;
            if let Err(error) = rollback_export_link(&directory, target, &transaction, file) {
                rollback_failure = Some(error);
            }
        }
    }
    for (name, _) in &staged {
        let _ = unlinkat(&transaction, *name, AtFlags::empty());
    }
    // A displaced foreign entry that cannot be restored is deliberately kept
    // here; remove_directory fails on nonempty transactions, never deleting it.
    let _ = unlinkat(&directory, transaction_name.as_str(), AtFlags::REMOVEDIR);
    if let Some(error) = rollback_failure {
        return Err(export_error(format!(
            "export failed; rollback recovery remains in {transaction_name}: {error}"
        )));
    }
    result
}

#[cfg(unix)]
fn rollback_export_link(
    directory: &impl std::os::fd::AsFd,
    target: &str,
    transaction: &impl std::os::fd::AsFd,
    owned_file: &File,
) -> std::io::Result<()> {
    rollback_export_link_with_hook(directory, target, transaction, owned_file, || {})
}

#[cfg(unix)]
fn rollback_export_link_with_hook(
    directory: &impl std::os::fd::AsFd,
    target: &str,
    transaction: &impl std::os::fd::AsFd,
    owned_file: &File,
    before_rename: impl FnOnce(),
) -> std::io::Result<()> {
    use rustix::fs::{fstat, linkat, renameat, statat, unlinkat, AtFlags};
    let owned = fstat(owned_file)?;
    let matches_owned =
        |stat: &rustix::fs::Stat| stat.st_dev == owned.st_dev && stat.st_ino == owned.st_ino;
    let current = match statat(directory, target, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(current) => current,
        Err(error) if error == rustix::io::Errno::NOENT => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if !matches_owned(&current) {
        return Ok(());
    }
    before_rename();
    // POSIX has no compare-and-unlink. Move the entry into our private pinned
    // directory first, then verify it again before deleting. A replacement in
    // the stat/rename race is restored without overwriting any newer entry.
    let quarantined = format!("rollback-{}", uuid::Uuid::new_v4().simple());
    match renameat(directory, target, transaction, quarantined.as_str()) {
        Ok(()) => {}
        Err(error) if error == rustix::io::Errno::NOENT => return Ok(()),
        Err(error) => return Err(error.into()),
    }
    let captured = statat(transaction, quarantined.as_str(), AtFlags::SYMLINK_NOFOLLOW)?;
    if !matches_owned(&captured) {
        linkat(
            transaction,
            quarantined.as_str(),
            directory,
            target,
            AtFlags::empty(),
        )?;
    }
    unlinkat(transaction, quarantined.as_str(), AtFlags::empty())?;
    Ok(())
}

#[cfg(windows)]
fn write_export(
    root: &crate::fs::loupist_root::BoundLoupistRoot,
    path: &str,
    bytes: &[u8],
    metadata: &[u8],
) -> Result<(), BrokerError> {
    write_export_with_hook(root, path, bytes, metadata, |_| Ok(()))
}

#[cfg(windows)]
fn write_export_with_hook(
    root: &crate::fs::loupist_root::BoundLoupistRoot,
    path: &str,
    bytes: &[u8],
    metadata: &[u8],
    mut hook: impl FnMut(ExportStep) -> Result<(), BrokerError>,
) -> Result<(), BrokerError> {
    let mut directory = root.path().to_path_buf();
    let mut guards =
        vec![crate::fs::windows::open_directory_no_reparse(&directory).map_err(export_error)?];
    let parts = path.split('/').collect::<Vec<_>>();
    for part in &parts[..parts.len() - 1] {
        directory.push(part);
        match std::fs::create_dir(&directory) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(export_error(error)),
        }
        guards
            .push(crate::fs::windows::open_directory_no_reparse(&directory).map_err(export_error)?);
    }
    let filename = parts.last().ok_or(BrokerError::Malformed)?;
    let data_path = directory.join(filename);
    let metadata_path = directory.join(format!("{filename}.provenance.json"));
    // Each handle denies rename/delete sharing from creation through commit.
    // Publication and rollback act on that same handle, never a mutable path.
    let mut data = windows_export::StagedFile::new(&directory).map_err(export_error)?;
    let mut provenance = windows_export::StagedFile::new(&directory).map_err(export_error)?;
    let result = (|| {
        hook(ExportStep::WritingData)?;
        data.file.write_all(bytes).map_err(export_error)?;
        data.file.sync_all().map_err(export_error)?;
        hook(ExportStep::WritingMetadata)?;
        provenance.file.write_all(metadata).map_err(export_error)?;
        provenance.file.sync_all().map_err(export_error)?;
        provenance.publish(&metadata_path).map_err(export_error)?;
        hook(ExportStep::PublishedMetadata)?;
        data.publish(&data_path).map_err(export_error)?;
        hook(ExportStep::PublishedData)?;
        Ok(())
    })();
    if result.is_err() {
        // Try both handles even if one rollback fails. Surface cleanup failure
        // rather than silently implying that the pair was removed.
        let data_cleanup = data.rollback();
        let metadata_cleanup = provenance.rollback();
        data_cleanup.and(metadata_cleanup).map_err(|error| {
            export_error(format!(
                "export failed; owned-file rollback failed: {error}"
            ))
        })?;
        return result;
    }
    data.committed = true;
    provenance.committed = true;
    Ok(())
}

#[cfg(windows)]
mod windows_export {
    use super::*;
    use std::{
        fs::OpenOptions,
        io,
        os::windows::{ffi::OsStrExt, fs::OpenOptionsExt, io::AsRawHandle},
        path::Path,
    };
    use windows_sys::Win32::{
        Foundation::{GENERIC_READ, GENERIC_WRITE},
        Storage::FileSystem::{
            FileDispositionInfo, FileRenameInfo, SetFileInformationByHandle, DELETE,
            FILE_DISPOSITION_INFO, FILE_FLAG_OPEN_REPARSE_POINT, FILE_RENAME_INFO, FILE_SHARE_READ,
        },
    };

    pub(super) struct StagedFile {
        pub file: File,
        pub committed: bool,
    }
    impl StagedFile {
        pub fn new(directory: &Path) -> io::Result<Self> {
            let path = directory.join(format!(".loupist-export-{}", uuid::Uuid::new_v4().simple()));
            let file = OpenOptions::new()
                .create_new(true)
                .access_mode(GENERIC_READ | GENERIC_WRITE | DELETE)
                .share_mode(FILE_SHARE_READ)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
                .open(path)?;
            Ok(Self {
                file,
                committed: false,
            })
        }
        pub fn publish(&self, destination: &Path) -> io::Result<()> {
            let name = destination.as_os_str().encode_wide().collect::<Vec<_>>();
            let offset = std::mem::offset_of!(FILE_RENAME_INFO, FileName);
            let bytes = offset + (name.len() + 1) * 2;
            // usize storage provides the alignment required by the HANDLE field.
            let mut storage = vec![0usize; bytes.div_ceil(std::mem::size_of::<usize>())];
            let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFO>();
            // SAFETY: the aligned allocation holds the complete variable-length
            // FILE_RENAME_INFO. Zero initialization means ReplaceIfExists=false
            // and RootDirectory=NULL; the pinned parent guards hold the path.
            unsafe {
                (*info).FileNameLength = (name.len() * 2) as u32;
                std::ptr::copy_nonoverlapping(
                    name.as_ptr(),
                    (*info).FileName.as_mut_ptr(),
                    name.len(),
                );
                if SetFileInformationByHandle(
                    self.file.as_raw_handle().cast(),
                    FileRenameInfo,
                    info.cast(),
                    bytes as u32,
                ) == 0
                {
                    return Err(io::Error::last_os_error());
                }
            }
            Ok(())
        }
        pub fn rollback(&self) -> io::Result<()> {
            let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
            // SAFETY: this retained DELETE-capable handle still owns the exact
            // file and forbids foreign rename/delete. Closing it completes the
            // disposition, even after it has been published under another name.
            let result = unsafe {
                SetFileInformationByHandle(
                    self.file.as_raw_handle().cast(),
                    FileDispositionInfo,
                    (&disposition as *const FILE_DISPOSITION_INFO).cast(),
                    std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
                )
            };
            if result == 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        }
    }
    impl Drop for StagedFile {
        fn drop(&mut self) {
            if self.committed {
                return;
            }
            let _ = self.rollback();
        }
    }
}

#[cfg(not(any(unix, windows)))]
fn write_export(
    _root: &crate::fs::loupist_root::BoundLoupistRoot,
    _path: &str,
    _bytes: &[u8],
    _metadata: &[u8],
) -> Result<(), BrokerError> {
    Err(export_error(
        "workspace export is unavailable on this platform",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn csv_preserves_numbers_nulls_unicode_and_quotes_in_stable_order() {
        let ExportContent {
            bytes,
            row_count: count,
            columns,
        } = csv_bytes(
            &json!([{"z":null,"a":"한글,\"value\"\nnext","n":1.25},{"a":"two","n":-3}]),
            &[],
        )
        .unwrap();
        assert_eq!(columns, vec!["a", "n", "z"]);
        assert_eq!(count, Some(2));
        assert_eq!(
            String::from_utf8(bytes).unwrap(),
            "a,n,z\r\n\"한글,\"\"value\"\"\nnext\",1.25,\r\ntwo,-3,\r\n"
        );
        assert!(csv_bytes(&json!([1, 2]), &[]).is_err());
        assert!(csv_bytes(&json!([{"a":1}]), &["missing".into()]).is_err());
    }
    #[cfg(any(unix, windows))]
    #[test]
    fn failed_export_leaves_no_partial_output_and_keeps_concurrent_data() {
        let directory = tempfile::tempdir().unwrap();
        let root = crate::fs::loupist_root::BoundLoupistRoot::open_unbound(directory.path().into())
            .unwrap();
        let result = write_export_with_hook(&root, "prices.json", b"ours", b"metadata", |step| {
            if step == ExportStep::WritingMetadata {
                return Err(export_error("injected disk write failure"));
            }
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);

        let result =
            write_export_with_hook(&root, "prices.json", b"ours", b"our provenance", |step| {
                if step == ExportStep::PublishedMetadata {
                    std::fs::write(directory.path().join("prices.json"), b"concurrent data")
                        .unwrap();
                }
                Ok(())
            });
        assert!(result.is_err());
        assert_eq!(
            std::fs::read(directory.path().join("prices.json")).unwrap(),
            b"concurrent data"
        );
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }
    #[cfg(unix)]
    #[test]
    fn export_rollback_restores_a_foreign_replacement_in_the_identity_check_race() {
        let directory = tempfile::tempdir().unwrap();
        let transaction = tempfile::tempdir_in(directory.path()).unwrap();
        let owned_path = transaction.path().join("owned");
        std::fs::write(&owned_path, b"ours").unwrap();
        let owned = File::open(&owned_path).unwrap();
        let target = directory.path().join("published");
        std::fs::hard_link(&owned_path, &target).unwrap();
        rollback_export_link_with_hook(
            &File::open(directory.path()).unwrap(),
            "published",
            &File::open(transaction.path()).unwrap(),
            &owned,
            || {
                std::fs::remove_file(&target).unwrap();
                std::fs::write(&target, b"foreign replacement").unwrap();
            },
        )
        .unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"foreign replacement");
        assert_eq!(std::fs::read(&owned_path).unwrap(), b"ours");
        assert_eq!(std::fs::read_dir(transaction.path()).unwrap().count(), 1);
    }
    #[cfg(unix)]
    #[test]
    fn export_rollback_uses_pinned_parent_after_directory_rebinding() {
        let directory = tempfile::tempdir().unwrap();
        let root = crate::fs::loupist_root::BoundLoupistRoot::open_unbound(directory.path().into())
            .unwrap();
        let result = write_export_with_hook(
            &root,
            "data/prices.json",
            b"ours",
            b"our provenance",
            |step| {
                if step == ExportStep::PublishedMetadata {
                    std::fs::rename(
                        directory.path().join("data"),
                        directory.path().join("moved"),
                    )
                    .unwrap();
                    std::fs::create_dir(directory.path().join("data")).unwrap();
                    std::fs::write(
                        directory.path().join("data/prices.json.provenance.json"),
                        b"new directory data",
                    )
                    .unwrap();
                    std::fs::write(
                        directory.path().join("moved/prices.json"),
                        b"conflicting data",
                    )
                    .unwrap();
                }
                Ok(())
            },
        );
        assert!(result.is_err());
        assert!(!directory
            .path()
            .join("moved/prices.json.provenance.json")
            .exists());
        assert_eq!(
            std::fs::read(directory.path().join("data/prices.json.provenance.json")).unwrap(),
            b"new directory data"
        );
        assert_eq!(
            std::fs::read_dir(directory.path().join("moved"))
                .unwrap()
                .count(),
            1
        );
    }
    #[cfg(unix)]
    #[test]
    fn export_refuses_existing_escaping_and_symlinked_targets() {
        use std::os::unix::fs::symlink;
        let directory = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let root = crate::fs::loupist_root::BoundLoupistRoot::open_unbound(directory.path().into())
            .unwrap();
        write_export(&root, "data/prices.json", b"[1,2,3]", b"{}").unwrap();
        assert!(write_export(&root, "data/prices.json", b"replacement", b"{}").is_err());
        assert_eq!(
            std::fs::read(directory.path().join("data/prices.json")).unwrap(),
            b"[1,2,3]"
        );
        assert!(crate::fs::workspace_files::relative_path("../out.json").is_err());
        assert!(crate::fs::workspace_files::relative_path("/out.json").is_err());
        symlink(outside.path(), directory.path().join("escape")).unwrap();
        assert!(write_export(&root, "escape/prices.json", b"[]", b"{}").is_err());
        symlink(
            outside.path().join("target"),
            directory.path().join("linked.json"),
        )
        .unwrap();
        assert!(write_export(&root, "linked.json", b"[]", b"{}").is_err());
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
    }
}
