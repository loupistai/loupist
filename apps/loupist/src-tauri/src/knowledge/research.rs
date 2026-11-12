use std::collections::BTreeMap;

use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};

use super::canonical::{canonical_string, digest_bytes, digest_json, from_json, to_value};
use super::error::{KnowledgeError, KnowledgeResult};
use super::model::{
    mint_id, validate_external_id, validate_knowledge_id, validate_loupist_id, validate_text,
    validate_time, validate_time_point, validate_token, EarlierResearch, JournalDraft,
    JournalInputs, JournalLensInput, JournalRecord, JournalSourceInput, JournalWorldInput,
    LaterNote, OpenTurn, ReadPurpose, ResearchClaimPin, ResearchHit, ResearchSearch, SaveResearch,
    SavedResearch, WorkingNote, WorkingNoteDraft, WorldGraphView,
};
use super::source::load_receipt;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResearchBody {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    inputs: Option<JournalInputs>,
    #[serde(default)]
    previous_note_ids: Vec<String>,
    observations: String,
    interpretation: String,
    assumptions: String,
    evaluation: String,
    journal_id: Option<String>,
    input_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    working_note: Option<WorkingNoteDraft>,
}

struct TurnRecord {
    loupist_id: String,
    chat_session_id: String,
}

pub(super) fn open_turn(tx: &Transaction<'_>, request: OpenTurn) -> KnowledgeResult<()> {
    validate_loupist_id(&request.loupist_id)?;
    validate_external_id(&request.chat_session_id, "chat id is invalid")?;
    validate_external_id(&request.turn_id, "turn id is invalid")?;
    validate_time(request.at_ms)?;
    if let Some(existing) = load_turn(tx, &request.turn_id)? {
        if existing.loupist_id != request.loupist_id
            || existing.chat_session_id != request.chat_session_id
        {
            return Err(KnowledgeError::Unauthorized("turn belongs to another chat"));
        }
        return Ok(());
    }
    tx.execute(
        "INSERT INTO knowledge_turns (turn_id, loupist_id, chat_session_id, opened_at_ms)
         VALUES (?1, ?2, ?3, ?4)",
        params![
            request.turn_id,
            request.loupist_id,
            request.chat_session_id,
            request.at_ms
        ],
    )?;
    Ok(())
}

// An acknowledged source receipt binds exact bytes and their complete provenance atomically.
#[allow(clippy::too_many_arguments)]
pub(super) fn record_acked_source(
    tx: &Transaction<'_>,
    loupist_id: &str,
    chat_id: &str,
    turn_id: &str,
    at_ms: i64,
    record_id: &str,
    span_id: &str,
    capture_id: &str,
    representation_id: &str,
    selected_digest: &str,
    revision_id: &str,
    revision_digest: &str,
    attribution: &str,
    coverage: &str,
    publication_time_json: Option<&str>,
    media_type: &str,
    bytes: &[u8],
) -> KnowledgeResult<String> {
    if digest_bytes(bytes) != selected_digest {
        return Err(KnowledgeError::DigestMismatch);
    }
    let receipt_id = mint_id("rcp");
    tx.execute(
        "INSERT INTO knowledge_read_receipts (
            receipt_id, loupist_id, record_id, span_id, capture_id, representation_id, selected_digest,
            purpose, chat_session_id, turn_id, issued_at_ms
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'chat_turn', ?8, ?9, ?10)",
        params![
            receipt_id,
            loupist_id,
            record_id,
            span_id,
            capture_id,
            representation_id,
            selected_digest,
            chat_id,
            turn_id,
            at_ms
        ],
    )?;
    tx.execute(
        "INSERT INTO knowledge_receipt_snapshots (
            receipt_id, revision_id, revision_digest, attribution, coverage,
            publication_time_json, media_type, selected_bytes
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            receipt_id,
            revision_id,
            revision_digest,
            attribution,
            coverage,
            publication_time_json,
            media_type,
            bytes
        ],
    )?;
    Ok(receipt_id)
}

// The World receipt binds the delivered projection to its revision, view digest, and turn.
#[allow(clippy::too_many_arguments)]
pub(super) fn record_acked_world(
    tx: &Transaction<'_>,
    loupist_id: &str,
    turn_id: &str,
    at_ms: i64,
    record_id: &str,
    revision_id: &str,
    revision_digest: &str,
    view_json: &str,
    view_digest: &str,
) -> KnowledgeResult<String> {
    if digest_bytes(view_json.as_bytes()) != view_digest {
        return Err(KnowledgeError::DigestMismatch);
    }
    let delivery_id = mint_id("dlv");
    tx.execute(
        "INSERT INTO knowledge_world_deliveries (
            delivery_id, turn_id, loupist_id, record_id, revision_id, revision_digest,
            view_json, view_digest, issued_at_ms
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            delivery_id,
            turn_id,
            loupist_id,
            record_id,
            revision_id,
            revision_digest,
            view_json,
            view_digest,
            at_ms
        ],
    )?;
    Ok(delivery_id)
}

// The Lens receipt binds its exact body and digest to the consuming turn.
#[allow(clippy::too_many_arguments)]
pub(super) fn record_acked_lens(
    tx: &Transaction<'_>,
    loupist_id: &str,
    turn_id: &str,
    at_ms: i64,
    record_id: &str,
    revision_id: &str,
    revision_digest: &str,
    document_json: &str,
    body_digest: &str,
) -> KnowledgeResult<String> {
    let page: super::lens::PageRead = from_json(document_json)?;
    if digest_bytes(page.delivery_text()?.as_bytes()) != body_digest {
        return Err(KnowledgeError::DigestMismatch);
    }
    let delivery_id = mint_id("dlv");
    tx.execute(
        "INSERT INTO knowledge_lens_deliveries (
            delivery_id, turn_id, loupist_id, record_id, revision_id, revision_digest,
            document_json, body_digest, issued_at_ms
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            delivery_id,
            turn_id,
            loupist_id,
            record_id,
            revision_id,
            revision_digest,
            document_json,
            body_digest,
            at_ms
        ],
    )?;
    Ok(delivery_id)
}

