//! Append-only, bounded descriptions of committed Memory changes.
use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{params, Transaction};
use serde::{Deserialize, Serialize};

use super::error::{KnowledgeError, KnowledgeResult};
use super::model::{mint_id, validate_loupist_id, validate_time, ClaimStatus, WorldSnapshotBody};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
    pub origin: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    pub summary: String,
    pub before_summary: Option<String>,
    pub after_summary: Option<String>,
    /// Set on reads when the event names a Source the host wrote: its summary
    /// is the stored English title, and the interface shows this key instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GrowthEventsQuery {
    pub loupist_id: String,
    pub since_ms: Option<i64>,
    pub until_ms: Option<i64>,
    pub limit: Option<u32>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    at_ms: i64,
    event_id: String,
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
        }
    }

    pub(super) fn insert(&self, tx: &Transaction<'_>) -> KnowledgeResult<()> {
        validate_loupist_id(&self.loupist_id)?;
        validate_time(self.at_ms)?;
        tx.execute(
            "INSERT INTO knowledge_growth_events (event_id, loupist_id, at_ms, kind, record_id, subject_id,
             origin_chat_id, origin_message_id, actor, summary, before_summary, after_summary, origin, rationale)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![self.event_id, self.loupist_id, self.at_ms, self.kind, self.record_id, self.subject_id,
                self.origin_chat_id, self.origin_message_id, self.actor, self.summary, self.before_summary, self.after_summary, self.origin.as_ref().map(serde_json::Value::to_string), self.rationale],
        )?;
        Ok(())
    }
}

pub(super) fn world_diff(
    tx: &Transaction<'_>,
    context: &GrowthEvent,
    previous: Option<&WorldSnapshotBody>,
    next: &WorldSnapshotBody,
) -> KnowledgeResult<()> {
    let prior_nodes: BTreeSet<_> = previous
        .into_iter()
        .flat_map(|body| &body.nodes)
        .map(|node| node.id.as_str())
        .collect();
    let prior_claims: BTreeMap<_, _> = previous
        .into_iter()
        .flat_map(|body| &body.claims)
        .map(|claim| (claim.id.as_str(), claim))
        .collect();
    let next_claims: BTreeSet<_> = next.claims.iter().map(|claim| claim.id.as_str()).collect();
    let mut changes = Vec::new();
    for node in &next.nodes {
        if !prior_nodes.contains(node.id.as_str()) {
            changes.push(("entity_added", node.id.as_str(), node.name.as_str()));
        }
    }
    for claim in &next.claims {
        let kind = match prior_claims.get(claim.id.as_str()) {
            None => Some("claim_added"),
            Some(old) if old.status != claim.status => match claim.status {
                ClaimStatus::Disputed => Some("claim_disputed"),
                ClaimStatus::Withdrawn => Some("claim_withdrawn"),
                ClaimStatus::Active => None,
            },
            _ => None,
        };
        if let Some(kind) = kind {
            changes.push((kind, claim.id.as_str(), claim.proposition.as_str()));
        }
    }
    // A removed claim is a withdrawal even when the producer omitted a tombstone.
    for (id, claim) in &prior_claims {
        if !next_claims.contains(id) && claim.status != ClaimStatus::Withdrawn {
            changes.push(("claim_withdrawn", *id, claim.proposition.as_str()));
        }
    }
    changes.sort_unstable();
    for (kind, id, summary) in changes.iter().take(2000) {
        let mut event = context.clone();
        event.event_id = mint_id("gev");
        event.kind = (*kind).into();
        event.subject_id = Some((*id).into());
        event.summary = one_line(summary);
        event.insert(tx)?;
    }
    if changes.len() > 2000 {
        // Count omitted changes by kind so the UI never treats an aggregate as a claim.
        let mut event = context.clone();
        event.event_id = mint_id("gev");
        event.kind = "growth_truncated".into();
        event.subject_id = None;
        let mut counts = BTreeMap::<&str, usize>::new();
        for (kind, _, _) in changes.iter().skip(2000) {
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
    Ok(())
}

pub(super) fn query(
    tx: &Transaction<'_>,
    request: &GrowthEventsQuery,
) -> KnowledgeResult<GrowthEventsPage> {
    validate_loupist_id(&request.loupist_id)?;
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
    }) {
        return Err(KnowledgeError::Invalid(
            "growth event cursor does not match the query",
        ));
    }
    let mut statement = tx.prepare(
        "SELECT event_id, loupist_id, at_ms, kind, record_id, subject_id, origin_chat_id, origin_message_id,
         actor, summary, before_summary, after_summary, origin, rationale FROM knowledge_growth_events
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
