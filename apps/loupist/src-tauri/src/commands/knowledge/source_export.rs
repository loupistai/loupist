use super::{admit, source_read, with_loupist_knowledge, KnowledgeSourceReadRequest};
use crate::app::{AppState, CommandError};
use crate::knowledge::{KnowledgeError, RecordKind, SourceRecordPayload};
use tauri::State;

/// Export an exact retained span through a native Save dialog. No caller-supplied
/// filesystem path and no automatic execution of retained original files.
#[tauri::command(rename_all = "snake_case")]
pub async fn knowledge_source_export(
    state: State<'_, AppState>,
    request: KnowledgeSourceReadRequest,
) -> Result<Option<String>, CommandError> {
    let (bytes, media_type, title) = export_bytes(state.inner(), &request)?;
    let extension = match media_type.as_str() {
        "application/pdf" => ".pdf",
        "application/json" => ".json",
        "text/plain" => ".txt",
        "text/markdown" => ".md",
        "image/png" => ".png",
        "image/jpeg" => ".jpg",
        _ => "",
    };
    let name: String = title
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '/' | '\\' | ':'))
        .take(120)
        .collect();
    let name = if name.trim().is_empty() {
        "source".to_string()
    } else {
        name
    };
    let name = if name.to_ascii_lowercase().ends_with(extension) {
        name
    } else {
        format!("{name}{extension}")
    };
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_title("Export retained source")
        .set_file_name(name)
        .save_file()
        .await
    else {
        return Ok(None);
    };
    // Recheck the Loupist after the dialog, which can remain open during deletion.
    admit(state.inner(), &request.loupist_id)?;
    ensure_export_location(state.inner(), file.path())?;
    let authorship = super::source_span_authorship(
        state.inner(),
        &request.loupist_id,
        &request.record_id,
        Some(&request.revision_id),
        request.span_id.as_deref().unwrap_or_default(),
    )?;
    if authorship.is_empty()
        || authorship.iter().any(|span| {
            span.untrusted(state.inner(), &request.loupist_id)
                || span.authorship == crate::knowledge::Authorship::ImportedContributor
        })
    {
        crate::commands::export_trust::record(state.inner(), &bytes)?;
    }
    file.write(&bytes)
        .await
        .map_err(|error| CommandError::internal(error.to_string()))?;
    Ok(Some(file.path().display().to_string()))
}

fn ensure_export_location(state: &AppState, path: &std::path::Path) -> Result<(), CommandError> {
    let parent = path
        .parent()
        .ok_or_else(|| CommandError::invalid("file has no parent"))?
        .canonicalize()
        .map_err(|error| CommandError::invalid(error.to_string()))?;
    if matches!(std::fs::symlink_metadata(path), Ok(metadata) if metadata.file_type().is_symlink())
    {
        return Err(CommandError::invalid("Choose a regular export file."));
    }
    if parent.starts_with(&state.artifacts.app_data_dir) {
        return Err(CommandError::invalid(
            "Choose a location outside Loupist's private storage.",
        ));
    }
    if state.store.is_folder_catalog() {
        for profile in state
            .store
            .list_loupists()
            .map_err(crate::commands::map_store)?
        {
            if parent.starts_with(std::path::Path::new(&profile.workspace_root).join(".loupist")) {
                return Err(CommandError::invalid(
                    "Choose a location outside the Loupist's .loupist data directory.",
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn export_bytes(
    state: &AppState,
    request: &KnowledgeSourceReadRequest,
) -> Result<(Vec<u8>, String, String), CommandError> {
    // Shares renderer inspection authority, including World -> historical Source binding.
    let detail = source_read(
        state,
        KnowledgeSourceReadRequest {
            loupist_id: request.loupist_id.clone(),
            record_id: request.record_id.clone(),
            revision_id: request.revision_id.clone(),
            source_id: request.source_id.clone(),
            span_id: request.span_id.clone(),
            purpose: request.purpose.clone(),
            world_record_id: request.world_record_id.clone(),
            world_revision_id: request.world_revision_id.clone(),
        },
    )?;
    let span_id = request
        .span_id
        .as_deref()
        .ok_or_else(|| CommandError::invalid("Choose a retained source span to export."))?;
    with_loupist_knowledge(state, &request.loupist_id, |tx, blobs| {
        let write = crate::knowledge::KnowledgeWrite::bind(tx, blobs);
        let revision = write.get_revision(&request.loupist_id, &request.revision_id)?;
        if revision.record_id != request.record_id || revision.kind != RecordKind::Source {
            return Err(KnowledgeError::Unauthorized(
                "revision belongs to another record",
            ));
        }
        let payload: SourceRecordPayload = serde_json::from_value(revision.payload)?;
        let source = &payload.source;
        if source.source_id != detail.source_id {
            return Err(KnowledgeError::NotFound);
        }
        for capture in &source.captures {
            for representation in &capture.representations {
                if let Some(span) = representation
                    .spans
                    .iter()
                    .find(|span| span.span_id == span_id)
                {
                    if !capture.coverage.retains_bytes() {
                        return Err(KnowledgeError::MetadataOnly);
                    }
                    let (bytes, media_type) = write.read_revision_span_bytes(
                        &request.record_id,
                        span_id,
                        &span.selected_digest,
                    )?;
                    return Ok((bytes, media_type, detail.title));
                }
            }
        }

        Err(KnowledgeError::NotFound)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_exports_allow_working_files_and_protect_all_registered_loupist_data() {
        let temp = tempfile::tempdir().unwrap();
        let state = AppState::for_folder_test(temp.path().join("app"));
        for id in ["loupist-a", "loupist-b"] {
            let folder = temp.path().join(id);
            std::fs::create_dir(&folder).unwrap();
            let profile = crate::commands::tests::profile(id, &folder, 10);
            state.store.create_loupist(&profile).unwrap();
            assert!(ensure_export_location(&state, &folder.join("report.md")).is_ok());
            assert!(
                ensure_export_location(&state, &folder.join(".loupist/loupist.sqlite3")).is_err()
            );
        }
        assert!(ensure_export_location(
            &state,
            &state.artifacts.app_data_dir.join("catalog.sqlite3")
        )
        .is_err());
    }
}