pub(super) fn save_research(
    tx: &Transaction<'_>,
    request: SaveResearch,
) -> KnowledgeResult<SavedResearch> {
    validate_loupist_id(&request.loupist_id)?;
    validate_external_id(&request.turn_id, "turn id is invalid")?;
    validate_time(request.at_ms)?;
    let turn = require_turn(tx, &request.turn_id)?;
    if turn.loupist_id != request.loupist_id {
        return Err(KnowledgeError::Unauthorized(
            "turn belongs to another Loupist",
        ));
    }
    for text in [
        &request.observations,
        &request.interpretation,
        &request.assumptions,
        &request.evaluation,
    ] {
        validate_text(text, 100_000, true, "research section is invalid")?;
    }
    if request.observations.trim().is_empty()
        && request.journal.is_none()
        && request.working_note.is_none()
    {
        return Err(KnowledgeError::Invalid("research record is empty"));
    }
    if let Some(note) = &request.working_note {
        super::model::validate_working_note(note)?;
        let actual = working_notes(tx, &request.loupist_id, &turn.chat_session_id, 1)?
            .first()
            .map(|item| item.research_id.clone());
        if note.expected_research_id != actual {
            return Err(KnowledgeError::RevisionConflict {
                expected: note.expected_research_id.clone(),
                actual,
            });
        }
    }
    let research_id = mint_id("rsh");
    let journal = match &request.journal {
        None => None,
        Some(draft) => {
            let mut record = build_journal(tx, &request, draft)?;
            if draft.outcome.is_none() {
                record.research_id = research_id.clone();
            }
            Some(record)
        }
    };
    let note_inputs = request
        .note_inputs
        .as_ref()
        .map(|draft| {
            // A frozen note includes both the note's evidence and the attached
            // journal's independently bounded inputs (32/8/8 each).
            if draft.source_receipt_ids.len() > 64
                || draft.world_delivery_ids.len() > 16
                || draft.lenses.len() > 16
                || draft.previous_note_ids.len() > 16
            {
                return Err(KnowledgeError::Invalid("note inputs are too large"));
            }
            for id in &draft.previous_note_ids {
                let prior = read_working_note_exact(tx, &request.loupist_id, id)?;
                if request.working_note.is_some() && prior.chat_id == turn.chat_session_id {
                    return Err(KnowledgeError::Invalid(
                        "previous notes must belong to another conversation; use the expected note identity for an update",
                    ));
                }
            }
            Ok(Some(JournalInputs {
                sources: draft
                    .source_receipt_ids
                    .iter()
                    .map(|id| copy_receipt(tx, &request, id))
                    .collect::<KnowledgeResult<Vec<_>>>()?,
                worlds: draft
                    .world_delivery_ids
                    .iter()
                    .map(|id| copy_world_delivery(tx, &request, id))
                    .collect::<KnowledgeResult<Vec<_>>>()?,
                lenses: draft
                    .lenses
                    .iter()
                    .map(|lens| copy_lens_delivery(tx, &request, lens))
                    .collect::<KnowledgeResult<Vec<_>>>()?,
                earlier: Vec::new(),
            }))
        })
        .transpose()?
        .flatten();
    // A note has one retained body per Chat. Keep the expected identity only
    // for optimistic concurrency; it is not a link to a historical version.
    let replaces_note = request.working_note.is_some();
    let working_note = request.working_note.map(|mut note| {
        note.expected_research_id = None;
        note
    });
    let body = ResearchBody {
        inputs: note_inputs,
        previous_note_ids: request
            .note_inputs
            .as_ref()
            .map(|item| item.previous_note_ids.clone())
            .unwrap_or_default(),
        observations: request.observations,
        interpretation: request.interpretation,
        assumptions: request.assumptions,
        evaluation: request.evaluation,
        journal_id: journal.as_ref().map(|record| record.journal_id.clone()),
        input_digest: journal.as_ref().map(|record| record.input_digest.clone()),
        working_note,
    };
    if replaces_note {
        tx.execute("UPDATE knowledge_erasure SET armed = 1 WHERE id = 1", [])?;
        tx.execute(
            "DELETE FROM knowledge_research_records
             WHERE loupist_id = ?1 AND origin_chat_id = ?2
               AND json_type(body_json, '$.workingNote') = 'object'",
            params![request.loupist_id, turn.chat_session_id],
        )?;
        tx.execute("UPDATE knowledge_erasure SET armed = 0 WHERE id = 1", [])?;
    }
    let record_digest = digest_json(&to_value(&body)?)?;
    tx.execute(
        "INSERT INTO knowledge_research_records (
            research_id, loupist_id, origin_chat_id, body_json, record_digest, created_at_ms
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            research_id,
            request.loupist_id,
            turn.chat_session_id,
            canonical_string(&to_value(&body)?),
            record_digest,
            request.at_ms
        ],
    )?;
    if let Some(record) = &journal {
        super::journal::persist(tx, record)?;
        super::journal::growth(
            tx,
            record,
            if record.outcome.is_some() {
                "journal_reviewed"
            } else {
                "journal_created"
            },
            None,
        )?;
    }
    Ok(SavedResearch {
        research_id,
        record_digest,
        journal,
    })
}

pub(super) fn working_notes(
    tx: &Transaction<'_>,
    loupist_id: &str,
    chat_id: &str,
    limit: usize,
) -> KnowledgeResult<Vec<WorkingNote>> {
    validate_loupist_id(loupist_id)?;
    validate_external_id(chat_id, "chat id is invalid")?;
    let mut statement = tx.prepare(
        "SELECT research_id, origin_chat_id, body_json, record_digest, created_at_ms
         FROM knowledge_research_records WHERE loupist_id = ?1 AND origin_chat_id = ?2
         AND json_type(body_json, '$.workingNote') = 'object' ORDER BY rowid DESC LIMIT ?3",
    )?;
    let rows = statement.query_map(
        params![loupist_id, chat_id, limit.clamp(1, 50) as i64],
        working_note_row,
    )?;
    rows.map(|row| decode_working_note(row?)).collect()
}

pub(super) fn read_working_note(
    tx: &Transaction<'_>,
    loupist_id: &str,
    research_id: &str,
) -> KnowledgeResult<WorkingNote> {
    let mut note = read_working_note_exact(tx, loupist_id, research_id)?;
    note.later_notes = later_notes(tx, loupist_id, &[research_id])?
        .remove(research_id)
        .unwrap_or_default();
    Ok(note)
}

fn read_working_note_exact(
    tx: &Transaction<'_>,
    loupist_id: &str,
    research_id: &str,
) -> KnowledgeResult<WorkingNote> {
    validate_loupist_id(loupist_id)?;
    validate_knowledge_id(research_id, "rsh")?;
    let row = tx
        .query_row(
            "SELECT research_id, origin_chat_id, body_json, record_digest, created_at_ms
         FROM knowledge_research_records WHERE loupist_id = ?1 AND research_id = ?2",
            params![loupist_id, research_id],
            working_note_row,
        )
        .optional()?
        .ok_or(KnowledgeError::NotFound)?;
    decode_working_note(row)
}

// Bound current rows, rather than rowid distance: replacing a Chat's note can
// leave arbitrarily large gaps between the surviving rows.
const LATER_NOTE_CANDIDATES_SQL: &str =
    "SELECT research_id, origin_chat_id, body_json, record_digest, created_at_ms, rowid
     FROM knowledge_research_records
     WHERE loupist_id = ?1 ORDER BY rowid DESC LIMIT ?2";

