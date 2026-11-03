//! Reset only provenance owned by this Loupist. Unknown authorship is preserved.
use super::{memory_controls, shared_pack, BlobStore, KnowledgeError, KnowledgeResult};
use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResetCounts {
    pub principles: usize,
    pub methods: usize,
    pub names: usize,
    pub relationships: usize,
    pub sources: usize,
}

struct ResetPlan {
    lenses: BTreeSet<String>,
    refs: shared_pack::ImportRefs,
    counts: ResetCounts,
}

fn plan(tx: &Transaction<'_>, loupist: &str) -> KnowledgeResult<ResetPlan> {
    let controls = memory_controls::load(tx, loupist)?;
    let mut refs = shared_pack::ImportRefs::default();
    let mut lenses = BTreeSet::new();
    let mut counts = ResetCounts::default();
    // The creation event owns authorship. Editing a shared or user-created item
    // never turns it into something Reset may remove.
    let mut q = tx.prepare("SELECT r.record_id, r.kind, COALESCE(m.lens_kind,'') FROM knowledge_records r LEFT JOIN knowledge_lens_metadata m ON m.record_id=r.record_id WHERE r.owner_loupist_id=?1 AND EXISTS(SELECT 1 FROM knowledge_growth_events e WHERE e.loupist_id=?1 AND e.record_id=r.record_id AND e.kind=CASE r.kind WHEN 'lens' THEN 'lens_created' ELSE 'source_retained' END AND e.actor='loupist') AND NOT EXISTS(SELECT 1 FROM knowledge_growth_events e WHERE e.loupist_id=?1 AND e.record_id=r.record_id AND e.actor IN ('user','import') AND e.kind IN ('lens_created','lens_revised','source_retained')) AND NOT EXISTS(SELECT 1 FROM knowledge_standing_selection s WHERE s.loupist_id=?1 AND s.record_id=r.record_id)")?;
    let rows = q
        .query_map([loupist], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (id, kind, lens_kind) in rows {
        if controls.removed.contains(&id) {
            continue;
        }
        if kind == "lens" && matches!(lens_kind.as_str(), "method" | "principle") {
            lenses.insert(id);
            if lens_kind == "principle" {
                counts.principles += 1;
            } else {
                counts.methods += 1;
            }
        } else if kind == "source" {
            let protected: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM knowledge_source_captures WHERE record_id=?1 AND authorship IN ('user','imported_contributor'))", [&id], |r| r.get(0))?;
            if !protected {
                refs.sources.insert(id);
            }
        }
    }
    // Only names with known Loupist authorship may disappear. Unknown and
    // user/shared authorship stays, including a name still used by another claim.
    let mut q = tx.prepare("SELECT subject_id FROM knowledge_growth_events e WHERE loupist_id=?1 AND kind='entity_added' AND actor='loupist' AND subject_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM knowledge_growth_events p WHERE p.loupist_id=e.loupist_id AND p.subject_id=e.subject_id AND p.kind='entity_added' AND p.actor IN ('user','import'))")?;
    refs.nodes = q
        .query_map([loupist], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    let pack_tables: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='pack_world_items')",
        [],
        |r| r.get(0),
    )?;
    let mut q = tx.prepare("SELECT subject_id FROM knowledge_growth_events e WHERE loupist_id=?1 AND kind='claim_added' AND actor='loupist' AND subject_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM knowledge_growth_events p WHERE p.loupist_id=e.loupist_id AND p.subject_id=e.subject_id AND p.kind IN ('claim_added','claim_disputed','claim_withdrawn') AND p.actor IN ('user','import'))")?;
    let mut owned_claims: BTreeSet<String> = q
        .query_map([loupist], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    if pack_tables {
        let mut q = tx.prepare("SELECT kind,subject_id FROM pack_world_items WHERE loupist_id=?1 AND kind IN ('node','claim','source')")?;
        for row in q.query_map([loupist], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })? {
            let (kind, id) = row?;
            if kind == "node" {
                refs.nodes.remove(&id);
            } else if kind == "claim" {
                owned_claims.remove(&id);
            } else {
                refs.sources.remove(&id);
            }
        }
    }
    let mut q = tx.prepare("SELECT record_id,span_id FROM knowledge_source_spans")?;
    let mut span_sources = BTreeMap::new();
    for row in q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (record, span) = row?;
        if refs.sources.contains(&record) {
            refs.spans.insert(span.clone());
            span_sources.insert(span, record);
        }
    }
    let world: Option<String> = tx.query_row("SELECT v.payload_json FROM knowledge_records r JOIN knowledge_record_heads h ON h.record_id=r.record_id JOIN knowledge_record_revisions v ON v.revision_id=h.revision_id WHERE r.owner_loupist_id=?1 AND r.kind='world'", [loupist], |r| r.get(0)).optional()?;
    if let Some(world) = world {
        let body: super::WorldSnapshotBody = serde_json::from_str(&world)?;
        // Independent user/shared support preserves the relationship. A known
        // owned relationship without evidence is still a Loupist addition.
        refs.claims = body
            .claims
            .iter()
            .filter(|claim| {
                owned_claims.contains(&claim.id)
                    && claim
                        .evidence
                        .iter()
                        .all(|binding| refs.spans.contains(&binding.span_id))
            })
            .map(|claim| claim.id.clone())
            .collect();
        // Keep the sole basis of a protected relationship. Otherwise deleting
        // that Source would erase user/shared work or make derived reload fail.
        for claim in &body.claims {
            if !refs.claims.contains(&claim.id)
                && !claim.evidence.is_empty()
                && claim
                    .evidence
                    .iter()
                    .all(|binding| refs.spans.contains(&binding.span_id))
            {
                for binding in &claim.evidence {
                    if let Some(source) = span_sources.get(&binding.span_id) {
                        refs.sources.remove(source);
                    }
                }
            }
        }
        refs.spans.retain(|span| {
            span_sources
                .get(span)
                .is_some_and(|source| refs.sources.contains(source))
        });
    }
    refs.preserve_other_claims = true;
    (counts.names, counts.relationships) = shared_pack::removable_world(tx, loupist, &refs)?;
    counts.sources = refs.sources.len();
    Ok(ResetPlan {
        lenses,
        refs,
        counts,
    })
}

