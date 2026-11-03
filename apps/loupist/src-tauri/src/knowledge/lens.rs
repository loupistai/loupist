use std::collections::{BTreeMap, BTreeSet};

use pulldown_cmark::{Event, Parser, Tag};

use rusqlite::{params, OptionalExtension, Transaction};

use super::authority::{require_edit, require_record};
use super::canonical::{canonical_string, from_json, to_value};
use super::error::{KnowledgeError, KnowledgeResult};
use super::model::{
    mint_id, validate_knowledge_id, validate_loupist_id, validate_optional_knowledge_id,
    validate_text, validate_time, EvidenceRevision, LensChangeInput, LensChangeInputs,
    LensChangeRecord, LensDocument, LensDraftRecord, LensNavigationLink, LensView, RecordKind,
    ReviseLens, MAX_MARKDOWN_BYTES,
};
use super::revision::{
    load_revision, normalize_dependencies, publish_revision, visible_by_digest, StoredRevision,
};
use super::world::require_snapshot;

pub(super) fn revise_lens(tx: &Transaction<'_>, request: ReviseLens) -> KnowledgeResult<LensView> {
    revise_lens_with_origin(tx, request, "user", None, None)
}

pub(super) fn revise_lens_with_origin(
    tx: &Transaction<'_>,
    request: ReviseLens,
    actor: &str,
    origin_chat_id: Option<&str>,
    origin_message_id: Option<&str>,
) -> KnowledgeResult<LensView> {
    revise_lens_with_provenance(tx, request, actor, origin_chat_id, origin_message_id, None)
}

pub(super) fn revise_lens_with_provenance(
    tx: &Transaction<'_>,
    request: ReviseLens,
    actor: &str,
    origin_chat_id: Option<&str>,
    origin_message_id: Option<&str>,
    origin: Option<serde_json::Value>,
) -> KnowledgeResult<LensView> {
    validate_loupist_id(&request.loupist_id)?;
    if super::principles::principle_kind(tx, &request.loupist_id, &request.record_id)? {
        return Err(KnowledgeError::Invalid(
            "Use principle_save to revise a user principle",
        ));
    }
    validate_knowledge_id(&request.record_id, "rec")?;
    validate_optional_knowledge_id(&request.expected_revision_id, "rev")?;
    validate_time(request.at_ms)?;
    validate_lens_document(&request.document)?;
    let dependencies = normalize_dependencies(&request.dependencies, RecordKind::Lens)?;
    enforce_lens_closure(tx, &request.loupist_id, &dependencies)?;
    let module = require_record(tx, &request.loupist_id, &request.record_id)?;
    if module.kind != RecordKind::Lens {
        return Err(KnowledgeError::Invalid("record is not a Lens"));
    }
    enforce_lens_text(tx, &request.loupist_id, &module.title, &request.document)?;
    require_edit(&module, &request.loupist_id)?;
    validate_change(&request.change)?;
    authorize_change_inputs(tx, &request.loupist_id, &request.change)?;
    let before = module
        .head_revision_id
        .as_deref()
        .map(|id| {
            load_revision(tx, id)
                .and_then(|revision| from_json::<LensDocument>(&revision.payload_json))
        })
        .transpose()?;
    let published = publish_revision(
        tx,
        &module,
        &request.loupist_id,
        request.expected_revision_id.as_deref(),
        &to_value(&request.document)?,
        &dependencies,
        request.at_ms,
    )?;
    if before.as_ref() != Some(&request.document) {
        let mut event = super::growth::GrowthEvent::new(
            &request.loupist_id,
            &module.record_id,
            request.at_ms,
            if before.is_none() {
                "lens_created"
            } else {
                "lens_revised"
            },
            actor,
            &request.change.rationale,
        );
        event.before_summary = before
            .as_ref()
            .map(|document| super::growth::lens_summary(document, &module.title));
        event.after_summary = Some(super::growth::lens_summary(
            &request.document,
            &module.title,
        ));
        event.rationale = origin.as_ref().map(|_| request.change.rationale.clone());
        event.origin = origin.clone();
        event.origin_chat_id = origin_chat_id.map(str::to_owned);
        event.origin_message_id = origin_message_id.map(str::to_owned);
        event.insert(tx)?;
    }
    let inputs = LensChangeInputs {
        references: request.change.references,
        feedback: request.change.feedback,
    };
    tx.execute(
        "INSERT INTO knowledge_lens_changes (
            change_id, loupist_id, record_id, new_revision_id, rationale,
            inputs_json, created_at_ms, change_kind, actor, origin_chat_id, origin_message_id, origin_json
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'revise', ?8, ?9, ?10, ?11)",
        params![
            mint_id("chg"),
            request.loupist_id,
            module.record_id,
            published.revision_id,
            request.change.rationale,
            canonical_string(&to_value(&inputs)?),
            request.at_ms,
            actor,
            origin_chat_id,
            origin_message_id,
            origin.as_ref().map(serde_json::Value::to_string)
        ],
    )?;
    tx.execute(
        "UPDATE knowledge_lens_metadata SET updated_at_ms = ?1, learned = learned OR ?4 WHERE record_id = ?2 AND loupist_id = ?3",
        params![request.at_ms, module.record_id, request.loupist_id, request.expected_revision_id.is_some()],
    )?;
    replace_lens_links(
        tx,
        &request.loupist_id,
        &module.record_id,
        &request.document,
    )?;
    super::search_index::index_payload(
        tx,
        None,
        &module.record_id,
        &published.revision_id,
        RecordKind::Lens,
        &module.title,
        &to_value(&request.document)?,
    )?;
    Ok(LensView {
        record_id: module.record_id,
        revision_id: published.revision_id,
        revision_digest: published.revision_digest,
        document: request.document,
        dependencies,
    })
}