fn later_notes(
    tx: &Transaction<'_>,
    loupist_id: &str,
    research_ids: &[&str],
) -> KnowledgeResult<BTreeMap<String, Vec<LaterNote>>> {
    if research_ids.is_empty() {
        return Ok(BTreeMap::new());
    }
    // Batch all anchors in two statements, independent of table size and hits.
    // Primary-key lookups preserve same-Loupist and strictly-later authority.
    let mut anchors = tx.prepare(
        "SELECT research_id, rowid FROM knowledge_research_records
         WHERE research_id IN (SELECT value FROM json_each(?2)) AND loupist_id = ?1",
    )?;
    let anchors = anchors
        .query_map(
            params![loupist_id, serde_json::to_string(research_ids)?],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )?
        .collect::<rusqlite::Result<BTreeMap<_, _>>>()?;
    let mut statement = tx.prepare(LATER_NOTE_CANDIDATES_SQL)?;
    let rows = statement.query_map(params![loupist_id, RESEARCH_SEARCH_CANDIDATES], |row| {
        Ok((working_note_row(row)?, row.get::<_, i64>(5)?))
    })?;
    let mut candidates = Vec::new();
    for row in rows {
        // Discovery is optional. A corrupt later row cannot poison an exact read.
        let Ok((row, rowid)) = row else { continue };
        let Ok(body) = from_json::<ResearchBody>(&row.2) else {
            continue;
        };
        if body.working_note.is_none() {
            continue;
        }
        let Ok(note) = decode_working_note(row) else {
            continue;
        };
        candidates.push((rowid, note));
    }
    candidates.sort_by(|(left_row, left), (right_row, right)| {
        right
            .recorded_at_ms
            .cmp(&left.recorded_at_ms)
            .then_with(|| right_row.cmp(left_row))
    });
    let mut result: BTreeMap<String, Vec<LaterNote>> = BTreeMap::new();
    for (rowid, note) in candidates {
        for id in &note.previous_note_ids {
            if !anchors.get(id).is_some_and(|anchor| *anchor < rowid) {
                continue;
            }
            let later = result.entry(id.clone()).or_default();
            if later.len() < 3 && !later.iter().any(|item| item.note_id == note.research_id) {
                later.push(LaterNote {
                    note_id: note.research_id.clone(),
                    chat_id: note.chat_id.clone(),
                    title: note.title.chars().take(160).collect(),
                    updated_at_ms: note.recorded_at_ms,
                });
            }
        }
    }
    Ok(result)
}

type WorkingNoteRow = (String, String, String, String, i64);

fn working_note_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkingNoteRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
    ))
}

fn decode_working_note(
    (research_id, chat_id, body, expected_digest, recorded_at_ms): WorkingNoteRow,
) -> KnowledgeResult<WorkingNote> {
    let value: serde_json::Value = from_json(&body)?;
    if digest_json(&value)? != expected_digest {
        return Err(KnowledgeError::DigestMismatch);
    }
    let parsed: ResearchBody = serde_json::from_value(value)?;
    let note = parsed.working_note.unwrap_or_else(|| {
        let mut markdown = [
            parsed.observations,
            parsed.interpretation,
            parsed.assumptions,
            parsed.evaluation,
        ]
        .into_iter()
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
        if markdown.len() > 16_000 {
            let mut end = 15_900;
            while !markdown.is_char_boundary(end) {
                end -= 1;
            }
            markdown.truncate(end);
            markdown.push_str("\n\n[Excerpt of the saved note]");
        }
        WorkingNoteDraft {
            title: markdown
                .lines()
                .next()
                .unwrap_or("Saved work")
                .chars()
                .take(160)
                .collect(),
            markdown,
            change_summary: "Saved work.".into(),
            expected_research_id: None,
        }
    });
    Ok(WorkingNote {
        later_notes: Vec::new(),
        inputs: parsed.inputs,
        previous_note_ids: parsed.previous_note_ids,
        research_id,
        chat_id,
        title: note.title,
        markdown: note.markdown,
        change_summary: note.change_summary,
        recorded_at_ms,
    })
}

