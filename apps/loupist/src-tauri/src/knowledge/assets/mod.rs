//! Asset pages: for everything the user follows, one page per Loupist that
//! answers "is my reason still true, and what changed?". The user's reasons
//! and lines are Journal entries tagged with the asset; the Loupist's read,
//! dated observations from structured tool rows and every move live here.
//! Model text never becomes an observation, and no row leaves Rust without a
//! fact date and a source. Holdings quantities never enter these tables.
pub mod class;
pub mod flows;
pub mod ladder;
pub mod observe;
pub mod plan;
pub mod quote;
pub mod sections;
#[cfg(test)]
mod tests;
pub mod upcoming;

use std::collections::{BTreeMap, BTreeSet};

use chrono::{Duration, NaiveDate};
use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub use class::{AssetClass, ClassSource};
pub use ladder::Verdict;
pub use observe::NewObservation;

use super::{
    model::{mint_id, validate_loupist_id, ClaimStatus, WorldSnapshotBody},
    JournalRecord, KnowledgeError, KnowledgeResult,
};
use crate::datasets::DatasetRef;
use crate::store::user_context::MapContextItem;

pub(crate) fn asset_error(code: &'static str, expected: &'static str) -> KnowledgeError {
    KnowledgeError::AssetField { code, expected }
}

pub const MAX_REASON_CHARS: usize = 280;
pub const MAX_EVIDENCE: usize = 3;
const MAX_FACTS_BYTES: usize = 4096;
/// The page lists 50 change rows, then "Show earlier".
pub const MAX_CHANGE_ROWS: usize = 200;
const DAY_MS: i64 = 86_400_000;

pub(crate) const DDL: &str = r#"
CREATE TABLE knowledge_asset_pages (
    loupist_id TEXT NOT NULL,
    asset_id TEXT NOT NULL,
    symbol TEXT NOT NULL,
    market TEXT NOT NULL,
    display_name TEXT NOT NULL,
    class TEXT NOT NULL CHECK (class IN ('equity','crypto','fund','commodity','rates_fx','real_asset','prediction','private')),
    class_source TEXT NOT NULL CHECK (class_source IN ('user','market','lookup','world','default')),
    entity_id TEXT,
    verdict TEXT CHECK (verdict IS NULL OR verdict IN ('holding','weakening','challenged','broken')),
    verdict_move_id TEXT,
    refresh_json TEXT NOT NULL DEFAULT '{}',
    refreshed_at_ms INTEGER,
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    revision INTEGER NOT NULL CHECK (revision >= 1),
    PRIMARY KEY (loupist_id, asset_id)
) STRICT;
CREATE TABLE knowledge_asset_observations (
    observation_id TEXT PRIMARY KEY NOT NULL,
    loupist_id TEXT NOT NULL,
    asset_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('filing','results','earnings','dividend','close','holdings','fees','positioning','line','insiders','holders','flows','capital','investor_flows')),
    fact_date TEXT NOT NULL CHECK (length(fact_date) = 10),
    recorded_at_ms INTEGER NOT NULL CHECK (recorded_at_ms >= 0),
    dedupe_key TEXT NOT NULL,
    facts_json TEXT NOT NULL CHECK (length(facts_json) <= 4096),
    evidence_json TEXT NOT NULL,
    origin TEXT NOT NULL CHECK (origin IN ('refresh','chat','briefing')),
    baseline INTEGER NOT NULL CHECK (baseline IN (0,1)),
    origin_chat_id TEXT,
    origin_message_id TEXT,
    UNIQUE (loupist_id, asset_id, dedupe_key)
) STRICT;
CREATE INDEX knowledge_asset_observations_by_time ON knowledge_asset_observations(loupist_id, asset_id, recorded_at_ms);
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
    actor TEXT NOT NULL CHECK (actor IN ('loupist','user')),
    origin_chat_id TEXT,
    origin_message_id TEXT,
    at_ms INTEGER NOT NULL CHECK (at_ms >= 0),
    undone INTEGER NOT NULL DEFAULT 0 CHECK (undone IN (0,1)),
    undoes_move_id TEXT,
    seen_at_ms INTEGER
) STRICT;
CREATE INDEX knowledge_asset_moves_by_asset ON knowledge_asset_moves(loupist_id, asset_id, at_ms);
CREATE TABLE knowledge_asset_section_deltas (
    delta_id TEXT PRIMARY KEY NOT NULL,
    loupist_id TEXT NOT NULL,
    asset_id TEXT NOT NULL,
    filing_dedupe_key TEXT NOT NULL,
    source_record_id TEXT NOT NULL,
    previous_record_id TEXT NOT NULL,
    sections_json TEXT NOT NULL,
    at_ms INTEGER NOT NULL CHECK (at_ms >= 0),
    UNIQUE (loupist_id, asset_id, filing_dedupe_key)
) STRICT;
"#;

/// `ast_` + the first 16 hex digits of sha256(SYMBOL:MARKET), per Loupist.
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationKind {
    Filing,
    Results,
    Earnings,
    Dividend,
    Close,
    Holdings,
    Fees,
    Positioning,
    Line,
    /// Several insiders bought within 14 days (watchers' insider route).
    Insiders,
    /// A big holder crossed 5% (reserved for W16.4 ownership reads).
    Holders,
    /// Korean foreign and institution buying turned (watchers' flows route).
    Flows,
    /// One Korean capital decision (주요사항보고: rights offering, bonds, buyback).
    Capital,
    /// The Korean investor-flows summary for the page's side section; never a change row.
    InvestorFlows,
}

impl ObservationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Capital => "capital",
            Self::InvestorFlows => "investor_flows",
            Self::Filing => "filing",
            Self::Results => "results",
            Self::Earnings => "earnings",
            Self::Dividend => "dividend",
            Self::Close => "close",
            Self::Holdings => "holdings",
            Self::Fees => "fees",
            Self::Positioning => "positioning",
            Self::Line => "line",
            Self::Insiders => "insiders",
            Self::Holders => "holders",
            Self::Flows => "flows",
        }
    }
}

/// Where a row came from: a retained dataset row, an original URL, a Source,
/// a World claim, or a cited result of the turn that recorded it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetEvidence {
    pub tool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dataset: Option<DatasetRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pointer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub world_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub citation_url: Option<String>,
    /// The evidence's own date (filing, publication or observation).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retrieved_at: Option<String>,
}

