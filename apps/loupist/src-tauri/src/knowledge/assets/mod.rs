//! One asset page per Loupist, with its Journal thesis and saved price read.
pub mod ladder;
pub(crate) mod saved_read;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use chrono::NaiveDate;
use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub use ladder::Verdict;

use super::{
    model::{mint_id, validate_loupist_id, ClaimStatus, WorldSnapshotBody},
    JournalRecord, KnowledgeError, KnowledgeResult,
};
use crate::{
    commands::assets::wire::{AssetRow, ThinAssetPageView},
    datasets::DatasetRef,
    store::user_context::MapContextItem,
};

pub(crate) fn asset_error(code: &'static str, expected: &'static str) -> KnowledgeError {
    KnowledgeError::AssetField { code, expected }
}

pub const MAX_REASON_CHARS: usize = 280;
pub const MAX_EVIDENCE: usize = 3;

pub(crate) const PAGE_DDL: &str = r#"
CREATE TABLE knowledge_asset_pages (
    loupist_id TEXT NOT NULL,
    asset_id TEXT NOT NULL,
    display_name TEXT NOT NULL,
    entity_id TEXT,
    verdict TEXT,
    verdict_move_id TEXT,
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    revision INTEGER NOT NULL CHECK (revision >= 1),
    entity_source TEXT,
    entity_evidence_json TEXT,
    PRIMARY KEY (loupist_id, asset_id)
) STRICT;
"#;

pub(crate) const DDL: &str = r#"
CREATE TABLE knowledge_asset_moves (
    move_id TEXT PRIMARY KEY NOT NULL,
    loupist_id TEXT NOT NULL,
    asset_id TEXT NOT NULL,
    from_verdict TEXT,
    to_verdict TEXT NOT NULL,
    fact_date TEXT NOT NULL CHECK (length(fact_date) = 10),
    reason TEXT NOT NULL CHECK (length(reason) <= 600),
    evidence_json TEXT NOT NULL,
    reason_id TEXT,
    line_json TEXT,
    actor TEXT NOT NULL,
    origin_chat_id TEXT,
    origin_message_id TEXT,
    at_ms INTEGER NOT NULL CHECK (at_ms >= 0),
    undone INTEGER NOT NULL DEFAULT 0 CHECK (undone IN (0,1)),
    undoes_move_id TEXT,
    seen_at_ms INTEGER
) STRICT;
CREATE INDEX knowledge_asset_moves_by_asset ON knowledge_asset_moves(loupist_id, asset_id, at_ms);
"#;

pub(crate) const READ_DDL: &str = r#"
CREATE TABLE knowledge_asset_line_readings (
    loupist_id TEXT NOT NULL,
    asset_id TEXT NOT NULL,
    journal_id TEXT NOT NULL,
    fact_date TEXT NOT NULL,
    observed REAL NOT NULL,
    evidence_json TEXT NOT NULL,
    recorded_at_ms INTEGER NOT NULL,
    PRIMARY KEY (loupist_id, asset_id, journal_id, fact_date, observed)
) STRICT;
CREATE TABLE knowledge_saved_price_reads (
    loupist_id TEXT NOT NULL,
    asset_id TEXT NOT NULL,
    read_json TEXT NOT NULL,
    price_json TEXT,
    generation TEXT NOT NULL,
    PRIMARY KEY (loupist_id, asset_id)
) STRICT;
CREATE TABLE knowledge_asset_price_points (
    loupist_id TEXT NOT NULL,
    asset_id TEXT NOT NULL,
    fact_date TEXT NOT NULL,
    value REAL NOT NULL,
    PRIMARY KEY (loupist_id, asset_id, fact_date)
) STRICT;
"#;

/// Legacy ID for fixtures that predate the registry.
#[cfg(test)]
pub fn asset_id(symbol: &str, market: &str) -> String {
    let key = format!(
        "{}:{}",
        symbol.trim().to_uppercase(),
        market.trim().to_uppercase()
    );
    format!("ast_{}", &crate::util::sha256(key.as_bytes())[..16])
}

pub fn valid_asset_id(value: &str) -> bool {
    value.len() == 20
        && value.starts_with("ast_")
        && value[4..]
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// Where a row came from: a retained dataset row, an original URL, a Source,
/// a World claim, or a cited result of the turn that recorded it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct AssetEvidence {
    pub tool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub provider: Option<String>,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub dataset: Option<DatasetRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub pointer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub record_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub world_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub citation_url: Option<String>,
    /// The evidence's own date (filing, publication or observation).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub retrieved_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PageRecord {
    pub asset_id: String,
    pub name: String,
    pub entity_id: Option<String>,
    pub verdict: Option<Verdict>,
    pub verdict_move_id: Option<String>,
    pub created_at_ms: i64,
    pub revision: i64,
    rejected_entity_ids: Vec<String>,
}

fn stored_enum<T: serde::de::DeserializeOwned>(index: usize, value: String) -> rusqlite::Result<T> {
    serde_json::from_value(Value::String(value)).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}

fn read_page_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PageRecord> {
    let verdict: Option<String> = row.get(3)?;
    Ok(PageRecord {
        asset_id: row.get(0)?,
        name: row.get(1)?,
        entity_id: row.get(2)?,
        verdict: verdict.map(|value| stored_enum(3, value)).transpose()?,
        verdict_move_id: row.get(4)?,
        created_at_ms: row.get(5)?,
        revision: row.get(6)?,
        rejected_entity_ids: row
            .get::<_, Option<String>>(7)?
            .and_then(|data| serde_json::from_str::<EntityRejection>(&data).ok())
            .map_or_else(Vec::new, |evidence| evidence.rejected_entity_ids),
    })
}

const PAGE_COLUMNS: &str = "asset_id,display_name,entity_id,verdict,verdict_move_id,created_at_ms,revision,entity_evidence_json";