pub(super) fn get_journal(
    tx: &Transaction<'_>,
    loupist_id: &str,
    journal_id: &str,
) -> KnowledgeResult<JournalRecord> {
    validate_loupist_id(loupist_id)?;
    validate_knowledge_id(journal_id, "jnl")?;
    let row = tx
        .query_row(
            "SELECT loupist_id, body_json FROM knowledge_journals WHERE journal_id = ?1",
            [journal_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    let Some((owner, body)) = row else {
        return Err(KnowledgeError::NotFound);
    };
    if owner != loupist_id {
        return Err(KnowledgeError::Unauthorized(
            "journal belongs to another Loupist",
        ));
    }
    from_json(&body)
}

pub(super) fn read_journal_source(
    tx: &Transaction<'_>,
    blobs: &super::blob::BlobStore,
    loupist_id: &str,
    journal_id: &str,
    receipt_id: &str,
) -> KnowledgeResult<(Vec<u8>, String, JournalSourceInput)> {
    let journal = get_journal(tx, loupist_id, journal_id)?;
    let pinned = journal
        .inputs
        .sources
        .into_iter()
        .find(|source| source.receipt_id == receipt_id)
        .ok_or(KnowledgeError::NotFound)?;
    if let Some((media_type, bytes)) = tx
        .query_row(
            "SELECT media_type, selected_bytes FROM knowledge_receipt_snapshots WHERE receipt_id = ?1",
            [receipt_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)),
        )
        .optional()?
    {
        if digest_bytes(&bytes) != pinned.selected_digest {
            return Err(KnowledgeError::DigestMismatch);
        }
        return Ok((bytes, media_type, pinned));
    }
    let (bytes, media_type) = super::source::read_span_bytes_unbound(
        tx,
        blobs,
        &pinned.record_id,
        &pinned.span_id,
        &pinned.selected_digest,
    )?;
    Ok((bytes, media_type, pinned))
}

// Bound the SQL candidate window as well as deserialized search results.
const RESEARCH_SEARCH_CANDIDATES: u32 = 1000;

/// Newest private research for one Loupist within the bounded candidate window.
///
/// An empty `query` lists recent records. `claim_id` keeps journals whose
/// frozen world edges use that claim. `limit` applies after both filters.
pub(super) fn research_history(
    tx: &Transaction<'_>,
    loupist_id: &str,
    query: &str,
    claim_id: Option<&str>,
    limit: Option<u32>,
) -> KnowledgeResult<ResearchSearch> {
    search_research(tx, loupist_id, query, claim_id, limit, false, true)
}

/// Desktop DTOs omit discovery pointers; do not scan for discarded results.
pub(super) fn research_history_without_later_notes(
    tx: &Transaction<'_>,
    loupist_id: &str,
    query: &str,
    claim_id: Option<&str>,
    limit: Option<u32>,
) -> KnowledgeResult<ResearchSearch> {
    search_research(tx, loupist_id, query, claim_id, limit, false, false)
}

/// Current working notes only. Journals and legacy research stay out of the page.
pub(super) fn search_working_notes(
    tx: &Transaction<'_>,
    loupist_id: &str,
    query: &str,
    limit: Option<u32>,
) -> KnowledgeResult<ResearchSearch> {
    // Relevance ranking reads the exact notes afterwards, including pointers.
    search_research(tx, loupist_id, query, None, limit, true, false)
}

fn search_research(
    tx: &Transaction<'_>,
    loupist_id: &str,
    query: &str,
    claim_id: Option<&str>,
    limit: Option<u32>,
    notes_only: bool,
    include_later_notes: bool,
) -> KnowledgeResult<ResearchSearch> {
    validate_loupist_id(loupist_id)?;
    if query.chars().count() > super::RESEARCH_QUERY_MAX_CHARS {
        return Err(KnowledgeError::Invalid("research query is invalid"));
    }
    let query = query.trim();
    let claim_id = match claim_id {
        Some(claim_id) => {
            validate_token(claim_id, 128, "research claim is invalid")?;
            Some(claim_id)
        }
        None => None,
    };
    if let Some(limit) = limit {
        if limit == 0 || limit > 40 {
            return Err(KnowledgeError::Invalid("research page limit is invalid"));
        }
    }
    let needle = query.to_lowercase();
    let terms = research_query_terms(&needle);
    // Stopword-only text still matches nothing. A short content token such as
    // "GM" has no lexical term, so the scan falls through to an exact boundary match.
    if !needle.is_empty() && terms.is_empty() && !research_has_content_token(&needle) {
        return Ok(ResearchSearch {
            loupist_id: loupist_id.into(),
            query: query.into(),
            records_scanned: 0,
            hits: Vec::new(),
        });
    }
    let recent_only = needle.is_empty() && claim_id.is_none();
    // Bound deserialization and matching on the turn hot path, including when
    // the requested query has no hits. Each kind admits at most this many rows.
    let scan_limit = if recent_only {
        limit.unwrap_or(RESEARCH_SEARCH_CANDIDATES)
    } else {
        RESEARCH_SEARCH_CANDIDATES
    };
    let mut ranked = Vec::new();
    let mut records_scanned = 0_u32;
    if !notes_only {
        records_scanned = records_scanned.saturating_add(collect_journals(
            tx,
            loupist_id,
            &needle,
            claim_id,
            scan_limit,
            &mut ranked,
        )?);
    }
    if claim_id.is_none() {
        records_scanned = records_scanned.saturating_add(collect_research_notes(
            tx,
            loupist_id,
            &needle,
            &terms,
            scan_limit,
            notes_only,
            &mut ranked,
        )?);
    }
    ranked.sort_by(|left, right| {
        right
            .match_score
            .cmp(&left.match_score)
            .then_with(|| right.at_ms.cmp(&left.at_ms))
            .then_with(|| right.sort_id.cmp(&left.sort_id))
    });
    if let Some(limit) = limit {
        ranked.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
    }
    if include_later_notes {
        let ids: Vec<_> = ranked
            .iter()
            .filter(|item| item.hit.journal_id.is_none())
            .map(|item| item.hit.research_id.as_str())
            .collect();
        let mut later = later_notes(tx, loupist_id, &ids)?;
        for item in &mut ranked {
            item.hit.later_notes = later.remove(&item.hit.research_id).unwrap_or_default();
        }
    }
    Ok(ResearchSearch {
        loupist_id: loupist_id.to_string(),
        query: query.to_string(),
        records_scanned,
        hits: ranked.into_iter().map(|item| item.hit).collect(),
    })
}

struct RankedHit {
    match_score: usize,
    at_ms: i64,
    sort_id: String,
    hit: ResearchHit,
}

fn collect_journals(
    tx: &Transaction<'_>,
    loupist_id: &str,
    needle: &str,
    claim_id: Option<&str>,
    limit: u32,
    ranked: &mut Vec<RankedHit>,
) -> KnowledgeResult<u32> {
    let mut statement = tx.prepare(
        "SELECT body_json, created_at_ms, journal_id FROM (
           SELECT body_json, created_at_ms, journal_id FROM knowledge_journals
           WHERE loupist_id = ?1 ORDER BY rowid DESC LIMIT ?3
         )
         ORDER BY created_at_ms DESC, journal_id DESC LIMIT ?2",
    )?;
    let rows = statement.query_map(
        params![loupist_id, limit, RESEARCH_SEARCH_CANDIDATES],
        journal_row,
    )?;
    let mut scanned = 0_u32;
    for row in rows {
        let (body, at_ms, journal_id) = row?;
        scanned = scanned.saturating_add(1);
        let record: JournalRecord = from_json(&body)?;
        if record.archived {
            continue;
        }
        if let Some(hit) = journal_hit(&record, at_ms, needle, claim_id) {
            ranked.push(RankedHit {
                match_score: if needle.is_empty() { 0 } else { usize::MAX },
                at_ms,
                sort_id: journal_id,
                hit,
            });
        }
    }
    Ok(scanned)
}

fn journal_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<(String, i64, String)> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
}

fn collect_research_notes(
    tx: &Transaction<'_>,
    loupist_id: &str,
    needle: &str,
    terms: &[String],
    limit: u32,
    notes_only: bool,
    ranked: &mut Vec<RankedHit>,
) -> KnowledgeResult<u32> {
    // Current notes are unique per Chat, so no historical anti-join is needed.
    let mut statement = tx.prepare(
        "SELECT research_id, body_json, created_at_ms, origin_chat_id FROM (
           SELECT research_id, body_json, created_at_ms, origin_chat_id, rowid AS candidate_rowid
           FROM knowledge_research_records WHERE loupist_id = ?1
             AND (?3 = 0 OR json_type(body_json, '$.workingNote') = 'object')
           ORDER BY rowid DESC LIMIT ?4
         ) ORDER BY created_at_ms DESC, candidate_rowid DESC LIMIT ?2",
    )?;
    let rows = statement.query_map(
        params![loupist_id, limit, notes_only, RESEARCH_SEARCH_CANDIDATES],
        research_row,
    )?;
    let mut scanned = 0_u32;
    for row in rows {
        let (research_id, body, at_ms, chat_id) = row?;
        scanned = scanned.saturating_add(1);
        let parsed: ResearchBody = from_json(&body)?;
        if parsed.journal_id.is_some() && parsed.working_note.is_none() {
            continue;
        }
        let Some(match_score) = research_note_score(&parsed, needle, terms) else {
            continue;
        };
        let conclusion = if let Some(note) = &parsed.working_note {
            format!("{}\n\n{}", note.title, note.markdown)
        } else if parsed.interpretation.trim().is_empty() {
            [
                &parsed.observations,
                &parsed.assumptions,
                &parsed.evaluation,
            ]
            .into_iter()
            .find(|text| !text.trim().is_empty())
            .map(|text| text.lines().next().unwrap_or("").to_string())
            .unwrap_or_default()
        } else {
            parsed.interpretation
        };
        ranked.push(RankedHit {
            match_score,
            at_ms,
            sort_id: research_id.clone(),
            hit: ResearchHit {
                later_notes: Vec::new(),
                research_id,
                chat_id: Some(chat_id),
                journal_id: None,
                conclusion,
                uncertainty: String::new(),
                recorded_at_ms: at_ms,
                world_view_digests: Vec::new(),
                lens_bodies: Vec::new(),
                claims: Vec::new(),
            },
        });
    }
    Ok(scanned)
}

