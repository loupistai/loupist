use super::{source_presentation, source_read, KnowledgeSourceReadRequest};
use crate::app::{AppState, CommandError};
use crate::commands::{ChatKnowledgePin, SourceTextSelection};
use crate::knowledge::{KnowledgeWrite, RecordRevision, SourceRecordPayload};
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceSelectionRequest {
    pub source: KnowledgeSourceReadRequest,
    pub selection: SourceTextSelection,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SourceSelectionPin {
    pub loupist_id: String,
    #[serde(flatten)]
    pub pin: ChatKnowledgePin,
}

/// Resolve a renderer selection to its immutable, host-created containing span.
/// No new Source, revision, span or model read receipt is written.
#[tauri::command]
pub fn knowledge_source_selection_resolve(
    request: SourceSelectionRequest,
    state: State<'_, AppState>,
) -> Result<SourceSelectionPin, CommandError> {
    resolve(state.inner(), request)
}

pub(super) fn resolve(
    state: &AppState,
    request: SourceSelectionRequest,
) -> Result<SourceSelectionPin, CommandError> {
    if !bounded(&request.selection) {
        return Err(CommandError::invalid(
            "Source selection is invalid or too large",
        ));
    }
    let span_id = request
        .source
        .span_id
        .clone()
        .ok_or_else(|| CommandError::invalid("select a retained Source passage"))?;
    // Reuse exact revision, owner and historical World inspection admission.
    let loupist_id = request.source.loupist_id.clone();
    let detail = source_read(state, request.source)?;
    let span = detail
        .spans
        .iter()
        .find(|span| span.span_id == span_id)
        .ok_or_else(|| CommandError::invalid("Source passage is unavailable"))?;
    let text = span
        .presentation
        .as_ref()
        .map(|view| view.text.as_str())
        .or(span.text.as_deref())
        .ok_or_else(|| CommandError::invalid("Source text is unavailable"))?;
    if !matches_text(text, &request.selection) {
        return Err(CommandError::invalid(
            "Source selection does not match the retained passage",
        ));
    }
    Ok(SourceSelectionPin {
        loupist_id,
        pin: ChatKnowledgePin {
            kind: "source".into(),
            record_id: Some(detail.record_id),
            revision_id: Some(detail.revision_id),
            object_id: detail.source_id,
            span_id: Some(span_id),
            label: detail.title,
            selection: Some(request.selection),
        },
    })
}

pub(super) fn bounded(selection: &SourceTextSelection) -> bool {
    selection.start_utf16 < selection.end_utf16
        && !selection.text.trim().is_empty()
        && selection.text.len() <= 8_000
}

/// DOM ranges count UTF-16 code units; reject cuts through surrogate pairs.
fn matches_text(text: &str, selection: &SourceTextSelection) -> bool {
    if !bounded(selection) {
        return false;
    }
    let mut units = 0;
    let mut start = None;
    let mut end = None;
    for (byte, character) in text.char_indices() {
        if units == selection.start_utf16 {
            start = Some(byte);
        }
        if units == selection.end_utf16 {
            end = Some(byte);
            break;
        }
        units += character.len_utf16() as u32;
    }
    if units == selection.end_utf16 && end.is_none() {
        end = Some(text.len());
    }
    match (start, end) {
        (Some(start), Some(end)) => text.get(start..end) == Some(selection.text.as_str()),
        _ => false,
    }
}

/// Revalidate the typed selection at Chat admission; renderer fields are intent.
pub(super) fn matches_revision(
    write: &KnowledgeWrite<'_>,
    revision: &RecordRevision,
    span_id: &str,
    selection: &SourceTextSelection,
) -> Option<()> {
    if !bounded(selection) {
        return None;
    }
    let payload: SourceRecordPayload = serde_json::from_value(revision.payload.clone()).ok()?;
    let source = payload.source;
    for capture in source.captures {
        if !capture.coverage.retains_bytes() {
            continue;
        }
        for representation in capture.representations {
            let Some(span) = representation
                .spans
                .iter()
                .find(|span| span.span_id == span_id)
            else {
                continue;
            };
            let (bytes, media_type) = write
                .read_revision_span_bytes(&revision.record_id, span_id, &span.selected_digest)
                .ok()?;
            if !(media_type.starts_with("text/")
                || media_type == "application/json"
                || media_type.ends_with("+json"))
            {
                return None;
            }
            let exact = String::from_utf8(bytes).ok()?;
            let presentation = source_presentation::source_presentation(
                source.source_class,
                representation.role,
                &media_type,
                &exact,
            );
            let text = presentation
                .as_ref()
                .map(|view| view.text.as_str())
                .unwrap_or(&exact);
            return matches_text(text, selection).then_some(());
        }
    }
    None
}

#[cfg(test)]
mod tests;
