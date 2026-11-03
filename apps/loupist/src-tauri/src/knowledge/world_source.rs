//! Resolve cited immutable spans through the current Source catalogs.
use std::collections::{BTreeMap, BTreeSet};

use rusqlite::Transaction;
use serde::{Deserialize, Serialize};

use super::authority::{require_record, revision_in_record};
use super::canonical::from_json;
use super::model::{EvidenceRevision, RecordKind, SourceRecordPayload, WorldSnapshotBody};
use super::revision::{load_revision, load_revision_by_digest};
use super::{KnowledgeError, KnowledgeResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldSourceTarget {
    pub record_id: String,
    pub revision_id: String,
    pub source_id: String,
    pub span_id: String,
    pub world_record_id: String,
    pub world_revision_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceWorldClaim {
    pub world_record_id: String,
    pub world_revision_id: String,
    pub claim_id: String,
    pub proposition: String,
    pub relation: String,
}

pub(super) fn related_claims(
    tx: &Transaction<'_>,
    loupist_id: &str,
    record_id: &str,
    revision_id: &str,
    source_id: &str,
) -> KnowledgeResult<Vec<SourceWorldClaim>> {
    let record = require_record(tx, loupist_id, record_id)?;
    if record.kind != RecordKind::Source {
        return Err(KnowledgeError::Invalid("record is not a source"));
    }
    let revision = load_revision(tx, revision_id)?;
    revision_in_record(&revision.record_id, record_id)?;
    let payload: SourceRecordPayload = from_json(&revision.payload_json)?;
    let source = payload.source;
    if source.source_id != source_id {
        return Err(KnowledgeError::NotFound);
    }
    let spans = source
        .captures
        .into_iter()
        .flat_map(|capture| capture.representations)
        .flat_map(|representation| representation.spans)
        .map(|span| span.span_id)
        .collect::<BTreeSet<_>>();
    let mut statement = tx.prepare("SELECT records.record_id, revisions.revision_id, revisions.payload_json FROM knowledge_records records JOIN knowledge_record_heads heads ON heads.record_id = records.record_id JOIN knowledge_record_revisions revisions ON revisions.revision_id = heads.revision_id WHERE records.owner_loupist_id = ?1 AND records.kind = 'world'")?;
    let rows = statement.query_map([loupist_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    let mut result = Vec::new();
    for row in rows {
        let (world_record_id, world_revision_id, payload) = row?;
        let body: WorldSnapshotBody = from_json(&payload)?;
        for claim in body.claims {
            if claim
                .evidence
                .iter()
                .any(|evidence| spans.contains(&evidence.span_id))
            {
                result.push(SourceWorldClaim {
                    world_record_id: world_record_id.clone(),
                    world_revision_id: world_revision_id.clone(),
                    claim_id: claim.id,
                    proposition: claim.proposition,
                    relation: claim.relation,
                });
            }
        }
    }
    Ok(result)
}

pub(super) fn resolve(
    tx: &Transaction<'_>,
    loupist_id: &str,
    world_record_id: &str,
    world_revision_id: &str,
    span_ids: &[String],
) -> KnowledgeResult<BTreeMap<String, WorldSourceTarget>> {
    if span_ids.len() > 256 {
        return Err(KnowledgeError::Invalid("too many Source targets"));
    }
    let module = require_record(tx, loupist_id, world_record_id)?;
    if module.kind != RecordKind::World {
        return Err(KnowledgeError::Invalid("record is not World"));
    }
    let world = load_revision(tx, world_revision_id)?;
    revision_in_record(&world.record_id, world_record_id)?;
    let body: WorldSnapshotBody = from_json(&world.payload_json)?;
    let cited: BTreeSet<_> = body
        .claims
        .iter()
        .flat_map(|claim| {
            claim
                .evidence
                .iter()
                .map(|evidence| evidence.span_id.as_str())
        })
        .chain(
            body.annotations
                .iter()
                .flat_map(|annotation| annotation.source_span_ids.iter().map(String::as_str)),
        )
        .collect();
    if span_ids.iter().any(|span| !cited.contains(span.as_str())) {
        return Err(KnowledgeError::Unauthorized(
            "span is outside the selected World",
        ));
    }
    let wanted: BTreeSet<_> = span_ids.iter().map(String::as_str).collect();
    let mut pending: Vec<EvidenceRevision> = from_json(&world.dependency_json)?;
    let mut visited = BTreeSet::new();
    let mut result = BTreeMap::new();
    while let Some(dependency) = pending.pop() {
        if !visited.insert((
            dependency.record_id.clone(),
            dependency.revision_digest.clone(),
        )) {
            continue;
        }
        if visited.len() > super::MAX_WORLD_DEPENDENCIES {
            return Err(KnowledgeError::Invalid(
                "World dependency closure is too large",
            ));
        }
        let record = require_record(tx, loupist_id, &dependency.record_id)?;
        let stored = if record.kind == RecordKind::Source {
            super::revision::current_source(tx, loupist_id, &dependency.record_id)?
        } else {
            load_revision_by_digest(tx, &dependency.record_id, &dependency.revision_digest)?
        };
        match stored.kind {
            RecordKind::Source => {
                let payload: SourceRecordPayload = from_json(&stored.payload_json)?;
                let source = payload.source;
                for capture in source.captures {
                    for representation in capture.representations {
                        for span in representation.spans {
                            if wanted.contains(span.span_id.as_str()) {
                                result
                                    .entry(span.span_id.clone())
                                    .or_insert(WorldSourceTarget {
                                        record_id: stored.record_id.clone(),
                                        revision_id: stored.revision_id.clone(),
                                        source_id: source.source_id.clone(),
                                        span_id: span.span_id,
                                        world_record_id: world_record_id.into(),
                                        world_revision_id: world_revision_id.into(),
                                    });
                            }
                        }
                    }
                }
            }
            RecordKind::World => {
                pending.extend(from_json::<Vec<EvidenceRevision>>(&stored.dependency_json)?)
            }
            RecordKind::Lens => {
                return Err(KnowledgeError::Invalid(
                    "World dependency target is invalid",
                ))
            }
        }
    }
    Ok(result)
}