fn research_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<(String, String, i64, String)> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
}

fn journal_hit(
    record: &JournalRecord,
    at_ms: i64,
    needle: &str,
    claim_id: Option<&str>,
) -> Option<ResearchHit> {
    let mut claims = Vec::new();
    let mut claim_match = claim_id.is_none();
    let mut text_match = needle.is_empty()
        || folded_contains(&record.judgment, needle)
        || folded_contains(&record.reasoning, needle);
    for world in &record.inputs.worlds {
        for edge in &world.edges {
            let matched = claim_id.is_some_and(|id| id == edge.claim.id || id == edge.id);
            if matched {
                claim_match = true;
            }
            if !text_match && folded_contains(&edge.claim.proposition, needle) {
                text_match = true;
            }
            let pin = ResearchClaimPin {
                record_id: world.record_id.clone(),
                revision_id: world.revision_id.clone(),
                claim_id: edge.claim.id.clone(),
                label: edge.claim.proposition.chars().take(80).collect(),
            };
            if matched {
                claims.insert(0, pin);
                claims.truncate(32);
            } else if claims.len() < 32 {
                claims.push(pin);
            }
        }
    }
    if !text_match {
        text_match = record
            .inputs
            .lenses
            .iter()
            .any(|lens| folded_contains(&lens.markdown, needle));
    }
    if !claim_match || !text_match {
        return None;
    }
    Some(ResearchHit {
        later_notes: Vec::new(),
        research_id: record.research_id.clone(),
        chat_id: None,
        journal_id: Some(record.journal_id.clone()),
        conclusion: record.judgment.clone(),
        uncertainty: String::new(),
        recorded_at_ms: at_ms,
        world_view_digests: record
            .inputs
            .worlds
            .iter()
            .map(|world| world.view_digest.clone())
            .collect(),
        lens_bodies: Vec::new(),
        claims,
    })
}

fn research_ignored_token(term: &str) -> bool {
    const STOP: &[&str] = &[
        "a",
        "an",
        "as",
        "at",
        "be",
        "by",
        "do",
        "if",
        "in",
        "is",
        "it",
        "me",
        "my",
        "of",
        "on",
        "or",
        "so",
        "to",
        "us",
        "we",
        "the",
        "and",
        "for",
        "with",
        "from",
        "this",
        "that",
        "what",
        "when",
        "where",
        "which",
        "how",
        "why",
        "are",
        "was",
        "were",
        "has",
        "have",
        "had",
        "can",
        "could",
        "would",
        "should",
        "will",
        "you",
        "your",
        "our",
        "please",
        "tell",
        "about",
        "관련",
        "대한",
        "그리고",
        "알려줘",
        "분석해줘",
    ];
    STOP.contains(&term)
}

fn research_has_content_token(query: &str) -> bool {
    query
        .split(|ch: char| !ch.is_alphanumeric())
        .any(|term| term.chars().count() >= 2 && !research_ignored_token(term))
}

fn research_query_terms(query: &str) -> Vec<String> {
    query
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|term| term.chars().count() >= if term.is_ascii() { 3 } else { 2 })
        .filter(|term| !research_ignored_token(term))
        .map(str::to_owned)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .take(32)
        .collect()
}

fn research_note_score(parsed: &ResearchBody, needle: &str, terms: &[String]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    let mut fields = vec![
        parsed.observations.as_str(),
        parsed.interpretation.as_str(),
        parsed.assumptions.as_str(),
        parsed.evaluation.as_str(),
    ];
    if let Some(note) = &parsed.working_note {
        fields.extend([note.title.as_str(), note.markdown.as_str()]);
    }
    let fields: Vec<_> = fields.into_iter().map(str::to_lowercase).collect();
    // Existing phrase matches keep their recency ordering ahead of all new
    // lexical matches, even a newer note that matches more individual terms.
    // ASCII needles use a word boundary, so "risk" does not exact-match "brisk"
    // and "he" does not exact-match "the".
    if fields
        .iter()
        .any(|field| ascii_bounded_contains(field, needle))
    {
        return Some(usize::MAX);
    }
    let score = terms
        .iter()
        .filter(|term| {
            fields.iter().any(|field| {
                field.match_indices(term.as_str()).any(|(at, _)| {
                    // Hangul suffixes/particles need substring matching; Latin words
                    // need boundaries so e.g. "risk" does not discover "brisk".
                    !term.is_ascii()
                        || (!field[..at]
                            .chars()
                            .next_back()
                            .is_some_and(|ch| ch.is_alphanumeric())
                            && !field[at + term.len()..]
                                .chars()
                                .next()
                                .is_some_and(|ch| ch.is_alphanumeric()))
                })
            })
        })
        .count();
    (score > 0).then_some(score)
}

fn folded_contains(haystack: &str, needle_lower: &str) -> bool {
    ascii_bounded_contains(&haystack.to_lowercase(), needle_lower)
}

fn ascii_bounded_contains(haystack_lower: &str, needle_lower: &str) -> bool {
    if needle_lower.is_empty() {
        return true;
    }
    if !needle_lower.is_ascii() {
        return haystack_lower.contains(needle_lower);
    }
    haystack_lower
        .match_indices(needle_lower)
        .any(|(offset, _)| {
            !haystack_lower[..offset]
                .chars()
                .next_back()
                .is_some_and(|ch| ch.is_alphanumeric())
                && !haystack_lower[offset + needle_lower.len()..]
                    .chars()
                    .next()
                    .is_some_and(|ch| ch.is_alphanumeric())
        })
}

