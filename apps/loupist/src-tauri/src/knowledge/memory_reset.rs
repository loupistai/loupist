//! Reset only provenance owned by this Loupist. Unknown authorship is preserved.
use super::{memory_controls, shared_pack, BlobStore, KnowledgeError, KnowledgeResult};
use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResetCounts {
    pub principles: usize,
    /// Whole Loupist-origin pages that disappear.
    pub pages: usize,
    /// Loupist-origin sections retired, and Loupist-written summaries
    /// cleared, on pages the user keeps.
    pub sections: usize,
    pub names: usize,
    pub relationships: usize,
    pub sources: usize,
}

struct ResetPlan {
    lenses: BTreeSet<String>,
    /// (record id, section key, section digest) of Loupist-origin sections on kept pages.
    sections: Vec<(String, String, String)>,
    /// Kept pages whose summary the Loupist wrote last.
    summaries: Vec<String>,
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
    let mut sections = Vec::new();
    let mut summaries = Vec::new();
    let mut q = tx.prepare("SELECT r.record_id, r.kind, COALESCE(m.kind,''), m.archived_at_ms IS NOT NULL FROM knowledge_records r LEFT JOIN knowledge_lens_metadata m ON m.record_id=r.record_id WHERE r.owner_loupist_id=?1 AND (r.kind='source' OR m.record_id IS NOT NULL)")?;
    let rows = q
        .query_map([loupist], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, bool>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (id, kind, lens_kind, archived) in rows {
        if controls.removed.contains(&id) {
            continue;
        }
        if kind == "lens" {
            if archived {
                continue;
            }
            // The creation event owns page authorship; sections carry their own.
            let created_by_loupist: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM knowledge_growth_events e WHERE e.loupist_id=?1 AND e.record_id=?2 AND e.kind='lens_created' AND e.actor='loupist')", params![loupist, id], |r| r.get(0))?;
            if lens_kind == "principle" {
                if created_by_loupist {
                    lenses.insert(id);
                    counts.principles += 1;
                }
                continue;
            }
            let page = super::lens::read_page(tx, loupist, &id)?;
            let (kept, loupist_sections): (Vec<_>, Vec<_>) =
                page.body.sections().iter().partition(|section| {
                    matches!(
                        section.origin,
                        super::lens_page::Origin::User | super::lens_page::Origin::Shared
                    )
                });
            if created_by_loupist && kept.is_empty() {
                lenses.insert(id);
                counts.pages += 1;
            } else {
                for section in loupist_sections {
                    sections.push((
                        id.clone(),
                        section.key.as_str().to_owned(),
                        section.digest.clone(),
                    ));
                    counts.sections += 1;
                }
                // The log knows who wrote the summary the page shows now.
                let summarized_by: Option<String> = tx.query_row("SELECT origin FROM knowledge_lens_log WHERE record_id=?1 AND outcome='summarized' ORDER BY created_at_ms DESC, rowid DESC LIMIT 1", [&id], |r| r.get(0)).optional()?;
                if !page.body.summary().is_empty()
                    && matches!(summarized_by.as_deref(), Some("turn" | "learning"))
                {
                    summaries.push(id.clone());
                    counts.sections += 1;
                }
            }
        } else {
            let created_by_loupist: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM knowledge_growth_events e WHERE e.loupist_id=?1 AND e.record_id=?2 AND e.kind='source_retained' AND e.actor='loupist') AND NOT EXISTS(SELECT 1 FROM knowledge_growth_events e WHERE e.loupist_id=?1 AND e.record_id=?2 AND e.actor IN ('user','import') AND e.kind='source_retained')", params![loupist, id], |r| r.get(0))?;
            let protected: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM knowledge_source_captures WHERE record_id=?1 AND authorship IN ('user','imported_contributor'))", [&id], |r| r.get(0))?;
            if created_by_loupist && !protected {
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
        sections,
        summaries,
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
    let mut by_page: BTreeMap<String, Vec<super::lens_page::LensOp>> = BTreeMap::new();
    for (record_id, key, digest) in plan.sections {
        by_page
            .entry(record_id)
            .or_default()
            .push(super::lens_page::LensOp::Retire {
                key: super::lens_page::SectionKey::new(&key)?,
                expected: digest,
                why: "Reset by you".into(),
            });
    }
    for record_id in plan.summaries {
        by_page
            .entry(record_id)
            .or_default()
            .push(super::lens_page::LensOp::Summary {
                text: String::new(),
            });
    }
    for (record_id, ops) in by_page {
        for chunk in ops.chunks(super::lens_page::MAX_OPS_PER_DELTA) {
            super::lens::file(
                tx,
                loupist,
                &super::lens_page::FilingOrigin::User {
                    chat_id: None,
                    message_id: None,
                },
                vec![super::lens_page::PageDelta {
                    target: super::lens_page::PageTarget::Existing {
                        record_id: record_id.clone(),
                    },
                    ops: chunk.to_vec(),
                }],
                at_ms,
            )?;
        }
    }
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
    // A reset archives the affected Lens records; keep its bulk counts on that
    // archive event's origin so readers can identify the reset.
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
