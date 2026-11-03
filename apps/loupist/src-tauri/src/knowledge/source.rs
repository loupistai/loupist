use rusqlite::{params, OptionalExtension, Transaction};
use serde_json::Value;

use super::authority::{require_edit, require_record, revision_in_record, OwnedRecordState};
use super::blob::BlobStore;
use super::canonical::{canonical_string, digest_bytes, from_json, to_value};
use super::error::{KnowledgeError, KnowledgeResult};
#[cfg(test)]
use super::model::SourceSummary;
use super::model::{
    mint_id, validate_external_id, validate_knowledge_id, validate_loupist_id, validate_media_type,
    validate_optional_digest, validate_optional_knowledge_id, validate_text, validate_time,
    validate_time_point, validate_title, validate_token, Authorship, Coverage, PublishedCapture,
    PublishedRepresentation, PublishedSource, PublishedSpan, ReadPurpose, RecordKind,
    RepresentationRole, RetainSource, RetainedSource, Selector, SourceClass, SourceReadReceipt,
    SourceRecordPayload, MAX_BLOB_BYTES,
};
#[cfg(feature = "eval-harness")]
use super::model::{ReadCapture, ReadSpan, SpanRead};
use super::revision::{publish_revision, StoredRevision};
use crate::util::json_pointer::valid_json_pointer;

/// The app reads only `record_id`; exact span reads (evals and tests) use the rest.
#[derive(Debug, Clone)]
#[cfg_attr(not(feature = "eval-harness"), allow(dead_code))]
pub(super) struct VisibleSpan {
    pub span_id: String,
    pub capture_id: String,
    pub representation_id: String,
    pub selector_json: String,
    pub selected_digest: String,
    pub coverage: Coverage,
    pub document_id: String,
    pub representation_digest: String,
    pub media_type: String,
    pub role: RepresentationRole,
    pub record_id: String,
}

#[cfg(any(test, feature = "eval-harness", feature = "e2e"))]
pub(super) fn retain_source(
    tx: &Transaction<'_>,
    blobs: &BlobStore,
    request: RetainSource,
) -> KnowledgeResult<RetainedSource> {
    retain_capture(tx, blobs, request, None, None)
}

fn retain_capture(
    tx: &Transaction<'_>,
    blobs: &BlobStore,
    request: RetainSource,
    acquisition_identity: Option<&str>,
    origin_message_id: Option<&str>,
) -> KnowledgeResult<RetainedSource> {
    validate_retention(&request)?;
    if let Some(origin) = &request.origin {
        if origin.kind != "custom_connection"
            || origin.name.is_empty()
            || origin.name.len() > 512
            || !crate::custom_connections::service::valid_id(&origin.connection_id)
            || request.provider_identity.split(':').next() != Some(origin.connection_id.as_str())
            || request.source_class != SourceClass::ToolResult
        {
            return Err(KnowledgeError::Invalid("invalid custom connection origin"));
        }
    }
    let origin_json = request
        .origin
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    let module = require_record(tx, &request.loupist_id, &request.record_id)?;
    if module.kind != super::model::RecordKind::Source {
        return Err(KnowledgeError::Invalid("record is not a Source"));
    }
    require_edit(&module, &request.loupist_id)?;
    if let Some(material) = &request.material {
        if let Some(parent) = &material.parent_representation_id {
            authorize_representation(tx, &request.loupist_id, parent)?;
        }
        for span_id in &material.input_span_ids {
            require_any_visible_span(tx, &request.loupist_id, span_id)?;
        }
    }
    let identical = has_identical_capture(tx, &request, module.head_revision_id.as_deref())?;
    let (public_locator, private_locator) = split_locator(&request.origin_locator)?;
    let publication_time_json = publication_json(&request.publication_time)?;
    let source_id = match &request.source_id {
        Some(source_id) => {
            let exists: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM knowledge_sources WHERE record_id = ?1 AND source_id = ?2)",
                    params![module.record_id, source_id],
                    |row| row.get(0),
                )?;
            if !exists {
                return Err(KnowledgeError::NotFound);
            }
            source_id.clone()
        }
        None => {
            let populated: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM knowledge_sources WHERE record_id = ?1)",
                [&module.record_id],
                |row| row.get(0),
            )?;
            if populated {
                return Err(KnowledgeError::Invalid(
                    "a source record already has its original source; revise its capture instead",
                ));
            }
            let source_id = mint_id("src");
            tx.execute(
                "INSERT INTO knowledge_sources (
                    source_id, record_id, title, source_class, origin_locator,
                    provider_identity, updated_at_ms, origin_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    source_id,
                    module.record_id,
                    request.title,
                    request.source_class.as_str(),
                    public_locator.clone(),
                    request.provider_identity,
                    request.at_ms,
                    origin_json
                ],
            )?;
            source_id
        }
    };
    let capture_id = mint_id("cap");
    let prepared = match &request.material {
        None => None,
        Some(material) => {
            let digest = blobs.store(tx, &material.bytes, &material.media_type, request.at_ms)?;
            let selected =
                select_bytes(&material.bytes, &material.selector, &material.document_id)?;
            Some((
                digest,
                digest_bytes(&selected),
                mint_id("rep"),
                mint_id("spn"),
            ))
        }
    };
    let (material_digest, representation_id, span_id, selected_digest) = match &prepared {
        None => (None, None, None, None),
        Some((digest, selected_digest, representation_id, span_id)) => (
            Some(digest.clone()),
            Some(representation_id.clone()),
            Some(span_id.clone()),
            Some(selected_digest.clone()),
        ),
    };
    tx.execute(
        "INSERT INTO knowledge_source_captures (
            record_id, capture_id, source_id, received_at_ms, coverage, declared_digest,
            material_digest, authorship, created_at_ms, publication_time_json, acquisition_identity,
            origin_locator, provider_identity
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            module.record_id,
            capture_id,
            source_id,
            request.at_ms,
            request.coverage.as_str(),
            request.declared_digest,
            material_digest,
            request.authorship.as_str(),
            request.at_ms,
            publication_time_json,
            acquisition_identity,
            request.origin_locator,
            request.provider_identity
        ],
    )?;
    if private_locator.is_some()
        || request.origin_chat_id.is_some()
        || request.request_ref.is_some()
    {
        tx.execute(
            "INSERT INTO knowledge_capture_lineage (
                record_id, capture_id, chat_id, request_ref, private_locator
             ) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                module.record_id,
                capture_id,
                request.origin_chat_id,
                request.request_ref,
                private_locator
            ],
        )?;
    }
    if let (Some(material), Some((digest, selected_digest, representation_id, span_id))) =
        (&request.material, &prepared)
    {
        let input_spans = canonical_string(&to_value(&material.input_span_ids)?);
        tx.execute(
            "INSERT INTO knowledge_source_representations (
                record_id, representation_id, capture_id, parent_representation_id, document_id,
                digest, media_type, byte_length, role, parser_version, input_spans_json, created_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                module.record_id,
                representation_id,
                capture_id,
                material.parent_representation_id,
                material.document_id,
                digest,
                material.media_type,
                material.bytes.len() as i64,
                material.role.as_str(),
                material.parser_version,
                input_spans,
                request.at_ms
            ],
        )?;
        tx.execute(
            "INSERT INTO knowledge_source_spans (
                record_id, span_id, capture_id, representation_id, selector_json, selected_digest,
                context_note, created_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                module.record_id,
                span_id,
                capture_id,
                representation_id,
                canonical_string(&to_value(&material.selector)?),
                selected_digest,
                material.context_note,
                request.at_ms
            ],
        )?;
    }
    if let Some(chat_id) = &request.origin_chat_id {
        tx.execute(
            "INSERT INTO knowledge_chat_origins (loupist_id, chat_id, record_id, capture_id)
             VALUES (?1, ?2, ?3, ?4)",
            params![request.loupist_id, chat_id, module.record_id, capture_id],
        )?;
    }
    let payload = catalog_payload(tx, &module.record_id)?;
    let published = publish_revision(
        tx,
        &module,
        &request.loupist_id,
        request.expected_revision_id.as_deref(),
        &to_value(&payload)?,
        &[],
        request.at_ms,
    )?;
    super::search_index::index_payload(
        tx,
        Some(blobs),
        &module.record_id,
        &published.revision_id,
        module.kind,
        &module.title,
        &to_value(&payload)?,
    )?;
    if !identical {
        let mut event = super::growth::GrowthEvent::new(
            &request.loupist_id,
            &module.record_id,
            request.at_ms,
            "source_retained",
            if request.origin_chat_id.is_some() {
                "loupist"
            } else {
                "user"
            },
            &request.title,
        );
        event.origin_chat_id = request.origin_chat_id.clone();
        event.origin_message_id = origin_message_id.map(str::to_owned);
        event.insert(tx)?;
    }
    Ok(RetainedSource {
        record_id: module.record_id.clone(),
        source_id,
        capture_id,
        representation_id,
        span_id,
        selected_digest,
        material_digest,
        revision_id: published.revision_id,
        revision_digest: published.revision_digest,
    })
}