impl AssetEvidence {
    pub fn has_source(&self) -> bool {
        self.url.is_some()
            || self.dataset.is_some()
            || self.record_id.is_some()
            || self.world_id.is_some()
            || self.citation_url.is_some()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshStatus {
    /// `ok`, `failed` or `needs_connection`.
    pub status: String,
    pub connection: String,
    pub at_ms: i64,
}

/// One asset identity, as the user follows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetIdentity {
    pub symbol: String,
    pub market: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PageRecord {
    pub asset_id: String,
    pub symbol: String,
    pub market: String,
    pub name: String,
    pub class: AssetClass,
    pub class_source: ClassSource,
    pub entity_id: Option<String>,
    pub verdict: Option<Verdict>,
    pub verdict_move_id: Option<String>,
    pub refresh: BTreeMap<String, RefreshStatus>,
    pub refreshed_at_ms: Option<i64>,
    pub created_at_ms: i64,
    pub revision: i64,
}

fn read_page_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PageRecord> {
    let class: String = row.get(4)?;
    let source: String = row.get(5)?;
    let verdict: Option<String> = row.get(7)?;
    let refresh: String = row.get(9)?;
    Ok(PageRecord {
        asset_id: row.get(0)?,
        symbol: row.get(1)?,
        market: row.get(2)?,
        name: row.get(3)?,
        class: AssetClass::parse(&class).unwrap_or(AssetClass::Equity),
        class_source: ClassSource::parse(&source).unwrap_or(ClassSource::Default),
        entity_id: row.get(6)?,
        verdict: verdict.as_deref().and_then(Verdict::parse),
        verdict_move_id: row.get(8)?,
        refresh: serde_json::from_str(&refresh).unwrap_or_default(),
        refreshed_at_ms: row.get(10)?,
        created_at_ms: row.get(11)?,
        revision: row.get(12)?,
    })
}

const PAGE_COLUMNS: &str = "asset_id,symbol,market,display_name,class,class_source,entity_id,verdict,verdict_move_id,refresh_json,refreshed_at_ms,created_at_ms,revision";

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

pub fn list_pages(tx: &Transaction<'_>, loupist_id: &str) -> KnowledgeResult<Vec<PageRecord>> {
    validate_loupist_id(loupist_id)?;
    let mut statement = tx.prepare(&format!(
        "SELECT {PAGE_COLUMNS} FROM knowledge_asset_pages WHERE loupist_id=?1 ORDER BY created_at_ms, asset_id"
    ))?;
    let rows = statement
        .query_map([loupist_id], read_page_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Whitespace-collapsed text of at most `chars` characters.
pub(crate) fn bounded_text_pub(text: &str, chars: usize) -> String {
    bounded_text(text, chars)
}

fn bounded_text(text: &str, chars: usize) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(chars)
        .collect()
}

fn validate_identity(identity: &AssetIdentity) -> KnowledgeResult<()> {
    let ok = |value: &str, max: usize| {
        !value.trim().is_empty() && value.chars().count() <= max && !value.contains('\0')
    };
    if !ok(&identity.symbol, 64) || !ok(&identity.market, 32) || identity.name.chars().count() > 200
    {
        return Err(asset_error(
            "asset_identity",
            "an asset needs a symbol (≤ 64) and a market (≤ 32)",
        ));
    }
    Ok(())
}

/// The page for an asset, created on first use with its initial class.
pub fn ensure_page(
    tx: &Transaction<'_>,
    loupist_id: &str,
    identity: &AssetIdentity,
    world_class: Option<&str>,
    at_ms: i64,
) -> KnowledgeResult<PageRecord> {
    validate_identity(identity)?;
    let id = asset_id(&identity.symbol, &identity.market);
    if let Some(page) = get_page(tx, loupist_id, &id)? {
        return Ok(page);
    }
    let (class, source) = class::initial_class(&identity.symbol, &identity.market, world_class);
    let name = if identity.name.trim().is_empty() {
        identity.symbol.trim().to_owned()
    } else {
        bounded_text(&identity.name, 200)
    };
    tx.execute(
        "INSERT INTO knowledge_asset_pages(loupist_id,asset_id,symbol,market,display_name,class,class_source,created_at_ms,revision) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,1)",
        params![loupist_id, id, identity.symbol.trim().to_uppercase(), identity.market.trim().to_uppercase(), name, class.as_str(), source.as_str(), at_ms],
    )?;
    get_page(tx, loupist_id, &id)?.ok_or(KnowledgeError::NotFound)
}

fn bump(tx: &Transaction<'_>, loupist_id: &str, asset_id: &str) -> KnowledgeResult<()> {
    tx.execute(
        "UPDATE knowledge_asset_pages SET revision=revision+1 WHERE loupist_id=?1 AND asset_id=?2",
        params![loupist_id, asset_id],
    )?;
    Ok(())
}

/// A class change. A user choice always wins; other sources only replace
/// classes they outrank.
pub fn set_class(
    tx: &Transaction<'_>,
    loupist_id: &str,
    asset_id: &str,
    class: AssetClass,
    source: ClassSource,
) -> KnowledgeResult<PageRecord> {
    let page = get_page(tx, loupist_id, asset_id)?.ok_or(KnowledgeError::NotFound)?;
    if page.class_source.yields_to(source) && (page.class != class || page.class_source != source) {
        // Statuses of checks the new class does not read no longer apply.
        let mut refresh = page.refresh;
        retain_planned(&mut refresh, class, &page.market);
        tx.execute(
            "UPDATE knowledge_asset_pages SET class=?3, class_source=?4, refresh_json=?5 WHERE loupist_id=?1 AND asset_id=?2",
            params![loupist_id, asset_id, class.as_str(), source.as_str(), serde_json::to_string(&refresh)?],
        )?;
        bump(tx, loupist_id, asset_id)?;
    }
    get_page(tx, loupist_id, asset_id)?.ok_or(KnowledgeError::NotFound)
}

/// Keeps only the statuses of checks this class reads.
fn retain_planned(refresh: &mut BTreeMap<String, RefreshStatus>, class: AssetClass, market: &str) {
    let kinds = plan::checked_kinds(class, market);
    refresh.retain(|kind, _| kinds.contains(&kind.as_str()));
}

pub fn set_entity(
    tx: &Transaction<'_>,
    loupist_id: &str,
    asset_id: &str,
    entity_id: Option<&str>,
) -> KnowledgeResult<()> {
    tx.execute(
        "UPDATE knowledge_asset_pages SET entity_id=?3 WHERE loupist_id=?1 AND asset_id=?2 AND entity_id IS NOT ?3",
        params![loupist_id, asset_id, entity_id],
    )?;
    Ok(())
}

pub fn record_refresh(
    tx: &Transaction<'_>,
    loupist_id: &str,
    asset_id: &str,
    statuses: BTreeMap<String, RefreshStatus>,
    at_ms: i64,
) -> KnowledgeResult<()> {
    let page = get_page(tx, loupist_id, asset_id)?.ok_or(KnowledgeError::NotFound)?;
    let mut refresh = page.refresh;
    refresh.extend(statuses);
    retain_planned(&mut refresh, page.class, &page.market);
    tx.execute(
        "UPDATE knowledge_asset_pages SET refresh_json=?3, refreshed_at_ms=?4, revision=revision+1 WHERE loupist_id=?1 AND asset_id=?2",
        params![loupist_id, asset_id, serde_json::to_string(&refresh)?, at_ms],
    )?;
    Ok(())
}

/// Who recorded an observation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObservationOrigin {
    Refresh,
    Chat,
    Briefing,
}

impl ObservationOrigin {
    fn as_str(self) -> &'static str {
        match self {
            Self::Refresh => "refresh",
            Self::Chat => "chat",
            Self::Briefing => "briefing",
        }
    }
}

fn series_of(observation: &NewObservation) -> Option<&str> {
    (observation.kind == ObservationKind::Close)
        .then(|| observation.facts["series"].as_str().unwrap_or("price"))
}

/// Stores new observations; identical rows (same dedupe key) are ignored.
/// The first observations of a kind (a close series counts on its own) are a
/// baseline and never show as new. Returns how many non-baseline rows were added.
pub fn record_observations(
    tx: &Transaction<'_>,
    loupist_id: &str,
    asset_id: &str,
    observations: Vec<NewObservation>,
    origin: ObservationOrigin,
    chat: Option<(&str, &str)>,
    at_ms: i64,
) -> KnowledgeResult<usize> {
    record_observations_with(
        tx,
        loupist_id,
        asset_id,
        observations,
        origin,
        chat,
        at_ms,
        None,
    )
    .map(|added| added.len())
}

/// `record_observations`, returning the non-baseline rows it added. `baseline`
/// overrides the per-kind rule: a watcher route decides its own starting point.
#[allow(clippy::too_many_arguments)]
pub fn record_observations_with(
    tx: &Transaction<'_>,
    loupist_id: &str,
    asset_id: &str,
    observations: Vec<NewObservation>,
    origin: ObservationOrigin,
    chat: Option<(&str, &str)>,
    at_ms: i64,
    baseline_override: Option<bool>,
) -> KnowledgeResult<Vec<NewObservation>> {
    let mut known_kinds: BTreeSet<String> = BTreeSet::new();
    {
        let mut statement = tx.prepare(
            "SELECT DISTINCT kind, CASE WHEN kind='close' THEN json_extract(facts_json,'$.series') ELSE NULL END FROM knowledge_asset_observations WHERE loupist_id=?1 AND asset_id=?2",
        )?;
        for row in statement.query_map(params![loupist_id, asset_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })? {
            let (kind, series) = row?;
            known_kinds.insert(match series {
                Some(series) => format!("{kind}:{series}"),
                None => kind,
            });
        }
    }
    let mut added = Vec::new();
    let mut replaced = 0;
    for mut observation in observations {
        if observe::parse_day(&observation.fact_date).is_none()
            || !observation.evidence.has_source()
        {
            continue;
        }
        let key = match series_of(&observation) {
            Some(series) => format!("close:{series}"),
            None => observation.kind.as_str().to_owned(),
        };
        let baseline = baseline_override.unwrap_or(!known_kinds.contains(&key));
        if observation.kind == ObservationKind::Results && observation.facts["prior"].is_null() {
            // The prior figure comes from the last results row with the same metric.
            let prior: Option<f64> = tx
                .query_row(
                    "SELECT json_extract(facts_json,'$.value') FROM knowledge_asset_observations WHERE loupist_id=?1 AND asset_id=?2 AND kind='results' AND json_extract(facts_json,'$.metric')=?3 ORDER BY fact_date DESC, recorded_at_ms DESC LIMIT 1",
                    params![loupist_id, asset_id, observation.facts["metric"].as_str().unwrap_or_default()],
                    |row| row.get(0),
                )
                .optional()?
                .flatten();
            if let (Some(prior), Some(object)) = (prior, observation.facts.as_object_mut()) {
                object.insert("prior".into(), json!(prior));
            }
        }
        if observation.kind == ObservationKind::Fees {
            let prior: Option<f64> = tx
                .query_row(
                    "SELECT json_extract(facts_json,'$.ratio') FROM knowledge_asset_observations WHERE loupist_id=?1 AND asset_id=?2 AND kind='fees' ORDER BY recorded_at_ms DESC LIMIT 1",
                    params![loupist_id, asset_id],
                    |row| row.get(0),
                )
                .optional()?
                .flatten();
            if let (Some(prior), Some(object)) = (prior, observation.facts.as_object_mut()) {
                object.insert("prior".into(), json!(prior));
            }
        }
        if observation.kind == ObservationKind::Holdings {
            let prior: Option<String> = tx
                .query_row(
                    "SELECT facts_json FROM knowledge_asset_observations WHERE loupist_id=?1 AND asset_id=?2 AND kind='holdings' ORDER BY recorded_at_ms DESC LIMIT 1",
                    params![loupist_id, asset_id],
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(prior) = prior.and_then(|text| serde_json::from_str::<Value>(&text).ok()) {
                holdings_changes(&mut observation.facts, &prior);
            }
        }
        let facts = serde_json::to_string(&observation.facts)?;
        if facts.len() > MAX_FACTS_BYTES {
            continue;
        }
        let dedupe_key = bounded_text(&observation.dedupe_key, 400);
        let evidence = serde_json::to_string(&observation.evidence)?;
        let inserted = tx.execute(
            "INSERT OR IGNORE INTO knowledge_asset_observations(observation_id,loupist_id,asset_id,kind,fact_date,recorded_at_ms,dedupe_key,facts_json,evidence_json,origin,baseline,origin_chat_id,origin_message_id) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            params![
                mint_id("aob"),
                loupist_id,
                asset_id,
                observation.kind.as_str(),
                observation.fact_date,
                at_ms,
                dedupe_key,
                facts,
                evidence,
                origin.as_str(),
                baseline,
                chat.map(|(chat, _)| chat),
                chat.map(|(_, message)| message),
            ],
        )?;
        if inserted > 0 && !baseline {
            observation.dedupe_key = dedupe_key.clone();
            added.push(observation.clone());
        }
        if inserted == 0 && observation.kind == ObservationKind::Close {
            // A later read of the same completed session wins: a provider
            // restates adjusted history after a split or a late correction.
            replaced += tx.execute(
                "UPDATE knowledge_asset_observations SET facts_json=?4, evidence_json=?5, recorded_at_ms=?6 WHERE loupist_id=?1 AND asset_id=?2 AND dedupe_key=?3 AND kind='close' AND facts_json<>?4",
                params![loupist_id, asset_id, dedupe_key, facts, evidence, at_ms],
            )?;
        }
    }
    if !added.is_empty() || replaced > 0 {
        bump(tx, loupist_id, asset_id)?;
    }
    Ok(added)
}

/// Names entering or leaving the top 10, and weights moving ≥ 1 point.
fn holdings_changes(facts: &mut Value, prior: &Value) {
    let weights = |value: &Value| -> BTreeMap<String, f64> {
        value["top"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| Some((item["name"].as_str()?.to_owned(), item["weight"].as_f64()?)))
            .collect()
    };
    let now = weights(facts);
    let before = weights(prior);
    let entered: Vec<_> = now
        .keys()
        .filter(|name| !before.contains_key(*name))
        .cloned()
        .collect();
    let left: Vec<_> = before
        .keys()
        .filter(|name| !now.contains_key(*name))
        .cloned()
        .collect();
    // Provider weights may be fractions (0.07) or percents (7); compare in points.
    let points = |weight: f64| {
        if weight.abs() <= 1.0 {
            weight * 100.0
        } else {
            weight
        }
    };
    let moved: Vec<_> = now
        .iter()
        .filter_map(|(name, weight)| {
            let previous = before.get(name)?;
            ((points(*weight) - points(*previous)).abs() >= 1.0)
                .then(|| json!({"name": name, "from": previous, "to": weight}))
        })
        .collect();
    if let Some(object) = facts.as_object_mut() {
        object.insert("entered".into(), json!(entered));
        object.insert("left".into(), json!(left));
        object.insert("moved".into(), json!(moved));
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct StoredObservation {
    pub observation_id: String,
    pub kind: ObservationKind,
    pub fact_date: String,
    pub recorded_at_ms: i64,
    pub facts: Value,
    pub evidence: AssetEvidence,
    pub baseline: bool,
    pub dedupe_key: String,
}

pub fn observations(
    tx: &Transaction<'_>,
    loupist_id: &str,
    asset_id: &str,
) -> KnowledgeResult<Vec<StoredObservation>> {
    let mut statement = tx.prepare(
        "SELECT observation_id,kind,fact_date,recorded_at_ms,facts_json,evidence_json,baseline,dedupe_key FROM knowledge_asset_observations WHERE loupist_id=?1 AND asset_id=?2 ORDER BY fact_date, recorded_at_ms, observation_id",
    )?;
    let rows = statement
        .query_map(params![loupist_id, asset_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, bool>(6)?,
                row.get::<_, String>(7)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(
            |(id, kind, fact_date, recorded_at_ms, facts, evidence, baseline, dedupe_key)| {
                Ok(StoredObservation {
                    observation_id: id,
                    kind: serde_json::from_value(Value::String(kind))?,
                    fact_date,
                    recorded_at_ms,
                    facts: serde_json::from_str(&facts)?,
                    evidence: serde_json::from_str(&evidence)?,
                    baseline,
                    dedupe_key,
                })
            },
        )
        .collect()
}

// ───────────────────────────── Thesis: reasons, lines, read ─────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThesisRole {
    Reason,
    Line,
}

/// A Journal entry's place in an asset's thesis.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
    if thesis.role == ThesisRole::Line && !has_check {
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
#[serde(rename_all = "camelCase")]
pub struct LineCrossing {
    pub journal_id: String,
    pub claim: String,
    pub metric: String,
    pub direction: String,
    pub threshold: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    pub observed: f64,
    pub crossed: bool,
    /// When the user wrote the line.
    pub set_at_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveView {
    pub move_id: String,
    pub asset_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<Verdict>,
    pub to: Verdict,
    pub fact_date: String,
    pub reason: String,
    pub evidence: Vec<AssetEvidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<LineCrossing>,
    /// `loupist` or `user`.
    pub actor: String,
    pub at_ms: i64,
    pub undone: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undoes_move_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_chat_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seen_at_ms: Option<i64>,
}

const MOVE_COLUMNS: &str = "move_id,asset_id,from_verdict,to_verdict,fact_date,reason,evidence_json,reason_id,line_json,actor,at_ms,undone,undoes_move_id,origin_chat_id,origin_message_id,seen_at_ms";

fn read_move(row: &rusqlite::Row<'_>) -> rusqlite::Result<(MoveView, String, Option<String>)> {
    let from: Option<String> = row.get(2)?;
    let to: String = row.get(3)?;
    Ok((
        MoveView {
            move_id: row.get(0)?,
            asset_id: row.get(1)?,
            from: from.as_deref().and_then(Verdict::parse),
            to: Verdict::parse(&to).unwrap_or(Verdict::Holding),
            fact_date: row.get(4)?,
            reason: row.get(5)?,
            evidence: Vec::new(),
            reason_id: row.get(7)?,
            line: None,
            actor: row.get(9)?,
            at_ms: row.get(10)?,
            undone: row.get(11)?,
            undoes_move_id: row.get(12)?,
            origin_chat_id: row.get(13)?,
            origin_message_id: row.get(14)?,
            seen_at_ms: row.get(15)?,
        },
        row.get::<_, String>(6)?,
        row.get::<_, Option<String>>(8)?,
    ))
}

fn finish_move(
    (mut view, evidence, line): (MoveView, String, Option<String>),
) -> KnowledgeResult<MoveView> {
    view.evidence = serde_json::from_str(&evidence)?;
    view.line = line.map(|text| serde_json::from_str(&text)).transpose()?;
    Ok(view)
}

/// Every move for an asset, oldest first.
pub fn moves(
    tx: &Transaction<'_>,
    loupist_id: &str,
    asset_id: &str,
) -> KnowledgeResult<Vec<MoveView>> {
    let mut statement = tx.prepare(&format!(
        "SELECT {MOVE_COLUMNS} FROM knowledge_asset_moves WHERE loupist_id=?1 AND asset_id=?2 ORDER BY at_ms, rowid"
    ))?;
    let rows = statement
        .query_map(params![loupist_id, asset_id], read_move)?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter().map(finish_move).collect()
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
        observe::parse_day(&last.fact_date)?,
    ))
}

/// Checks an update against the page as it is now, without writing. Shared by
/// the tool (at call time) and the commit (against the committed state).
pub fn check_update(
    tx: &Transaction<'_>,
    loupist_id: &str,
    update: &ThesisUpdate,
) -> KnowledgeResult<(Option<Verdict>, Option<LineCrossing>)> {
    plan_update(tx, loupist_id, update).map(|planned| (planned.to, planned.reading))
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
    let history = moves(tx, loupist_id, &update.asset_id)?;
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
        let evidence = update.evidence.first().cloned().unwrap_or_default();
        tx.execute(
            "INSERT OR IGNORE INTO knowledge_asset_observations(observation_id,loupist_id,asset_id,kind,fact_date,recorded_at_ms,dedupe_key,facts_json,evidence_json,origin,baseline,origin_chat_id,origin_message_id) VALUES(?1,?2,?3,'line',?4,?5,?6,?7,?8,'chat',0,?9,?10)",
            params![
                mint_id("aob"),
                loupist_id,
                update.asset_id,
                fact_date,
                at_ms,
                format!("line:{}:{}:{}", reading.journal_id, fact_date, trim_number(reading.observed)),
                serde_json::to_string(&json!({"journalId": reading.journal_id, "observed": reading.observed, "crossed": reading.crossed, "evidence": update.evidence}))?,
                serde_json::to_string(&evidence)?,
                origin.map(|(chat, _)| chat),
                origin.map(|(_, message)| message),
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
        moved = moves(tx, loupist_id, &update.asset_id)?
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
    let history = moves(tx, loupist_id, asset_id)?;
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
                symbol: page.symbol.clone(),
                market: page.market.clone(),
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
#[serde(rename_all = "camelCase")]
pub struct ReasonView {
    pub journal_id: String,
    pub text: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin_chat_id: Option<String>,
    /// The last move that named this reason.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_move: Option<MoveView>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LineReadingView {
    pub observed: f64,
    pub fact_date: String,
    pub evidence: Vec<AssetEvidence>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LineView {
    pub journal_id: String,
    pub claim: String,
    pub metric: String,
    pub direction: String,
    pub threshold: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub by_date: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin_chat_id: Option<String>,
    /// `not_checked`, `not_crossed` or `crossed`.
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last: Option<LineReadingView>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadView {
    pub verdict: Verdict,
    /// The fact date of the move that set this step, or the first reason's day.
    pub since: String,
    /// True while the read still stands where it started.
    pub started: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_move: Option<MoveView>,
    /// Set when the latest move is the Loupist's and can be undone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub undo_move_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlanceRow {
    /// lastFiling, lastResults, nextEarnings, dividend, price, drawdown, expenseRatio, level.
    pub key: String,
    pub facts: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    pub evidence: Vec<AssetEvidence>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckFailure {
    pub kind: String,
    pub connection: String,
    /// `failed` or `needs_connection`.
    pub reason: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChecksView {
    /// Kinds the last check read (the whole plan before the first check).
    pub kinds: Vec<String>,
    /// Connections at least one of those reads came from.
    pub connections: Vec<String>,
    pub not_yet: Vec<String>,
    pub failures: Vec<CheckFailure>,
    pub sources_only: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MapCounts {
    pub suppliers: usize,
    pub customers: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetPageView {
    pub asset_id: String,
    pub symbol: String,
    pub market: String,
    pub name: String,
    pub class: AssetClass,
    pub class_source: ClassSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_id: Option<String>,
    /// `watching`, `holding` or `none`; never a quantity.
    pub watch: String,
    pub reasons: Vec<ReasonView>,
    pub lines: Vec<LineView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read: Option<ReadView>,
    /// Newest first.
    pub history: Vec<MoveView>,
    pub glance: Vec<GlanceRow>,
    pub checks: ChecksView,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub map: Option<MapCounts>,
    /// Korean equities with Korea Investment on: the latest investor flows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flows: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refreshed_at_ms: Option<i64>,
    pub created_at_ms: i64,
    pub revision: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionCountView {
    pub section: sections::Section,
    pub added: u32,
    pub removed: u32,
    pub changed: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetChangeRow {
    pub id: String,
    /// filing, results, earnings, dividend, price, holdings, fees, positioning, rates, map, source, thesis.
    pub kind: String,
    pub fact_date: String,
    pub recorded_at_ms: i64,
    pub facts: Value,
    pub evidence: Vec<AssetEvidence>,
    /// For a 10-K/10-Q/사업보고서/분기보고서 filing: compared sections, or empty when not compared yet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sections: Option<Vec<SectionCountView>>,
    /// Recorded after the chosen date (the page's accent dot).
    pub new: bool,
    /// Worth a Thread or briefing line (a material price move, any other kind).
    pub material: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetDelta {
    pub asset_id: String,
    pub symbol: String,
    pub market: String,
    pub name: String,
    pub class: AssetClass,
    /// The World entity the page matched, for the Map inspector.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verdict: Option<Verdict>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verdict_since: Option<String>,
    /// The move behind the current read; absent while it stands where it started.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_move: Option<MoveView>,
    /// Newest fact first, most material first within a day.
    pub rows: Vec<AssetChangeRow>,
    /// A read move recorded after the date.
    pub moved: bool,
    pub crossed: bool,
}

/// What the page needs from outside the Loupist database.
#[derive(Clone, Debug, Default)]
pub struct ViewContext {
    pub watchlist: Vec<MapContextItem>,
    pub holdings: Vec<MapContextItem>,
    /// Connection display names for the plan's capabilities.
    pub connection_names: BTreeMap<String, String>,
    pub today: Option<NaiveDate>,
}

impl ViewContext {
    fn today(&self) -> NaiveDate {
        self.today
            .unwrap_or_else(|| chrono::Local::now().date_naive())
    }

    fn watch(&self, symbol: &str, market: &str) -> &'static str {
        let same = |item: &&MapContextItem| {
            item.symbol.eq_ignore_ascii_case(symbol) && item.market.eq_ignore_ascii_case(market)
        };
        if self.holdings.iter().any(|item| same(&item)) {
            "holding"
        } else if self.watchlist.iter().any(|item| same(&item)) {
            "watching"
        } else {
            "none"
        }
    }
}

fn day_of_ms(at_ms: i64) -> NaiveDate {
    chrono::DateTime::from_timestamp_millis(at_ms)
        .map(|time| time.with_timezone(&chrono::Local).date_naive())
        .unwrap_or_default()
}

fn round(value: f64, places: i32) -> f64 {
    let scale = 10f64.powi(places);
    (value * scale).round() / scale
}

/// The World snapshot for this Loupist, if any.
fn world(tx: &Transaction<'_>, loupist_id: &str) -> KnowledgeResult<Option<WorldSnapshotBody>> {
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

/// The World entity for an asset (name, symbol, local names) and its stated class.
pub fn match_entity(
    body: &WorldSnapshotBody,
    page: &PageRecord,
) -> Option<(String, Option<String>)> {
    let item = MapContextItem {
        symbol: page.symbol.clone(),
        market: page.market.clone(),
        display_name: Some(page.name.clone()),
    };
    let (matched, _) = super::match_items(&[item], &body.nodes);
    let id = matched.into_iter().next()?;
    let class = body
        .nodes
        .iter()
        .find(|node| node.id == id)
        .and_then(|node| {
            ["asset_class", "class", "assetClass"]
                .iter()
                .find_map(|key| {
                    node.properties
                        .get(*key)
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
        });
    Some((id, class))
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

/// Price (or rate) change between the last close on or before `since` and the latest.
fn close_change(
    closes: &[&StoredObservation],
    since: NaiveDate,
) -> Option<(Value, Vec<AssetEvidence>, String)> {
    let latest = closes.last()?;
    let base = closes
        .iter()
        .rev()
        .find(|close| close.fact_date.as_str() <= since.to_string().as_str())?;
    if base.observation_id == latest.observation_id {
        return None;
    }
    let from = base.facts["close"].as_f64()?;
    let to = latest.facts["close"].as_f64()?;
    let series = latest.facts["series"].as_str().unwrap_or(observe::PRICE);
    let facts = if observe::is_price_series(series) {
        if from <= 0.0 {
            return None;
        }
        // A split between the two closes of an unadjusted series: the raw
        // change is not a move (`material_price` does not flag it).
        let split = series != observe::PRICE_ADJUSTED
            && closes.iter().any(|close| {
                close.fact_date > base.fact_date
                    && close.fact_date <= latest.fact_date
                    && close.facts.get("split").is_some_and(Value::is_number)
            });
        // Same arithmetic as `calculate` percent_change: (to − from) / from × 100.
        json!({"series": series, "change": round((to - from) / from * 100.0, 1), "unit": "%", "from": from, "to": to, "fromDate": base.fact_date, "currency": latest.facts["currency"], "splitBetween": split})
    } else {
        json!({"series": series, "change": round((to - from) * 100.0, 0), "unit": "bp", "from": from, "to": to, "fromDate": base.fact_date})
    };
    Some((
        facts,
        vec![base.evidence.clone(), latest.evidence.clone()],
        latest.fact_date.clone(),
    ))
}

/// The closes of the page's one price series: adjusted closes when recorded,
/// then closes of unstated adjustment, then raw ones. Histories never mix.
fn price_closes(observed: &[StoredObservation], since: NaiveDate) -> Vec<&StoredObservation> {
    let of = |name: &str| -> Vec<&StoredObservation> {
        observed
            .iter()
            .filter(|item| {
                item.kind == ObservationKind::Close
                    && item.facts["series"].as_str().unwrap_or(observe::PRICE) == name
            })
            .collect()
    };
    let series = [observe::PRICE_ADJUSTED, observe::PRICE, observe::PRICE_RAW].map(of);
    series
        .iter()
        .find(|closes| close_change(closes, since).is_some())
        .or_else(|| series.iter().find(|closes| !closes.is_empty()))
        .cloned()
        .unwrap_or_default()
}

/// Beyond this, a price change across a known split is the split, not a move.
const SPLIT_SANITY_PERCENT: f64 = 20.0;

fn material_price(class: AssetClass, facts: &Value) -> bool {
    let change = facts["change"].as_f64().unwrap_or_default().abs();
    if facts["unit"] == "bp" {
        return change >= 25.0;
    }
    if facts["splitBetween"] == true && change >= SPLIT_SANITY_PERCENT {
        return false;
    }
    match class {
        AssetClass::Crypto => change >= 10.0,
        AssetClass::RatesFx => change >= 2.0,
        _ => change >= 5.0,
    }
}

fn kind_rank(kind: &str) -> u8 {
    match kind {
        "thesis" => 0,
        "filing" | "capital" => 1,
        "results" | "insiders" | "holders" | "flows" => 2,
        "earnings" => 3,
        "dividend" => 4,
        "price" | "rates" => 5,
        "holdings" | "fees" | "positioning" => 6,
        "map" => 7,
        _ => 8,
    }
}

fn section_views(
    tx: &Transaction<'_>,
    loupist_id: &str,
    asset_id: &str,
    dedupe_key: &str,
) -> KnowledgeResult<Option<Vec<SectionCountView>>> {
    let json: Option<String> = tx
        .query_row(
            "SELECT sections_json FROM knowledge_asset_section_deltas WHERE loupist_id=?1 AND asset_id=?2 AND filing_dedupe_key=?3",
            params![loupist_id, asset_id, dedupe_key],
            |row| row.get(0),
        )
        .optional()?;
    let Some(json) = json else {
        return Ok(Some(Vec::new()));
    };
    let deltas: Vec<sections::SectionDelta> = serde_json::from_str(&json)?;
    Ok(Some(
        deltas
            .into_iter()
            .map(|delta| SectionCountView {
                section: delta.section,
                added: delta.added,
                removed: delta.removed,
                changed: delta.changed,
            })
            .collect(),
    ))
}

/// Rows dated or recorded after `since_ms`: observations (baseline rows only when
/// their fact falls in the window), one computed price row per series, World
/// growth touching the entity or a neighbor, Sources naming it, and moves.
pub fn delta_for(
    tx: &Transaction<'_>,
    loupist_id: &str,
    page: &PageRecord,
    world: Option<&WorldSnapshotBody>,
    since_ms: i64,
) -> KnowledgeResult<AssetDelta> {
    let since_day = day_of_ms(since_ms);
    let mut rows = Vec::new();
    let observed = observations(tx, loupist_id, &page.asset_id)?;
    for item in &observed {
        if matches!(
            item.kind,
            ObservationKind::Close | ObservationKind::Line | ObservationKind::InvestorFlows
        ) {
            continue;
        }
        let fact = observe::parse_day(&item.fact_date).unwrap_or_default();
        let new = item.recorded_at_ms > since_ms && !item.baseline;
        if !new && fact < since_day {
            continue;
        }
        let kind = match item.kind {
            ObservationKind::Filing => "filing",
            ObservationKind::Results => "results",
            ObservationKind::Earnings => "earnings",
            ObservationKind::Dividend => "dividend",
            ObservationKind::Holdings => "holdings",
            ObservationKind::Fees => "fees",
            ObservationKind::Positioning => "positioning",
            ObservationKind::Insiders => "insiders",
            ObservationKind::Holders => "holders",
            ObservationKind::Flows => "flows",
            ObservationKind::Capital => "capital",
            ObservationKind::Close | ObservationKind::Line | ObservationKind::InvestorFlows => {
                continue
            }
        };
        // A first holdings snapshot or fee reading has nothing to compare with.
        if (kind == "holdings" && item.facts["entered"].is_null())
            || (kind == "fees" && item.facts["prior"].is_null())
        {
            continue;
        }
        let sections = if kind == "filing"
            && item.facts["form"]
                .as_str()
                .is_some_and(sections::sectionable)
        {
            section_views(tx, loupist_id, &page.asset_id, &item.dedupe_key)?
        } else {
            None
        };
        rows.push(AssetChangeRow {
            id: item.observation_id.clone(),
            kind: kind.into(),
            fact_date: item.fact_date.clone(),
            recorded_at_ms: item.recorded_at_ms,
            facts: item.facts.clone(),
            evidence: vec![item.evidence.clone()],
            sections,
            new,
            material: true,
        });
    }
    let mut series: BTreeMap<String, Vec<&StoredObservation>> = BTreeMap::new();
    for item in observed.iter().filter(|item| {
        item.kind == ObservationKind::Close
            && !observe::is_price_series(item.facts["series"].as_str().unwrap_or(observe::PRICE))
    }) {
        series
            .entry(item.facts["series"].as_str().unwrap_or_default().to_owned())
            .or_default()
            .push(item);
    }
    series.insert(
        observe::PRICE.to_owned(),
        price_closes(&observed, since_day),
    );
    for (name, closes) in &series {
        if let Some((facts, evidence, date)) = close_change(closes, since_day) {
            rows.push(AssetChangeRow {
                id: format!("{}:{name}:{}", page.asset_id, since_day),
                kind: if name == observe::PRICE {
                    "price"
                } else {
                    "rates"
                }
                .into(),
                fact_date: date,
                recorded_at_ms: closes.last().map_or(0, |close| close.recorded_at_ms),
                material: material_price(page.class, &facts),
                facts,
                evidence,
                sections: None,
                new: false,
            });
        }
    }
    if let (Some(world), Some(entity)) = (world, page.entity_id.as_deref()) {
        rows.extend(world_rows(tx, loupist_id, world, entity, page, since_ms)?);
    }
    let history = moves(tx, loupist_id, &page.asset_id)?;
    let mut moved = false;
    let mut crossed = false;
    // The Loupist's standing moves are rows; an Undo and the move it undid stay
    // in the thesis history.
    for item in history
        .iter()
        .filter(|item| item.at_ms > since_ms && !item.undone && item.actor == "loupist")
    {
        moved = true;
        crossed |= item.line.as_ref().is_some_and(|line| line.crossed);
        rows.push(AssetChangeRow {
            id: item.move_id.clone(),
            kind: "thesis".into(),
            fact_date: item.fact_date.clone(),
            recorded_at_ms: item.at_ms,
            facts: json!({"from": item.from, "to": item.to, "reason": item.reason, "actor": item.actor, "undone": item.undone, "line": item.line}),
            evidence: item.evidence.clone(),
            sections: None,
            new: true,
            material: true,
        });
    }
    // Rust never emits a row without a date and a source.
    rows.retain(|row| {
        observe::parse_day(&row.fact_date).is_some()
            && row.evidence.iter().any(AssetEvidence::has_source)
    });
    rows.sort_by(|a, b| {
        b.fact_date
            .cmp(&a.fact_date)
            .then(kind_rank(&a.kind).cmp(&kind_rank(&b.kind)))
            .then(b.recorded_at_ms.cmp(&a.recorded_at_ms))
    });
    rows.truncate(MAX_CHANGE_ROWS);
    let entries = thesis_entries(tx, loupist_id, &page.asset_id)?;
    let reasons: Vec<_> = entries
        .iter()
        .filter(|entry| {
            entry
                .thesis
                .as_ref()
                .is_some_and(|t| t.role == ThesisRole::Reason)
        })
        .cloned()
        .collect();
    let verdict = effective_verdict(page, &reasons);
    let since = verdict.map(|_| read_since(page, &reasons, &history));
    let latest_move = verdict
        .and(page.verdict_move_id.as_ref())
        .and_then(|id| history.iter().find(|item| &item.move_id == id).cloned());
    Ok(AssetDelta {
        asset_id: page.asset_id.clone(),
        symbol: page.symbol.clone(),
        market: page.market.clone(),
        name: page.name.clone(),
        class: page.class,
        entity_id: page.entity_id.clone(),
        verdict,
        verdict_since: since.map(|(date, _)| date),
        latest_move,
        rows,
        moved,
        crossed,
    })
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

/// `needle` as a whole word of `haystack` (both lowercase). Only ASCII letters
/// and digits bind a word, so a Korean particle may follow a name.
fn contains_word(haystack: &str, needle: &str) -> bool {
    !needle.is_empty()
        && haystack.match_indices(needle).any(|(start, _)| {
            let before = haystack[..start].chars().next_back();
            let after = haystack[start + needle.len()..].chars().next();
            !before.is_some_and(|c| c.is_ascii_alphanumeric())
                && !after.is_some_and(|c| c.is_ascii_alphanumeric())
        })
}

/// Whether a Source title names the asset: its name, or its symbol as a whole
/// word. A ticker of three characters or fewer ("T", "GE", "AMD") also needs
/// its exchange beside it, since alone it is an ordinary word or abbreviation.
pub(crate) fn names_asset(text: &str, page: &PageRecord) -> bool {
    let text = text.to_lowercase();
    let name = page.name.trim().to_lowercase();
    let symbol = page.symbol.trim().to_lowercase();
    if name.chars().count() >= 2 && name != symbol && contains_word(&text, &name) {
        return true;
    }
    contains_word(&text, &symbol)
        && (symbol.chars().count() > 3 || contains_word(&text, &page.market.trim().to_lowercase()))
}

fn world_rows(
    tx: &Transaction<'_>,
    loupist_id: &str,
    world: &WorldSnapshotBody,
    entity: &str,
    page: &PageRecord,
    since_ms: i64,
) -> KnowledgeResult<Vec<AssetChangeRow>> {
    let mut neighborhood = BTreeSet::from([entity.to_owned()]);
    for claim in world
        .claims
        .iter()
        .filter(|claim| claim.status != ClaimStatus::Withdrawn)
    {
        if claim.endpoints.iter().any(|end| end.node_id == entity) {
            neighborhood.extend(claim.endpoints.iter().map(|end| end.node_id.clone()));
        }
    }
    let touching: BTreeSet<&str> = world
        .claims
        .iter()
        .filter(|claim| {
            claim
                .endpoints
                .iter()
                .any(|end| neighborhood.contains(&end.node_id))
        })
        .map(|claim| claim.id.as_str())
        .collect();
    let mut statement = tx.prepare(
        "SELECT event_id, at_ms, kind, record_id, subject_id, summary FROM knowledge_growth_events WHERE loupist_id=?1 AND at_ms>?2 AND kind IN ('claim_added','claim_disputed','source_retained') ORDER BY at_ms DESC, event_id DESC LIMIT 400",
    )?;
    let events = statement
        .query_map(params![loupist_id, since_ms], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut rows = Vec::new();
    for (event_id, at_ms, kind, record_id, subject_id, summary) in events {
        let date = day_of_ms(at_ms).to_string();
        if kind == "source_retained" {
            if !names_asset(&summary, page) {
                continue;
            }
            rows.push(AssetChangeRow {
                id: event_id,
                kind: "source".into(),
                fact_date: date.clone(),
                recorded_at_ms: at_ms,
                facts: json!({"title": summary}),
                evidence: vec![AssetEvidence {
                    tool: "source".into(),
                    title: summary.clone(),
                    record_id,
                    date: Some(date),
                    ..AssetEvidence::default()
                }],
                sections: None,
                new: true,
                material: true,
            });
            continue;
        }
        let Some(subject) = subject_id.filter(|id| touching.contains(id.as_str())) else {
            continue;
        };
        rows.push(AssetChangeRow {
            id: event_id,
            kind: "map".into(),
            fact_date: date.clone(),
            recorded_at_ms: at_ms,
            facts: json!({"summary": summary, "disputed": kind == "claim_disputed"}),
            evidence: vec![AssetEvidence {
                tool: "world".into(),
                title: summary.clone(),
                world_id: Some(subject),
                date: Some(date),
                ..AssetEvidence::default()
            }],
            sections: None,
            new: true,
            material: true,
        });
    }
    Ok(rows)
}

/// "Next earnings" from the results events when no calendar announced a
/// date: an IR notice's date, or a Korean legal deadline marked `dueBy` (a
/// bound, never an expected date).
fn results_date(
    tx: &Transaction<'_>,
    loupist_id: &str,
    page: &PageRecord,
    mut rows: Vec<GlanceRow>,
    today: NaiveDate,
) -> KnowledgeResult<Vec<GlanceRow>> {
    if page.class != AssetClass::Equity || rows.iter().any(|row| row.key == "nextEarnings") {
        return Ok(rows);
    }
    let next = super::earnings::events_for(tx, loupist_id, &page.asset_id)?
        .into_iter()
        .filter(|event| event.release.is_none() && event.report_date >= today)
        .min_by_key(|event| event.report_date);
    if let Some(event) = next {
        let due_by = event.date_source == super::earnings::DateSource::LegalDeadline;
        rows.push(GlanceRow {
            key: "nextEarnings".into(),
            facts: json!({"date": event.report_date.to_string(), "period": event.period, "dueBy": due_by}),
            date: None,
            evidence: Vec::new(),
        });
    }
    Ok(rows)
}

fn glance(
    page: &PageRecord,
    observed: &[StoredObservation],
    since_day: NaiveDate,
    today: NaiveDate,
) -> Vec<GlanceRow> {
    let latest = |kind: ObservationKind| {
        observed
            .iter()
            .filter(|item| item.kind == kind)
            .max_by(|a, b| {
                a.fact_date
                    .cmp(&b.fact_date)
                    .then(a.recorded_at_ms.cmp(&b.recorded_at_ms))
            })
    };
    let row = |key: &str, item: &StoredObservation| GlanceRow {
        key: key.into(),
        facts: item.facts.clone(),
        date: Some(item.fact_date.clone()),
        evidence: vec![item.evidence.clone()],
    };
    let mut rows = Vec::new();
    if let Some(item) = latest(ObservationKind::Filing) {
        rows.push(row("lastFiling", item));
    }
    if let Some(item) = latest(ObservationKind::Results) {
        rows.push(row("lastResults", item));
    }
    let next = observed
        .iter()
        .filter(|item| item.kind == ObservationKind::Earnings)
        .filter(|item| {
            item.facts["date"]
                .as_str()
                .and_then(observe::parse_day)
                .is_some_and(|date| date >= today)
        })
        .min_by(|a, b| a.facts["date"].as_str().cmp(&b.facts["date"].as_str()));
    if let Some(item) = next {
        rows.push(row("nextEarnings", item));
    }
    if let Some(item) = latest(ObservationKind::Dividend) {
        rows.push(row("dividend", item));
    }
    let closes = price_closes(observed, since_day);
    if let Some((facts, evidence, date)) = close_change(&closes, since_day) {
        rows.push(GlanceRow {
            key: "price".into(),
            facts,
            date: Some(date),
            evidence,
        });
    }
    if page.class == AssetClass::Crypto {
        let window = (today - Duration::days(90)).to_string();
        let recent: Vec<_> = closes
            .iter()
            .filter(|item| item.fact_date >= window)
            .collect();
        let high = recent.iter().max_by(|a, b| {
            a.facts["close"]
                .as_f64()
                .unwrap_or_default()
                .total_cmp(&b.facts["close"].as_f64().unwrap_or_default())
        });
        if let (Some(high), Some(last)) = (high, closes.last()) {
            let (peak, now) = (
                high.facts["close"].as_f64().unwrap_or_default(),
                last.facts["close"].as_f64().unwrap_or_default(),
            );
            if peak > 0.0 {
                rows.push(GlanceRow {
                    key: "drawdown".into(),
                    facts: json!({"change": round((now - peak) / peak * 100.0, 1), "high": peak, "highDate": high.fact_date}),
                    date: Some(last.fact_date.clone()),
                    evidence: vec![high.evidence.clone(), last.evidence.clone()],
                });
            }
        }
    }
    if let Some(item) = latest(ObservationKind::Fees) {
        rows.push(row("expenseRatio", item));
    }
    if page.class == AssetClass::RatesFx {
        let level = observed
            .iter()
            .filter(|item| item.kind == ObservationKind::Close)
            .max_by(|a, b| a.fact_date.cmp(&b.fact_date));
        if let Some(item) = level {
            rows.push(GlanceRow { key: "level".into(), facts: json!({"value": item.facts["close"], "series": item.facts["series"], "unit": item.facts["unit"]}), date: Some(item.fact_date.clone()), evidence: vec![item.evidence.clone()] });
        }
    }
    rows
}

fn checks(page: &PageRecord, context: &ViewContext) -> ChecksView {
    let mut plan = plan::reads(
        page.class,
        &page.symbol,
        &page.market,
        context.today(),
        false,
    );
    // Korean investor flows come from Korea Investment, outside the four reads.
    if page.class == AssetClass::Equity && class::is_korean_market(&page.market) {
        let [investor, ..] = plan::flows_reads(&page.symbol, context.today());
        plan.push(investor);
    }
    // What the page says it checks is what the last check read: a kind that failed
    // or whose connection is off is listed only as a failure, and a connection
    // none of whose reads succeeded is not named as a source. Before the first
    // check, the whole plan is listed.
    let unread = |kind: &str| {
        page.refresh
            .get(kind)
            .is_some_and(|status| status.status != "ok")
    };
    let mut connections = Vec::new();
    for read in plan.iter().filter(|read| !unread(read.kind)) {
        let name = context
            .connection_names
            .get(read.connection)
            .cloned()
            .unwrap_or_else(|| read.connection.to_owned());
        if !connections.contains(&name) {
            connections.push(name);
        }
    }
    let failures = page
        .refresh
        .iter()
        .filter(|(_, status)| status.status != "ok")
        .map(|(kind, status)| CheckFailure {
            kind: kind.clone(),
            connection: status.connection.clone(),
            reason: status.status.clone(),
        })
        .collect();
    ChecksView {
        kinds: plan::checked_kinds(page.class, &page.market)
            .into_iter()
            // The drop from the high is computed from the price read.
            .filter(|kind| !unread(kind) && !(*kind == "drawdown" && unread("price")))
            .map(str::to_owned)
            .collect(),
        connections,
        not_yet: plan::not_checked_yet(page.class, &page.market)
            .into_iter()
            .map(str::to_owned)
            .collect(),
        failures,
        sources_only: page.class.sources_only(),
    }
}

/// The asset page. `since_ms` sets the "Price since your last look" row.
pub fn page_view(
    tx: &Transaction<'_>,
    loupist_id: &str,
    asset_id: &str,
    since_ms: i64,
    context: &ViewContext,
) -> KnowledgeResult<AssetPageView> {
    let mut page = get_page(tx, loupist_id, asset_id)?.ok_or(KnowledgeError::NotFound)?;
    let world = world(tx, loupist_id)?;
    let map = match &world {
        Some(body) => {
            let matched = match_entity(body, &page);
            set_entity(
                tx,
                loupist_id,
                asset_id,
                matched.as_ref().map(|(id, _)| id.as_str()),
            )?;
            if let Some((_, Some(class))) = &matched {
                if let Some(class) = class::class_from_world(class) {
                    page = set_class(tx, loupist_id, asset_id, class, ClassSource::World)?;
                }
            }
            page.entity_id = matched.as_ref().map(|(id, _)| id.clone());
            page.entity_id
                .as_deref()
                .map(|entity| map_counts(body, entity))
        }
        None => None,
    };
    let entries = thesis_entries(tx, loupist_id, asset_id)?;
    let history = moves(tx, loupist_id, asset_id)?;
    let observed = observations(tx, loupist_id, asset_id)?;
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
        .filter_map(|entry| {
            let check = entry.check.as_ref()?;
            let reading = observed
                .iter()
                .filter(|item| {
                    item.kind == ObservationKind::Line
                        && item.facts["journalId"] == entry.journal_id.as_str()
                })
                .max_by(|a, b| a.recorded_at_ms.cmp(&b.recorded_at_ms));
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
                None => reading.map(|item| LineReadingView {
                    observed: item.facts["observed"].as_f64().unwrap_or_default(),
                    fact_date: item.fact_date.clone(),
                    evidence: serde_json::from_value(item.facts["evidence"].clone())
                        .unwrap_or_else(|_| vec![item.evidence.clone()]),
                }),
            };
            Some(LineView {
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
        .collect();
    let read = effective_verdict(&page, &reasons).map(|verdict| {
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
    let since_day = day_of_ms(since_ms);
    Ok(AssetPageView {
        asset_id: page.asset_id.clone(),
        symbol: page.symbol.clone(),
        market: page.market.clone(),
        name: page.name.clone(),
        class: page.class,
        class_source: page.class_source,
        entity_id: page.entity_id.clone(),
        watch: context.watch(&page.symbol, &page.market).into(),
        reasons: reason_views,
        lines: line_views,
        read,
        history: history.iter().rev().cloned().collect(),
        glance: results_date(
            tx,
            loupist_id,
            &page,
            glance(&page, &observed, since_day, context.today()),
            context.today(),
        )?,
        checks: checks(&page, context),
        map,
        flows: flows::view(&observed),
        refreshed_at_ms: page.refreshed_at_ms,
        created_at_ms: page.created_at_ms,
        revision: page.revision,
    })
}

/// Deltas for one asset or for every page, ordered for the Thread and briefings:
/// a moved read, a crossed line, a filing or results row, a material price move.
pub fn deltas(
    tx: &Transaction<'_>,
    loupist_id: &str,
    asset_id: Option<&str>,
    since_ms: i64,
) -> KnowledgeResult<Vec<AssetDelta>> {
    let world = world(tx, loupist_id)?;
    let pages = match asset_id {
        Some(id) => vec![get_page(tx, loupist_id, id)?.ok_or(KnowledgeError::NotFound)?],
        None => list_pages(tx, loupist_id)?,
    };
    let mut out = Vec::new();
    for mut page in pages {
        if page.entity_id.is_none() {
            page.entity_id = world
                .as_ref()
                .and_then(|body| match_entity(body, &page))
                .map(|(id, _)| id);
        }
        out.push(delta_for(tx, loupist_id, &page, world.as_ref(), since_ms)?);
    }
    out.sort_by_key(delta_rank);
    Ok(out)
}

/// Lower is more important (spec §2.4).
pub fn delta_rank(delta: &AssetDelta) -> (u8, std::cmp::Reverse<String>) {
    let has = |kinds: &[&str]| {
        delta
            .rows
            .iter()
            .any(|row| kinds.contains(&row.kind.as_str()) && row.material)
    };
    let rank = if delta.moved && !delta.crossed {
        0
    } else if delta.crossed {
        1
    } else if has(&["filing", "results"]) {
        2
    } else if has(&["price", "rates"]) {
        3
    } else if delta.rows.iter().any(|row| row.material) {
        4
    } else {
        5
    };
    (
        rank,
        std::cmp::Reverse(
            delta
                .rows
                .first()
                .map(|row| row.fact_date.clone())
                .unwrap_or_default(),
        ),
    )
}

/// Filings compared per turn; more wait for a later turn that retains them.
const MAX_FILINGS_COMPARED: usize = 4;
/// Text read from each copy; the compared sections sit well inside it.
const MAX_FILING_TEXT_BYTES: usize = 4 * 1024 * 1024;

/// The digest of a retained Source's newest readable text.
fn source_digest(tx: &Transaction<'_>, record_id: &str) -> KnowledgeResult<Option<String>> {
    Ok(tx
        .query_row(
            "SELECT digest FROM knowledge_source_representations WHERE record_id=?1 AND role IN ('extracted_text','original') AND media_type LIKE 'text/%' ORDER BY created_at_ms DESC, representation_id DESC LIMIT 1",
            [record_id],
            |row| row.get(0),
        )
        .optional()?)
}

fn blob_text(blobs: &super::BlobStore, digest: &str) -> KnowledgeResult<Option<String>> {
    let mut bytes = blobs.read(digest)?;
    if bytes.len() > MAX_FILING_TEXT_BYTES {
        bytes.truncate(MAX_FILING_TEXT_BYTES);
        // Drop a character cut at the boundary.
        if let Err(error) = std::str::from_utf8(&bytes) {
            if error.error_len().is_none() {
                bytes.truncate(error.valid_up_to());
            }
        }
    }
    Ok(String::from_utf8(bytes).ok())
}

fn source_for(tx: &Transaction<'_>, locator: &str) -> KnowledgeResult<Option<String>> {
    Ok(tx
        .query_row(
            "SELECT record_id FROM knowledge_sources WHERE origin_locator=?1 ORDER BY updated_at_ms DESC LIMIT 1",
            [locator],
            |row| row.get(0),
        )
        .optional()?)
}

/// A filing a turn retained and the last same-form copy to compare it with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilingPair {
    pub asset_id: String,
    pub dedupe_key: String,
    pub record_id: String,
    pub previous_record_id: String,
    newer_digest: String,
    older_digest: String,
}

/// When a turn retains a 10-K, 10-Q, 사업보고서 or 분기보고서 that a filing row
/// links to, and an earlier same-form copy for the asset was retained too, the
/// pair to compare. Reads the database only; the comparison runs outside it
/// (`compare_filing`) and `record_sections` writes the counts.
pub fn retained_filing_pairs(
    tx: &Transaction<'_>,
    loupist_id: &str,
    message_id: &str,
) -> KnowledgeResult<Vec<FilingPair>> {
    let records: Vec<String> = {
        let mut statement = tx.prepare(
            "SELECT DISTINCT record_id FROM knowledge_growth_events WHERE loupist_id=?1 AND origin_message_id=?2 AND kind='source_retained' AND record_id IS NOT NULL",
        )?;
        let rows = statement
            .query_map(params![loupist_id, message_id], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    let mut pairs = Vec::new();
    for record in records {
        if pairs.len() >= MAX_FILINGS_COMPARED {
            break;
        }
        let locator: Option<String> = tx
            .query_row(
                "SELECT origin_locator FROM knowledge_sources WHERE record_id=?1 LIMIT 1",
                [&record],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        let Some(locator) = locator else { continue };
        let filings: Vec<(String, String, String, String)> = {
            let mut statement = tx.prepare(
                "SELECT asset_id, dedupe_key, json_extract(facts_json,'$.form'), fact_date FROM knowledge_asset_observations WHERE loupist_id=?1 AND kind='filing' AND json_extract(evidence_json,'$.url')=?2 AND json_extract(facts_json,'$.form') IS NOT NULL",
            )?;
            let rows = statement
                .query_map(params![loupist_id, locator], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        for (asset_id, dedupe_key, form, fact_date) in filings {
            if !sections::sectionable(&form) {
                continue;
            }
            let earlier: Vec<String> = {
                let mut statement = tx.prepare(
                    "SELECT json_extract(evidence_json,'$.url') FROM knowledge_asset_observations WHERE loupist_id=?1 AND asset_id=?2 AND kind='filing' AND json_extract(facts_json,'$.form')=?3 AND fact_date<?4 AND json_extract(evidence_json,'$.url') IS NOT NULL ORDER BY fact_date DESC",
                )?;
                let rows = statement
                    .query_map(params![loupist_id, asset_id, form, fact_date], |row| {
                        row.get(0)
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                rows
            };
            let mut previous = None;
            for url in earlier {
                if let Some(found) = source_for(tx, &url)? {
                    previous = Some(found);
                    break;
                }
            }
            let Some(previous) = previous else { continue };
            let (Some(newer_digest), Some(older_digest)) =
                (source_digest(tx, &record)?, source_digest(tx, &previous)?)
            else {
                continue;
            };
            if pairs.len() < MAX_FILINGS_COMPARED {
                pairs.push(FilingPair {
                    asset_id,
                    dedupe_key,
                    record_id: record.clone(),
                    previous_record_id: previous,
                    newer_digest,
                    older_digest,
                });
            }
        }
    }
    Ok(pairs)
}

/// Compares a pair's sections (bounded text, bounded paragraphs). No database.
pub fn compare_filing(
    blobs: &super::BlobStore,
    pair: &FilingPair,
) -> KnowledgeResult<Vec<sections::SectionDelta>> {
    let (Some(newer), Some(older)) = (
        blob_text(blobs, &pair.newer_digest)?,
        blob_text(blobs, &pair.older_digest)?,
    ) else {
        return Ok(Vec::new());
    };
    Ok(sections::align(&newer, &older))
}

/// Records the section comparison of a filing against the last same-form copy.
#[allow(clippy::too_many_arguments)]
pub fn record_sections(
    tx: &Transaction<'_>,
    loupist_id: &str,
    asset_id: &str,
    filing_dedupe_key: &str,
    source_record_id: &str,
    previous_record_id: &str,
    deltas: &[sections::SectionDelta],
    at_ms: i64,
) -> KnowledgeResult<()> {
    tx.execute(
        "INSERT OR REPLACE INTO knowledge_asset_section_deltas(delta_id,loupist_id,asset_id,filing_dedupe_key,source_record_id,previous_record_id,sections_json,at_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        params![mint_id("asd"), loupist_id, asset_id, filing_dedupe_key, source_record_id, previous_record_id, serde_json::to_string(deltas)?, at_ms],
    )?;
    bump(tx, loupist_id, asset_id)
}

/// Model-facing exact read: reasons, lines, the read and the last 20 rows by
/// fact date (baseline rows included, since the model asks about any window).
/// Under an as-of cutoff only rows and readings dated on or before it remain,
/// and the read (today's view, moved by later turns) is left out.
pub fn model_read(
    tx: &Transaction<'_>,
    loupist_id: &str,
    asset_id: &str,
    context: &ViewContext,
    as_of: Option<NaiveDate>,
) -> KnowledgeResult<Value> {
    let known = |date: &str| {
        as_of.is_none_or(|cutoff| observe::parse_day(date).is_some_and(|day| day <= cutoff))
    };
    // A reason or line the user wrote after the cutoff was not known then.
    let written = |at_ms: i64| as_of.is_none_or(|cutoff| day_of_ms(at_ms) <= cutoff);
    let view = page_view(
        tx,
        loupist_id,
        asset_id,
        chrono::Utc::now().timestamp_millis() - 30 * DAY_MS,
        context,
    )?;
    let delta = delta_for(
        tx,
        loupist_id,
        &get_page(tx, loupist_id, asset_id)?.ok_or(KnowledgeError::NotFound)?,
        world(tx, loupist_id)?.as_ref(),
        0,
    )?;
    let rows: Vec<Value> = delta
        .rows
        .iter()
        // A move is the Loupist's later judgment, not a dated fact of the time.
        .filter(|row| known(&row.fact_date) && (as_of.is_none() || row.kind != "thesis"))
        .take(20)
        .map(|row| {
            json!({
                "kind": row.kind,
                "factDate": row.fact_date,
                "facts": row.facts,
                "sections": row.sections,
                "sources": row.evidence.iter().map(|evidence| json!({"title": evidence.title, "url": evidence.url, "date": evidence.date})).collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(json!({
        "assetId": view.asset_id,
        "name": view.name,
        "symbol": view.symbol,
        "market": view.market,
        "class": view.class,
        "read": view.read.as_ref().filter(|_| as_of.is_none()).map(|read| json!({"verdict": read.verdict.label(), "since": read.since, "latestReason": read.latest_move.as_ref().map(|item| &item.reason)})),
        "reasons": view.reasons.iter().filter(|reason| written(reason.created_at_ms)).map(|reason| json!({"journalId": reason.journal_id, "text": reason.text, "written": day_of_ms(reason.created_at_ms).to_string()})).collect::<Vec<_>>(),
        "lines": view.lines.iter().filter(|line| written(line.created_at_ms)).map(|line| {
            let last = line.last.as_ref().filter(|last| known(&last.fact_date));
            let status = if last.is_some() { line.status.as_str() } else { "not_checked" };
            json!({"journalId": line.journal_id, "claim": line.claim, "metric": line.metric, "direction": line.direction, "threshold": line.threshold, "unit": line.unit, "status": status, "last": last.map(|last| json!({"observed": last.observed, "factDate": last.fact_date}))})
        }).collect::<Vec<_>>(),
        "rows": rows,
        "investorFlows": view.flows.as_ref().filter(|flows| flows["to"].as_str().is_some_and(known)).map(flows::model_summary),
        "asOf": as_of.map(|cutoff| cutoff.to_string()),
        "notCheckedYet": view.checks.not_yet,
        "note": if as_of.is_some() {
            "As-of read: only rows and line readings dated on or before the cutoff. The Loupist's read is today's view, so it is left out, and thesis_update is refused in an as-of answer."
        } else {
            "Rows are host observations from structured tool reads, each with its fact date and source. To move the read, call thesis_update with evidence you read in this turn."
        }
    }))
}
