//! Append-only, bounded descriptions of committed Memory changes.
use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{params, Transaction};
use serde::{Deserialize, Serialize};

use super::error::{KnowledgeError, KnowledgeResult};
use super::model::{mint_id, validate_loupist_id, validate_time, ClaimStatus, WorldSnapshotBody};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct GrowthEvent {
    pub event_id: String,
    pub loupist_id: String,
    pub at_ms: i64,
    pub kind: String,
    pub record_id: Option<String>,
    pub subject_id: Option<String>,
    pub origin_chat_id: Option<String>,
    pub origin_message_id: Option<String>,
    pub actor: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub origin: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub rationale: Option<String>,
    pub summary: String,
    pub before_summary: Option<String>,
    pub after_summary: Option<String>,
    /// Set on reads when the event names a Source the host wrote: its summary
    /// is the stored English title, and the interface shows this key instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub title_key: Option<String>,
    /// Immutable identity of the World publication that wrote this change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub publication: Option<WorldGrowthPublication>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct WorldGrowthPublication {
    pub revision_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub source: Option<WorldGrowthSource>,
    /// Subject of a fact, including when it is outside the bounded Map projection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub fact_subject_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct WorldGrowthSource {
    pub record_id: String,
    pub revision_id: String,
    pub source_id: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
// Received as a request: missing optional fields are accepted.
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GrowthEventsQuery {
    pub loupist_id: String,
    pub since_ms: Option<i64>,
    pub until_ms: Option<i64>,
    /// World readers pin growth to the revision shown by their overview.
    pub world_revision_id: Option<String>,
    pub limit: Option<u32>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct GrowthEventsPage {
    pub events: Vec<GrowthEvent>,
    pub next_cursor: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    loupist_id: String,
    since_ms: Option<i64>,
    until_ms: Option<i64>,
    world_revision_id: Option<String>,
    at_ms: i64,
    event_id: String,
}

/// Growth event kinds and actors. Rust owns these lists (Knowledge schema 7
/// dropped the SQL whitelists), so a new kind needs no schema step.
const GROWTH_KINDS: &[&str] = &[
    "entity_added",
    "claim_added",
    "claim_disputed",
    "claim_withdrawn",
    "claim_ended",
    "claim_reopened",
    "claim_conflict",
    "lens_created",
    "lens_revised",
    "lens_renamed",
    "lens_archived",
    "lens_restored",
    "lens_contested",
    "lens_superseded",
    "source_retained",
    "journal_created",
    "journal_updated",
    "journal_archived",
    "journal_restored",
    "journal_reviewed",
    "schema_extended",
    "schema_reduced",
    "thesis_moved",
    "growth_truncated",
];
const GROWTH_ACTORS: &[&str] = &["user", "loupist", "import", "processing"];

#[cfg(test)]
#[test]
fn temporal_growth_kinds_round_trip() {
    let store = crate::store::SqliteStore::open_in_memory().unwrap();
    let mut db = store.lock().unwrap();
    let tx = db.transaction().unwrap();
    let kinds = [
        "claim_ended",
        "claim_reopened",
        "claim_conflict",
        "lens_superseded",
    ];
    for (index, kind) in kinds.iter().enumerate() {
        GrowthEvent::new(
            "owner",
            "record",
            index as i64,
            kind,
            "processing",
            "change",
        )
        .insert(&tx)
        .unwrap();
    }
    let page = query(
        &tx,
        &GrowthEventsQuery {
            loupist_id: "owner".into(),
            since_ms: None,
            until_ms: None,
            world_revision_id: None,
            limit: None,
            cursor: None,
        },
    )
    .unwrap();
    assert_eq!(
        page.events
            .iter()
            .map(|event| event.kind.as_str())
            .collect::<Vec<_>>(),
        kinds,
    );
}

pub(super) fn one_line(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(240)
        .collect()
}

impl GrowthEvent {
    pub(super) fn new(
        loupist_id: &str,
        record_id: &str,
        at_ms: i64,
        kind: &str,
        actor: &str,
        summary: &str,
    ) -> Self {
        Self {
            event_id: mint_id("gev"),
            loupist_id: loupist_id.into(),
            at_ms,
            kind: kind.into(),
            record_id: Some(record_id.into()),
            subject_id: None,
            origin_chat_id: None,
            origin_message_id: None,
            actor: actor.into(),
            origin: None,
            rationale: None,
            summary: one_line(summary),
            before_summary: None,
            after_summary: None,
            title_key: None,
            publication: None,
        }
    }

    pub(super) fn insert(&self, tx: &Transaction<'_>) -> KnowledgeResult<()> {
        validate_loupist_id(&self.loupist_id)?;
        validate_time(self.at_ms)?;
        if !GROWTH_KINDS.contains(&self.kind.as_str()) {
            return Err(KnowledgeError::Invalid("growth event kind is unknown"));
        }
        if !GROWTH_ACTORS.contains(&self.actor.as_str()) {
            return Err(KnowledgeError::Invalid("growth event actor is unknown"));
        }
        tx.execute(
            "INSERT INTO knowledge_growth_events (event_id, loupist_id, at_ms, kind, record_id, subject_id,
             origin_chat_id, origin_message_id, actor, summary, before_summary, after_summary, origin, rationale, publication_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![self.event_id, self.loupist_id, self.at_ms, self.kind, self.record_id, self.subject_id,
                self.origin_chat_id, self.origin_message_id, self.actor, self.summary, self.before_summary, self.after_summary, self.origin.as_ref().map(serde_json::Value::to_string), self.rationale,
                self.publication.as_ref().map(serde_json::to_string).transpose()?],
        )?;
        Ok(())
    }
}

pub(super) fn world_diff(
    tx: &Transaction<'_>,
    context: &GrowthEvent,
    previous: Option<&WorldSnapshotBody>,
    next: &WorldSnapshotBody,
) -> KnowledgeResult<super::world_episodes::EpisodeChanges> {
    use super::model::{TimePoint, CONFLICTS_WITH_CLAIM, END_BASIS, SUPERSEDED_BY_CLAIM};
    use super::world_episodes::{EndBasis, EpisodeChanges, EpisodeConflict, EpisodeEnd};
    let at = TimePoint::from_unix_ms(context.at_ms);
    let mut episode = EpisodeChanges::default();
    let prior_nodes: BTreeMap<_, _> = previous
        .into_iter()
        .flat_map(|body| &body.nodes)
        .map(|node| (node.id.as_str(), node))
        .collect();
    let prior_claims: BTreeMap<_, _> = previous
        .into_iter()
        .flat_map(|body| &body.claims)
        .map(|claim| (claim.id.as_str(), claim))
        .collect();
    let next_claims: BTreeSet<_> = next.claims.iter().map(|claim| claim.id.as_str()).collect();
    let mut changes = Vec::new();
    for node in &next.nodes {
        match prior_nodes.get(node.id.as_str()) {
            None => {
                episode.entities_added.push(node.id.clone());
                changes.push(("entity_added", node.id.as_str(), node.name.clone(), None));
            }
            Some(prior) if **prior != *node => episode.entities_touched.push(node.id.clone()),
            _ => {}
        }
    }
    let after_claims: BTreeMap<_, _> = next
        .claims
        .iter()
        .map(|claim| (claim.id.as_str(), claim))
        .collect();
    for claim in &next.claims {
        let previous = prior_claims.get(claim.id.as_str()).copied();
        let kind = match previous {
            None => {
                episode.created.push(claim.id.clone());
                Some("claim_added")
            }
            Some(old) if old.status != claim.status => match claim.status {
                ClaimStatus::Disputed => Some("claim_disputed"),
                ClaimStatus::Withdrawn => Some("claim_withdrawn"),
                ClaimStatus::Active => None,
            },
            _ => None,
        };
        let subject = if claim.connects_names() {
            None
        } else {
            claim.endpoints.first().map(|end| end.node_id.as_str())
        };
        if let Some(kind) = kind {
            changes.push((kind, claim.id.as_str(), claim.proposition.clone(), subject));
        }
        if let Some(old) = previous {
            if claim
                .evidence
                .iter()
                .any(|item| !old.evidence.contains(item))
            {
                episode.supported.push(claim.id.clone());
            }
            // Current again: a restatement reopened it or its ending moved away.
            if old.has_ended(&at) && claim.is_current(&at) {
                episode.reopened.push(claim.id.clone());
                changes.push((
                    "claim_reopened",
                    claim.id.as_str(),
                    claim.proposition.clone(),
                    subject,
                ));
            }
        }
        if let Some(end) = claim
            .invalid_at
            .as_ref()
            .filter(|end| end.at_or_before(&at))
        {
            if previous.is_none_or(|old| !old.has_ended(&at)) {
                let by = claim.qualifier(SUPERSEDED_BY_CLAIM);
                episode.ended.push(EpisodeEnd {
                    claim: claim.id.clone(),
                    by: by.map(str::to_owned),
                    end: end.clone(),
                    basis: if claim.qualifier(END_BASIS) == Some("reference") {
                        EndBasis::Reference
                    } else {
                        EndBasis::Stated
                    },
                });
                let summary = by.and_then(|id| after_claims.get(id)).map_or_else(
                    || claim.proposition.clone(),
                    |ender| format!("{} · Ended by {}", claim.proposition, ender.proposition),
                );
                changes.push(("claim_ended", claim.id.as_str(), summary, subject));
            }
        }
        let mut partners = Vec::new();
        for qualifier in claim
            .qualifiers
            .iter()
            .filter(|item| item.key == CONFLICTS_WITH_CLAIM)
        {
            if previous.is_some_and(|old| old.qualifiers.contains(qualifier)) {
                continue;
            }
            episode.conflicts.push(EpisodeConflict {
                claim: claim.id.clone(),
                with: qualifier.value.clone(),
            });
            if let Some(partner) = after_claims.get(qualifier.value.as_str()) {
                partners.push(partner.proposition.as_str());
            }
        }
        if !partners.is_empty() {
            changes.push((
                "claim_conflict",
                claim.id.as_str(),
                format!(
                    "{} · Conflicts with {}",
                    claim.proposition,
                    partners.join(" / ")
                ),
                subject,
            ));
        }
    }
    // A removed claim is a withdrawal even when the producer omitted a tombstone.
    for (id, claim) in &prior_claims {
        if !next_claims.contains(id) && claim.status != ClaimStatus::Withdrawn {
            changes.push(("claim_withdrawn", *id, claim.proposition.clone(), None));
        }
    }
    changes.sort_unstable();
    for (kind, id, summary, fact_subject_id) in changes.iter().take(2000) {
        let mut event = context.clone();
        event.event_id = mint_id("gev");
        event.kind = (*kind).into();
        event.subject_id = Some((*id).into());
        event.summary = one_line(summary);
        if let Some(publication) = &mut event.publication {
            publication.fact_subject_id = fact_subject_id.map(str::to_owned);
        }
        event.insert(tx)?;
    }
    if changes.len() > 2000 {
        // Count omitted changes by kind so the UI never treats an aggregate as a claim.
        let mut event = context.clone();
        event.event_id = mint_id("gev");
        event.kind = "growth_truncated".into();
        event.subject_id = None;
        let mut counts = BTreeMap::<&str, usize>::new();
        for (kind, _, _, _) in changes.iter().skip(2000) {
            *counts.entry(kind).or_default() += 1;
        }
        event.summary = format!(
            "+{} more changes in this update ({})",
            changes.len() - 2000,
            counts
                .iter()
                .map(|(kind, count)| format!("{kind}: {count}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        event.insert(tx)?;
    }
    Ok(episode)
}

pub(super) fn query(
    tx: &Transaction<'_>,
    request: &GrowthEventsQuery,
) -> KnowledgeResult<GrowthEventsPage> {
    validate_loupist_id(&request.loupist_id)?;
    if let Some(expected) = &request.world_revision_id {
        let actual = super::catalog::world_record(tx, &request.loupist_id)?
            .and_then(|record| record.revision_id);
        if actual.as_ref() != Some(expected) {
            return Err(KnowledgeError::RevisionConflict {
                expected: Some(expected.clone()),
                actual,
            });
        }
    }
    let limit = request.limit.unwrap_or(500);
    if !(1..=5000).contains(&limit) {
        return Err(KnowledgeError::Invalid(
            "growth event limit must be between 1 and 5000",
        ));
    }
    for time in [request.since_ms, request.until_ms].into_iter().flatten() {
        validate_time(time)?;
    }
    if request
        .since_ms
        .zip(request.until_ms)
        .is_some_and(|(since, until)| since > until)
    {
        return Err(KnowledgeError::Invalid(
            "growth event time range is invalid",
        ));
    }
    let cursor: Option<Cursor> = request
        .cursor
        .as_ref()
        .map(|value| {
            if value.len() > 4096 {
                return Err(KnowledgeError::Invalid("growth event cursor is invalid"));
            }
            serde_json::from_str(value)
                .map_err(|_| KnowledgeError::Invalid("growth event cursor is invalid"))
        })
        .transpose()?;
    if cursor.as_ref().is_some_and(|cursor| {
        cursor.loupist_id != request.loupist_id
            || cursor.since_ms != request.since_ms
            || cursor.until_ms != request.until_ms
            || cursor.world_revision_id != request.world_revision_id
    }) {
        return Err(KnowledgeError::Invalid(
            "growth event cursor does not match the query",
        ));
    }
    let mut statement = tx.prepare(
        "SELECT event_id, loupist_id, at_ms, kind, record_id, subject_id, origin_chat_id, origin_message_id,
         actor, summary, before_summary, after_summary, origin, rationale, publication_json FROM knowledge_growth_events
         WHERE loupist_id = ?1 AND (?2 IS NULL OR at_ms >= ?2) AND (?3 IS NULL OR at_ms <= ?3)
         AND (?4 IS NULL OR (at_ms, event_id) > (?4, ?5)) ORDER BY at_ms, event_id LIMIT ?6"
    )?;
    let mut events = statement
        .query_map(
            params![
                request.loupist_id,
                request.since_ms,
                request.until_ms,
                cursor.as_ref().map(|cursor| cursor.at_ms),
                cursor.as_ref().map(|cursor| &cursor.event_id),
                limit + 1
            ],
            |row| {
                Ok(GrowthEvent {
                    event_id: row.get(0)?,
                    loupist_id: row.get(1)?,
                    at_ms: row.get(2)?,
                    kind: row.get(3)?,
                    record_id: row.get(4)?,
                    subject_id: row.get(5)?,
                    origin_chat_id: row.get(6)?,
                    origin_message_id: row.get(7)?,
                    actor: row.get(8)?,
                    summary: row.get(9)?,
                    rationale: row.get(13)?,
                    before_summary: row.get(10)?,
                    after_summary: row.get(11)?,
                    title_key: None,
                    publication: row
                        .get::<_, Option<String>>(14)?
                        .map(|s| {
                            serde_json::from_str(&s).map_err(|error| {
                                rusqlite::Error::FromSqlConversionFailure(
                                    14,
                                    rusqlite::types::Type::Text,
                                    Box::new(error),
                                )
                            })
                        })
                        .transpose()?,
                    origin: row
                        .get::<_, Option<String>>(12)?
                        .map(|s| {
                            serde_json::from_str(&s).map_err(|error| {
                                rusqlite::Error::FromSqlConversionFailure(
                                    12,
                                    rusqlite::types::Type::Text,
                                    Box::new(error),
                                )
                            })
                        })
                        .transpose()?,
                })
            },
        )?
        .collect::<Result<Vec<_>, _>>()?;
    let more = events.len() > limit as usize;
    events.truncate(limit as usize);
    let followed = super::followed::followed_record_ids(tx, &request.loupist_id)?;
    for event in &mut events {
        if event.kind == "source_retained"
            && event
                .record_id
                .as_ref()
                .is_some_and(|record_id| followed.contains(record_id))
        {
            event.title_key = Some(super::followed::FOLLOWED_TITLE_KEY.to_owned());
        }
    }
    let next_cursor = if more {
        let last = events.last().expect("positive page limit");
        Some(serde_json::to_string(&Cursor {
            loupist_id: request.loupist_id.clone(),
            since_ms: request.since_ms,
            until_ms: request.until_ms,
            world_revision_id: request.world_revision_id.clone(),
            at_ms: last.at_ms,
            event_id: last.event_id.clone(),
        })?)
    } else {
        None
    };
    Ok(GrowthEventsPage {
        events,
        next_cursor,
    })
}