fn build_journal(
    tx: &Transaction<'_>,
    request: &SaveResearch,
    draft: &JournalDraft,
) -> KnowledgeResult<JournalRecord> {
    super::journal::validate_fields(
        &draft.judgment,
        &draft.reasoning,
        &draft.entities,
        draft.check.as_ref(),
        draft.thesis.as_ref(),
    )?;
    super::journal::validate_entities(tx, &request.loupist_id, &draft.entities)?;
    if let Some(thesis) = &draft.thesis {
        if super::assets::get_page(tx, &request.loupist_id, &thesis.asset_id)?.is_none() {
            return Err(super::journal::field(
                "journal_thesis",
                "journal.thesis must name an asset page",
            ));
        }
    }
    let today = super::journal::journal_date(request.at_ms)?;
    if draft.source_receipt_ids.len() > 32
        || draft.world_delivery_ids.len() > 8
        || draft.lenses.len() > 8
        || draft.earlier.len() > 16
    {
        return Err(KnowledgeError::Invalid("journal inputs are too large"));
    }
    let mut seen_receipts = std::collections::BTreeSet::new();
    let mut sources = Vec::new();
    for receipt_id in &draft.source_receipt_ids {
        if !seen_receipts.insert(receipt_id.clone()) {
            return Err(KnowledgeError::Invalid("journal receipt is duplicated"));
        }
        sources.push(copy_receipt(tx, request, receipt_id)?);
    }
    let mut seen_worlds = std::collections::BTreeSet::new();
    let mut worlds = Vec::new();
    for delivery_id in &draft.world_delivery_ids {
        if !seen_worlds.insert(delivery_id.clone()) {
            return Err(KnowledgeError::Invalid("journal delivery is duplicated"));
        }
        worlds.push(copy_world_delivery(tx, request, delivery_id)?);
    }
    let mut seen_lenses = std::collections::BTreeSet::new();
    let mut lenses = Vec::new();
    for lens in &draft.lenses {
        if !seen_lenses.insert(lens.delivery_id.clone()) {
            return Err(KnowledgeError::Invalid("journal delivery is duplicated"));
        }
        lenses.push(copy_lens_delivery(tx, request, lens)?);
    }
    for earlier in &draft.earlier {
        authorize_earlier(tx, &request.loupist_id, earlier)?;
    }
    let inputs = JournalInputs {
        sources,
        worlds,
        lenses,
        earlier: draft.earlier.clone(),
    };
    let input_digest = digest_json(&to_value(&inputs)?)?;
    if let Some(mut reviewed) = super::journal::review(tx, &request.loupist_id, draft, today)? {
        let by_date = reviewed
            .check
            .as_ref()
            .and_then(|check| check.by_date.clone())
            .expect("review validated a dated check");
        let by_date = &by_date;
        if !inputs.sources.iter().any(|source| {
            super::journal::dated_evidence(
                source.publication_time.as_ref(),
                by_date,
                &today.to_string(),
            )
        }) {
            return Err(super::journal::field("journal_review_evidence", "journal review requires acknowledged Source evidence dated on or after byDate and no later than today"));
        }
        reviewed.inputs.sources.extend(inputs.sources);
        reviewed.inputs.worlds.extend(inputs.worlds);
        reviewed.inputs.earlier.extend(inputs.earlier);
        reviewed.input_digest = digest_json(&to_value(&reviewed.inputs)?)?;
        reviewed.updated_at_ms = request.at_ms;
        return Ok(reviewed);
    }
    Ok(JournalRecord {
        check: draft.check.clone(),
        thesis: draft.thesis.clone(),
        outcome: None,
        journal_id: mint_id("jnl"),
        research_id: String::new(),
        loupist_id: request.loupist_id.clone(),
        judgment: draft.judgment.clone(),
        reasoning: draft.reasoning.clone(),
        entities: draft.entities.clone(),
        origin_chat_id: Some(require_turn(tx, &request.turn_id)?.chat_session_id),
        inputs,
        input_digest,
        created_at_ms: request.at_ms,
        updated_at_ms: request.at_ms,
        archived: false,
    })
}

fn copy_receipt(
    tx: &Transaction<'_>,
    request: &SaveResearch,
    receipt_id: &str,
) -> KnowledgeResult<JournalSourceInput> {
    let receipt = load_receipt(tx, receipt_id)?;
    if receipt.loupist_id != request.loupist_id {
        return Err(KnowledgeError::Unauthorized(
            "receipt belongs to another Loupist",
        ));
    }
    let ReadPurpose::ChatTurn {
        chat_session_id,
        turn_id,
    } = &receipt.purpose
    else {
        return Err(KnowledgeError::InspectionReceiptNotCitable);
    };
    let turn = require_turn(tx, &request.turn_id)?;
    if turn.loupist_id != request.loupist_id
        || &turn.chat_session_id != chat_session_id
        || &request.turn_id != turn_id
    {
        return Err(KnowledgeError::Invalid("receipt turn does not match"));
    }
    let record_id: String = tx.query_row(
        "SELECT record_id FROM knowledge_read_receipts WHERE receipt_id = ?1",
        [&receipt.receipt_id],
        |row| row.get(0),
    )?;
    if let Some((revision_id, revision_digest, attribution, coverage, publication_json)) = tx
        .query_row(
            "SELECT revision_id, revision_digest, attribution, coverage, publication_time_json
             FROM knowledge_receipt_snapshots WHERE receipt_id = ?1",
            [&receipt.receipt_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            },
        )
        .optional()?
    {
        let publication_time = match publication_json {
            None => None,
            Some(text) => {
                let time: super::model::TimePoint = from_json(&text)?;
                Some(validate_time_point(&time)?)
            }
        };
        return Ok(JournalSourceInput {
            capture_id: receipt.capture_id,
            representation_id: receipt.representation_id,
            span_id: receipt.span_id,
            selected_digest: receipt.selected_digest,
            receipt_id: receipt.receipt_id,
            record_id,
            revision_id,
            revision_digest,
            attribution,
            coverage,
            publication_time,
        });
    }
    Err(KnowledgeError::Invalid("receipt has no frozen source"))
}

