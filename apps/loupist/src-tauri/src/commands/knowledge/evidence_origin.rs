//! Provenance of an exact Memory read, projected from the retained revisions.
use super::*;
use crate::{
    commands::types::{MemoryEvidenceDate, MemoryEvidenceDateKind, MemoryEvidenceOrigin},
    domain::{ChatEvidenceKind, ChatEvidenceRef, MemoryAccess, MemoryRefSnapshot},
    knowledge::{
        lens_page::{BasisRef, Page},
        Authorship, KnowledgeWrite, PublishedCapture, RecordRevision, SourceRecordPayload,
        WorldGraphView,
    },
};
use serde_json::Value;

fn origin(route: RecordKind, saved_at_ms: i64) -> MemoryEvidenceOrigin {
    MemoryEvidenceOrigin {
        route,
        publisher: None,
        publication: None,
        saved_at: crate::commands::iso_time(saved_at_ms).ok(),
    }
}

struct BackingCapture {
    record_id: String,
    capture_id: String,
    origin: MemoryEvidenceOrigin,
}

fn backing_capture(
    write: &KnowledgeWrite<'_>,
    revision: &RecordRevision,
    span_id: &str,
) -> Option<BackingCapture> {
    if revision.kind != RecordKind::Source {
        return None;
    }
    let source = serde_json::from_value::<SourceRecordPayload>(revision.payload.clone())
        .ok()?
        .source;
    let (capture, span) = source.captures.iter().find_map(|capture| {
        capture.representations.iter().find_map(|representation| {
            let span = representation
                .spans
                .iter()
                .find(|span| span.span_id == span_id)?;
            Some((capture, span))
        })
    })?;

    let mut origin = origin(RecordKind::Source, capture.received_at_ms);
    // Imported statements and user-provided labels do not become publishers.
    // A custom connection has its own host-saved identity.
    if capture.provided.is_none()
        && (capture.authorship == Authorship::Publisher || source.origin.is_some())
    {
        origin.publisher = Some(source.provider_identity);
    }
    if capture.authorship == Authorship::Publisher && capture.provided.is_none() {
        origin.publication = publisher_date(
            write,
            revision,
            capture,
            span_id,
            &span.selected_digest,
            origin.publisher.as_deref(),
        );
    }

    Some(BackingCapture {
        record_id: revision.record_id.clone(),
        capture_id: capture.capture_id.clone(),
        origin,
    })
}