pub(super) fn read_lens(
    tx: &Transaction<'_>,
    loupist_id: &str,
    record_id: &str,
) -> KnowledgeResult<LensView> {
    let module = require_record(tx, loupist_id, record_id)?;
    if module.kind != RecordKind::Lens {
        return Err(KnowledgeError::Invalid("record is not a Lens"));
    }
    let revision_id = module
        .head_revision_id
        .clone()
        .ok_or(KnowledgeError::NotFound)?;
    let revision = load_revision(tx, &revision_id)?;
    let view = lens_view(&revision)?;
    enforce_lens_text(tx, loupist_id, &module.title, &view.document)?;
    Ok(view)
}

/// Links are navigational, not evidence dependencies. Cycles need no traversal.
/// Outgoing links come from the document being read. Backlinks come from the
/// current-head link index.
pub(super) fn lens_navigation(
    tx: &Transaction<'_>,
    loupist_id: &str,
    record_id: &str,
    document: &LensDocument,
) -> KnowledgeResult<super::model::LensNavigation> {
    if require_record(tx, loupist_id, record_id)?.kind != RecordKind::Lens {
        return Err(KnowledgeError::Invalid("record is not a Lens"));
    }
    const LIMIT: usize = 64;
    let mut outgoing = lens_link_ids(document);
    let mut navigation = super::model::LensNavigation::default();
    let mut resolved = lookup_lens_links(tx, loupist_id, &outgoing)?;
    resolved.sort_by(|left, right| {
        left.title
            .cmp(&right.title)
            .then(left.record_id.cmp(&right.record_id))
    });
    for link in resolved {
        outgoing.remove(&link.record_id);
        if navigation.links.len() < LIMIT {
            navigation.links.push(link);
        } else {
            navigation.links_truncated = true;
        }
    }
    let mut backlinks = current_backlinks(tx, loupist_id, record_id)?;
    navigation.backlinks_truncated = backlinks.len() > LIMIT;
    backlinks.truncate(LIMIT);
    navigation.backlinks = backlinks;
    navigation.unresolved_links_truncated = outgoing.len() > LIMIT;
    navigation.unresolved_links = outgoing.into_iter().take(LIMIT).collect();
    Ok(navigation)
}

fn lens_link_ids(document: &LensDocument) -> BTreeSet<String> {
    lens_publication_text("", document)
        .into_iter()
        .flat_map(Parser::new)
        .filter_map(|event| match event {
            Event::Start(Tag::Link { dest_url, .. }) => dest_url
                .strip_prefix("lens:")
                .filter(|id| {
                    !id.is_empty()
                        && id.len() <= 128
                        && !id.chars().any(|c| {
                            c.is_whitespace() || c.is_control() || matches!(c, '/' | '?' | '#')
                        })
                })
                .map(str::to_owned),
            _ => None,
        })
        .collect()
}

pub(super) fn list_lens_changes(
    tx: &Transaction<'_>,
    loupist_id: &str,
    record_id: &str,
) -> KnowledgeResult<Vec<LensChangeRecord>> {
    let module = require_record(tx, loupist_id, record_id)?;
    if module.kind != RecordKind::Lens {
        return Err(KnowledgeError::Invalid("record is not a Lens"));
    }
    let mut statement = tx.prepare(
        "SELECT change_id, new_revision_id, rationale, inputs_json, created_at_ms,
                change_kind, actor, origin_chat_id, origin_message_id, new_title, origin_json
         FROM knowledge_lens_changes WHERE record_id = ?1 AND loupist_id = ?2",
    )?;
    let rows = statement.query_map(params![record_id, loupist_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, Option<String>>(8)?,
            row.get::<_, Option<String>>(9)?,
            row.get::<_, Option<String>>(10)?,
        ))
    })?;
    let mut records = Vec::new();
    for row in rows {
        let (
            change_id,
            new_revision_id,
            rationale,
            inputs_json,
            created_at_ms,
            change_kind,
            actor,
            origin_chat_id,
            origin_message_id,
            new_title,
            origin_json,
        ) = row?;
        records.push(LensChangeRecord {
            origin: origin_json.map(|s| serde_json::from_str(&s)).transpose()?,
            change_id,
            loupist_id: loupist_id.into(),
            record_id: record_id.into(),
            new_revision_id,
            rationale,
            inputs: from_json(&inputs_json)?,
            created_at_ms,
            change_kind,
            actor,
            origin_chat_id,
            origin_message_id,
            new_title,
        });
    }
    Ok(records)
}

pub(super) fn enforce_lens_text(
    tx: &Transaction<'_>,
    loupist_id: &str,
    title: &str,
    document: &LensDocument,
) -> KnowledgeResult<()> {
    enforce_lens_text_allowing(tx, loupist_id, title, document, &BTreeSet::new())
}