fn copy_world_delivery(
    tx: &Transaction<'_>,
    request: &SaveResearch,
    delivery_id: &str,
) -> KnowledgeResult<JournalWorldInput> {
    validate_knowledge_id(delivery_id, "dlv")?;
    let row = tx
        .query_row(
            "SELECT turn_id, loupist_id, view_json, view_digest
             FROM knowledge_world_deliveries WHERE delivery_id = ?1",
            [delivery_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?;
    let Some((turn_id, loupist_id, view_json, view_digest)) = row else {
        return Err(KnowledgeError::NotFound);
    };
    if loupist_id != request.loupist_id || turn_id != request.turn_id {
        return Err(KnowledgeError::Unauthorized(
            "world delivery belongs to another turn",
        ));
    }
    if digest_bytes(view_json.as_bytes()) != view_digest {
        return Err(KnowledgeError::DigestMismatch);
    }
    let view: WorldGraphView = from_json(&view_json)?;
    Ok(JournalWorldInput {
        delivered_view_json: view_json,
        delivery_id: delivery_id.to_string(),
        record_id: view.record_id,
        revision_id: view.revision_id,
        revision_digest: view.revision_digest,
        view_digest,
        nodes: view.nodes,
        edges: view.edges,
    })
}

fn copy_lens_delivery(
    tx: &Transaction<'_>,
    request: &SaveResearch,
    lens: &super::model::LensAcknowledgement,
) -> KnowledgeResult<JournalLensInput> {
    validate_knowledge_id(&lens.delivery_id, "dlv")?;
    let row = tx
        .query_row(
            "SELECT turn_id, loupist_id, record_id, revision_id, revision_digest, document_json, body_digest
             FROM knowledge_lens_deliveries WHERE delivery_id = ?1",
            [&lens.delivery_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                ))
            },
        )
        .optional()?;
    let Some((
        turn_id,
        loupist_id,
        record_id,
        revision_id,
        revision_digest,
        document_json,
        body_digest,
    )) = row
    else {
        return Err(KnowledgeError::NotFound);
    };
    if loupist_id != request.loupist_id || turn_id != request.turn_id {
        return Err(KnowledgeError::Unauthorized(
            "lens delivery belongs to another turn",
        ));
    }
    let page: super::lens::PageRead = from_json(&document_json)?;
    if digest_bytes(page.delivery_text()?.as_bytes()) != body_digest {
        return Err(KnowledgeError::DigestMismatch);
    }
    Ok(JournalLensInput {
        delivery_id: lens.delivery_id.clone(),
        record_id,
        revision_id,
        revision_digest,
        body_digest,
        markdown: page.markdown(true),
        status: lens.status,
    })
}