#[cfg(feature = "eval-harness")]
pub(super) fn read_span(
    tx: &Transaction<'_>,
    blobs: &BlobStore,
    request: ReadSpan,
) -> KnowledgeResult<SpanRead> {
    validate_loupist_id(&request.loupist_id)?;
    validate_knowledge_id(&request.record_id, "rec")?;
    validate_knowledge_id(&request.span_id, "spn")?;
    validate_time(request.at_ms)?;
    let span = require_visible_span(
        tx,
        &request.loupist_id,
        &request.record_id,
        &request.span_id,
    )?;
    read_visible_span(
        tx,
        blobs,
        &span,
        &request.loupist_id,
        request.at_ms,
        &ReadPurpose::RendererInspection,
    )
}

#[cfg(feature = "eval-harness")]
pub(super) fn read_capture_material(
    tx: &Transaction<'_>,
    blobs: &BlobStore,
    request: ReadCapture,
) -> KnowledgeResult<SpanRead> {
    validate_loupist_id(&request.loupist_id)?;
    validate_knowledge_id(&request.record_id, "rec")?;
    validate_knowledge_id(&request.capture_id, "cap")?;
    validate_time(request.at_ms)?;
    let coverage = capture_coverage(tx, &request.record_id, &request.capture_id)?;
    if coverage == Coverage::MetadataOnly {
        return Err(KnowledgeError::MetadataOnly);
    }
    if coverage == Coverage::Omitted {
        return Err(KnowledgeError::MissingBytes);
    }
    let span_ids = span_ids_for_capture(tx, &request.record_id, &request.capture_id)?;
    if span_ids.len() != 1 {
        return Err(KnowledgeError::Invalid("capture does not have one span"));
    }
    let span = require_visible_span(tx, &request.loupist_id, &request.record_id, &span_ids[0])?;
    read_visible_span(
        tx,
        blobs,
        &span,
        &request.loupist_id,
        request.at_ms,
        &ReadPurpose::RendererInspection,
    )
}

#[cfg(test)]
pub(super) fn list_sources(
    tx: &Transaction<'_>,
    loupist_id: &str,
    record_id: &str,
) -> KnowledgeResult<Vec<SourceSummary>> {
    let module = require_record(tx, loupist_id, record_id)?;
    if module.kind != super::model::RecordKind::Source {
        return Err(KnowledgeError::Invalid("record is not a Source"));
    }
    let Some(revision_id) = visible_catalog_revision(&module) else {
        return Ok(Vec::new());
    };
    let revision = super::revision::load_revision(tx, &revision_id)?;
    let payload: SourceRecordPayload = from_json(&revision.payload_json)?;
    let source = payload.source;
    Ok(vec![SourceSummary {
        capture_count: source.captures.len() as u32,
        source_id: source.source_id,
        title: source.title,
        source_class: source.source_class,
    }])
}

pub(super) fn note_chat_deleted(
    tx: &Transaction<'_>,
    loupist_id: &str,
    chat_id: &str,
) -> KnowledgeResult<u32> {
    validate_loupist_id(loupist_id)?;
    validate_external_id(chat_id, "chat id is invalid")?;
    tx.execute(
        "DELETE FROM knowledge_turn_pins WHERE loupist_id = ?1 AND chat_id = ?2",
        params![loupist_id, chat_id],
    )?;
    let changed = tx.execute(
        "DELETE FROM knowledge_chat_origins WHERE loupist_id = ?1 AND chat_id = ?2",
        params![loupist_id, chat_id],
    )?;
    Ok(changed as u32)
}