pub fn get_page(
    tx: &Transaction<'_>,
    loupist_id: &str,
    asset_id: &str,
) -> KnowledgeResult<Option<PageRecord>> {
    validate_loupist_id(loupist_id)?;
    Ok(tx
        .query_row(
            &format!("SELECT {PAGE_COLUMNS} FROM knowledge_asset_pages WHERE loupist_id=?1 AND asset_id=?2"),
            params![loupist_id, asset_id],
            read_page_row,
        )
        .optional()?)
}

pub(crate) fn list_pages(
    tx: &Transaction<'_>,
    loupist_id: &str,
) -> KnowledgeResult<Vec<PageRecord>> {
    let mut statement = tx.prepare(&format!(
        "SELECT {PAGE_COLUMNS} FROM knowledge_asset_pages WHERE loupist_id=?1 ORDER BY asset_id"
    ))?;
    let rows = statement.query_map([loupist_id], read_page_row)?;
    Ok(crate::store::collect_readable_rows(rows, "asset pages")?)
}

pub(crate) fn ensure_asset_page(
    tx: &Transaction<'_>,
    loupist_id: &str,
    asset: &AssetRow,
    at_ms: i64,
) -> KnowledgeResult<PageRecord> {
    validate_loupist_id(loupist_id)?;
    if !valid_asset_id(&asset.asset_id) {
        return Err(asset_error("unknown_asset", "Invalid asset ID."));
    }
    tx.execute(
        "INSERT OR IGNORE INTO knowledge_asset_pages(loupist_id,asset_id,display_name,created_at_ms,revision)
         VALUES(?1,?2,?3,?4,1)",
        params![loupist_id, asset.asset_id, asset.name, at_ms],
    )?;
    get_page(tx, loupist_id, &asset.asset_id)?.ok_or(KnowledgeError::NotFound)
}

fn bounded_text(text: &str, chars: usize) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(chars)
        .collect()
}

fn bump(tx: &Transaction<'_>, loupist_id: &str, asset_id: &str) -> KnowledgeResult<()> {
    tx.execute(
        "UPDATE knowledge_asset_pages SET revision=revision+1 WHERE loupist_id=?1 AND asset_id=?2",
        params![loupist_id, asset_id],
    )?;
    Ok(())
}

/// A provider name fills a ticker-only page; an explicit name always wins.
pub fn set_entity(
    tx: &Transaction<'_>,
    loupist_id: &str,
    asset_id: &str,
    entity_id: Option<&str>,
) -> KnowledgeResult<()> {
    tx.execute(
        "UPDATE knowledge_asset_pages SET entity_id=?3,
         entity_source=CASE WHEN ?3 IS NOT NULL THEN 'auto' ELSE entity_source END
         WHERE loupist_id=?1 AND asset_id=?2 AND entity_id IS NOT ?3",
        params![loupist_id, asset_id, entity_id],
    )?;
    Ok(())
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EntityRejection {
    rejected_entity_ids: Vec<String>,
    rejected_at_ms: i64,
}

pub(crate) fn reject_entity(
    tx: &Transaction<'_>,
    loupist: &str,
    asset_id: &str,
    at: i64,
) -> KnowledgeResult<()> {
    let page = get_page(tx, loupist, asset_id)?.ok_or(KnowledgeError::NotFound)?;
    let mut rejected = page.rejected_entity_ids;
    if let Some(entity) = page.entity_id {
        if !rejected.contains(&entity) {
            rejected.push(entity);
        }
    }
    let evidence = EntityRejection {
        rejected_entity_ids: rejected,
        rejected_at_ms: at,
    };
    tx.execute(
        "UPDATE knowledge_asset_pages SET entity_id=NULL,entity_source='user',entity_evidence_json=?3,
         revision=revision+1 WHERE loupist_id=?1 AND asset_id=?2",
        params![loupist, asset_id, serde_json::to_string(&evidence)?],
    )?;
    Ok(())
}

// ───────────────────────────── Thesis: reasons, lines, read ─────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ThesisRole {
    Reason,
    Line,
}

/// A Journal entry's place in an asset's thesis.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JournalThesis {
    pub asset_id: String,
    pub role: ThesisRole,
}

pub fn validate_thesis(thesis: &JournalThesis, has_check: bool) -> KnowledgeResult<()> {
    if !valid_asset_id(&thesis.asset_id) {
        return Err(super::journal::field(
            "journal_thesis",
            "journal.thesis.assetId must name an asset page",
        ));
    }
    validate_thesis_role(Some(thesis.role), has_check)
}

pub(crate) fn validate_thesis_role(
    role: Option<ThesisRole>,
    has_check: bool,
) -> KnowledgeResult<()> {
    if role == Some(ThesisRole::Line) && !has_check {
        return Err(super::journal::field(
            "journal_thesis_line",
            "a line that would change your mind needs a check: claim, metric, direction and threshold",
        ));
    }
    Ok(())
}

fn thesis_entries(
    tx: &Transaction<'_>,
    loupist_id: &str,
    asset_id: &str,
) -> KnowledgeResult<Vec<JournalRecord>> {
    let mut entries: Vec<JournalRecord> = super::journal::list(tx, loupist_id)?
        .into_iter()
        .filter(|entry| {
            !entry.archived
                && entry
                    .thesis
                    .as_ref()
                    .is_some_and(|thesis| thesis.asset_id == asset_id)
        })
        .collect();
    entries.sort_by_key(|entry| (entry.created_at_ms, entry.journal_id.clone()));
    Ok(entries)
}