fn authorize_earlier(
    tx: &Transaction<'_>,
    loupist_id: &str,
    earlier: &EarlierResearch,
) -> KnowledgeResult<()> {
    if earlier.journal_id.is_none() || !earlier.research_id.is_empty() {
        validate_knowledge_id(&earlier.research_id, "rsh")?;
    }
    super::model::validate_optional_knowledge_id(&earlier.journal_id, "jnl")?;
    if let Some(journal_id) = &earlier.journal_id {
        let linked: Option<(String, String)> = tx
            .query_row(
                "SELECT research_id, loupist_id FROM knowledge_journals WHERE journal_id = ?1",
                [journal_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        match linked {
            Some((research_id, owner))
                if research_id == earlier.research_id && owner == loupist_id => {}
            Some(_) => {
                return Err(KnowledgeError::Unauthorized(
                    "journal belongs to another Loupist",
                ))
            }
            None => return Err(KnowledgeError::NotFound),
        }
    } else {
        let owner: Option<String> = tx
            .query_row(
                "SELECT loupist_id FROM knowledge_research_records WHERE research_id = ?1",
                [&earlier.research_id],
                |row| row.get(0),
            )
            .optional()?;
        if owner.as_deref() != Some(loupist_id) {
            return Err(if owner.is_none() {
                KnowledgeError::NotFound
            } else {
                KnowledgeError::Unauthorized("research belongs to another Loupist")
            });
        }
    }
    Ok(())
}

fn require_turn(tx: &Transaction<'_>, turn_id: &str) -> KnowledgeResult<TurnRecord> {
    load_turn(tx, turn_id)?.ok_or(KnowledgeError::NotFound)
}

fn load_turn(tx: &Transaction<'_>, turn_id: &str) -> KnowledgeResult<Option<TurnRecord>> {
    let row = tx
        .query_row(
            "SELECT loupist_id, chat_session_id FROM knowledge_turns WHERE turn_id = ?1",
            [turn_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    Ok(row.map(|(loupist_id, chat_session_id)| TurnRecord {
        loupist_id,
        chat_session_id,
    }))
}

#[cfg(test)]
mod search_tests {
    use super::*;
    use crate::knowledge::KnowledgeStore;

    fn save_note(
        write: &crate::knowledge::KnowledgeWrite<'_>,
        chat: &str,
        body: &str,
        at_ms: i64,
    ) -> String {
        write
            .open_turn(OpenTurn {
                loupist_id: "loupist-a".into(),
                chat_session_id: chat.into(),
                turn_id: chat.into(),
                at_ms,
            })
            .unwrap();
        write
            .save_research(SaveResearch {
                loupist_id: "loupist-a".into(),
                turn_id: chat.into(),
                at_ms,
                observations: String::new(),
                interpretation: String::new(),
                assumptions: String::new(),
                evaluation: String::new(),
                journal: None,
                note_inputs: None,
                working_note: Some(WorkingNoteDraft {
                    title: chat.into(),
                    markdown: body.into(),
                    change_summary: "Saved premise".into(),
                    expected_research_id: None,
                }),
            })
            .unwrap()
            .research_id
    }

    #[test]
    fn research_queries_count_korean_characters_instead_of_utf8_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(dir.path().join("knowledge.sqlite")).unwrap();
        let query = "가".repeat(super::super::RESEARCH_QUERY_MAX_CHARS);
        let id = store
            .transact(|w| Ok(save_note(w, "korean", &query, 1)))
            .unwrap();
        let found = store
            .research_history("loupist-a", &query, None, Some(20))
            .unwrap();
        assert_eq!(found.hits[0].research_id, id);
        assert!(store
            .research_history("loupist-a", &(query + "나"), None, Some(20))
            .is_err());
    }

    fn insert_later_note(
        tx: &Transaction<'_>,
        anchor: &str,
        loupist_id: &str,
        at_ms: i64,
    ) -> String {
        let id = mint_id("rsh");
        let body = serde_json::json!({
            "previousNoteIds": [anchor], "observations": "", "interpretation": "",
            "assumptions": "", "evaluation": "", "journalId": null, "inputDigest": null,
            "workingNote": {"title": "Correction", "markdown": "Updated premise",
                "changeSummary": "Correction", "expectedResearchId": null}
        });
        tx.execute(
            "INSERT INTO knowledge_research_records
             (research_id, loupist_id, origin_chat_id, body_json, record_digest, created_at_ms)
             VALUES (?1, ?2, 'correction-' || ?1, ?3, ?4, ?5)",
            params![
                id,
                loupist_id,
                canonical_string(&body),
                digest_json(&body).unwrap(),
                at_ms
            ],
        )
        .unwrap();
        id
    }

    #[test]
    fn research_desktop_search_omits_later_note_discovery() {
        let dir = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(dir.path().join("knowledge.sqlite")).unwrap();
        store
            .transact(|write| {
                let anchor = save_note(write, "original", "Original premise", 1);
                insert_later_note(write.tx, &anchor, "loupist-a", 2);
                let chat =
                    write.research_history("loupist-a", "Original premise", None, Some(1))?;
                assert_eq!(chat.hits[0].later_notes.len(), 1);
                let desktop = write.research_history_without_later_notes(
                    "loupist-a",
                    "Original premise",
                    None,
                    Some(1),
                )?;
                assert_eq!(desktop.hits[0].research_id, anchor);
                assert!(desktop.hits[0].later_notes.is_empty());
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn research_history_preserves_exact_phrase_priority_and_ranks_lexical_notes() {
        let dir = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(dir.path().join("knowledge.sqlite")).unwrap();
        let notes = store
            .transact(|write| {
                Ok([
                    ("exact", "Acme covenant headroom is USD 250 million."),
                    ("both", "Headroom of USD 300 million against the covenant."),
                    ("partial", "Headroom is thin."),
                    ("korean", "유동성은 충분하지만 약정의 한도를 점검한다."),
                    ("GM", "GM"),
                    ("AI", "AI valuation"),
                    ("Brisk", "brisk walk"),
                    ("Risk", "portfolio risk"),
                ]
                .into_iter()
                .zip(1..)
                .map(|((chat, body), at_ms)| save_note(write, chat, body, at_ms))
                .collect::<Vec<_>>())
            })
            .unwrap();
        let ids = |loupist: &str, query: &str| {
            store
                .research_history(loupist, query, None, Some(20))
                .unwrap()
                .hits
                .into_iter()
                .map(|hit| hit.research_id)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids("loupist-a", "covenant headroom"), notes[..3]);
        assert_eq!(ids("loupist-a", "유동성 약정"), [notes[3].clone()]);
        // Short exact tokens match whole words, never inside another word.
        assert_eq!(ids("loupist-a", "GM"), [notes[4].clone()]);
        assert_eq!(ids("loupist-a", "AI"), [notes[5].clone()]);
        assert_eq!(ids("loupist-a", "risk"), [notes[7].clone()]);
        assert!(ids("loupist-b", "covenant headroom").is_empty());
        for query in ["the and please", "a to", "?!", "he", "rosemary cultivation"] {
            assert!(
                ids("loupist-a", query).is_empty(),
                "unexpected hits for {query}"
            );
        }
    }

    #[test]
    fn replacing_notes_does_not_evict_other_current_notes_or_corrections() {
        let dir = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(dir.path().join("knowledge.sqlite")).unwrap();
        let anchor = store
            .transact(|write| {
                let anchor = save_note(write, "original", "Original premise", 1);
                insert_later_note(write.tx, &anchor, "loupist-a", 2);
                let mut ids = [
                    save_note(write, "busy-a", "Routine", 3),
                    save_note(write, "busy-b", "Routine", 4),
                ];
                for i in 0..1002 {
                    let slot = i % 2;
                    ids[slot] = write
                        .save_research(SaveResearch {
                            loupist_id: "loupist-a".into(),
                            turn_id: if slot == 0 { "busy-a" } else { "busy-b" }.into(),
                            at_ms: 5 + i as i64,
                            observations: String::new(),
                            interpretation: String::new(),
                            assumptions: String::new(),
                            evaluation: String::new(),
                            journal: None,
                            note_inputs: None,
                            working_note: Some(WorkingNoteDraft {
                                title: "Routine".into(),
                                markdown: "Routine".into(),
                                change_summary: "Updated".into(),
                                expected_research_id: Some(ids[slot].clone()),
                            }),
                        })?
                        .research_id;
                }
                Ok(anchor)
            })
            .unwrap();
        let result = store
            .research_history("loupist-a", "Original premise", None, Some(20))
            .unwrap();
        let hit = result
            .hits
            .iter()
            .find(|hit| hit.research_id == anchor)
            .unwrap();
        assert_eq!(hit.later_notes.len(), 1);
        let note = store
            .read(|w| read_working_note(w.tx, "loupist-a", &anchor))
            .unwrap();
        assert_eq!(note.later_notes.len(), 1);
        let listed = store
            .research_history("loupist-a", "", None, Some(20))
            .unwrap();
        assert_eq!(listed.hits.len(), 4);
    }

    #[test]
    fn candidate_windows_bound_rows_before_sorting_by_recorded_time() {
        let dir = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(dir.path().join("knowledge.sqlite")).unwrap();
        store
            .transact(|write| {
                save_note(write, "outside", "Outside boundary", 10_000);
                super::super::journal::create(
                    write.tx,
                    "loupist-a",
                    super::super::journal::JournalInput {
                        judgment: "Outside boundary".into(),
                        reasoning: String::new(),
                        entities: vec![],
                        check: None,
                        thesis: None,
                    },
                    10_000,
                )?;
                for i in 1..=1000 {
                    save_note(write, &format!("bounded-{i}"), "Routine note", i);
                    super::super::journal::create(
                        write.tx,
                        "loupist-a",
                        super::super::journal::JournalInput {
                            judgment: "Routine judgment".into(),
                            reasoning: String::new(),
                            entities: vec![],
                            check: None,
                            thesis: None,
                        },
                        i,
                    )?;
                }
                Ok(())
            })
            .unwrap();
        let result = store
            .research_history("loupist-a", "Outside boundary", None, Some(20))
            .unwrap();
        assert!(result.hits.is_empty());
        assert_eq!(result.records_scanned, 2000);
        let notes = store
            .read(|w| search_working_notes(w.tx, "loupist-a", "Outside boundary", Some(20)))
            .unwrap();
        assert!(notes.hits.is_empty());
        assert_eq!(notes.records_scanned, 1000);
        let recent = store
            .research_history("loupist-a", "", None, Some(3))
            .unwrap();
        assert_eq!(recent.hits.len(), 3);
        assert_eq!(recent.records_scanned, 6);
    }

    #[test]
    fn research_history_bounds_candidate_scans_even_when_query_has_no_hits() {
        let dir = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(dir.path().join("knowledge.sqlite")).unwrap();
        store
            .transact(|write| {
                for index in 0..1005 {
                    save_note(write, &format!("chat-{index}"), "Routine note", index + 1);
                }
                Ok(())
            })
            .unwrap();
        let result = store
            .research_history("loupist-a", "unmatched liquidity", None, Some(20))
            .unwrap();
        assert!(result.hits.is_empty());
        assert!(result.records_scanned <= 1000);
        assert!(result.records_scanned > 0);
        let recent = store
            .research_history("loupist-a", "", None, Some(3))
            .unwrap();
        assert_eq!(recent.hits.len(), 3);
        assert_eq!(recent.records_scanned, 3);
    }
}
