use super::*;
use crate::knowledge::{KnowledgeWrite, WorkingNote};

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkingNoteReadRequest {
    pub loupist_id: String,
    pub note_id: String,
}

#[tauri::command]
pub fn knowledge_working_note_read(
    request: WorkingNoteReadRequest,
    state: State<'_, AppState>,
) -> Result<WorkingNote, CommandError> {
    read_working_context(&state, &request.loupist_id, &request.note_id)
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkingNotesRequest {
    pub loupist_id: String,
    pub chat_id: String,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct WorkingNotesPage {
    pub current: Option<WorkingNote>,
}

fn admit_chat(state: &AppState, loupist_id: &str, chat_id: &str) -> Result<(), CommandError> {
    admit(state, loupist_id)?;
    let chat = state
        .store
        .get_chat(chat_id)
        .map_err(|error| CommandError::internal(error.to_string()))?
        .ok_or_else(|| CommandError::not_found("conversation"))?;
    if chat.loupist_id != loupist_id {
        return Err(CommandError::new(
            "unauthorized",
            "conversation belongs to another Loupist",
        ));
    }
    Ok(())
}

#[tauri::command]
pub fn knowledge_working_notes(
    request: WorkingNotesRequest,
    state: State<'_, AppState>,
) -> Result<WorkingNotesPage, CommandError> {
    working_notes_page(&state, request)
}

pub(super) fn working_notes_page(
    state: &AppState,
    request: WorkingNotesRequest,
) -> Result<WorkingNotesPage, CommandError> {
    admit_chat(state, &request.loupist_id, &request.chat_id)?;
    let notes = with_loupist_knowledge(state, &request.loupist_id, |tx, blobs| {
        KnowledgeWrite::bind(tx, blobs).working_notes(&request.loupist_id, &request.chat_id, 1)
    })?;
    Ok(WorkingNotesPage {
        current: notes.first().cloned(),
    })
}

pub(in crate::commands) fn validate_working_note_revision(
    state: &AppState,
    loupist_id: &str,
    chat_id: &str,
    expected: Option<&str>,
) -> Result<(), CommandError> {
    let current = with_loupist_knowledge(state, loupist_id, |tx, blobs| {
        KnowledgeWrite::bind(tx, blobs).working_notes(loupist_id, chat_id, 1)
    })?;
    if current.first().map(|note| note.research_id.as_str()) != expected {
        // This validator is used by note_save only; UI conflict copy stays in the store.
        let guidance = match current.first() {
            Some(note) => format!(
                "workingNote.expectedResearchId does not match this Chat's current working note, {}; pass that current id to update it. To supersede or extend an earlier Chat's note, keep the earlier note id in previousNoteIds (after reading it with conversation_memory).",
                note.research_id.chars().take(128).collect::<String>()
            ),
            None => "The supplied workingNote.expectedResearchId is not this Chat's current working note: this Chat has no current note. Pass null in workingNote.expectedResearchId to start one, and keep an earlier Chat's note id in previousNoteIds to supersede or extend it (after reading it with conversation_memory).".into(),
        };
        return Err(CommandError::conflict(guidance));
    }
    Ok(())
}

pub(in crate::commands) fn current_working_context(
    state: &AppState,
    loupist_id: &str,
    chat_id: &str,
) -> Result<Option<WorkingNote>, CommandError> {
    admit_chat(state, loupist_id, chat_id)?;
    with_loupist_knowledge(state, loupist_id, |tx, blobs| {
        KnowledgeWrite::bind(tx, blobs).working_notes(loupist_id, chat_id, 1)
    })
    .map(|notes| notes.into_iter().next())
}

pub(in crate::commands) fn attach_working_context(
    envelope: &str,
    current: Option<&WorkingNote>,
) -> String {
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(envelope) else {
        return envelope.to_string();
    };
    let Some(protocol) = value
        .get_mut("memory_protocol")
        .and_then(serde_json::Value::as_object_mut)
    else {
        return envelope.to_string();
    };
    if protocol.get("active").and_then(serde_json::Value::as_bool) != Some(true) {
        return envelope.to_string();
    }
    // A dated task summary is context, never an independent source or a tool citation.
    protocol.insert(
        "workingNote".into(),
        serde_json::json!({
            "current": current, "notEvidence": true,
        }),
    );
    serde_json::to_string(&value).unwrap_or_else(|_| envelope.to_string())
}

pub(in crate::commands) fn read_working_context(
    state: &AppState,
    loupist_id: &str,
    research_id: &str,
) -> Result<WorkingNote, CommandError> {
    admit(state, loupist_id)?;
    with_loupist_knowledge(state, loupist_id, |tx, blobs| {
        KnowledgeWrite::bind(tx, blobs).read_working_note(loupist_id, research_id)
    })
}

#[cfg(test)]
pub(in crate::commands) fn attach_journal_context(
    state: &AppState,
    loupist_id: &str,
    envelope: &str,
) -> Result<String, CommandError> {
    attach_journal_context_as_of(state, loupist_id, envelope, None).map(|(text, _)| text)
}

/// Adds due Journal entries and the outcome record to the envelope. Under a
/// cutoff, entries written later are left out (and returned as left out) and
/// a later outcome reads as pending.
pub(in crate::commands) fn attach_journal_context_as_of(
    state: &AppState,
    loupist_id: &str,
    envelope: &str,
    cutoff: Option<super::as_of::Cutoff>,
) -> Result<(String, super::as_of::LeftOut), CommandError> {
    let mut left = super::as_of::LeftOut::default();
    let mut value: serde_json::Value =
        serde_json::from_str(envelope).map_err(|e| CommandError::internal(e.to_string()))?;
    let Some(protocol) = value
        .get_mut("memory_protocol")
        .and_then(serde_json::Value::as_object_mut)
    else {
        return Ok((envelope.into(), left));
    };
    if protocol.get("active").and_then(serde_json::Value::as_bool) != Some(true) {
        return Ok((envelope.into(), left));
    }
    let today = cutoff.map_or_else(
        || chrono::Utc::now().format("%Y-%m-%d").to_string(),
        |cutoff| cutoff.day(),
    );
    let store = crate::knowledge::KnowledgeStore::attach_loupist(
        state.store.clone(),
        knowledge_blobs_for_loupist(state, loupist_id)?,
        loupist_id,
    );
    let mut known = |record: crate::knowledge::JournalRecord| match cutoff {
        None => Some(record),
        Some(cutoff) => {
            let id = record.journal_id.clone();
            let kept = super::as_of::journal_known(record, cutoff);
            if kept.is_none() {
                left.notes.insert(id);
            }
            kept
        }
    };
    let context = store
        .read(|read| read.journal_context(loupist_id, &today, &mut known))
        .map_err(map_knowledge)?;
    if let Some(context) = context.as_object() {
        protocol.extend(context.clone());
    }
    Ok((value.to_string(), left))
}

#[cfg(test)]
mod tests;