/// The unit of one of this asset's active lines, for reading its number;
/// `None` when `journal_id` is not one of them.
pub fn line_unit(
    tx: &Transaction<'_>,
    loupist_id: &str,
    asset_id: &str,
    journal_id: &str,
) -> KnowledgeResult<Option<Option<String>>> {
    Ok(thesis_entries(tx, loupist_id, asset_id)?
        .into_iter()
        .find(|entry| {
            entry.journal_id == journal_id
                && entry
                    .thesis
                    .as_ref()
                    .is_some_and(|thesis| thesis.role == ThesisRole::Line)
        })
        .map(|entry| entry.check.and_then(|check| check.unit)))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct LineCrossing {
    pub journal_id: String,
    pub claim: String,
    pub metric: String,
    pub direction: String,
    pub threshold: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub unit: Option<String>,
    pub observed: f64,
    pub crossed: bool,
    /// When the user wrote the line.
    pub set_at_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct MoveView {
    pub move_id: String,
    pub asset_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub from: Option<Verdict>,
    pub to: Verdict,
    pub fact_date: String,
    pub reason: String,
    pub evidence: Vec<AssetEvidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub reason_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub line: Option<LineCrossing>,
    /// `loupist` or `user`.
    pub actor: String,
    pub at_ms: i64,
    pub undone: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub undoes_move_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub origin_chat_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub origin_message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub seen_at_ms: Option<i64>,
}

const MOVE_COLUMNS: &str = "move_id,asset_id,from_verdict,to_verdict,fact_date,reason,evidence_json,reason_id,line_json,actor,at_ms,undone,undoes_move_id,origin_chat_id,origin_message_id,seen_at_ms";

fn read_move(row: &rusqlite::Row<'_>) -> rusqlite::Result<MoveView> {
    let from: Option<String> = row.get(2)?;
    let to: String = row.get(3)?;
    let evidence: String = row.get(6)?;
    let line: Option<String> = row.get(8)?;
    let conversion = |index, error: serde_json::Error| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    };
    Ok(MoveView {
        move_id: row.get(0)?,
        asset_id: row.get(1)?,
        from: from.map(|value| stored_enum(2, value)).transpose()?,
        to: stored_enum(3, to)?,
        fact_date: row.get(4)?,
        reason: row.get(5)?,
        evidence: serde_json::from_str(&evidence).map_err(|e| conversion(6, e))?,
        reason_id: row.get(7)?,
        line: line
            .map(|text| serde_json::from_str(&text).map_err(|e| conversion(8, e)))
            .transpose()?,
        actor: row.get(9)?,
        at_ms: row.get(10)?,
        undone: row.get(11)?,
        undoes_move_id: row.get(12)?,
        origin_chat_id: row.get(13)?,
        origin_message_id: row.get(14)?,
        seen_at_ms: row.get(15)?,
    })
}

/// Every move for an asset, oldest first.
pub fn moves(
    tx: &Transaction<'_>,
    loupist_id: &str,
    asset_id: &str,
) -> KnowledgeResult<Vec<MoveView>> {
    read_moves(tx, loupist_id, asset_id, true)
}

fn read_moves(
    tx: &Transaction<'_>,
    loupist_id: &str,
    asset_id: &str,
    skip_unreadable: bool,
) -> KnowledgeResult<Vec<MoveView>> {
    let mut statement = tx.prepare(&format!(
        "SELECT {MOVE_COLUMNS} FROM knowledge_asset_moves WHERE loupist_id=?1 AND asset_id=?2 ORDER BY at_ms, rowid"
    ))?;
    let rows = statement.query_map(params![loupist_id, asset_id], read_move)?;
    Ok(if skip_unreadable {
        crate::store::collect_readable_rows(rows, "asset moves")?
    } else {
        rows.collect::<Result<Vec<_>, _>>()?
    })
}

/// The read, if a thesis exists: the stored step, or Holding up from the first reason.
fn effective_verdict(page: &PageRecord, reasons: &[JournalRecord]) -> Option<Verdict> {
    if reasons.is_empty() {
        return None;
    }
    Some(page.verdict.unwrap_or(Verdict::Holding))
}

/// A requested change to the read, already checked against this turn's evidence.
#[derive(Clone, Debug, PartialEq)]
pub struct ThesisUpdate {
    pub asset_id: String,
    pub to: Option<Verdict>,
    pub reason: Option<String>,
    pub reason_id: Option<String>,
    /// A reading of one of the user's lines; `observed` was found in the evidence.
    pub line: Option<(String, f64)>,
    pub evidence: Vec<AssetEvidence>,
    /// The latest evidence date; becomes the move's fact date.
    pub fact_date: NaiveDate,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AppliedUpdate {
    pub moved: Option<MoveView>,
    pub reading: Option<LineCrossing>,
    pub name: String,
}

fn crossing_reason(claim: &str, observed: f64, unit: Option<&str>, fact_date: &str) -> String {
    let value = format!(
        "{}{}",
        trim_number(observed),
        unit.map(|unit| if unit == "%" {
            unit.to_owned()
        } else {
            format!(" {unit}")
        })
        .unwrap_or_default()
    );
    bounded_text(
        &format!(
            "Crossed your line: {} ({value}, {fact_date})",
            bounded_text(claim, 160)
        ),
        MAX_REASON_CHARS,
    )
}

fn trim_number(value: f64) -> String {
    let text = format!("{value}");
    text.strip_suffix(".0").map(str::to_owned).unwrap_or(text)
}

fn last_loupist_undone(moves: &[MoveView]) -> Option<(Verdict, Verdict, NaiveDate)> {
    let last = moves.iter().rev().find(|item| item.actor == "loupist")?;
    if !last.undone {
        return None;
    }
    Some((
        last.from.unwrap_or(Verdict::Holding),
        last.to,
        NaiveDate::parse_from_str(&last.fact_date, "%Y-%m-%d").ok()?,
    ))
}

/// Checks an update against the page as it is now, without writing. Shared by
/// the tool (at call time) and the commit (against the committed state).
pub(crate) struct CheckedThesisUpdate {
    pub name: String,
    pub to: Option<Verdict>,
    pub reading: Option<LineCrossing>,
}

pub(crate) fn check_update(
    tx: &Transaction<'_>,
    loupist_id: &str,
    update: &ThesisUpdate,
) -> KnowledgeResult<CheckedThesisUpdate> {
    plan_update(tx, loupist_id, update).map(|planned| CheckedThesisUpdate {
        name: planned.page.name,
        to: planned.to,
        reading: planned.reading,
    })
}

struct PlannedUpdate {
    page: PageRecord,
    current: Verdict,
    to: Option<Verdict>,
    reason: String,
    reading: Option<LineCrossing>,
}

fn plan_update(
    tx: &Transaction<'_>,
    loupist_id: &str,
    update: &ThesisUpdate,
) -> KnowledgeResult<PlannedUpdate> {
    let page = get_page(tx, loupist_id, &update.asset_id)?.ok_or_else(|| {
        asset_error(
            "thesis_no_page",
            "thesis_update.assetId must name an asset page from asset_page_read",
        )
    })?;
    let entries = thesis_entries(tx, loupist_id, &update.asset_id)?;
    let reasons: Vec<_> = entries
        .iter()
        .filter(|entry| {
            entry
                .thesis
                .as_ref()
                .is_some_and(|thesis| thesis.role == ThesisRole::Reason)
        })
        .cloned()
        .collect();
    let current = effective_verdict(&page, &reasons).ok_or_else(|| {
        asset_error(
            "thesis_none",
            "the user has not written a reason for following this asset; there is no read to move",
        )
    })?;
    if update.evidence.is_empty() || update.evidence.len() > MAX_EVIDENCE {
        return Err(asset_error(
            "thesis_evidence",
            "thesis_update.evidence needs 1–3 citation URLs from this turn",
        ));
    }
    if let Some(reason_id) = &update.reason_id {
        if !reasons.iter().any(|entry| &entry.journal_id == reason_id) {
            return Err(asset_error(
                "thesis_reason_id",
                "thesis_update.reasonId must name one of this asset's active reasons",
            ));
        }
    }
    let history = read_moves(tx, loupist_id, &update.asset_id, false)?;
    match (&update.to, &update.line) {
        (Some(to), None) => {
            let reason = update.reason.as_deref().map(str::trim).unwrap_or_default();
            if reason.is_empty() || reason.chars().count() > MAX_REASON_CHARS {
                return Err(asset_error(
                    "thesis_reason",
                    "thesis_update.reason must contain 1–280 characters",
                ));
            }
            ladder::validate_step(current, *to)?;
            ladder::validate_after_undo(
                last_loupist_undone(&history),
                current,
                *to,
                update.fact_date,
            )?;
            Ok(PlannedUpdate {
                page,
                current,
                to: Some(*to),
                reason: bounded_text(reason, MAX_REASON_CHARS),
                reading: None,
            })
        }
        (None, Some((journal_id, observed))) => {
            let line = entries
                .iter()
                .find(|entry| {
                    &entry.journal_id == journal_id
                        && entry
                            .thesis
                            .as_ref()
                            .is_some_and(|thesis| thesis.role == ThesisRole::Line)
                })
                .ok_or_else(|| {
                    asset_error(
                        "thesis_line",
                        "thesis_update.lineReading.journalId must name one of this asset's lines",
                    )
                })?;
            let check = line
                .check
                .as_ref()
                .ok_or_else(|| asset_error("thesis_line_unchecked", "this line has no number to compare; a line without a number never moves the read"))?;
            if !observed.is_finite() {
                return Err(asset_error(
                    "thesis_line_observed",
                    "thesis_update.lineReading.observed must be a finite number",
                ));
            }
            let crossed = ladder::crossed(&check.direction, check.threshold, *observed);
            let reading = LineCrossing {
                journal_id: line.journal_id.clone(),
                claim: bounded_text(&check.claim, 400),
                metric: bounded_text(&check.metric, 200),
                direction: check.direction.clone(),
                threshold: check.threshold,
                unit: check.unit.clone(),
                observed: *observed,
                crossed,
                set_at_ms: line.created_at_ms,
            };
            let fact = update.fact_date.to_string();
            let to = (crossed && current != Verdict::Broken).then_some(Verdict::Broken);
            // A crossing moves the read like any move, so an Undo binds it too.
            if let Some(to) = to {
                ladder::validate_after_undo(
                    last_loupist_undone(&history),
                    current,
                    to,
                    update.fact_date,
                )?;
            }
            Ok(PlannedUpdate {
                page,
                current,
                to,
                reason: crossing_reason(&check.claim, *observed, check.unit.as_deref(), &fact),
                reading: Some(reading),
            })
        }
        _ => Err(asset_error(
            "thesis_update_shape",
            "thesis_update needs either `to` with a reason, or `lineReading`, not both",
        )),
    }
}

/// Applies a checked update in the turn's transaction: a move (with a growth
/// event) and, for a line reading, the reading itself.
pub fn apply_update(
    tx: &Transaction<'_>,
    loupist_id: &str,
    update: &ThesisUpdate,
    origin: Option<(&str, &str)>,
    at_ms: i64,
) -> KnowledgeResult<AppliedUpdate> {
    let planned = plan_update(tx, loupist_id, update)?;
    let fact_date = update.fact_date.to_string();
    if let Some(reading) = &planned.reading {
        tx.execute(
            "INSERT OR IGNORE INTO knowledge_asset_line_readings
             (loupist_id,asset_id,journal_id,fact_date,observed,evidence_json,recorded_at_ms)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                loupist_id,
                update.asset_id,
                reading.journal_id,
                fact_date,
                reading.observed,
                serde_json::to_string(&update.evidence)?,
                at_ms
            ],
        )?;
    }
    let mut moved = None;
    if let Some(to) = planned.to {
        let move_id = mint_id("amv");
        let line = planned.reading.as_ref().filter(|reading| reading.crossed);
        tx.execute(
            "INSERT INTO knowledge_asset_moves(move_id,loupist_id,asset_id,from_verdict,to_verdict,fact_date,reason,evidence_json,reason_id,line_json,actor,at_ms,undone,undoes_move_id,origin_chat_id,origin_message_id,seen_at_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'loupist',?11,0,NULL,?12,?13,NULL)",
            params![
                move_id,
                loupist_id,
                update.asset_id,
                planned.current.as_str(),
                to.as_str(),
                fact_date,
                planned.reason,
                serde_json::to_string(&update.evidence)?,
                update.reason_id,
                line.map(serde_json::to_string).transpose()?,
                at_ms,
                origin.map(|(chat, _)| chat),
                origin.map(|(_, message)| message),
            ],
        )?;
        tx.execute(
            "UPDATE knowledge_asset_pages SET verdict=?3, verdict_move_id=?4, revision=revision+1 WHERE loupist_id=?1 AND asset_id=?2",
            params![loupist_id, update.asset_id, to.as_str(), move_id],
        )?;
        growth(
            tx,
            loupist_id,
            &planned.page,
            Some(planned.current),
            to,
            "loupist",
            origin,
            at_ms,
            &planned.reason,
        )?;
        moved = read_moves(tx, loupist_id, &update.asset_id, false)?
            .into_iter()
            .find(|item| item.move_id == move_id);
    } else if planned.reading.is_some() {
        bump(tx, loupist_id, &update.asset_id)?;
    }
    Ok(AppliedUpdate {
        moved,
        reading: planned.reading,
        name: planned.page.name,
    })
}

#[allow(clippy::too_many_arguments)]
fn growth(
    tx: &Transaction<'_>,
    loupist_id: &str,
    page: &PageRecord,
    from: Option<Verdict>,
    to: Verdict,
    actor: &str,
    origin: Option<(&str, &str)>,
    at_ms: i64,
    reason: &str,
) -> KnowledgeResult<()> {
    // Summary names the asset; before/after carry the steps for receipts.
    let mut event = super::growth::GrowthEvent::new(
        loupist_id,
        &page.asset_id,
        at_ms,
        "thesis_moved",
        actor,
        &page.name,
    );
    event.subject_id = Some(page.asset_id.clone());
    event.before_summary = from.map(|verdict| verdict.as_str().to_owned());
    event.after_summary = Some(to.as_str().to_owned());
    event.rationale = Some(bounded_text(reason, 400));
    event.origin_chat_id = origin.map(|(chat, _)| chat.to_owned());
    event.origin_message_id = origin.map(|(_, message)| message.to_owned());
    event.insert(tx)
}

/// The user's Undo: only the latest move, only a Loupist move, only once.
/// Restores the previous step and records the user's own move. The conflict
/// check is the move itself: a refresh that bumps the page revision does not
/// stale an Undo, but a newer move does.
pub fn undo(
    tx: &Transaction<'_>,
    loupist_id: &str,
    asset_id: &str,
    move_id: &str,
    at_ms: i64,
) -> KnowledgeResult<PageRecord> {
    let page = get_page(tx, loupist_id, asset_id)?.ok_or(KnowledgeError::NotFound)?;
    let history = read_moves(tx, loupist_id, asset_id, false)?;
    let latest = history.last().ok_or(KnowledgeError::NotFound)?;
    if latest.move_id != move_id {
        return Err(KnowledgeError::RevisionConflict {
            expected: Some(move_id.to_owned()),
            actual: Some(latest.move_id.clone()),
        });
    }
    if latest.actor != "loupist" || latest.undone {
        return Err(asset_error(
            "thesis_undo",
            "only the latest change the Loupist made can be undone",
        ));
    }
    let restored = latest.from.unwrap_or(Verdict::Holding);
    let user_move = mint_id("amv");
    tx.execute(
        "UPDATE knowledge_asset_moves SET undone=1 WHERE loupist_id=?1 AND move_id=?2",
        params![loupist_id, move_id],
    )?;
    tx.execute(
        "INSERT INTO knowledge_asset_moves(move_id,loupist_id,asset_id,from_verdict,to_verdict,fact_date,reason,evidence_json,reason_id,line_json,actor,origin_chat_id,origin_message_id,at_ms,undone,undoes_move_id,seen_at_ms) VALUES(?1,?2,?3,?4,?5,?6,'Undone by you','[]',NULL,NULL,'user',NULL,NULL,?7,0,?8,?7)",
        params![user_move, loupist_id, asset_id, latest.to.as_str(), restored.as_str(), latest.fact_date, at_ms, move_id],
    )?;
    tx.execute(
        "UPDATE knowledge_asset_pages SET verdict=?3, verdict_move_id=?4, revision=revision+1 WHERE loupist_id=?1 AND asset_id=?2",
        params![loupist_id, asset_id, restored.as_str(), user_move],
    )?;
    growth(
        tx,
        loupist_id,
        &page,
        Some(latest.to),
        restored,
        "user",
        None,
        at_ms,
        "Undone by you",
    )?;
    get_page(tx, loupist_id, asset_id)?.ok_or(KnowledgeError::NotFound)
}

/// Opening the page acknowledges its crossed lines (Needs attention clears).
pub fn mark_seen(
    tx: &Transaction<'_>,
    loupist_id: &str,
    asset_id: &str,
    at_ms: i64,
) -> KnowledgeResult<usize> {
    Ok(tx.execute(
        "UPDATE knowledge_asset_moves SET seen_at_ms=?3 WHERE loupist_id=?1 AND asset_id=?2 AND seen_at_ms IS NULL",
        params![loupist_id, asset_id, at_ms],
    )?)
}

/// Crossed lines the user has not seen on the page yet, newest first.
pub fn unseen_crossings(
    tx: &Transaction<'_>,
    loupist_id: &str,
) -> KnowledgeResult<Vec<(String, String, i64)>> {
    let mut statement = tx.prepare(
        "SELECT page.asset_id, page.display_name, max(move.at_ms) FROM knowledge_asset_moves move JOIN knowledge_asset_pages page ON page.loupist_id=move.loupist_id AND page.asset_id=move.asset_id WHERE move.loupist_id=?1 AND move.line_json IS NOT NULL AND move.undone=0 AND move.seen_at_ms IS NULL GROUP BY page.asset_id ORDER BY 3 DESC",
    )?;
    let rows = statement
        .query_map([loupist_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// A crossed line, as the home Thread announces it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineAlert {
    pub move_id: String,
    pub asset_id: String,
    pub name: String,
    pub symbol: String,
    pub market: String,
    pub line: LineCrossing,
    pub fact_date: String,
    pub evidence: Vec<AssetEvidence>,
    pub at_ms: i64,
}

/// Crossings at or after `since_ms`, oldest first, for the Thread to post once each.
pub fn line_alerts(
    tx: &Transaction<'_>,
    loupist_id: &str,
    since_ms: i64,
) -> KnowledgeResult<Vec<LineAlert>> {
    let mut alerts = Vec::new();
    for page in list_pages(tx, loupist_id)? {
        for item in moves(tx, loupist_id, &page.asset_id)? {
            let Some(line) = item.line.clone().filter(|line| line.crossed) else {
                continue;
            };
            // An undone crossing is never announced.
            if item.at_ms < since_ms || item.actor != "loupist" || item.undone {
                continue;
            }
            alerts.push(LineAlert {
                move_id: item.move_id,
                asset_id: page.asset_id.clone(),
                name: page.name.clone(),
                symbol: String::new(),
                market: String::new(),
                line,
                fact_date: item.fact_date,
                evidence: item.evidence,
                at_ms: item.at_ms,
            });
        }
    }
    alerts.sort_by_key(|alert| (alert.at_ms, alert.move_id.clone()));
    Ok(alerts)
}

// ───────────────────────────── Views ─────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ReasonView {
    pub journal_id: String,
    pub text: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub origin_chat_id: Option<String>,
    /// The last move that named this reason.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub last_move: Option<MoveView>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct LineReadingView {
    pub observed: f64,
    pub fact_date: String,
    pub evidence: Vec<AssetEvidence>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct LineView {
    pub journal_id: String,
    pub claim: String,
    pub metric: String,
    pub direction: String,
    pub threshold: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub unit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub by_date: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub origin_chat_id: Option<String>,
    /// `not_checked`, `not_crossed` or `crossed`.
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub last: Option<LineReadingView>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ReadView {
    pub verdict: Verdict,
    /// The fact date of the move that set this step, or the first reason's day.
    pub since: String,
    /// True while the read still stands where it started.
    pub started: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub latest_move: Option<MoveView>,
    /// Set when the latest move is the Loupist's and can be undone.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub undo_move_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct MapCounts {
    pub suppliers: usize,
    pub customers: usize,
}

/// The World snapshot for this Loupist, if any.
pub(crate) fn world(
    tx: &Transaction<'_>,
    loupist_id: &str,
) -> KnowledgeResult<Option<WorldSnapshotBody>> {
    let json: Option<String> = tx
        .query_row(
            "SELECT snapshot.snapshot_json FROM knowledge_records record JOIN knowledge_record_heads head ON head.record_id=record.record_id JOIN knowledge_world_snapshots snapshot ON snapshot.revision_id=head.revision_id WHERE record.owner_loupist_id=?1 AND record.kind='world' LIMIT 1",
            [loupist_id],
            |row| row.get(0),
        )
        .optional()?;
    json.map(|json| serde_json::from_str(&json).map_err(KnowledgeError::from))
        .transpose()
}

pub(crate) fn match_entity(
    body: &WorldSnapshotBody,
    asset: &AssetRow,
    page: &PageRecord,
) -> Option<String> {
    if let Some(id) = page
        .entity_id
        .as_ref()
        .filter(|id| body.nodes.iter().any(|node| &node.id == *id))
    {
        return Some(id.clone());
    }
    let item = MapContextItem::from(asset);
    match super::identity::resolve_followed_excluding(&item, &body.nodes, &page.rejected_entity_ids)
    {
        super::IdentityMatch::Exact(id) => Some(id),
        _ => None,
    }
}

fn is_source_role(role: &str) -> bool {
    matches!(role, "source" | "subject")
}

fn map_counts(body: &WorldSnapshotBody, entity: &str) -> MapCounts {
    let mut suppliers = BTreeSet::new();
    let mut customers = BTreeSet::new();
    for claim in body.claims.iter().filter(|claim| {
        claim.status != ClaimStatus::Withdrawn && claim.relation.eq_ignore_ascii_case("SUPPLIES")
    }) {
        let (Some(first), Some(second)) = (claim.endpoints.first(), claim.endpoints.get(1)) else {
            continue;
        };
        let (source, target) = if is_source_role(&second.role) && !is_source_role(&first.role) {
            (second, first)
        } else {
            (first, second)
        };
        if target.node_id == entity {
            suppliers.insert(source.node_id.clone());
        } else if source.node_id == entity {
            customers.insert(target.node_id.clone());
        }
    }
    MapCounts {
        suppliers: suppliers.len(),
        customers: customers.len(),
    }
}

fn day_of_ms(at_ms: i64) -> NaiveDate {
    crate::local_time::date_at_ms(at_ms).unwrap_or_default()
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct SinceRead {
    pub since_ms: i64,
    pub counts: BTreeMap<String, usize>,
    pub rows: Vec<super::growth::GrowthEvent>,
}

/// Changes near the asset's linked entity since the latest thesis move.
#[allow(dead_code)] // The asset reader wires this in with the change feed.
pub(crate) fn since_read(
    tx: &Transaction<'_>,
    loupist_id: &str,
    asset_id: &str,
) -> KnowledgeResult<Option<SinceRead>> {
    let Some(entity) = get_page(tx, loupist_id, asset_id)?.and_then(|page| page.entity_id) else {
        return Ok(None);
    };
    let since_ms: Option<i64> = tx.query_row(
        "SELECT max(at_ms) FROM knowledge_asset_moves WHERE loupist_id = ?1 AND asset_id = ?2",
        params![loupist_id, asset_id],
        |row| row.get(0),
    )?;
    let Some(since_ms) = since_ms else {
        return Ok(None);
    };
    let controls = super::memory_controls::load(tx, loupist_id)?;
    let body = world(tx, loupist_id)?;
    let claims: BTreeSet<&str> = body
        .iter()
        .flat_map(|body| &body.claims)
        .filter(|claim| {
            claim
                .endpoints
                .iter()
                .any(|endpoint| endpoint.node_id == entity)
        })
        .map(|claim| claim.id.as_str())
        .collect();
    let mut anchors = tx.prepare(
        "SELECT a.record_id FROM knowledge_lens_anchors a
         JOIN knowledge_lens_metadata m ON m.record_id = a.record_id
         WHERE a.loupist_id = ?1 AND a.node_id = ?2 AND a.role = 'subject'
           AND m.archived_at_ms IS NULL",
    )?;
    let pages: BTreeSet<String> = anchors
        .query_map(params![loupist_id, entity], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    let mut counts = BTreeMap::new();
    let mut rows = VecDeque::new();
    let mut request = super::GrowthEventsQuery {
        loupist_id: loupist_id.to_owned(),
        since_ms: Some(since_ms),
        until_ms: None,
        world_revision_id: None,
        cursor: None,
        limit: Some(256),
    };
    loop {
        let page = super::growth::query(tx, &request)?;
        for event in page.events {
            let publication = event.publication.as_ref();
            if event.at_ms <= since_ms
                || event
                    .record_id
                    .as_ref()
                    .is_some_and(|record| controls.removed.contains(record))
                || publication
                    .and_then(|publication| publication.source.as_ref())
                    .is_some_and(|source| controls.removed.contains(&source.record_id))
            {
                continue;
            }
            let relevant = match event.kind.as_str() {
                "claim_added" | "claim_ended" | "claim_conflict" => {
                    event
                        .subject_id
                        .as_deref()
                        .is_some_and(|id| claims.contains(id))
                        || publication.and_then(|item| item.fact_subject_id.as_deref())
                            == Some(entity.as_str())
                }
                "lens_superseded" | "lens_contested" => event
                    .record_id
                    .as_ref()
                    .is_some_and(|record| pages.contains(record)),
                _ => false,
            };
            if relevant {
                *counts.entry(event.kind.clone()).or_default() += 1;
                if rows.len() == 8 {
                    rows.pop_front();
                }
                rows.push_back(event);
            }
        }
        request.cursor = page.next_cursor;
        if request.cursor.is_none() {
            break;
        }
    }
    Ok(Some(SinceRead {
        since_ms,
        counts,
        rows: rows.into_iter().rev().collect(),
    }))
}

#[cfg(test)]
#[test]
fn since_read_counts_all_scoped_changes_and_keeps_eight_newest() {
    use super::growth::{GrowthEvent, WorldGrowthPublication};
    let store = crate::store::SqliteStore::open_in_memory().unwrap();
    let mut connection = store.lock().unwrap();
    let tx = connection.transaction().unwrap();
    tx.execute(
        "INSERT INTO knowledge_asset_pages
         (loupist_id, asset_id, display_name, entity_id, created_at_ms, revision)
         VALUES ('owner', 'asset', 'Asset', 'entity', 1, 1)",
        [],
    )
    .unwrap();
    assert!(since_read(&tx, "owner", "asset").unwrap().is_none());
    tx.execute(
        "INSERT INTO knowledge_asset_moves
         (move_id, loupist_id, asset_id, to_verdict, fact_date, reason, evidence_json, actor, at_ms)
         VALUES ('move', 'owner', 'asset', 'unsupported', '2026-04-01', '', '[]', 'user', 4)",
        [],
    )
    .unwrap();
    for at in 3..=15 {
        let mut event =
            GrowthEvent::new("owner", "world", at, "claim_ended", "processing", "change");
        event.publication = Some(WorldGrowthPublication {
            revision_id: "rev".into(),
            source: None,
            fact_subject_id: Some(if at == 15 { "other" } else { "entity" }.into()),
        });
        event.insert(&tx).unwrap();
    }
    let changes = since_read(&tx, "owner", "asset").unwrap().unwrap();
    assert_eq!(changes.since_ms, 4);
    assert_eq!(changes.counts["claim_ended"], 10);
    assert_eq!(changes.rows.len(), 8);
    assert_eq!(changes.rows[0].at_ms, 14);
    assert_eq!(changes.rows[7].at_ms, 7);
}

fn read_since(
    page: &PageRecord,
    reasons: &[JournalRecord],
    history: &[MoveView],
) -> (String, bool) {
    let latest_effective = page
        .verdict_move_id
        .as_ref()
        .and_then(|id| history.iter().find(|item| &item.move_id == id));
    match latest_effective {
        Some(item) => (item.fact_date.clone(), false),
        None => (
            reasons
                .first()
                .map(|entry| day_of_ms(entry.created_at_ms).to_string())
                .unwrap_or_default(),
            true,
        ),
    }
}

pub(crate) fn page_view(
    tx: &Transaction<'_>,
    loupist_id: &str,
    asset: AssetRow,
) -> KnowledgeResult<ThinAssetPageView> {
    page_view_at(tx, loupist_id, asset, None, true)
}

pub(crate) fn stored_page_view(
    tx: &Transaction<'_>,
    loupist_id: &str,
    asset: AssetRow,
) -> KnowledgeResult<ThinAssetPageView> {
    page_view_at(tx, loupist_id, asset, None, false)
}

fn page_view_at(
    tx: &Transaction<'_>,
    loupist_id: &str,
    asset: AssetRow,
    as_of: Option<NaiveDate>,
    record_match: bool,
) -> KnowledgeResult<ThinAssetPageView> {
    let asset_id = asset.asset_id.as_str();
    let page = get_page(tx, loupist_id, asset_id)?.ok_or(KnowledgeError::NotFound)?;
    let (entity_id, map) = if as_of.is_none() {
        let body = world(tx, loupist_id)?;
        let entity_id = body
            .as_ref()
            .and_then(|body| match_entity(body, &asset, &page));
        if record_match {
            set_entity(tx, loupist_id, asset_id, entity_id.as_deref())?;
        }
        let map = body
            .as_ref()
            .zip(entity_id.as_deref())
            .map(|(body, id)| map_counts(body, id));
        (entity_id, map)
    } else {
        (None, None)
    };
    let mut entries = thesis_entries(tx, loupist_id, asset_id)?;
    let mut history = moves(tx, loupist_id, asset_id)?;
    let cutoff = as_of.map(|date| date.to_string());
    if let Some(as_of) = as_of {
        entries.retain(|entry| day_of_ms(entry.updated_at_ms) <= as_of);
        history.retain(|item| {
            day_of_ms(item.at_ms) <= as_of
                && NaiveDate::parse_from_str(&item.fact_date, "%Y-%m-%d")
                    .is_ok_and(|fact| fact <= as_of)
        });
        let undone = history
            .iter()
            .filter_map(|item| item.undoes_move_id.clone())
            .collect::<std::collections::BTreeSet<_>>();
        for item in &mut history {
            item.undone = undone.contains(&item.move_id);
        }
    }
    let reasons: Vec<JournalRecord> = entries
        .iter()
        .filter(|entry| {
            entry
                .thesis
                .as_ref()
                .is_some_and(|t| t.role == ThesisRole::Reason)
        })
        .cloned()
        .collect();
    let effective: Vec<&MoveView> = history
        .iter()
        .filter(|item| !item.undone && item.undoes_move_id.is_none())
        .collect();
    let reason_views = reasons
        .iter()
        .map(|entry| ReasonView {
            journal_id: entry.journal_id.clone(),
            text: entry.judgment.clone(),
            created_at_ms: entry.created_at_ms,
            updated_at_ms: entry.updated_at_ms,
            origin_chat_id: entry.origin_chat_id.clone(),
            last_move: effective
                .iter()
                .rev()
                .find(|item| item.reason_id.as_deref() == Some(entry.journal_id.as_str()))
                .map(|item| (*item).clone()),
        })
        .collect();
    let line_views = entries
        .iter()
        .filter(|entry| {
            entry
                .thesis
                .as_ref()
                .is_some_and(|t| t.role == ThesisRole::Line)
        })
        .filter(|entry| entry.check.is_some())
        .map(|entry| -> KnowledgeResult<LineView> {
            let check = entry.check.as_ref().expect("checked above");
            let mut readings = tx.prepare(
                "SELECT observed,fact_date,evidence_json,recorded_at_ms
                 FROM knowledge_asset_line_readings
                 WHERE loupist_id=?1 AND asset_id=?2 AND journal_id=?3
                   AND (?4 IS NULL OR fact_date<=?4)
                 ORDER BY recorded_at_ms DESC,rowid DESC",
            )?;
            let mut rows =
                readings.query(params![loupist_id, asset_id, entry.journal_id, cutoff])?;
            let mut reading: Option<(f64, String, String)> = None;
            while let Some(row) = rows.next()? {
                let recorded_at_ms = row.get(3)?;
                if as_of.is_some_and(|cutoff| day_of_ms(recorded_at_ms) > cutoff) {
                    continue;
                }
                reading = Some((row.get(0)?, row.get(1)?, row.get(2)?));
                break;
            }
            let crossed_move = history.iter().rev().find(|item| {
                !item.undone
                    && item
                        .line
                        .as_ref()
                        .is_some_and(|line| line.crossed && line.journal_id == entry.journal_id)
            });
            let status = if crossed_move.is_some() {
                "crossed"
            } else if reading.is_some() {
                "not_crossed"
            } else {
                "not_checked"
            };
            let last = match crossed_move {
                Some(item) => item.line.as_ref().map(|line| LineReadingView {
                    observed: line.observed,
                    fact_date: item.fact_date.clone(),
                    evidence: item.evidence.clone(),
                }),
                None => reading
                    .map(|(observed, fact_date, evidence)| {
                        serde_json::from_str(&evidence).map(|evidence| LineReadingView {
                            observed,
                            fact_date,
                            evidence,
                        })
                    })
                    .transpose()?,
            };
            Ok(LineView {
                journal_id: entry.journal_id.clone(),
                claim: check.claim.clone(),
                metric: check.metric.clone(),
                direction: check.direction.clone(),
                threshold: check.threshold,
                unit: check.unit.clone(),
                by_date: check.by_date.clone(),
                created_at_ms: entry.created_at_ms,
                updated_at_ms: entry.updated_at_ms,
                origin_chat_id: entry.origin_chat_id.clone(),
                status: status.into(),
                last,
            })
        })
        .collect::<KnowledgeResult<Vec<_>>>()?;
    let verdict = as_of
        .is_none()
        .then(|| effective_verdict(&page, &reasons))
        .flatten();
    let read = verdict.map(|verdict| {
        let (since, started) = read_since(&page, &reasons, &history);
        let latest_move = history.last().cloned();
        let undo_move_id = latest_move
            .as_ref()
            .filter(|item| item.actor == "loupist" && !item.undone)
            .map(|item| item.move_id.clone());
        ReadView {
            verdict,
            since,
            started,
            latest_move,
            undo_move_id,
        }
    });
    let saved = if as_of.is_none() {
        saved_read::load(tx, loupist_id, asset_id)?
    } else {
        None
    };
    Ok(ThinAssetPageView {
        asset,
        price_setup_running: false,
        current_price: saved.as_ref().and_then(|entry| entry.price.clone()),
        saved_read: saved.map(|entry| entry.read),
        entity_id,
        reasons: reason_views,
        lines: line_views,
        read,
        history: history.into_iter().rev().collect(),
        map,
        revision: page.revision,
        since_read: None,
    })
}

pub(crate) fn model_read(
    tx: &Transaction<'_>,
    loupist_id: &str,
    asset: AssetRow,
    as_of: Option<NaiveDate>,
) -> KnowledgeResult<Value> {
    let view = page_view_at(tx, loupist_id, asset, as_of, false)?;
    let mut result = json!({
        "assetId": view.asset.asset_id,
        "name": view.asset.name,
        "reference": view.asset.reference,
        "reasons": view.reasons,
        "lines": view.lines,
        "history": view.history,
        "asOf": as_of.map(|date| date.to_string()),
    });
    if as_of.is_none() {
        result["savedRead"] = json!(view.saved_read);
        result["currentPrice"] = json!(view.current_price);
        result["read"] = json!(view.read);
        result["entityId"] = json!(view.entity_id);
    }
    Ok(result)
}
