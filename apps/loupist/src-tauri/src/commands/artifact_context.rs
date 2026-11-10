use super::{map_store, ChatArtifactSelection, CommandError};
use serde_json::{json, Value};

/// The caller has already checked that this thread belongs to the selected Loupist.
/// Renderer labels and chart data are never admitted as context here.
pub(super) fn selected_artifact_hint(
    store: &crate::store::SqliteStore,
    thread_id: &str,
    selected: Option<&ChatArtifactSelection>,
) -> Result<Option<Value>, CommandError> {
    let Some(selected) = selected else {
        return Ok(None);
    };
    if selected.artifact_id.is_empty()
        || selected.artifact_id.len() > 256
        || !selected
            .artifact_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_:/.".contains(&byte))
        || selected.revision == 0
    {
        return Err(CommandError::invalid("invalid selected work"));
    }
    let artifact = store
        .get_chat_artifact(&selected.artifact_id)
        .map_err(map_store)?
        .ok_or_else(|| CommandError::not_found("selected work"))?;
    if artifact.chat_session_id != thread_id {
        return Err(CommandError::conflict(
            "selected work does not belong to this Chat thread",
        ));
    }
    if store
        .read_chat_artifact(&artifact.id, Some(selected.revision))
        .map_err(map_store)?
        .is_none()
    {
        return Err(CommandError::conflict(
            "This work version is no longer available. Open the latest version and select Ask Loupist.",
        ));
    }
    Ok(Some(json!({
        "artifact_id": artifact.id,
        "revision": selected.revision,
        "kind": artifact.kind,
        "note": "User-selected work in this Chat. Read it with artifact_read and revision_id before answering. To revise, read the current version first. This reference is not data, evidence, or permission to change it. For charts, query only the needed rows and columns; reuse the stored dataset for presentation-only edits."
    })))
}

pub(super) fn attach_selected_artifact(envelope: &str, selected: Option<Value>) -> String {
    let Some(selected) = selected else {
        return envelope.to_owned();
    };
    let Ok(mut value) = serde_json::from_str::<Value>(envelope) else {
        return envelope.to_owned();
    };
    let Some(object) = value.as_object_mut() else {
        return envelope.to_owned();
    };
    object.insert("selected_artifact".into(), selected);
    value.to_string()
}