pub(super) fn enforce_lens_text_allowing(
    tx: &Transaction<'_>,
    loupist_id: &str,
    title: &str,
    document: &LensDocument,
    pending_lens_ids: &BTreeSet<String>,
) -> KnowledgeResult<()> {
    for text in lens_publication_text(title, document) {
        if portable_lens_text_forbidden(text)
            || unresolved_or_world_reference(tx, loupist_id, text, pending_lens_ids)?
        {
            return Err(KnowledgeError::LensBodyForbidden);
        }
    }
    Ok(())
}

pub(super) fn validate_lens_publication(
    tx: &Transaction<'_>,
    loupist_id: &str,
    title: &str,
    document: &LensDocument,
    pending_lens_ids: &BTreeSet<String>,
) -> KnowledgeResult<()> {
    validate_lens_document(document)?;
    enforce_lens_text_allowing(tx, loupist_id, title, document, pending_lens_ids)
}

pub(super) fn portable_lens_text_forbidden(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("world://")
        || lower.contains("journal://")
        || lower.contains("[[world:")
        || lower.contains("[[journal:")
        || reserved_knowledge_id(text, "jnl_")
        || reserved_knowledge_id(text, "snp_")
}

fn lens_publication_text<'a>(title: &'a str, document: &'a LensDocument) -> [&'a str; 7] {
    [
        title,
        &document.markdown,
        &document.applicability,
        &document.assumptions,
        &document.procedure,
        &document.principles,
        &document.limitations,
    ]
}

