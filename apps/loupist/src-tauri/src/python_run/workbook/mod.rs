//! Desktop Files preview. This lane has its own cache and worker admission.
//! It never takes the agent execution permit, snapshots a chat, or writes back.
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    io::{Cursor, Read},
    path::Path,
    sync::Arc,
};
use tokio::sync::Mutex;

use crate::{
    app::CommandError,
    fs::{loupist_root::BoundLoupistRoot, workspace_files},
};

const MAX_CACHE_ENTRIES: usize = 16;

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable))]
#[serde(deny_unknown_fields)]
pub struct WorkbookRequest {
    pub loupist_id: String,
    pub path: String,
    pub sheet_index: Option<u16>,
}

#[derive(Clone, Copy, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum WorkbookProgress {
    CheckingFile,
    ReadingSheet,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum WorkbookRead {
    Ready(WorkbookProjection),
    UnsupportedFormat,
    Encrypted,
    InvalidWorkbook,
    UnknownSheet,
    UnsupportedSheet,
    LimitExceeded,
    WorkerUnavailable,
    TimedOut,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WorkbookProjection {
    pub sheets: Vec<WorkbookSheet>,
    pub selected_sheet: Option<u16>,
    pub first_row: Option<u32>,
    pub first_column: Option<u32>,
    pub rows_total: u32,
    pub columns_total: u32,
    pub rows: Vec<Vec<WorkbookCell>>,
    pub truncated: bool,
    pub omitted: Vec<WorkbookOmission>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WorkbookSheet {
    pub name: String,
    pub hidden: bool,
    pub worksheet: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WorkbookCell {
    pub kind: WorkbookCellKind,
    pub display: String,
    pub formula: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub number_format: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum WorkbookCellKind {
    Number,
    Text,
    Date,
    Boolean,
    Error,
    Empty,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum WorkbookOmission {
    Charts,
    Images,
    VisualStyles,
    Comments,
    Macros,
}

struct Cached {
    digest: String,
    default_sheet: bool,
    projection: WorkbookProjection,
}

#[derive(Clone, Default)]
pub(crate) struct WorkbookCache(Arc<Mutex<VecDeque<Cached>>>);

fn guard(bytes: &[u8]) -> Result<(), WorkbookRead> {
    if bytes.starts_with(b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1") {
        // Encrypted OOXML uses an OLE container. A renamed legacy workbook
        // can have the same signature, so only identify encryption by its stream.
        let stream: Vec<u8> = "EncryptedPackage"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        return Err(
            if bytes.windows(stream.len()).any(|window| window == stream) {
                WorkbookRead::Encrypted
            } else {
                WorkbookRead::UnsupportedFormat
            },
        );
    }
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| WorkbookRead::InvalidWorkbook)?;
    let count = archive.len();
    if !crate::web::document::archive_budget_allows(count, 0, 0) {
        return Err(WorkbookRead::LimitExceeded);
    }
    archive
        .validate_header_consistency()
        .map_err(|_| WorkbookRead::InvalidWorkbook)?;
    let mut total = 0_u64;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    for i in 0..archive.len() {
        if std::time::Instant::now() >= deadline {
            return Err(WorkbookRead::TimedOut);
        }
        let entry = archive
            .by_index_raw(i)
            .map_err(|_| WorkbookRead::InvalidWorkbook)?;
        if entry.encrypted() {
            return Err(WorkbookRead::Encrypted);
        }
        let size = entry.size();
        if size > entry.compressed_size().saturating_mul(200)
            || !crate::web::document::archive_budget_allows(count, total.saturating_add(size), size)
        {
            return Err(WorkbookRead::LimitExceeded);
        }
        total += size;
        drop(entry);
        let file = archive
            .by_index(i)
            .map_err(|_| WorkbookRead::InvalidWorkbook)?;
        if std::io::copy(&mut file.take(size + 1), &mut std::io::sink())
            .map_err(|_| WorkbookRead::InvalidWorkbook)?
            != size
        {
            return Err(WorkbookRead::InvalidWorkbook);
        }
    }
    Ok(())
}

pub(crate) async fn read(
    root: BoundLoupistRoot,
    request: WorkbookRequest,
    previews: &WorkbookCache,
    cache_root: &Path,
    progress: impl Fn(WorkbookProgress),
) -> Result<WorkbookRead, CommandError> {
    progress(WorkbookProgress::CheckingFile);
    let ext = Path::new(&request.path)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    if !["xlsx", "xlsm"].iter().any(|e| ext.eq_ignore_ascii_case(e)) {
        return Ok(WorkbookRead::UnsupportedFormat);
    }
    let mut entries = previews.0.lock().await;
    let input = tokio::task::spawn_blocking(move || {
        let bytes =
            workspace_files::exact_bytes(&root, &request.path, super::MAX_FILE_BYTES as usize)?;
        let digest = crate::util::sha256(&bytes);
        Ok::<_, CommandError>((digest, bytes))
    })
    .await
    .map_err(|_| CommandError::internal("workbook read interrupted"))?;
    let (digest, bytes) = match input {
        Ok(pair) => pair,
        Err(e)
            if e.detail
                .as_ref()
                .is_some_and(|d| d.get("limit_bytes").is_some()) =>
        {
            return Ok(WorkbookRead::LimitExceeded)
        }
        Err(e) => return Err(e),
    };
    if let Some(entry) = entries.iter().find(|e| {
        e.digest == digest
            && request.sheet_index.map_or(e.default_sheet, |index| {
                e.projection.selected_sheet == Some(index)
            })
    }) {
        return Ok(WorkbookRead::Ready(entry.projection.clone()));
    }
    progress(WorkbookProgress::ReadingSheet);
    let snapshot = tokio::task::spawn_blocking(move || {
        guard(&bytes)?;
        let mut builder = tempfile::Builder::new();
        builder.prefix("loupist-workbook-");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            builder.permissions(std::fs::Permissions::from_mode(0o700));
        }
        let dir = builder
            .tempdir()
            .map_err(|_| WorkbookRead::WorkerUnavailable)?;
        for name in ["workspace", "attachments", "scratch"] {
            std::fs::create_dir(dir.path().join(name))
                .map_err(|_| WorkbookRead::WorkerUnavailable)?;
        }
        std::fs::write(dir.path().join("workspace/workbook.xlsx"), bytes)
            .map_err(|_| WorkbookRead::WorkerUnavailable)?;
        Ok(dir)
    })
    .await
    .map_err(|_| CommandError::internal("workbook preparation interrupted"))?;
    let snapshot = match snapshot {
        Ok(dir) => dir,
        Err(reason) => return Ok(reason),
    };
    let result = project(snapshot.path(), cache_root, request.sheet_index).await;
    if let WorkbookRead::Ready(projection) = &result {
        if entries.len() == MAX_CACHE_ENTRIES {
            entries.pop_front();
        }
        entries.push_back(Cached {
            digest,
            default_sheet: request.sheet_index.is_none(),
            projection: projection.clone(),
        });
    }
    Ok(result)
}

#[cfg(unix)]
async fn project(snapshot: &Path, cache_root: &Path, index: Option<u16>) -> WorkbookRead {
    use super::{compiled_identity, supervise_worker, PythonCache, WorkerRequest};
    use crate::web::SearchCancel;
    let (Some(worker), Some(runtime)) = (super::worker_path(), super::runtime_dir()) else {
        return WorkbookRead::WorkerUnavailable;
    };
    let Ok(cache) = PythonCache::prepare(cache_root) else {
        return WorkbookRead::WorkerUnavailable;
    };
    let identity = compiled_identity(&worker, &runtime);
    let timeout_seconds = 60;
    let wall = cache.wall_clock(identity.as_deref(), timeout_seconds);
    let index = index.map_or_else(|| "None".into(), |i| i.to_string());
    let request = WorkerRequest {
        code: format!("sheet_index = {index}\n{}", include_str!("project.py")),
        timeout_seconds,
        runtime,
        cache: cache.compiled.clone(),
        snapshot: snapshot.into(),
        writable: Vec::new(),
    };
    let Ok(bytes) = serde_json::to_vec(&request) else {
        return WorkbookRead::WorkerUnavailable;
    };
    let Ok(out) = supervise_worker(
        &worker,
        snapshot,
        &bytes,
        wall,
        SearchCancel::never(),
        SearchCancel::never(),
    )
    .await
    else {
        return WorkbookRead::WorkerUnavailable;
    };
    if let (true, Some(id)) = (out.exited_itself, identity) {
        cache.mark_compiled(&id);
    }
    match out.status {
        "completed" if !out.stdout_truncated => {
            serde_json::from_slice(&out.stdout).unwrap_or(WorkbookRead::InvalidWorkbook)
        }
        "timed_out" => WorkbookRead::TimedOut,
        _ if out.exit_code == 126 || out.stdout_truncated => WorkbookRead::LimitExceeded,
        _ => WorkbookRead::WorkerUnavailable,
    }
}

#[cfg(not(unix))]
async fn project(_: &Path, _: &Path, _: Option<u16>) -> WorkbookRead {
    WorkbookRead::WorkerUnavailable
}

#[cfg(test)]
mod tests;
