//! Profile-local presentation preferences. References never grant access to
//! their targets; the existing file, artifact and Memory readers
//! recheck Loupist ownership when the renderer opens a restored tab.

use std::{collections::BTreeSet, fs::File, io::Read, path::Path};

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::{
    app::{AppState, CommandError},
    chat::artifacts::ChatArtifactRef,
    store::LoupistStore,
};

use super::{map_internal, map_store};

const DIRECTORY: &str = "preferences";
const FILE_NAME: &str = "workspace-ui.json";
const MAX_BYTES: usize = 512 * 1024;
const MAX_SESSIONS: usize = 100;
const MAX_TABS: usize = 20;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceUiState {
    version: u8,
    open: bool,
    placement: WorkspacePlacement,
    /// The user's dock choice per content kind (side-panel.md P7); unset kinds use defaults.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dock_by_kind: Option<DockByKind>,
    width: f64,
    height: f64,
    sessions: Vec<WorkspaceUiSession>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum WorkspacePlacement {
    Side,
    Bottom,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DockByKind {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    chart: Option<WorkspacePlacement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reading: Option<WorkspacePlacement>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WorkspaceUiSession {
    loupist_id: String,
    thread_id: String,
    tabs: Vec<WorkspaceUiTab>,
    #[serde(skip_serializing_if = "Option::is_none")]
    active_tab_id: Option<String>,
    /// The user closed the panel in this chat; the renderer does not reopen it by itself.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    dismissed: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum WorkspaceUiTab {
    Files {
        id: String,
        title: String,
        path: String,
        #[serde(rename = "filePath", skip_serializing_if = "Option::is_none")]
        file_path: Option<String>,
    },
    Memory {
        id: String,
        title: String,
        record_id: String,
        location: Box<WorkspaceMemoryLocation>,
    },
    /// The Work tab: the Loupist's charts and documents, optionally with one open. An
    /// item may come from another chat of the same Loupist, so its chat travels with it.
    Work {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        artifact: Option<ChatArtifactRef>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        artifact_thread_id: Option<String>,
    },
    Browser {
        id: String,
        title: String,
        url: String,
    },
    // Unsupported presentation references are discarded, never reopened.
    #[serde(other)]
    Unsupported,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkspaceMemoryLocation {
    loupist_id: String,
    surface: WorkspaceMemorySurface,
    #[serde(skip_serializing_if = "Option::is_none")]
    record_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    revision_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    object_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    selection: Option<WorkspaceMemorySelection>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum WorkspaceMemorySurface {
    Home,
    World,
    Lens,
    Source,
    Note,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum WorkspaceMemorySelection {
    Node {
        id: String,
    },
    Claim {
        id: String,
    },
    Source {
        #[serde(skip_serializing_if = "Option::is_none")]
        source_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        span_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        world_record_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        world_revision_id: Option<String>,
    },
    Lens {
        record_id: String,
    },
    Research {
        record_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        section_id: Option<String>,
    },
    Journal {
        journal_id: String,
    },
    Snapshot {
        pin: WorkspaceMemoryPin,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkspaceMemoryPin {
    kind: WorkspaceMemoryPinKind,
    loupist_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    record_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    revision_id: Option<String>,
    object_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    span_id: Option<String>,
    label: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum WorkspaceMemoryPinKind {
    Source,
    WorldNode,
    WorldClaim,
    Lens,
    Journal,
    Research,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceUiWriteRequest {
    state: WorkspaceUiState,
}

/// Unsent text lives in the app profile, outside the agent's workspace.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DraftIdentity {
    loupist_id: String,
    thread_id: String,
}

fn draft_table(state: &AppState, id: &DraftIdentity) -> Result<(), CommandError> {
    text(&id.loupist_id, 256, false)?;
    text(&id.thread_id, 256, false)?;
    state.ensure_loupist_available(&id.loupist_id)?;
    if id.thread_id != "__empty-chat__"
        && !state
            .store
            .lock_loupist(&id.loupist_id)
            .map_err(map_store)?
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM chat_sessions WHERE loupist_id=?1 AND id=?2)",
                [&id.loupist_id, &id.thread_id],
                |row| row.get::<_, bool>(0),
            )
            .map_err(map_internal)?
    {
        return Err(CommandError::not_found("draft chat"));
    }
    state
        .store
        .lock()
        .map_err(map_store)?
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS composer_drafts (
        loupist_id TEXT NOT NULL REFERENCES loupist_profiles(id) ON DELETE CASCADE,
        thread_id TEXT NOT NULL, prompt TEXT NOT NULL, PRIMARY KEY(loupist_id,thread_id)
        ) STRICT;
        CREATE TABLE IF NOT EXISTS composer_draft_context (
            loupist_id TEXT NOT NULL, thread_id TEXT NOT NULL, context TEXT NOT NULL,
            PRIMARY KEY(loupist_id,thread_id),
            FOREIGN KEY(loupist_id,thread_id) REFERENCES composer_drafts(loupist_id,thread_id) ON DELETE CASCADE
        ) STRICT;
        CREATE TABLE IF NOT EXISTS composer_draft_files (
            loupist_id TEXT NOT NULL REFERENCES loupist_profiles(id) ON DELETE CASCADE,
            thread_id TEXT NOT NULL, files TEXT NOT NULL, PRIMARY KEY(loupist_id,thread_id)
        ) STRICT;",
        )
        .map_err(map_internal)?;
    Ok(())
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DraftAttachment {
    id: String,
    filename: String,
    media_type: String,
    url: String,
}

#[tauri::command]
pub fn chat_draft_files_write(
    state: State<'_, AppState>,
    request: DraftIdentity,
    files: Vec<DraftAttachment>,
) -> Result<(), CommandError> {
    use base64::Engine;
    if files.len() > 4 {
        return Err(invalid("too many draft attachments"));
    }
    for file in &files {
        text(&file.id, 256, false)?;
        text(&file.filename, 1024, false)?;
        text(&file.media_type, 255, true)?;
        let (meta, encoded) = file
            .url
            .split_once(',')
            .ok_or_else(|| invalid("invalid draft attachment"))?;
        let mime = meta
            .strip_prefix("data:")
            .and_then(|s| s.strip_suffix(";base64"))
            .ok_or_else(|| invalid("invalid draft attachment"))?;
        text(mime, 255, true)?;
        if !file.media_type.is_empty() && !mime.eq_ignore_ascii_case(&file.media_type) {
            return Err(invalid("draft attachment type mismatch"));
        }
        let max: usize = if mime.starts_with("image/") { 5 } else { 25 } * 1024 * 1024;
        if encoded.len() > max.div_ceil(3) * 4
            || base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .map_err(map_internal)?
                .len()
                > max
        {
            return Err(invalid("draft attachment too large"));
        }
    }
    draft_table(&state, &request)?;
    let db = state.store.lock().map_err(map_store)?;
    if files.is_empty() {
        db.execute("DELETE FROM composer_draft_files WHERE loupist_id=?1 AND thread_id=?2", [request.loupist_id,request.thread_id])
    } else {
        db.execute("INSERT INTO composer_draft_files VALUES(?1,?2,?3) ON CONFLICT(loupist_id,thread_id) DO UPDATE SET files=excluded.files",
            [request.loupist_id,request.thread_id,serde_json::to_string(&files).map_err(map_internal)?])
    }.map_err(map_internal)?;
    Ok(())
}

#[tauri::command]
pub fn chat_draft_files_read(
    state: State<'_, AppState>,
    request: DraftIdentity,
) -> Result<Vec<DraftAttachment>, CommandError> {
    use rusqlite::OptionalExtension;
    draft_table(&state, &request)?;
    let saved: Option<String> = state
        .store
        .lock()
        .map_err(map_store)?
        .query_row(
            "SELECT files FROM composer_draft_files WHERE loupist_id=?1 AND thread_id=?2",
            [request.loupist_id, request.thread_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(map_internal)?;
    saved
        .map(|s| serde_json::from_str(&s).map_err(map_internal))
        .unwrap_or(Ok(vec![]))
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoredChatDraft {
    prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    context: Option<serde_json::Map<String, serde_json::Value>>,
}

#[tauri::command]
pub fn chat_draft_read(
    state: State<'_, AppState>,
    request: DraftIdentity,
) -> Result<Option<StoredChatDraft>, CommandError> {
    use rusqlite::OptionalExtension;
    draft_table(&state, &request)?;
    let row: Option<(String, Option<String>)> = state.store.lock().map_err(map_store)?
        .query_row(
            "SELECT prompt, context FROM composer_drafts LEFT JOIN composer_draft_context USING(loupist_id,thread_id) WHERE loupist_id=?1 AND thread_id=?2",
            [request.loupist_id, request.thread_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional().map_err(map_internal)?;
    row.map(|(prompt, context)| {
        Ok(StoredChatDraft {
            prompt,
            context: context
                .map(|json| serde_json::from_str(&json))
                .transpose()
                .map_err(map_internal)?,
        })
    })
    .transpose()
}

#[tauri::command]
pub fn chat_draft_write(
    state: State<'_, AppState>,
    request: DraftIdentity,
    draft: StoredChatDraft,
) -> Result<(), CommandError> {
    let context = draft
        .context
        .map(|value| serde_json::to_string(&value))
        .transpose()
        .map_err(map_internal)?;
    if draft.prompt.len() > 1024 * 1024
        || context.as_ref().is_some_and(|text| text.len() > 512 * 1024)
    {
        return Err(CommandError::invalid("draft is too long"));
    }
    draft_table(&state, &request)?;
    let mut connection = state.store.lock().map_err(map_store)?;
    let tx = connection.transaction().map_err(map_internal)?;
    let key = [&request.loupist_id, &request.thread_id];
    if draft.prompt.is_empty() && context.is_none() {
        tx.execute(
            "DELETE FROM composer_drafts WHERE loupist_id=?1 AND thread_id=?2",
            key,
        )
        .map_err(map_internal)?;
    } else {
        tx.execute("INSERT INTO composer_drafts VALUES (?1,?2,?3) ON CONFLICT(loupist_id,thread_id) DO UPDATE SET prompt=excluded.prompt", rusqlite::params![key[0],key[1],draft.prompt]).map_err(map_internal)?;
        if let Some(json) = context {
            tx.execute("INSERT INTO composer_draft_context VALUES (?1,?2,?3) ON CONFLICT(loupist_id,thread_id) DO UPDATE SET context=excluded.context", rusqlite::params![key[0],key[1],json]).map_err(map_internal)?;
        } else {
            tx.execute(
                "DELETE FROM composer_draft_context WHERE loupist_id=?1 AND thread_id=?2",
                key,
            )
            .map_err(map_internal)?;
        }
    }
    tx.commit().map_err(map_internal)
}

#[tauri::command(rename_all = "snake_case")]
pub fn workspace_ui_read(
    state: State<'_, AppState>,
) -> Result<Option<WorkspaceUiState>, CommandError> {
    read_state(&state)
}

#[tauri::command(rename_all = "snake_case")]
pub fn workspace_ui_write(
    request: WorkspaceUiWriteRequest,
    state: State<'_, AppState>,
) -> Result<WorkspaceUiState, CommandError> {
    write_state(&state, request.state)
}

fn read_state(state: &AppState) -> Result<Option<WorkspaceUiState>, CommandError> {
    let root = &state.artifacts.deletion_root;
    let relative = Path::new(DIRECTORY).join(FILE_NAME);
    if !root.entry_exists(&relative)? {
        return Ok(None);
    }
    let directory = root.open_private_subdirectory(Path::new(DIRECTORY))?;
    #[cfg(unix)]
    let mut file = {
        use rustix::fs::{openat, Mode, OFlags};
        File::from(
            openat(
                directory.file(),
                FILE_NAME,
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
                Mode::empty(),
            )
            .map_err(map_internal)?,
        )
    };
    #[cfg(windows)]
    let mut file =
        crate::fs::windows::open_atomic_snapshot_no_reparse(&root.absolute_path(&relative)?)
            .map_err(map_internal)?;
    #[cfg(not(any(unix, windows)))]
    let mut file = File::open(root.absolute_path(&relative)?).map_err(map_internal)?;
    let _ = directory;
    let metadata = file.metadata().map_err(map_internal)?;
    if !metadata.is_file() {
        return Err(invalid("stored preferences are not a regular file"));
    }
    // Unusable stored content (too large, malformed, an older shape or failing
    // validation) reads as absent, so the next write replaces it instead of the
    // panel never saving again. I/O failures still surface as errors.
    if metadata.len() > MAX_BYTES as u64 {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(map_internal)?;
    if bytes.len() > MAX_BYTES {
        return Ok(None);
    }
    let Ok(mut preferences) = serde_json::from_slice::<WorkspaceUiState>(&bytes) else {
        return Ok(None);
    };
    if validate_and_normalize(state, &mut preferences).is_err() {
        return Ok(None);
    }
    Ok(Some(preferences))
}

fn write_state(
    state: &AppState,
    mut preferences: WorkspaceUiState,
) -> Result<WorkspaceUiState, CommandError> {
    validate_and_normalize(state, &mut preferences)?;
    let bytes = serde_json::to_vec(&preferences).map_err(map_internal)?;
    let root = &state.artifacts.deletion_root;
    let relative = Path::new(DIRECTORY);
    let directory = root.ensure_private_subdirectory(relative)?;
    directory.write_atomic(&root.absolute_path(relative)?, FILE_NAME, &bytes, true)?;
    Ok(preferences)
}

fn validate_and_normalize(
    state: &AppState,
    preferences: &mut WorkspaceUiState,
) -> Result<(), CommandError> {
    if preferences.version != 1
        || !preferences.width.is_finite()
        || !(360.0..=1280.0).contains(&preferences.width)
        || !preferences.height.is_finite()
        || !(260.0..=720.0).contains(&preferences.height)
        || preferences.sessions.len() > MAX_SESSIONS
    {
        return Err(invalid("version, dimensions or session count is invalid"));
    }
    let mut session_ids = BTreeSet::new();
    for session in &mut preferences.sessions {
        text(&session.loupist_id, 256, false)?;
        text(&session.thread_id, 256, false)?;
        if !session_ids.insert((&session.loupist_id, &session.thread_id))
            || session.tabs.len() > MAX_TABS
        {
            return Err(invalid("session is duplicated or has too many tabs"));
        }
        session
            .tabs
            .retain(|tab| !matches!(tab, WorkspaceUiTab::Unsupported));
        let mut tab_ids = BTreeSet::new();
        for tab in &session.tabs {
            tab.validate(&session.loupist_id)?;
            if !tab_ids.insert(tab.id()) {
                return Err(invalid("tab is duplicated"));
            }
        }
        if let Some(id) = &session.active_tab_id {
            text(id, 8192, false)?;
        }
        if !session
            .active_tab_id
            .as_deref()
            .is_some_and(|id| tab_ids.contains(id))
        {
            session.active_tab_id = session.tabs.first().map(|tab| tab.id().to_owned());
        }
    }
    // Check the original request's aggregate size before removing stale owners.
    if serde_json::to_vec(preferences).map_err(map_internal)?.len() > MAX_BYTES {
        return Err(invalid("preferences exceed their storage limit"));
    }
    let mut live_sessions = Vec::with_capacity(preferences.sessions.len());
    for session in preferences.sessions.drain(..) {
        if state
            .store
            .get_loupist(&session.loupist_id)
            .map_err(map_store)?
            .is_some()
            && state
                .store
                .get_chat(&session.thread_id)
                .map_err(map_store)?
                .is_some_and(|chat| chat.loupist_id == session.loupist_id)
        {
            live_sessions.push(session);
        }
    }
    preferences.sessions = live_sessions;
    Ok(())
}

impl WorkspaceUiTab {
    fn id(&self) -> &str {
        match self {
            Self::Files { id, .. }
            | Self::Memory { id, .. }
            | Self::Work { id, .. }
            | Self::Browser { id, .. } => id,
            Self::Unsupported => "",
        }
    }

    fn validate(&self, loupist_id: &str) -> Result<(), CommandError> {
        text(self.id(), 8192, false)?;
        match self {
            Self::Unsupported => return Err(invalid("unsupported reader")),
            Self::Files {
                title,
                path,
                file_path,
                ..
            } => {
                text(title, 512, false)?;
                for path in std::iter::once(path).chain(file_path.iter()) {
                    text(path, 4096, true)?;
                    crate::fs::workspace_files::relative_path(path)?;
                }
            }
            Self::Memory {
                title,
                record_id,
                location,
                ..
            } => {
                text(title, 512, false)?;
                text(record_id, 256, false)?;
                location.validate(loupist_id)?;
            }
            Self::Work {
                artifact,
                artifact_thread_id,
                ..
            } => {
                if let Some(artifact) = artifact {
                    artifact.validate().map_err(map_internal)?;
                }
                if let Some(thread) = artifact_thread_id {
                    text(thread, 256, false)?;
                }
                if artifact.is_some() != artifact_thread_id.is_some() {
                    return Err(invalid("an open Work item needs its chat"));
                }
            }
            Self::Browser { title, url, .. } => {
                text(title, 512, false)?;
                text(url, 4096, true)?;
                if !url.is_empty() {
                    crate::web::browser::validated_http_url(url)?;
                }
            }
        }
        Ok(())
    }
}

impl WorkspaceMemoryLocation {
    fn validate(&self, loupist_id: &str) -> Result<(), CommandError> {
        if self.loupist_id != loupist_id {
            return Err(invalid("Memory belongs to another Loupist"));
        }
        optional_ids([&self.record_id, &self.revision_id, &self.object_id])?;
        if let Some(selection) = &self.selection {
            match selection {
                WorkspaceMemorySelection::Node { id } | WorkspaceMemorySelection::Claim { id } => {
                    text(id, 256, false)?
                }
                WorkspaceMemorySelection::Source {
                    source_id,
                    span_id,
                    world_record_id,
                    world_revision_id,
                } => optional_ids([source_id, span_id, world_record_id, world_revision_id])?,
                WorkspaceMemorySelection::Lens { record_id } => text(record_id, 256, false)?,
                WorkspaceMemorySelection::Research {
                    record_id,
                    section_id,
                } => {
                    text(record_id, 256, false)?;
                    optional_ids([section_id])?;
                }
                WorkspaceMemorySelection::Journal { journal_id } => text(journal_id, 256, false)?,
                WorkspaceMemorySelection::Snapshot { pin } => {
                    if pin.loupist_id != loupist_id {
                        return Err(invalid("Memory pin belongs to another Loupist"));
                    }
                    optional_ids([&pin.record_id, &pin.revision_id, &pin.span_id])?;
                    text(&pin.object_id, 256, false)?;
                    text(&pin.label, 512, false)?;
                }
            }
        }
        Ok(())
    }
}

fn optional_ids<'a>(
    values: impl IntoIterator<Item = &'a Option<String>>,
) -> Result<(), CommandError> {
    for value in values.into_iter().flatten() {
        text(value, 256, false)?;
    }
    Ok(())
}

fn text(value: &str, limit: usize, empty: bool) -> Result<(), CommandError> {
    if value.len() > limit
        || (!empty && value.trim().is_empty())
        || value.chars().any(char::is_control)
    {
        return Err(invalid(
            "text reference exceeds its limit or contains invalid characters",
        ));
    }
    Ok(())
}

fn invalid(reason: &str) -> CommandError {
    CommandError::invalid(format!("workspace preferences: {reason}"))
}

#[cfg(test)]
mod tests;