pub(super) fn require_visible_span(
    tx: &Transaction<'_>,
    loupist_id: &str,
    record_id: &str,
    span_id: &str,
) -> KnowledgeResult<VisibleSpan> {
    validate_knowledge_id(record_id, "rec")?;
    validate_knowledge_id(span_id, "spn")?;
    let row = tx
        .query_row(
            "SELECT s.span_id, s.capture_id, s.representation_id, s.selector_json,
                    s.selected_digest, c.coverage, r.document_id, r.digest, r.media_type, r.role
             FROM knowledge_source_spans s
             JOIN knowledge_source_captures c
               ON c.record_id = s.record_id AND c.capture_id = s.capture_id
             JOIN knowledge_source_representations r
               ON r.record_id = s.record_id AND r.representation_id = s.representation_id
             WHERE s.record_id = ?1 AND s.span_id = ?2",
            params![record_id, span_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                ))
            },
        )
        .optional()?;
    let Some((
        span_id,
        capture_id,
        representation_id,
        selector_json,
        selected_digest,
        coverage,
        document_id,
        representation_digest,
        media_type,
        role,
    )) = row
    else {
        return Err(KnowledgeError::NotFound);
    };
    let module = require_record(tx, loupist_id, record_id)?;
    ensure_capture_visible(tx, &module, &capture_id)?;
    Ok(VisibleSpan {
        span_id,
        capture_id,
        representation_id,
        selector_json,
        selected_digest,
        coverage: Coverage::parse(&coverage)?,
        document_id,
        representation_digest,
        media_type,
        role: RepresentationRole::parse(&role)?,
        record_id: module.record_id,
    })
}

pub(super) struct SpanMaterial {
    pub source_id: String,
    pub title: String,
    pub source_class: super::model::SourceClass,
    pub capture_id: String,
    pub representation_id: String,
    pub span_id: String,
    pub bytes: Vec<u8>,
    pub selected_digest: String,
    pub provider_identity: String,
    pub publication_time: Option<super::model::TimePoint>,
    /// When the capture was retained. Undated sources use this, not the later enqueue time.
    pub received_at_ms: i64,
}

/// Copy one span from the selected Source revision.
///
/// Membership, selector, digest, provider, and publication time come from that
/// revision's payload. The current module head is not consulted.
pub(super) fn load_revision_span(
    tx: &Transaction<'_>,
    blobs: &BlobStore,
    loupist_id: &str,
    record_id: &str,
    revision_id: &str,
    span_id: &str,
) -> KnowledgeResult<SpanMaterial> {
    validate_loupist_id(loupist_id)?;
    validate_knowledge_id(record_id, "rec")?;
    validate_knowledge_id(revision_id, "rev")?;
    validate_knowledge_id(span_id, "spn")?;
    let module = require_record(tx, loupist_id, record_id)?;
    if module.kind != RecordKind::Source {
        return Err(KnowledgeError::Invalid("record is not a Source"));
    }
    read_closed_revision_span(tx, blobs, record_id, revision_id, span_id)
}

/// Read a span from a revision the caller already accepted, such as a World
/// dependency. This does not follow the current Source head.
pub(super) fn read_closed_revision_span(
    tx: &Transaction<'_>,
    blobs: &BlobStore,
    record_id: &str,
    revision_id: &str,
    span_id: &str,
) -> KnowledgeResult<SpanMaterial> {
    validate_knowledge_id(record_id, "rec")?;
    validate_knowledge_id(revision_id, "rev")?;
    validate_knowledge_id(span_id, "spn")?;
    let stored = super::revision::load_revision(tx, revision_id)?;
    revision_in_record(&stored.record_id, record_id)?;
    if stored.kind != RecordKind::Source {
        return Err(KnowledgeError::Invalid("record is not a Source"));
    }
    let payload: SourceRecordPayload = from_json(&stored.payload_json)?;
    let mut matched = None;
    let source = &payload.source;
    for capture in &source.captures {
        for representation in &capture.representations {
            let Some(span) = representation
                .spans
                .iter()
                .find(|span| span.span_id == span_id)
            else {
                continue;
            };
            if matched.is_some() {
                return Err(KnowledgeError::Invalid(
                    "source revision contains the span more than once",
                ));
            }
            matched = Some((
                source.title.clone(),
                source.source_class,
                source.source_id.clone(),
                source.provider_identity.clone(),
                capture.capture_id.clone(),
                capture.coverage,
                capture.publication_time.clone(),
                capture.received_at_ms,
                representation.representation_id.clone(),
                representation.document_id.clone(),
                representation.digest.clone(),
                span.selector.clone(),
                span.selected_digest.clone(),
                span.span_id.clone(),
            ));
        }
    }

    let Some((
        title,
        source_class,
        source_id,
        provider_identity,
        capture_id,
        coverage,
        publication_time,
        received_at_ms,
        representation_id,
        document_id,
        representation_digest,
        selector,
        selected_digest,
        span_id,
    )) = matched
    else {
        return Err(KnowledgeError::NotFound);
    };
    if coverage == Coverage::MetadataOnly {
        return Err(KnowledgeError::MetadataOnly);
    }
    if coverage == Coverage::Omitted {
        return Err(KnowledgeError::MissingBytes);
    }
    if let Some(time) = &publication_time {
        validate_time_point(time)?;
    }
    let stored_bytes = blobs.read(&representation_digest)?;
    if digest_bytes(&stored_bytes) != representation_digest {
        return Err(KnowledgeError::DigestMismatch);
    }
    let selected = select_bytes(&stored_bytes, &selector, &document_id)?;
    if digest_bytes(&selected) != selected_digest {
        return Err(KnowledgeError::DigestMismatch);
    }
    Ok(SpanMaterial {
        source_id,
        title,
        source_class,
        capture_id,
        representation_id,
        span_id,
        bytes: selected,
        selected_digest,
        provider_identity,
        publication_time,
        received_at_ms,
    })
}