fn publisher_date(
    write: &KnowledgeWrite<'_>,
    revision: &RecordRevision,
    capture: &PublishedCapture,
    span_id: &str,
    digest: &str,
    provider: Option<&str>,
) -> Option<MemoryEvidenceDate> {
    let time = capture.publication_time.as_ref()?;
    let day = time.value.as_deref()?;
    // The World Bank connector documents a series update date, and the
    // immutable provider payload lets us recognize it without guessing.
    let updated = provider == Some(crate::finance_data::WORLD_BANK_SOURCE_ID)
        && write
            .read_revision_span_bytes(&revision.record_id, span_id, digest)
            .ok()
            .and_then(|(bytes, _)| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .is_some_and(|value| {
                value
                    .pointer("/data/series/provider_updated_at")
                    .and_then(Value::as_str)
                    == Some(day)
            });
    Some(MemoryEvidenceDate {
        kind: if updated {
            MemoryEvidenceDateKind::Updated
        } else {
            MemoryEvidenceDateKind::Published
        },
        time: time.clone(),
    })
}

/// A read can be attributed to a capture when every backing reference points
/// to that capture. Keep the Memory route when the read has mixed evidence.
fn single_capture(
    items: impl IntoIterator<Item = Option<BackingCapture>>,
    route: RecordKind,
) -> Option<MemoryEvidenceOrigin> {
    let mut items = items.into_iter();
    let mut first = items.next()??;
    for next in items {
        let next = next?;
        if next.record_id != first.record_id || next.capture_id != first.capture_id {
            return None;
        }
    }
    first.origin.route = route;
    Some(first.origin)
}

fn world_origin(
    write: &KnowledgeWrite<'_>,
    loupist_id: &str,
    revision: &RecordRevision,
    text: &str,
) -> Option<MemoryEvidenceOrigin> {
    let view: WorldGraphView = serde_json::from_str(text).ok()?;
    if view.record_id != revision.record_id
        || view.revision_id != revision.revision_id
        || view.revision_digest != revision.revision_digest
        || view.edges.iter().any(|edge| edge.claim.evidence.is_empty())
    {
        return None;
    }
    let spans: Vec<_> = view
        .edges
        .iter()
        .flat_map(|edge| &edge.claim.evidence)
        .map(|evidence| evidence.span_id.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let targets = write
        .world_source_targets(
            loupist_id,
            &revision.record_id,
            &revision.revision_id,
            &spans,
        )
        .ok()?;
    single_capture(
        spans.iter().map(|span| {
            let target = targets.get(span)?;
            let revision = write.get_revision(loupist_id, &target.revision_id).ok()?;
            backing_capture(write, &revision, &target.span_id)
        }),
        RecordKind::World,
    )
}

fn lens_origin(
    write: &KnowledgeWrite<'_>,
    loupist_id: &str,
    revision: &RecordRevision,
) -> Option<MemoryEvidenceOrigin> {
    // The immutable Lens revision retains the exact Source span and revision
    // digest for each direct Source basis. An undated Claim or another Lens
    // does not pin a Source revision here.
    let payload = revision.payload.get("page").unwrap_or(&revision.payload);
    let page = serde_json::from_value::<Page>(payload.clone()).ok()?;
    if page
        .sections
        .iter()
        .any(|section| section.basis.refs.is_empty())
    {
        return None;
    }
    single_capture(
        page.sections
            .iter()
            .flat_map(|section| &section.basis.refs)
            .map(|basis| {
                let BasisRef::Span {
                    record_id,
                    revision_digest,
                    span_id,
                } = basis
                else {
                    return None;
                };
                let revision = write
                    .visible_revision_by_digest(loupist_id, record_id, revision_digest)
                    .ok()?;
                backing_capture(write, &revision, span_id)
            }),
        RecordKind::Lens,
    )
}

pub(super) fn exact_read(
    write: &KnowledgeWrite<'_>,
    loupist_id: &str,
    revision: &RecordRevision,
    span_id: Option<&str>,
    world_text: Option<&str>,
) -> MemoryEvidenceOrigin {
    match revision.kind {
        RecordKind::Source => span_id
            .and_then(|span| backing_capture(write, revision, span))
            .map(|capture| capture.origin),
        RecordKind::World => {
            world_text.and_then(|text| world_origin(write, loupist_id, revision, text))
        }
        RecordKind::Lens => lens_origin(write, loupist_id, revision),
    }
    .unwrap_or_else(|| origin(revision.kind, revision.created_at_ms))
}

/// Recover a renderer projection for a saved turn from its exact read's
/// existing revision references. A truncated World view cannot prove which
/// subset was delivered, so its attribution stays at the route.
pub(super) fn stored_read(
    write: &KnowledgeWrite<'_>,
    loupist_id: &str,
    evidence: &ChatEvidenceRef,
    memory: &MemoryRefSnapshot,
) -> Option<MemoryEvidenceOrigin> {
    if evidence.kind != ChatEvidenceKind::SavedContext
        || memory.access != MemoryAccess::ExactRead
        || evidence.read_ref.as_deref() != Some(memory.read_ref.as_str())
        || !matches!(evidence.tool_name.as_str(), "world_read" | "lens_read")
    {
        return None;
    }
    let revision = write.get_revision(loupist_id, &memory.revision_id).ok()?;
    if revision.record_id != memory.record_id || revision.kind.as_str() != memory.kind {
        return None;
    }
    let world_text = (!evidence.excerpt_truncated).then(|| {
        crate::custom_connections::taint::delimited_body(&evidence.excerpt)
            .unwrap_or(&evidence.excerpt)
    });
    Some(exact_read(
        write,
        loupist_id,
        &revision,
        memory.span_id.as_deref(),
        world_text,
    ))
}

fn saved_data_record(
    write: &KnowledgeWrite<'_>,
    loupist_id: &str,
    evidence: &ChatEvidenceRef,
    read: &MemoryRefSnapshot,
) -> Option<Value> {
    if evidence.tool_name != "world_read"
        || read.kind != "source"
        || read.receipt_id.as_deref() != Some(read.read_ref.as_str())
        || !evidence.excerpt.trim_start().starts_with('{')
    {
        return None;
    }
    let span_id = read.span_id.as_deref()?;
    let revision = write.get_revision(loupist_id, &read.revision_id).ok()?;
    let source = serde_json::from_value::<SourceRecordPayload>(revision.payload)
        .ok()?
        .source;
    if revision.record_id != read.record_id
        || source.source_id != read.object_id
        || source.source_class != crate::knowledge::SourceClass::ToolResult
    {
        return None;
    }
    let span = source
        .captures
        .iter()
        .filter(|capture| capture.coverage.retains_bytes())
        .flat_map(|capture| &capture.representations)
        .filter(|rep| {
            matches!(
                rep.role,
                crate::knowledge::RepresentationRole::ProviderPayload
                    | crate::knowledge::RepresentationRole::Excerpt
                    | crate::knowledge::RepresentationRole::Structured
            )
        })
        .flat_map(|rep| &rep.spans)
        .find(|span| span.span_id == span_id)?;
    let (bytes, media_type) = write
        .read_revision_span_bytes(&read.record_id, span_id, &span.selected_digest)
        .ok()?;
    if media_type != "application/json"
        || evidence.excerpt.is_empty()
        || !bytes.starts_with(evidence.excerpt.as_bytes())
    {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

fn project_message(
    write: &KnowledgeWrite<'_>,
    loupist_id: &str,
    message: &crate::domain::ChatMessage,
    evidence: &mut [crate::commands::types::ChatEvidenceViewDto],
    mut checks: Option<&mut Vec<crate::domain::NumberCheck>>,
) {
    let memory: std::collections::BTreeMap<&str, &MemoryRefSnapshot> = message
        .memory_refs
        .iter()
        .filter(|item| item.access == MemoryAccess::ExactRead)
        .map(|item| (item.read_ref.as_str(), item))
        .collect();
    for item in evidence {
        let Some(read) = item
            .snapshot
            .read_ref
            .as_deref()
            .and_then(|key| memory.get(key))
        else {
            continue;
        };
        item.memory_origin = stored_read(write, loupist_id, &item.snapshot, read);
        if item.memory_origin.is_some() {
            item.snapshot.title = read.title.clone();
            if let Some(checks) = checks.as_deref_mut() {
                use crate::commands::tool_executor::number_checks::SavedFigureChecks;
                use crate::datasets::saved_rows::SavedRows;
                if let Some(record) = saved_data_record(write, loupist_id, &item.snapshot, read) {
                    if let Some(rows) = SavedRows::parse(&record) {
                        SavedFigureChecks::new(&rows).project(
                            &message.content,
                            &item.snapshot.id,
                            checks,
                        );
                    }
                }
            }
        }
    }
}

pub(in crate::commands) fn project_chat_memory(
    state: &AppState,
    chat: &crate::domain::ChatSession,
    dto: &mut crate::commands::types::ChatThreadDto,
) {
    if !chat.messages.iter().any(|message| {
        !message.memory_refs.is_empty()
            && message
                .evidence_refs
                .iter()
                .any(|item| item.read_ref.is_some())
    }) {
        return;
    }
    let _ = with_loupist_knowledge(state, &chat.loupist_id, |tx, blobs| {
        let write = KnowledgeWrite::bind(tx, blobs);
        for (message, rendered) in chat.messages.iter().zip(&mut dto.messages) {
            project_message(
                &write,
                &chat.loupist_id,
                message,
                &mut rendered.evidence_refs,
                Some(&mut rendered.number_checks),
            );
        }
        Ok(())
    });
}

pub(in crate::commands) fn project_artifact_memory(
    state: &AppState,
    chat: &crate::domain::ChatSession,
    dto: &mut crate::commands::types::ChatArtifactViewDto,
) {
    let files = crate::chat::local_files::FileLabels::for_chat(chat);
    for item in &mut dto.evidence_refs {
        files.project_evidence(&mut item.snapshot);
    }
    if !dto
        .evidence_refs
        .iter()
        .any(|item| item.snapshot.read_ref.is_some())
    {
        return;
    }
    let Some(message) = chat
        .messages
        .iter()
        .chain(chat.versions.iter().flat_map(|version| &version.messages))
        .find(|message| message.id == dto.revision.source_message_id)
    else {
        return;
    };
    let _ = with_loupist_knowledge(state, &chat.loupist_id, |tx, blobs| {
        project_message(
            &KnowledgeWrite::bind(tx, blobs),
            &chat.loupist_id,
            message,
            &mut dto.evidence_refs,
            None,
        );
        Ok(())
    });
}
