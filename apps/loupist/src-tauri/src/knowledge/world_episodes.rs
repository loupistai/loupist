//! Immutable provenance of individual Source chunk publications.
use std::collections::BTreeMap;

use rusqlite::{params, Params, Row, Transaction};
use serde::{Deserialize, Serialize};

use super::{mint_id, TimePoint};
use crate::knowledge::{memory_controls, KnowledgeError, KnowledgeResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceBasis {
    Published,
    Received,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndBasis {
    Stated,
    Reference,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeModel {
    pub model_profile: String,
    pub thinking_level: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpisodeEnd {
    pub claim: String,
    pub by: Option<String>,
    pub end: TimePoint,
    pub basis: EndBasis,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpisodeConflict {
    pub claim: String,
    pub with: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EpisodeChanges {
    pub created: Vec<String>,
    pub supported: Vec<String>,
    pub ended: Vec<EpisodeEnd>,
    pub reopened: Vec<String>,
    pub conflicts: Vec<EpisodeConflict>,
    pub entities_added: Vec<String>,
    pub entities_touched: Vec<String>,
    pub truncated: BTreeMap<String, usize>,
}

impl EpisodeChanges {
    fn bound(&mut self) {
        fn cap<T>(rows: &mut Vec<T>, limit: usize) -> usize {
            let dropped = rows.len().saturating_sub(limit);
            rows.truncate(limit);
            dropped
        }

        for (kind, dropped) in [
            ("created", cap(&mut self.created, 256)),
            ("supported", cap(&mut self.supported, 256)),
            ("ended", cap(&mut self.ended, 64)),
            ("reopened", cap(&mut self.reopened, 64)),
            ("conflicts", cap(&mut self.conflicts, 64)),
            ("entities_added", cap(&mut self.entities_added, 256)),
            ("entities_touched", cap(&mut self.entities_touched, 256)),
        ] {
            if dropped > 0 {
                let count = self.truncated.entry(kind.to_owned()).or_default();
                *count = count.saturating_add(dropped);
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldEpisode {
    pub episode_id: String,
    pub loupist_id: String,
    pub world_record_id: String,
    pub job_id: String,
    pub sealed_revision_id: String,
    pub previous_revision_id: Option<String>,
    pub source_record_id: String,
    pub source_revision_id: String,
    pub span_id: String,
    pub chunk_index: Option<u32>,
    pub chunk_count: Option<u32>,
    pub char_start: Option<u32>,
    pub char_end: Option<u32>,
    pub reference_time: Option<String>,
    pub reference_basis: Option<ReferenceBasis>,
    pub publication_time: Option<TimePoint>,
    pub model: Option<EpisodeModel>,
    pub changes: EpisodeChanges,
    pub backfilled: bool,
    pub created_at_ms: i64,
}

impl WorldEpisode {
    fn read(row: &Row<'_>) -> KnowledgeResult<Self> {
        fn json<T: serde::de::DeserializeOwned>(
            row: &Row<'_>,
            index: usize,
        ) -> KnowledgeResult<Option<T>> {
            Ok(row
                .get::<_, Option<String>>(index)?
                .map(|value| serde_json::from_str(&value))
                .transpose()?)
        }

        let reference_basis = match row.get::<_, Option<String>>(14)?.as_deref() {
            Some("published") => Some(ReferenceBasis::Published),
            Some("received") => Some(ReferenceBasis::Received),
            None => None,
            _ => {
                return Err(KnowledgeError::Invalid(
                    "episode reference basis is invalid",
                ))
            }
        };
        Ok(Self {
            episode_id: row.get(0)?,
            loupist_id: row.get(1)?,
            world_record_id: row.get(2)?,
            job_id: row.get(3)?,
            sealed_revision_id: row.get(4)?,
            previous_revision_id: row.get(5)?,
            source_record_id: row.get(6)?,
            source_revision_id: row.get(7)?,
            span_id: row.get(8)?,
            chunk_index: row.get(9)?,
            chunk_count: row.get(10)?,
            char_start: row.get(11)?,
            char_end: row.get(12)?,
            reference_time: row.get(13)?,
            reference_basis,
            publication_time: json(row, 15)?,
            model: json(row, 16)?,
            changes: serde_json::from_str(&row.get::<_, String>(17)?)?,
            backfilled: row.get(18)?,
            created_at_ms: row.get(19)?,
        })
    }
}

/// Inserts a live episode in the caller's World publication transaction.
pub fn insert(tx: &Transaction<'_>, episode: &WorldEpisode) -> KnowledgeResult<()> {
    if episode.backfilled && !cfg!(test) {
        return Err(KnowledgeError::Invalid(
            "only an upgrade can backfill an episode",
        ));
    }
    let basis = episode.reference_basis.map(|basis| match basis {
        ReferenceBasis::Published => "published",
        ReferenceBasis::Received => "received",
    });
    let mut changes = episode.changes.clone();
    changes.bound();
    let id = if episode.episode_id.is_empty() {
        mint_id("eps")
    } else {
        episode.episode_id.clone()
    };
    tx.execute(
        "INSERT INTO knowledge_world_episodes (
            episode_id, loupist_id, world_record_id, job_id, sealed_revision_id,
            previous_revision_id, source_record_id, source_revision_id, span_id,
            chunk_index, chunk_count, char_start, char_end, reference_time, reference_basis,
            publication_json, model_json, changes_json, backfilled, created_at_ms
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20
         )",
        params![
            id,
            episode.loupist_id,
            episode.world_record_id,
            episode.job_id,
            episode.sealed_revision_id,
            episode.previous_revision_id,
            episode.source_record_id,
            episode.source_revision_id,
            episode.span_id,
            episode.chunk_index,
            episode.chunk_count,
            episode.char_start,
            episode.char_end,
            episode.reference_time,
            basis,
            episode
                .publication_time
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?,
            episode
                .model
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?,
            serde_json::to_string(&changes)?,
            i64::from(episode.backfilled),
            episode.created_at_ms,
        ],
    )?;
    Ok(())
}

fn read(
    tx: &Transaction<'_>,
    loupist_id: &str,
    suffix: &str,
    params: impl Params,
) -> KnowledgeResult<Vec<WorldEpisode>> {
    let controls = memory_controls::load(tx, loupist_id)?;
    let mut query = tx.prepare(&format!(
        "SELECT episode_id, loupist_id, world_record_id, job_id, sealed_revision_id,
                previous_revision_id, source_record_id, source_revision_id, span_id,
                chunk_index, chunk_count, char_start, char_end, reference_time, reference_basis,
                publication_json, model_json, changes_json, backfilled, created_at_ms
         FROM knowledge_world_episodes WHERE loupist_id = ?1 {suffix}"
    ))?;
    let mut rows = query.query(params)?;
    let mut episodes = Vec::new();
    while let Some(row) = rows.next()? {
        let source: String = row.get(6)?;
        if !controls.removed.contains(&source) {
            episodes.push(WorldEpisode::read(row)?);
        }
    }
    Ok(episodes)
}

pub fn by_source(
    tx: &Transaction<'_>,
    loupist_id: &str,
    source_record_id: &str,
) -> KnowledgeResult<Vec<WorldEpisode>> {
    read(
        tx,
        loupist_id,
        "AND source_record_id = ?2 ORDER BY chunk_index, created_at_ms, episode_id",
        params![loupist_id, source_record_id],
    )
}

pub fn for_claim(
    tx: &Transaction<'_>,
    loupist_id: &str,
    claim_id: &str,
) -> KnowledgeResult<Vec<WorldEpisode>> {
    read(
        tx,
        loupist_id,
        "AND (
           ?2 IN (SELECT value FROM json_each(changes_json, '$.created'))
           OR ?2 IN (SELECT value FROM json_each(changes_json, '$.supported'))
           OR ?2 IN (SELECT value FROM json_each(changes_json, '$.reopened'))
           OR EXISTS (SELECT 1 FROM json_each(changes_json, '$.ended')
                      WHERE ?2 IN (value ->> 'claim', value ->> 'by'))
           OR EXISTS (SELECT 1 FROM json_each(changes_json, '$.conflicts')
                      WHERE ?2 IN (value ->> 'claim', value ->> 'with'))
           OR EXISTS (
             SELECT 1 FROM json_each(changes_json, '$.reopened') AS reopened
             WHERE reopened.value IN (
               SELECT ended.value ->> 'claim'
               FROM knowledge_world_episodes AS prior,
                    json_each(prior.changes_json, '$.ended') AS ended
               WHERE prior.loupist_id = knowledge_world_episodes.loupist_id
                 AND ended.value ->> 'by' = ?2
             )
           )
         ) ORDER BY created_at_ms, episode_id",
        params![loupist_id, claim_id],
    )
}

/// Outbox creation time on a backfill shell is not publication time.
pub fn since(
    tx: &Transaction<'_>,
    loupist_id: &str,
    after_ms: i64,
) -> KnowledgeResult<Vec<WorldEpisode>> {
    read(
        tx,
        loupist_id,
        "AND backfilled = 0 AND created_at_ms > ?2 ORDER BY created_at_ms, episode_id",
        params![loupist_id, after_ms],
    )
}

/// One World's recorded episodes inside a time window. Callers must not scan
/// the whole history to build one page.
pub fn recorded_window(
    tx: &Transaction<'_>,
    loupist_id: &str,
    world_id: &str,
    after_ms: i64,
    until_ms: i64,
) -> KnowledgeResult<Vec<WorldEpisode>> {
    read(
        tx,
        loupist_id,
        "AND world_record_id = ?2 AND backfilled = 0 AND created_at_ms > ?3 AND created_at_ms <= ?4
         ORDER BY created_at_ms, episode_id",
        params![loupist_id, world_id, after_ms, until_ms],
    )
}