fn unresolved_or_world_reference(
    tx: &Transaction<'_>,
    _loupist_id: &str,
    text: &str,
    pending_lens_ids: &BTreeSet<String>,
) -> KnowledgeResult<bool> {
    for record_id in knowledge_id_candidates(text, "rec_")? {
        if pending_lens_ids.contains(&record_id) {
            continue;
        }
        // Another Loupist's Source or Lens stays in the text as an unresolved link.
        // Only a World record, or an id this database cannot identify, is forbidden.
        if !matches!(
            any_record_kind(tx, &record_id)?.as_deref(),
            Some("source" | "lens")
        ) {
            return Ok(true);
        }
    }
    for revision_id in knowledge_id_candidates(text, "rev_")? {
        let kind: Option<String> = tx
            .query_row(
                "SELECT m.kind FROM knowledge_record_revisions r
                 JOIN knowledge_records m ON m.record_id = r.record_id
                 WHERE r.revision_id = ?1",
                params![revision_id],
                |row| row.get(0),
            )
            .optional()?;
        if !matches!(kind.as_deref(), Some("source" | "lens")) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn any_record_kind(tx: &Transaction<'_>, record_id: &str) -> KnowledgeResult<Option<String>> {
    Ok(tx
        .query_row(
            "SELECT kind FROM knowledge_records WHERE record_id = ?1",
            params![record_id],
            |row| row.get(0),
        )
        .optional()?)
}

fn knowledge_id_candidates(text: &str, prefix: &str) -> KnowledgeResult<Vec<String>> {
    let mut found = Vec::new();
    let bytes = text.as_bytes();
    let prefix_bytes = prefix.as_bytes();
    let mut index = 0;
    while index + prefix_bytes.len() + 32 <= bytes.len() {
        if bytes[index..].starts_with(prefix_bytes)
            && boundary_before(text, index)
            && bytes[index + prefix_bytes.len()..index + prefix_bytes.len() + 32]
                .iter()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            && boundary_after(text, index + prefix_bytes.len() + 32)
        {
            found.push(text[index..index + prefix_bytes.len() + 32].to_string());
            if found.len() > 32 {
                return Err(KnowledgeError::LensBodyForbidden);
            }
            index += prefix_bytes.len() + 32;
            continue;
        }
        index += 1;
    }
    Ok(found)
}

fn reserved_knowledge_id(text: &str, prefix: &str) -> bool {
    matches!(knowledge_id_candidates(text, prefix), Ok(found) if !found.is_empty())
        || knowledge_id_candidates(text, prefix).is_err()
}

fn boundary_before(text: &str, index: usize) -> bool {
    index == 0 || !is_id_character(text[..index].chars().next_back().unwrap_or(' '))
}

fn boundary_after(text: &str, index: usize) -> bool {
    text[index..]
        .chars()
        .next()
        .is_none_or(|character| !is_id_character(character))
}

fn is_id_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
}

pub(super) fn enforce_lens_closure(
    tx: &Transaction<'_>,
    loupist_id: &str,
    dependencies: &[EvidenceRevision],
) -> KnowledgeResult<()> {
    let mut stack = Vec::new();
    let mut seen = BTreeMap::new();
    for dependency in dependencies {
        if !dependency.role.allowed_in_lens() {
            return Err(KnowledgeError::LensDependencyForbidden);
        }
        walk_lens_dependency(
            tx,
            loupist_id,
            &dependency.record_id,
            &dependency.revision_digest,
            &mut stack,
            &mut seen,
        )?;
    }
    Ok(())
}

pub(super) fn lens_view(revision: &StoredRevision) -> KnowledgeResult<LensView> {
    Ok(LensView {
        record_id: revision.record_id.clone(),
        revision_id: revision.revision_id.clone(),
        revision_digest: revision.revision_digest.clone(),
        document: from_json(&revision.payload_json)?,
        dependencies: from_json(&revision.dependency_json)?,
    })
}

fn walk_lens_dependency(
    tx: &Transaction<'_>,
    loupist_id: &str,
    record_id: &str,
    digest: &str,
    stack: &mut Vec<String>,
    seen: &mut BTreeMap<String, String>,
) -> KnowledgeResult<()> {
    if stack.iter().any(|existing| existing == record_id) {
        return Err(KnowledgeError::DependencyCycle);
    }
    if let Some(prior) = seen.get(record_id) {
        return if prior == digest {
            Ok(())
        } else {
            Err(KnowledgeError::ConflictingDependencyRevision)
        };
    }
    let module = require_record(tx, loupist_id, record_id)?;
    let revision = visible_by_digest(tx, &module, digest)?;
    stack.push(record_id.to_string());
    match revision.kind {
        RecordKind::World => return Err(KnowledgeError::LensDependencyForbidden),
        RecordKind::Source => {
            let dependencies: Vec<EvidenceRevision> = from_json(&revision.dependency_json)?;
            for dependency in &dependencies {
                walk_lens_dependency(
                    tx,
                    loupist_id,
                    &dependency.record_id,
                    &dependency.revision_digest,
                    stack,
                    seen,
                )?;
            }
        }
        RecordKind::Lens => {
            let document: LensDocument = from_json(&revision.payload_json)?;
            enforce_lens_text(tx, loupist_id, &module.title, &document)?;
            let dependencies: Vec<EvidenceRevision> = from_json(&revision.dependency_json)?;
            for dependency in &dependencies {
                if !dependency.role.allowed_in_lens() {
                    return Err(KnowledgeError::LensDependencyForbidden);
                }
                walk_lens_dependency(
                    tx,
                    loupist_id,
                    &dependency.record_id,
                    &dependency.revision_digest,
                    stack,
                    seen,
                )?;
            }
        }
    }
    stack.pop();
    seen.insert(record_id.to_string(), digest.to_string());
    Ok(())
}

fn validate_lens_document(document: &LensDocument) -> KnowledgeResult<()> {
    validate_text(
        &document.markdown,
        MAX_MARKDOWN_BYTES,
        false,
        "lens markdown is invalid",
    )?;
    for text in [
        &document.applicability,
        &document.assumptions,
        &document.procedure,
        &document.principles,
        &document.limitations,
    ] {
        validate_text(text, MAX_MARKDOWN_BYTES, true, "lens section is invalid")?;
    }
    Ok(())
}

fn validate_change(change: &LensChangeInput) -> KnowledgeResult<()> {
    validate_text(
        &change.rationale,
        16_000,
        false,
        "change rationale is invalid",
    )?;
    if let Some(feedback) = &change.feedback {
        validate_text(feedback, 16_000, true, "change feedback is invalid")?;
    }
    if change.references.len() > 56 {
        return Err(KnowledgeError::Invalid("change inputs are too large"));
    }
    Ok(())
}

fn authorize_change_inputs(
    tx: &Transaction<'_>,
    loupist_id: &str,
    change: &LensChangeInput,
) -> KnowledgeResult<()> {
    for reference in &change.references {
        match reference.kind.as_str() {
            "source" => {
                let record = require_record(tx, loupist_id, &reference.record_id)?;
                let revision = load_revision(tx, &reference.revision_id)?;
                super::authority::revision_in_record(&revision.record_id, &record.record_id)?;
                if revision.kind != RecordKind::Source {
                    return Err(KnowledgeError::Invalid("Lens evidence is not a Source"));
                }
                let payload: super::model::SourceRecordPayload = from_json(&revision.payload_json)?;
                if !(payload.source.source_id == reference.object_id
                    && payload.source.captures.iter().any(|capture| {
                        capture.representations.iter().any(|representation| {
                            representation
                                .spans
                                .iter()
                                .any(|span| Some(&span.span_id) == reference.span_id.as_ref())
                        })
                    }))
                {
                    return Err(KnowledgeError::NotFound);
                }
            }
            "world" => {
                let record_id = require_snapshot(tx, loupist_id, &reference.object_id)?;
                let revision_id: String = tx.query_row(
                    "SELECT revision_id FROM knowledge_world_snapshots WHERE snapshot_id = ?1",
                    [&reference.object_id],
                    |row| row.get(0),
                )?;
                if record_id != reference.record_id || revision_id != reference.revision_id {
                    return Err(KnowledgeError::Invalid(
                        "World evidence revision does not match",
                    ));
                }
            }
            "research" => {
                super::research::read_working_note(tx, loupist_id, &reference.object_id)?;
            }
            "journal" => {
                let owner: Option<String> = tx
                    .query_row(
                        "SELECT loupist_id FROM knowledge_journals WHERE journal_id = ?1",
                        [&reference.object_id],
                        |row| row.get(0),
                    )
                    .optional()?;
                if owner.as_deref() != Some(loupist_id) {
                    return Err(KnowledgeError::Unauthorized(
                        "journal belongs to another Loupist",
                    ));
                }
            }
            _ => {
                return Err(KnowledgeError::Invalid(
                    "Lens change evidence kind is invalid",
                ))
            }
        }
    }
    Ok(())
}

pub(super) fn fold_lens_title(title: &str) -> String {
    title
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

pub(super) fn require_unique_lens_title(
    tx: &Transaction<'_>,
    loupist_id: &str,
    title: &str,
    except_record_id: Option<&str>,
) -> KnowledgeResult<()> {
    let fold = fold_lens_title(title);
    let duplicate: Option<String> = tx
        .query_row(
            "SELECT record_id FROM knowledge_lens_metadata
             WHERE loupist_id = ?1 AND title_fold = ?2 AND archived_at_ms IS NULL AND lens_kind <> 'principle' AND record_id != ?3",
            params![loupist_id, fold, except_record_id.unwrap_or("")],
            |row| row.get(0),
        )
        .optional()?;
    if duplicate.is_some() {
        return Err(KnowledgeError::Invalid(
            "an active Lens already uses this title",
        ));
    }
    Ok(())
}

pub(super) fn replace_lens_links(
    tx: &Transaction<'_>,
    loupist_id: &str,
    record_id: &str,
    document: &LensDocument,
) -> KnowledgeResult<()> {
    tx.execute(
        "DELETE FROM knowledge_lens_links WHERE source_record_id = ?1",
        [record_id],
    )?;
    for target in lens_link_ids(document) {
        tx.execute(
            "INSERT INTO knowledge_lens_links (loupist_id, source_record_id, target_record_id) VALUES (?1, ?2, ?3)",
            params![loupist_id, record_id, target],
        )?;
    }
    Ok(())
}

fn lookup_lens_links(
    tx: &Transaction<'_>,
    loupist_id: &str,
    ids: &BTreeSet<String>,
) -> KnowledgeResult<Vec<LensNavigationLink>> {
    let mut links = Vec::new();
    for record_id in ids {
        let found = tx
            .query_row(
                "SELECT meta.title, head.revision_id, meta.archived_at_ms
                 FROM knowledge_lens_metadata meta
                 JOIN knowledge_record_heads head ON head.record_id = meta.record_id
                 WHERE meta.loupist_id = ?1 AND meta.record_id = ?2",
                params![loupist_id, record_id],
                |row| {
                    Ok(LensNavigationLink {
                        record_id: record_id.clone(),
                        title: row.get(0)?,
                        revision_id: row.get(1)?,
                        archived: row.get::<_, Option<i64>>(2)?.is_some(),
                    })
                },
            )
            .optional()?;
        if let Some(link) = found {
            links.push(link);
        }
    }
    Ok(links)
}

fn current_backlinks(
    tx: &Transaction<'_>,
    loupist_id: &str,
    record_id: &str,
) -> KnowledgeResult<Vec<LensNavigationLink>> {
    let mut statement = tx.prepare(
        "SELECT link.source_record_id, meta.title, head.revision_id
         FROM knowledge_lens_links link
         JOIN knowledge_lens_metadata meta
           ON meta.record_id = link.source_record_id AND meta.loupist_id = link.loupist_id
         JOIN knowledge_record_heads head ON head.record_id = link.source_record_id
         WHERE link.loupist_id = ?1 AND link.target_record_id = ?2 AND meta.archived_at_ms IS NULL
         ORDER BY meta.title, link.source_record_id",
    )?;
    let rows = statement.query_map(params![loupist_id, record_id], |row| {
        Ok(LensNavigationLink {
            record_id: row.get(0)?,
            title: row.get(1)?,
            revision_id: row.get(2)?,
            archived: false,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(KnowledgeError::from)
}

#[allow(clippy::too_many_arguments)]
fn record_metadata_change(
    tx: &Transaction<'_>,
    loupist_id: &str,
    record_id: &str,
    kind: &str,
    rationale: &str,
    at_ms: i64,
    actor: &str,
    origin_chat_id: Option<&str>,
    origin_message_id: Option<&str>,
    new_title: Option<&str>,
    head_revision_id: Option<&str>,
) -> KnowledgeResult<()> {
    let inputs = LensChangeInputs {
        references: Vec::new(),
        feedback: None,
    };
    tx.execute(
        "DELETE FROM knowledge_lens_changes WHERE record_id = ?1",
        [record_id],
    )?;
    tx.execute(
        "INSERT INTO knowledge_lens_changes (
            change_id, loupist_id, record_id, new_revision_id, rationale,
            inputs_json, created_at_ms, change_kind, actor, origin_chat_id, origin_message_id,
            new_title
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            mint_id("chg"),
            loupist_id,
            record_id,
            head_revision_id,
            rationale,
            canonical_string(&to_value(&inputs)?),
            at_ms,
            kind,
            actor,
            origin_chat_id,
            origin_message_id,
            new_title
        ],
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn rename_lens(
    tx: &Transaction<'_>,
    loupist_id: &str,
    record_id: &str,
    title: &str,
    rationale: &str,
    at_ms: i64,
    actor: &str,
    origin_chat_id: Option<&str>,
    origin_message_id: Option<&str>,
) -> KnowledgeResult<String> {
    validate_text(title, 200, false, "lens title is invalid")?;
    validate_text(rationale, 16_000, false, "change rationale is invalid")?;
    let module = require_record(tx, loupist_id, record_id)?;
    if module.kind != RecordKind::Lens {
        return Err(KnowledgeError::Invalid("record is not a Lens"));
    }
    if module.archived_at_ms.is_some() {
        return Err(KnowledgeError::Invalid(
            "restore this Lens before renaming it",
        ));
    }
    let next = title.trim();
    if next == module.title {
        return Ok(module.title);
    }
    require_unique_lens_title(tx, loupist_id, next, Some(record_id))?;
    let revision_id = module
        .head_revision_id
        .clone()
        .ok_or(KnowledgeError::NotFound)?;
    let revision = load_revision(tx, &revision_id)?;
    let document: LensDocument = from_json(&revision.payload_json)?;
    enforce_lens_text(tx, loupist_id, next, &document)?;
    tx.execute(
        "UPDATE knowledge_lens_metadata SET title = ?1, title_fold = ?2, updated_at_ms = ?3 WHERE record_id = ?4 AND loupist_id = ?5",
        params![next, fold_lens_title(next), at_ms, record_id, loupist_id],
    )?;
    super::search_index::index_payload(
        tx,
        None,
        record_id,
        &revision_id,
        RecordKind::Lens,
        next,
        &revision
            .payload_json
            .parse()
            .map_err(|_| KnowledgeError::Invalid("lens payload is invalid"))?,
    )?;
    record_metadata_change(
        tx,
        loupist_id,
        record_id,
        "rename",
        rationale,
        at_ms,
        actor,
        origin_chat_id,
        origin_message_id,
        Some(next),
        Some(&revision_id),
    )?;
    let mut event = super::growth::GrowthEvent::new(
        loupist_id,
        record_id,
        at_ms,
        "lens_renamed",
        actor,
        rationale,
    );
    event.before_summary = Some(super::growth::one_line(&module.title));
    event.after_summary = Some(super::growth::one_line(next));
    event.origin_chat_id = origin_chat_id.map(str::to_owned);
    event.origin_message_id = origin_message_id.map(str::to_owned);
    event.insert(tx)?;
    Ok(next.to_string())
}

pub(super) fn set_lens_archived(
    tx: &Transaction<'_>,
    loupist_id: &str,
    record_id: &str,
    archived: bool,
    rationale: &str,
    at_ms: i64,
    actor: &str,
) -> KnowledgeResult<()> {
    validate_text(rationale, 16_000, false, "change rationale is invalid")?;
    let module = require_record(tx, loupist_id, record_id)?;
    if module.kind != RecordKind::Lens {
        return Err(KnowledgeError::Invalid("record is not a Lens"));
    }
    let standing = tx
        .query_row(
            "SELECT record_id FROM knowledge_standing_selection WHERE loupist_id = ?1",
            [loupist_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if archived && standing.as_deref() == Some(record_id) {
        return Err(KnowledgeError::Invalid(
            "the default perspective cannot be archived",
        ));
    }
    if archived == module.archived_at_ms.is_some() {
        return Ok(());
    }
    if !archived {
        if super::principles::principle_kind(tx, loupist_id, record_id)? {
            let p = super::principles::principle_read(tx, loupist_id, record_id)?;
            super::principles::require_unique_statement(
                tx,
                loupist_id,
                &p.statement,
                Some(record_id),
            )?;
        } else {
            require_unique_lens_title(tx, loupist_id, &module.title, Some(record_id))?;
        }
    }
    tx.execute(
        "UPDATE knowledge_lens_metadata SET archived_at_ms = ?1, updated_at_ms = ?2 WHERE record_id = ?3 AND loupist_id = ?4",
        params![archived.then_some(at_ms), at_ms, record_id, loupist_id],
    )?;
    record_metadata_change(
        tx,
        loupist_id,
        record_id,
        if archived { "archive" } else { "restore" },
        rationale,
        at_ms,
        actor,
        None,
        None,
        None,
        module.head_revision_id.as_deref(),
    )?;
    let mut event = super::growth::GrowthEvent::new(
        loupist_id,
        record_id,
        at_ms,
        if archived {
            "lens_archived"
        } else {
            "lens_restored"
        },
        actor,
        rationale,
    );
    event.before_summary = Some(super::growth::one_line(&format!(
        "{}: {}",
        module.title,
        if archived { "active" } else { "archived" }
    )));
    event.after_summary = Some(super::growth::one_line(&format!(
        "{}: {}",
        module.title,
        if archived { "archived" } else { "active" }
    )));
    if super::principles::principle_kind(tx, loupist_id, record_id)? {
        event.origin = Some("settings".into());
    }
    event.insert(tx)
}

pub(super) fn list_lens_drafts(
    tx: &Transaction<'_>,
    loupist_id: &str,
    record_id: Option<&str>,
    author: Option<&str>,
) -> KnowledgeResult<Vec<LensDraftRecord>> {
    validate_loupist_id(loupist_id)?;
    let mut statement = tx.prepare(
        "SELECT draft_id, loupist_id, author, record_id, base_revision_id, document_json, rationale,
                title, origin_chat_id, origin_message_id, updated_at_ms
         FROM knowledge_lens_drafts
         WHERE loupist_id = ?1
           AND (?2 = '' OR record_id = ?2)
           AND (?3 = '' OR author = ?3)
         ORDER BY updated_at_ms DESC, draft_id",
    )?;
    let rows = statement.query_map(
        params![loupist_id, record_id.unwrap_or(""), author.unwrap_or("")],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, Option<String>>(8)?,
                row.get::<_, Option<String>>(9)?,
                row.get::<_, i64>(10)?,
            ))
        },
    )?;
    let mut drafts = Vec::new();
    for row in rows {
        let (
            draft_id,
            loupist_id,
            author,
            record_id,
            base_revision_id,
            document_json,
            rationale,
            title,
            origin_chat_id,
            origin_message_id,
            updated_at_ms,
        ) = row?;
        drafts.push(LensDraftRecord {
            draft_id,
            loupist_id,
            author,
            record_id,
            base_revision_id,
            document: from_json(&document_json)?,
            rationale,
            title,
            origin_chat_id,
            origin_message_id,
            updated_at_ms,
        });
    }
    Ok(drafts)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn save_lens_proposal(
    tx: &Transaction<'_>,
    loupist_id: &str,
    record_id: &str,
    base_revision_id: &str,
    document: &LensDocument,
    rationale: &str,
    title: Option<&str>,
    origin_chat_id: &str,
    origin_message_id: &str,
    at_ms: i64,
) -> KnowledgeResult<LensDraftRecord> {
    let module = require_record(tx, loupist_id, record_id)?;
    if module.kind != RecordKind::Lens {
        return Err(KnowledgeError::Invalid("record is not a Lens"));
    }
    validate_lens_document(document)?;
    let draft_id = mint_id("dft");
    tx.execute(
        "INSERT INTO knowledge_lens_drafts (
            draft_id, loupist_id, author, record_id, base_revision_id, document_json, rationale,
            title, origin_chat_id, origin_message_id, updated_at_ms
         ) VALUES (?1, ?2, 'loupist', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            draft_id,
            loupist_id,
            record_id,
            base_revision_id,
            canonical_string(&to_value(document)?),
            rationale,
            title,
            origin_chat_id,
            origin_message_id,
            at_ms
        ],
    )?;
    Ok(LensDraftRecord {
        draft_id,
        loupist_id: loupist_id.to_string(),
        author: "loupist".into(),
        record_id: record_id.to_string(),
        base_revision_id: base_revision_id.to_string(),
        document: document.clone(),
        rationale: rationale.to_string(),
        title: title.map(str::to_string),
        origin_chat_id: Some(origin_chat_id.to_string()),
        origin_message_id: Some(origin_message_id.to_string()),
        updated_at_ms: at_ms,
    })
}

pub(super) fn dismiss_lens_proposal(
    tx: &Transaction<'_>,
    loupist_id: &str,
    draft_id: &str,
) -> KnowledgeResult<()> {
    validate_knowledge_id(draft_id, "dft")?;
    let changed = tx.execute(
        "DELETE FROM knowledge_lens_drafts WHERE draft_id = ?1 AND loupist_id = ?2 AND author = 'loupist'",
        params![draft_id, loupist_id],
    )?;
    if changed == 1 {
        Ok(())
    } else {
        Err(KnowledgeError::NotFound)
    }
}

pub(super) fn lens_listing(
    tx: &Transaction<'_>,
    loupist_id: &str,
    record_id: &str,
) -> KnowledgeResult<(i64, bool, u32, bool)> {
    let (updated_at_ms, archived_at_ms, learned) = tx.query_row(
        "SELECT updated_at_ms, archived_at_ms, learned FROM knowledge_lens_metadata WHERE loupist_id = ?1 AND record_id = ?2",
        params![loupist_id, record_id],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?, row.get::<_, bool>(2)?)),
    )?;
    let proposals: i64 = tx.query_row(
        "SELECT COUNT(*) FROM knowledge_lens_drafts WHERE loupist_id = ?1 AND record_id = ?2 AND author = 'loupist'",
        params![loupist_id, record_id],
        |row| row.get(0),
    )?;
    Ok((
        updated_at_ms,
        archived_at_ms.is_some(),
        proposals as u32,
        learned,
    ))
}

pub(super) fn archived_lens_ids(
    tx: &Transaction<'_>,
    loupist_id: &str,
) -> KnowledgeResult<BTreeSet<String>> {
    let mut statement = tx.prepare(
        "SELECT record_id FROM knowledge_lens_metadata WHERE loupist_id = ?1 AND archived_at_ms IS NOT NULL",
    )?;
    let ids = statement
        .query_map(params![loupist_id], |row| row.get::<_, String>(0))?
        .collect::<Result<BTreeSet<_>, _>>()?;
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::portable_lens_text_forbidden;
    use crate::knowledge::{
        Authorship, Coverage, CreateRecord, KnowledgeError, KnowledgeStore, LensChangeInput,
        LensDocument, RecordKind, RepresentationRole, RetainMaterial, RetainNewSource, ReviseLens,
        SealWorld, Selector, SourceClass,
    };

    fn create(
        store: &KnowledgeStore,
        kind: RecordKind,
        title: &str,
    ) -> crate::knowledge::KnowledgeResult<String> {
        store
            .create_record(CreateRecord {
                loupist_id: "loupist-a".into(),
                kind,
                title: title.into(),
                at_ms: 1,
            })
            .map(|record| record.record_id)
    }

    #[test]
    fn lens_rejects_world_snapshot_and_journal_ids_and_keeps_source_links() {
        let directory = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(directory.path().join("knowledge.sqlite")).unwrap();
        let bytes = b"account text";
        let source = store
            .retain_new_source(RetainNewSource {
                origin: None,
                loupist_id: "loupist-a".into(),
                at_ms: 2,
                title: "Notes".into(),
                source_class: SourceClass::UserStatement,
                origin_locator: "pasted".into(),
                provider_identity: "editor".into(),
                authorship: Authorship::User,
                coverage: Coverage::IncludedFull,
                origin_chat_id: None,
                request_ref: None,
                declared_digest: None,
                publication_time: None,
                material: Some(RetainMaterial {
                    media_type: "text/plain".into(),
                    role: RepresentationRole::Original,
                    document_id: "notes.txt".into(),
                    parser_version: None,
                    parent_representation_id: None,
                    input_span_ids: Vec::new(),
                    bytes: bytes.to_vec(),
                    selector: Selector::ByteRange {
                        start: 0,
                        end: bytes.len() as u64,
                    },
                    context_note: "Original text".into(),
                }),
            })
            .unwrap();
        let lens = create(&store, RecordKind::Lens, "Method").unwrap();
        let linked = store
            .revise_lens(lens_revision(
                &lens,
                None,
                format!("Use source {} when comparing accounts.", source.record_id),
            ))
            .unwrap();
        let prose = store
            .revise_lens(lens_revision(
                &lens,
                Some(linked.revision_id),
                "Compare the world of the accounts before preferring one.".into(),
            ))
            .unwrap();
        assert!(!portable_lens_text_forbidden(&prose.document.markdown));

        let world = create(&store, RecordKind::World, "Accounts").unwrap();
        let sealed = store
            .seal_world(SealWorld {
                loupist_id: "loupist-a".into(),
                record_id: world.clone(),
                expected_revision_id: None,
                at_ms: 5,
                nodes: Vec::new(),
                claims: Vec::new(),
                annotations: Vec::new(),
                dependencies: Vec::new(),
            })
            .unwrap();
        for markdown in [
            format!("Apply after reading snapshot {}.", sealed.snapshot_id),
            format!("Use world revision {}.", sealed.revision_id),
            format!("Open world {world}."),
            "See world://accounts before applying this method.".into(),
            format!("Cite journal jnl_{}.", "ab".repeat(16)),
            format!("Open module rec_{}.", "cd".repeat(16)),
        ] {
            let request = lens_revision(&lens, Some(prose.revision_id.clone()), markdown);
            let error = store.revise_lens(request).unwrap_err();
            assert!(
                matches!(error, KnowledgeError::LensBodyForbidden),
                "{error}"
            );
        }
        let title = create(&store, RecordKind::Lens, "Method for journal://private");
        assert!(matches!(title, Err(KnowledgeError::LensBodyForbidden)));
    }

    #[test]
    fn lens_rename_rejects_collisions_and_archive_hides_backlinks() {
        let directory = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(directory.path().join("knowledge.sqlite")).unwrap();
        let lens = create(&store, RecordKind::Lens, "Method").unwrap();
        let related = create(&store, RecordKind::Lens, "Related method").unwrap();
        let saved = store
            .revise_lens(lens_revision(
                &lens,
                None,
                format!("Start from [Related method](lens:{related})."),
            ))
            .unwrap();
        store
            .revise_lens(lens_revision(
                &related,
                None,
                format!("Check [Method](lens:{lens})."),
            ))
            .unwrap();
        let navigation = |store: &KnowledgeStore| {
            store
                .read(|write| write.lens_navigation("loupist-a", &lens, &saved.document))
                .unwrap()
        };
        assert_eq!(navigation(&store).backlinks[0].record_id, related);
        let rename = |title: &str| {
            store.transact(|write| {
                write.rename_lens(
                    "loupist-a",
                    &lens,
                    title,
                    "Clearer name.",
                    8,
                    "user",
                    None,
                    None,
                )
            })
        };
        assert_eq!(rename("Renamed method").unwrap(), "Renamed method");
        assert!(matches!(
            rename("related method"),
            Err(KnowledgeError::Invalid(_))
        ));
        store
            .transact(|write| {
                write.set_lens_archived("loupist-a", &related, true, "Hide it.", 10, "user")
            })
            .unwrap();
        let after = navigation(&store);
        assert!(after.backlinks.is_empty());
        assert!(after.links[0].archived);
    }

    fn lens_revision(record_id: &str, expected: Option<String>, markdown: String) -> ReviseLens {
        ReviseLens {
            loupist_id: "loupist-a".into(),
            record_id: record_id.to_string(),
            expected_revision_id: expected,
            at_ms: 20,
            document: LensDocument {
                markdown,
                applicability: String::new(),
                assumptions: String::new(),
                procedure: String::new(),
                principles: String::new(),
                limitations: String::new(),
            },
            dependencies: Vec::new(),
            change: LensChangeInput {
                rationale: "Check the reference.".into(),
                references: Vec::new(),
                feedback: None,
            },
        }
    }
}