pub fn preview(tx: &Transaction<'_>, loupist: &str) -> KnowledgeResult<ResetCounts> {
    super::model::validate_loupist_id(loupist)?;
    Ok(plan(tx, loupist)?.counts)
}

pub fn reset(
    tx: &Transaction<'_>,
    blobs: &BlobStore,
    loupist: &str,
    expected: &ResetCounts,
    at_ms: i64,
) -> KnowledgeResult<ResetCounts> {
    if shared_pack::world_job_running(tx, loupist)? {
        return Err(KnowledgeError::Invalid("world processing is busy"));
    }
    let plan = plan(tx, loupist)?;
    if &plan.counts != expected {
        return Err(KnowledgeError::Invalid(
            "Memory changed; review the reset counts again",
        ));
    }
    shared_pack::remove_world_additions(
        tx,
        blobs,
        loupist,
        &plan.refs,
        at_ms,
        "user",
        Some(serde_json::json!({"kind":"memory_reset"})),
    )?;
    let mut controls = memory_controls::load(tx, loupist)?;
    for id in &plan.lenses {
        // Release the active-title/statement uniqueness reservation as well.
        tx.execute("UPDATE knowledge_lens_metadata SET archived_at_ms=?1 WHERE loupist_id=?2 AND record_id=?3", params![at_ms,loupist,id])?;
    }
    controls.removed.extend(plan.lenses);
    controls.removed.extend(plan.refs.sources);
    controls.paused.retain(|id| !controls.removed.contains(id));
    memory_controls::save(tx, loupist, &controls)?;
    // Pending extraction of a removed Source must not put it back later.
    tx.execute("UPDATE knowledge_graph_outbox SET status='cancelled_before_start' WHERE loupist_id=?1 AND status IN ('queued','paused_budget') AND json_extract(payload_json,'$.sourceBinding.sourceRecordId') IN (SELECT value FROM json_each(?2))", params![loupist,serde_json::to_string(&controls.removed)?])?;
    // The existing archive event envelope carries this bulk reset audit without
    // adding a new schema-constrained event kind.
    let mut event = super::growth::GrowthEvent::new(
        loupist,
        "",
        at_ms,
        "lens_archived",
        "user",
        "Reset by you",
    );
    event.record_id = None;
    event.origin = Some(serde_json::json!({"kind":"memory_reset", "counts":plan.counts}));
    event.insert(tx)?;
    Ok(plan.counts)
}