pub(super) fn require_any_visible_span(
    tx: &Transaction<'_>,
    loupist_id: &str,
    span_id: &str,
) -> KnowledgeResult<VisibleSpan> {
    validate_knowledge_id(span_id, "spn")?;
    let mut statement =
        tx.prepare("SELECT record_id FROM knowledge_source_spans WHERE span_id = ?1")?;
    let modules = statement
        .query_map(params![span_id], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    let mut last = Err(KnowledgeError::NotFound);
    for record_id in modules {
        match require_visible_span(tx, loupist_id, &record_id, span_id) {
            Ok(span) => return Ok(span),
            Err(error) => last = Err(error),
        }
    }
    last
}

pub(super) fn load_receipt(
    tx: &Transaction<'_>,
    receipt_id: &str,
) -> KnowledgeResult<SourceReadReceipt> {
    validate_knowledge_id(receipt_id, "rcp")?;
    let receipt = tx
        .query_row(
            "SELECT receipt_id, loupist_id, span_id, capture_id, representation_id, selected_digest,
                    purpose, chat_session_id, turn_id, issued_at_ms
             FROM knowledge_read_receipts WHERE receipt_id = ?1",
            [receipt_id],
            receipt_from_row,
        )
        .optional()?;
    receipt.ok_or(KnowledgeError::NotFound)?
}

#[cfg(feature = "eval-harness")]
pub(super) fn read_visible_span(
    tx: &Transaction<'_>,
    blobs: &BlobStore,
    span: &VisibleSpan,
    loupist_id: &str,
    at_ms: i64,
    purpose: &ReadPurpose,
) -> KnowledgeResult<SpanRead> {
    if span.coverage == Coverage::MetadataOnly {
        return Err(KnowledgeError::MetadataOnly);
    }
    if span.coverage == Coverage::Omitted {
        return Err(KnowledgeError::MissingBytes);
    }
    let bytes = blobs.read(&span.representation_digest)?;
    if digest_bytes(&bytes) != span.representation_digest {
        return Err(KnowledgeError::DigestMismatch);
    }
    let selector: Selector = from_json(&span.selector_json)?;
    let selected = select_bytes(&bytes, &selector, &span.document_id)?;
    let selected_digest = digest_bytes(&selected);
    if selected_digest != span.selected_digest {
        return Err(KnowledgeError::DigestMismatch);
    }
    if matches!(purpose, ReadPurpose::RendererInspection) {
        return Ok(SpanRead {
            bytes: selected,
            media_type: span.media_type.clone(),
            representation_role: span.role,
            coverage: span.coverage,
            selected_digest: selected_digest.clone(),
            receipt: SourceReadReceipt {
                receipt_id: String::new(),
                loupist_id: loupist_id.to_string(),
                span_id: span.span_id.clone(),
                capture_id: span.capture_id.clone(),
                representation_id: span.representation_id.clone(),
                selected_digest,
                purpose: purpose.clone(),
                issued_at_ms: at_ms,
            },
        });
    }
    let ReadPurpose::ChatTurn {
        chat_session_id,
        turn_id,
    } = purpose
    else {
        return Err(KnowledgeError::Invalid("read purpose is invalid"));
    };
    let purpose_name = "chat_turn";
    let receipt_id = mint_id("rcp");
    tx.execute(
        "INSERT INTO knowledge_read_receipts (
            receipt_id, loupist_id, record_id, span_id, capture_id, representation_id, selected_digest,
            purpose, chat_session_id, turn_id, issued_at_ms
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            receipt_id,
            loupist_id,
            span.record_id,
            span.span_id,
            span.capture_id,
            span.representation_id,
            selected_digest,
            purpose_name,
            Some(chat_session_id.as_str()),
            Some(turn_id.as_str()),
            at_ms
        ],
    )?;
    Ok(SpanRead {
        bytes: selected,
        media_type: span.media_type.clone(),
        representation_role: span.role,
        coverage: span.coverage,
        selected_digest: selected_digest.clone(),
        receipt: SourceReadReceipt {
            receipt_id,
            loupist_id: loupist_id.to_string(),
            span_id: span.span_id.clone(),
            capture_id: span.capture_id.clone(),
            representation_id: span.representation_id.clone(),
            selected_digest,
            purpose: purpose.clone(),
            issued_at_ms: at_ms,
        },
    })
}

fn catalog_payload(tx: &Transaction<'_>, record_id: &str) -> KnowledgeResult<SourceRecordPayload> {
    let (source_id, title, source_class, origin_locator, provider_identity, origin_json) = tx.query_row(
        "SELECT source_id, title, source_class, origin_locator, provider_identity, origin_json FROM knowledge_sources WHERE record_id = ?1",
        [record_id], |row| Ok((row.get::<_,String>(0)?, row.get::<_,String>(1)?, row.get::<_,String>(2)?, row.get::<_,Option<String>>(3)?, row.get::<_,String>(4)?, row.get::<_,Option<String>>(5)?)),
    ).optional()?.ok_or(KnowledgeError::NotFound)?;
    Ok(SourceRecordPayload {
        source: PublishedSource {
            origin: origin_json
                .map(|value| serde_json::from_str(&value))
                .transpose()?,
            captures: captures_for_source(tx, record_id, &source_id)?,
            source_id,
            title,
            source_class: SourceClass::parse(&source_class)?,
            origin_locator: origin_locator.and_then(|value| portable_locator(&value)),
            provider_identity,
        },
    })
}

pub(super) fn portable_locator(value: &str) -> Option<String> {
    split_locator(value).ok().and_then(|(public, _)| public)
}

pub(super) fn authorize_representation(
    tx: &Transaction<'_>,
    loupist_id: &str,
    representation_id: &str,
) -> KnowledgeResult<()> {
    validate_knowledge_id(representation_id, "rep")?;
    let mut statement = tx.prepare(
        "SELECT record_id FROM knowledge_source_representations WHERE representation_id = ?1",
    )?;
    let modules = statement
        .query_map(params![representation_id], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    if modules.is_empty() {
        return Err(KnowledgeError::NotFound);
    }
    let mut last_error = Err(KnowledgeError::NotFound);
    for record_id in modules {
        let module = match require_record(tx, loupist_id, &record_id) {
            Ok(module) => module,
            Err(error) => {
                last_error = Err(error);
                continue;
            }
        };
        if representation_visible(tx, &module, representation_id)? {
            return Ok(());
        }
        last_error = Err(KnowledgeError::NotFound);
    }
    last_error
}

fn representation_visible(
    tx: &Transaction<'_>,
    module: &OwnedRecordState,
    representation_id: &str,
) -> KnowledgeResult<bool> {
    let Some(revision_id) = visible_catalog_revision(module) else {
        return Ok(false);
    };
    let revision = super::revision::load_revision(tx, &revision_id)?;
    if revision.kind != super::model::RecordKind::Source {
        return Ok(false);
    }
    let payload: SourceRecordPayload = from_json(&revision.payload_json)?;
    Ok(payload.source.captures.iter().any(|capture| {
        capture
            .representations
            .iter()
            .any(|representation| representation.representation_id == representation_id)
    }))
}

fn split_locator(value: &str) -> KnowledgeResult<(Option<String>, Option<String>)> {
    reject_locator_userinfo(value)?;
    let private = value.starts_with('/')
        || value.starts_with("file:")
        || value.contains(":\\")
        || value.contains("\\\\");
    if private {
        Ok((None, Some(value.to_string())))
    } else {
        Ok((Some(value.to_string()), None))
    }
}

pub fn reject_locator_userinfo(value: &str) -> KnowledgeResult<()> {
    let Some((_, after_scheme)) = value.split_once("://") else {
        return Ok(());
    };
    let authority = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(after_scheme);
    if authority.contains('@') {
        return Err(KnowledgeError::Invalid(
            "origin locator contains URL userinfo",
        ));
    }
    Ok(())
}

fn publication_json(time: &Option<super::model::TimePoint>) -> KnowledgeResult<Option<String>> {
    let Some(time) = time else {
        return Ok(None);
    };
    validate_time_point(time)?;
    Ok(Some(canonical_string(&to_value(time)?)))
}

fn publication_from_json(
    value: Option<String>,
) -> KnowledgeResult<Option<super::model::TimePoint>> {
    let Some(text) = value else {
        return Ok(None);
    };
    let time = from_json(&text)?;
    validate_time_point(&time)?;
    Ok(Some(time))
}

fn captures_for_source(
    tx: &Transaction<'_>,
    record_id: &str,
    source_id: &str,
) -> KnowledgeResult<Vec<PublishedCapture>> {
    let mut statement = tx.prepare(
        "SELECT capture_id, received_at_ms, coverage, declared_digest, material_digest, authorship,
                publication_time_json
         FROM knowledge_source_captures
         WHERE record_id = ?1 AND source_id = ?2
         ORDER BY received_at_ms, capture_id",
    )?;
    let rows = statement
        .query_map(params![record_id, source_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, Option<String>>(6)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    let mut captures = Vec::new();
    for (
        capture_id,
        received_at_ms,
        coverage,
        declared_digest,
        material_digest,
        authorship,
        publication_time_json,
    ) in rows
    {
        captures.push(PublishedCapture {
            capture_id: capture_id.clone(),
            received_at_ms,
            coverage: Coverage::parse(&coverage)?,
            declared_digest,
            material_digest,
            authorship: Authorship::parse(&authorship)?,
            representations: representations_for_capture(tx, record_id, &capture_id)?,
            publication_time: publication_from_json(publication_time_json)?,
        });
    }
    Ok(captures)
}

fn representations_for_capture(
    tx: &Transaction<'_>,
    record_id: &str,
    capture_id: &str,
) -> KnowledgeResult<Vec<PublishedRepresentation>> {
    let mut statement = tx.prepare(
        "SELECT representation_id, parent_representation_id, document_id, digest, media_type,
                byte_length, role, parser_version, input_spans_json
         FROM knowledge_source_representations
         WHERE record_id = ?1 AND capture_id = ?2
         ORDER BY representation_id",
    )?;
    let rows = statement
        .query_map(params![record_id, capture_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, String>(8)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    let mut representations = Vec::new();
    for (
        representation_id,
        parent_representation_id,
        document_id,
        digest,
        media_type,
        byte_length,
        role,
        parser_version,
        input_spans_json,
    ) in rows
    {
        representations.push(PublishedRepresentation {
            representation_id: representation_id.clone(),
            parent_representation_id,
            document_id,
            digest,
            media_type,
            byte_length: u64::try_from(byte_length)
                .map_err(|_| KnowledgeError::Invalid("byte length is invalid"))?,
            role: RepresentationRole::parse(&role)?,
            parser_version,
            input_span_ids: from_json(&input_spans_json)?,
            spans: spans_for_representation(tx, record_id, &representation_id)?,
        });
    }
    Ok(representations)
}

fn spans_for_representation(
    tx: &Transaction<'_>,
    record_id: &str,
    representation_id: &str,
) -> KnowledgeResult<Vec<PublishedSpan>> {
    let mut statement = tx.prepare(
        "SELECT span_id, selector_json, selected_digest, context_note
         FROM knowledge_source_spans
         WHERE record_id = ?1 AND representation_id = ?2
         ORDER BY span_id",
    )?;
    let rows = statement.query_map(params![record_id, representation_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;
    let mut spans = Vec::new();
    for row in rows {
        let (span_id, selector_json, selected_digest, context_note) = row?;
        spans.push(PublishedSpan {
            span_id,
            selector: from_json(&selector_json)?,
            selected_digest,
            context_note,
        });
    }
    Ok(spans)
}

fn ensure_capture_visible(
    tx: &Transaction<'_>,
    module: &OwnedRecordState,
    capture_id: &str,
) -> KnowledgeResult<()> {
    let Some(revision_id) = visible_catalog_revision(module) else {
        return Err(KnowledgeError::NotFound);
    };
    let revision: StoredRevision = super::revision::load_revision(tx, &revision_id)?;
    let payload: SourceRecordPayload = from_json(&revision.payload_json)?;
    if payload
        .source
        .captures
        .iter()
        .any(|capture| capture.capture_id == capture_id)
    {
        Ok(())
    } else {
        Err(KnowledgeError::NotFound)
    }
}

fn visible_catalog_revision(module: &OwnedRecordState) -> Option<String> {
    module.head_revision_id.clone()
}

#[cfg(feature = "eval-harness")]
fn capture_coverage(
    tx: &Transaction<'_>,
    record_id: &str,
    capture_id: &str,
) -> KnowledgeResult<Coverage> {
    let coverage: String = tx
        .query_row(
            "SELECT coverage FROM knowledge_source_captures
             WHERE record_id = ?1 AND capture_id = ?2",
            params![record_id, capture_id],
            |row| row.get(0),
        )
        .optional()?
        .ok_or(KnowledgeError::NotFound)?;
    Coverage::parse(&coverage)
}

#[cfg(feature = "eval-harness")]
fn span_ids_for_capture(
    tx: &Transaction<'_>,
    record_id: &str,
    capture_id: &str,
) -> KnowledgeResult<Vec<String>> {
    let mut statement = tx.prepare(
        "SELECT span_id FROM knowledge_source_spans
         WHERE record_id = ?1 AND capture_id = ?2 ORDER BY span_id",
    )?;
    let rows = statement.query_map(params![record_id, capture_id], |row| row.get(0))?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn receipt_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<KnowledgeResult<SourceReadReceipt>> {
    let receipt_id: String = row.get(0)?;
    let loupist_id: String = row.get(1)?;
    let span_id: String = row.get(2)?;
    let capture_id: String = row.get(3)?;
    let representation_id: String = row.get(4)?;
    let selected_digest: String = row.get(5)?;
    let purpose_name: String = row.get(6)?;
    let chat_session_id: Option<String> = row.get(7)?;
    let turn_id: Option<String> = row.get(8)?;
    let issued_at_ms: i64 = row.get(9)?;
    let purpose = match purpose_name.as_str() {
        "renderer_inspection" => Ok(ReadPurpose::RendererInspection),
        "chat_turn" => match (chat_session_id, turn_id) {
            (Some(chat_session_id), Some(turn_id)) => Ok(ReadPurpose::ChatTurn {
                chat_session_id,
                turn_id,
            }),
            _ => Err(KnowledgeError::Invalid("stored receipt is invalid")),
        },
        _ => Err(KnowledgeError::Invalid("stored receipt is invalid")),
    };
    Ok(purpose.map(|purpose| SourceReadReceipt {
        receipt_id,
        loupist_id,
        span_id,
        capture_id,
        representation_id,
        selected_digest,
        purpose,
        issued_at_ms,
    }))
}

fn validate_retention(request: &RetainSource) -> KnowledgeResult<()> {
    validate_loupist_id(&request.loupist_id)?;
    validate_knowledge_id(&request.record_id, "rec")?;
    validate_optional_knowledge_id(&request.expected_revision_id, "rev")?;
    validate_optional_knowledge_id(&request.source_id, "src")?;
    validate_time(request.at_ms)?;
    validate_title(&request.title)?;
    validate_text(
        &request.origin_locator,
        2_000,
        false,
        "origin locator is invalid",
    )?;
    validate_text(
        &request.provider_identity,
        200,
        false,
        "provider identity is invalid",
    )?;
    if let Some(chat_id) = &request.origin_chat_id {
        validate_external_id(chat_id, "chat id is invalid")?;
    }
    if let Some(request_ref) = &request.request_ref {
        validate_token(request_ref, 128, "request ref is invalid")?;
    }
    validate_optional_digest(&request.declared_digest)?;
    if let Some(time) = &request.publication_time {
        validate_time_point(time)?;
    }
    match (&request.coverage, &request.material) {
        (coverage, None) if !coverage.retains_bytes() => Ok(()),
        (coverage, Some(material)) if coverage.retains_bytes() => {
            validate_media_type(&material.media_type)?;
            validate_token(&material.document_id, 128, "document id is invalid")?;
            validate_text(
                &material.context_note,
                2_000,
                true,
                "context note is invalid",
            )?;
            if let Some(parser) = &material.parser_version {
                validate_token(parser, 64, "parser version is invalid")?;
            }
            if material.bytes.len() > MAX_BLOB_BYTES {
                return Err(KnowledgeError::Invalid("blob is too large"));
            }
            if matches!(material.selector, Selector::Multi { .. }) {
                return Err(KnowledgeError::Invalid(
                    "multi-document selector is invalid",
                ));
            }
            if material.role == RepresentationRole::Derived
                && material.parent_representation_id.is_none()
                && material.input_span_ids.is_empty()
            {
                return Err(KnowledgeError::Invalid(
                    "derived material requires input lineage",
                ));
            }
            if *coverage == Coverage::UpstreamTruncated
                && material.role == RepresentationRole::Original
            {
                return Err(KnowledgeError::Invalid(
                    "truncated material is not an original",
                ));
            }
            validate_optional_knowledge_id(&material.parent_representation_id, "rep")?;
            for span_id in &material.input_span_ids {
                validate_knowledge_id(span_id, "spn")?;
            }
            Ok(())
        }
        _ => Err(KnowledgeError::Invalid(
            "coverage does not match retained bytes",
        )),
    }
}

/// Idempotently retain an acquisition with its complete attribution identity.
/// Retrieval time, tool call IDs, and display title do not change identity.
pub(super) fn retain_new_source(
    tx: &Transaction<'_>,
    blobs: &BlobStore,
    request: super::model::RetainNewSource,
) -> KnowledgeResult<RetainedSource> {
    retain_new_source_with_origin(tx, blobs, request, None)
}

pub(super) fn retain_new_source_with_origin(
    tx: &Transaction<'_>,
    blobs: &BlobStore,
    request: super::model::RetainNewSource,
    origin_message_id: Option<&str>,
) -> KnowledgeResult<RetainedSource> {
    let record_id = mint_id("rec");
    let capture = request.into_capture(record_id, None, None);
    validate_retention(&capture)?;
    let identity = capture_identity(&capture)?;
    let saved = tx
        .query_row(
            "SELECT caps.record_id, caps.source_id, caps.capture_id, reps.representation_id,
                spans.span_id, spans.selected_digest, caps.material_digest,
                heads.revision_id, revs.revision_digest
         FROM knowledge_source_captures caps
         JOIN knowledge_records records ON records.record_id = caps.record_id
         JOIN knowledge_record_heads heads ON heads.record_id = caps.record_id
         JOIN knowledge_record_revisions revs ON revs.revision_id = heads.revision_id
         LEFT JOIN knowledge_source_representations reps
           ON reps.record_id = caps.record_id AND reps.capture_id = caps.capture_id
         LEFT JOIN knowledge_source_spans spans
           ON spans.record_id = reps.record_id AND spans.representation_id = reps.representation_id
         WHERE caps.acquisition_identity = ?1 AND records.owner_loupist_id = ?2",
            params![identity, capture.loupist_id],
            |row| {
                Ok(RetainedSource {
                    record_id: row.get(0)?,
                    source_id: row.get(1)?,
                    capture_id: row.get(2)?,
                    representation_id: row.get(3)?,
                    span_id: row.get(4)?,
                    selected_digest: row.get(5)?,
                    material_digest: row.get(6)?,
                    revision_id: row.get(7)?,
                    revision_digest: row.get(8)?,
                })
            },
        )
        .optional()?;
    if let Some(saved) = saved {
        verify_acquisition_identity(tx, &capture.loupist_id, &saved, &identity)?;
        if let Some(chat_id) = &capture.origin_chat_id {
            tx.execute("INSERT OR IGNORE INTO knowledge_chat_origins (loupist_id, chat_id, record_id, capture_id) VALUES (?1, ?2, ?3, ?4)", params![capture.loupist_id, chat_id, saved.record_id, saved.capture_id])?;
        }
        return Ok(saved);
    }
    let record = super::catalog::create_record(
        tx,
        super::model::CreateRecord {
            loupist_id: capture.loupist_id.clone(),
            kind: RecordKind::Source,
            title: capture.title.clone(),
            at_ms: capture.at_ms,
        },
    )?;
    retain_capture(
        tx,
        blobs,
        RetainSource {
            record_id: record.record_id,
            ..capture
        },
        Some(&identity),
        origin_message_id,
    )
}

// The acquisition index is an optimization, not authority. Its candidate must
// match immutable capture provenance. Metadata corrections do not rewrite the
// original attribution or require a retained document version. A copied database
// cannot redirect bytes to another publisher by changing only the index column.
fn verify_acquisition_identity(
    tx: &Transaction<'_>,
    loupist_id: &str,
    saved: &RetainedSource,
    expected: &str,
) -> KnowledgeResult<()> {
    let payload: String = tx
        .query_row(
            "SELECT payload_json FROM knowledge_record_revisions WHERE record_id = ?1",
            [&saved.record_id],
            |row| row.get(0),
        )
        .optional()?
        .ok_or(KnowledgeError::DigestMismatch)?;
    let payload: SourceRecordPayload = from_json(&payload)?;
    let source = &payload.source;
    if source.source_id != saved.source_id {
        return Err(KnowledgeError::DigestMismatch);
    }
    let capture = source
        .captures
        .iter()
        .find(|capture| capture.capture_id == saved.capture_id)
        .ok_or(KnowledgeError::DigestMismatch)?;
    let (locator, provider_identity): (String, String) = tx.query_row(
        "SELECT origin_locator, provider_identity FROM knowledge_source_captures
         WHERE record_id = ?1 AND capture_id = ?2",
        params![saved.record_id, saved.capture_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let material = if let Some(representation_id) = &saved.representation_id {
        let representation = capture
            .representations
            .iter()
            .find(|representation| &representation.representation_id == representation_id)
            .ok_or(KnowledgeError::DigestMismatch)?;
        let span = representation
            .spans
            .iter()
            .find(|span| Some(&span.span_id) == saved.span_id.as_ref())
            .ok_or(KnowledgeError::DigestMismatch)?;
        Some(serde_json::json!({
            "documentId": representation.document_id,
            "mediaType": representation.media_type,
            "role": representation.role,
            "parserVersion": representation.parser_version,
            "parentRepresentationId": representation.parent_representation_id,
            "inputSpanIds": representation.input_span_ids,
            "selector": span.selector,
            "digest": representation.digest,
        }))
    } else {
        None
    };
    let identity = super::canonical::digest_json(&serde_json::json!({
        "loupistId": loupist_id,
        "originLocator": locator,
        "providerIdentity": provider_identity,
        "sourceClass": source.source_class,
        "authorship": capture.authorship,
        "coverage": capture.coverage,
        "declaredDigest": capture.declared_digest,
        "publicationTime": capture.publication_time,
        "material": material,
    }))?;
    if identity != expected {
        return Err(KnowledgeError::DigestMismatch);
    }
    Ok(())
}

pub(super) fn read_span_bytes_unbound(
    tx: &Transaction<'_>,
    blobs: &BlobStore,
    record_id: &str,
    span_id: &str,
    expected_digest: &str,
) -> KnowledgeResult<(Vec<u8>, String)> {
    validate_knowledge_id(record_id, "rec")?;
    validate_knowledge_id(span_id, "spn")?;
    let (selector_json, representation_digest, media_type, document_id): (
        String,
        String,
        String,
        String,
    ) = tx
        .query_row(
            "SELECT s.selector_json, r.digest, r.media_type, r.document_id
             FROM knowledge_source_spans s
             JOIN knowledge_source_representations r
               ON r.record_id = s.record_id AND r.representation_id = s.representation_id
             WHERE s.record_id = ?1 AND s.span_id = ?2",
            params![record_id, span_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?
        .ok_or(KnowledgeError::NotFound)?;
    let stored = blobs.read(&representation_digest)?;
    if digest_bytes(&stored) != representation_digest {
        return Err(KnowledgeError::DigestMismatch);
    }
    let selector: Selector = from_json(&selector_json)?;
    let selected = select_bytes(&stored, &selector, &document_id)?;
    if digest_bytes(&selected) != expected_digest {
        return Err(KnowledgeError::DigestMismatch);
    }
    Ok((selected, media_type))
}

pub(super) fn select_bytes(
    bytes: &[u8],
    selector: &Selector,
    document_id: &str,
) -> KnowledgeResult<Vec<u8>> {
    match selector {
        Selector::Multi { .. } => Err(KnowledgeError::Invalid(
            "multi-document selector is invalid",
        )),
        Selector::ByteRange { start, end } => byte_range(bytes, *start, *end),
        Selector::Part {
            document_id: part_document,
            part_id,
            start,
            end,
        } => {
            validate_token(part_document, 128, "document id is invalid")?;
            validate_token(part_id, 128, "part id is invalid")?;
            if part_document != document_id {
                return Err(KnowledgeError::Invalid(
                    "selector addresses a different document",
                ));
            }
            byte_range(bytes, *start, *end)
        }
        Selector::Utf8LineRange {
            start_line,
            end_line,
        } => line_range(bytes, *start_line, *end_line),
        Selector::JsonPointer { pointer } => pointer_bytes(bytes, pointer),
        Selector::Row { index } => jsonl_row(bytes, *index),
    }
}

fn byte_range(bytes: &[u8], start: u64, end: u64) -> KnowledgeResult<Vec<u8>> {
    if start > end || end > bytes.len() as u64 {
        return Err(KnowledgeError::Invalid("byte range is invalid"));
    }
    Ok(bytes[start as usize..end as usize].to_vec())
}

fn line_range(bytes: &[u8], start_line: u32, end_line: u32) -> KnowledgeResult<Vec<u8>> {
    if start_line == 0 || end_line < start_line {
        return Err(KnowledgeError::Invalid("line range is invalid"));
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| KnowledgeError::Invalid("representation is not utf-8"))?;
    let mut lines: Vec<&str> = text.split('\n').collect();
    if text.ends_with('\n') {
        lines.pop();
    }
    let start = start_line as usize - 1;
    let end = end_line as usize;
    if start >= lines.len() || end > lines.len() {
        return Err(KnowledgeError::Invalid("line range is invalid"));
    }
    Ok(lines[start..end].join("\n").into_bytes())
}

fn pointer_bytes(bytes: &[u8], pointer: &str) -> KnowledgeResult<Vec<u8>> {
    if !valid_json_pointer(pointer, 512, true) {
        return Err(KnowledgeError::Invalid("json pointer is invalid"));
    }
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|_| KnowledgeError::Invalid("representation is not json"))?;
    let selected = walk_pointer(&value, pointer)?;
    Ok(canonical_string(selected).into_bytes())
}

fn walk_pointer<'a>(value: &'a Value, pointer: &str) -> KnowledgeResult<&'a Value> {
    if pointer.is_empty() {
        return Ok(value);
    }
    let mut current = value;
    for raw in pointer.split('/').skip(1) {
        let token = raw.replace("~1", "/").replace("~0", "~");
        current = match current {
            Value::Object(map) => map
                .get(&token)
                .ok_or(KnowledgeError::Invalid("json pointer is invalid"))?,
            Value::Array(items) => {
                let index: usize = token
                    .parse()
                    .map_err(|_| KnowledgeError::Invalid("json pointer is invalid"))?;
                items
                    .get(index)
                    .ok_or(KnowledgeError::Invalid("json pointer is invalid"))?
            }
            _ => return Err(KnowledgeError::Invalid("json pointer is invalid")),
        };
    }
    Ok(current)
}

fn jsonl_row(bytes: &[u8], index: u32) -> KnowledgeResult<Vec<u8>> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| KnowledgeError::Invalid("representation is not utf-8"))?;
    let mut rows: Vec<&str> = text.split('\n').collect();
    if text.ends_with('\n') {
        rows.pop();
    }
    let line = rows
        .get(index as usize)
        .ok_or(KnowledgeError::Invalid("row is invalid"))?;
    serde_json::from_str::<Value>(line).map_err(|_| KnowledgeError::Invalid("row is not json"))?;
    Ok(line.as_bytes().to_vec())
}

fn capture_identity(capture: &RetainSource) -> KnowledgeResult<String> {
    let material = capture.material.as_ref().map(|material| {
        serde_json::json!({
            "documentId": material.document_id,
            "mediaType": material.media_type,
            "role": material.role,
            "parserVersion": material.parser_version,
            "parentRepresentationId": material.parent_representation_id,
            "inputSpanIds": material.input_span_ids,
            "selector": material.selector,
            "digest": digest_bytes(&material.bytes),
        })
    });
    super::canonical::digest_json(&serde_json::json!({
        "loupistId": capture.loupist_id,
        "originLocator": capture.origin_locator,
        "providerIdentity": capture.provider_identity,
        "sourceClass": capture.source_class,
        "authorship": capture.authorship,
        "coverage": capture.coverage,
        "declaredDigest": capture.declared_digest,
        "publicationTime": capture.publication_time,
        "material": material,
    }))
}

// Direct capture revisions also suppress duplicate growth without changing the
// acquisition index or the existing retained-capture write semantics.
fn has_identical_capture(
    tx: &Transaction<'_>,
    request: &RetainSource,
    head: Option<&str>,
) -> KnowledgeResult<bool> {
    let Some(head) = head else {
        return Ok(false);
    };
    let revision = super::revision::load_revision(tx, head)?;
    let payload: SourceRecordPayload = from_json(&revision.payload_json)?;
    if payload.source.source_class != request.source_class {
        return Ok(false);
    }
    let digest = request
        .material
        .as_ref()
        .map(|material| digest_bytes(&material.bytes));
    for capture in &payload.source.captures {
        if capture.authorship != request.authorship
            || capture.coverage != request.coverage
            || capture.declared_digest != request.declared_digest
            || capture.material_digest != digest
            || capture.publication_time != request.publication_time
        {
            continue;
        }
        let material_matches = match &request.material {
            None => capture.representations.is_empty(),
            Some(material) => capture.representations.iter().any(|representation| {
                representation.document_id == material.document_id
                    && representation.media_type == material.media_type
                    && representation.role == material.role
                    && representation.parser_version == material.parser_version
                    && representation.parent_representation_id == material.parent_representation_id
                    && representation.input_span_ids == material.input_span_ids
                    && representation
                        .spans
                        .iter()
                        .any(|span| span.selector == material.selector)
            }),
        };
        if material_matches {
            let same_origin: bool = tx.query_row(
                "SELECT origin_locator = ?3 AND provider_identity = ?4 FROM knowledge_source_captures
                 WHERE record_id = ?1 AND capture_id = ?2",
                params![request.record_id, capture.capture_id, request.origin_locator, request.provider_identity],
                |row| row.get(0),
            )?;
            if same_origin {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
