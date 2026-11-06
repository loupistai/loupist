//! Saved Memory under an as-of cutoff (finance-point-in-time.md §3.2). A turn
//! with a cutoff reads only what was known by the end of that day: Sources
//! published by then (or the user's own statements saved by then), World
//! relationships published by then, Journal entries, notes and messages
//! created by then, and methods whose current version was written by then.
//! Principles still apply as they are today. What a filter removes is counted
//! by id, so the answer's note can say what the Loupist learned later.
use std::collections::{BTreeMap, BTreeSet};

use chrono::NaiveDate;
use serde_json::Value;

use super::*;
use crate::domain::AsOfLeftOut;
use crate::knowledge::{
    Authorship, JournalRecord, KnowledgeWrite, PublishedCapture, TimePoint, WorldSnapshotBody,
};

/// The last calendar day a turn may know about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Cutoff(pub NaiveDate);

impl Cutoff {
    pub(crate) fn parse(value: Option<&str>) -> Option<Self> {
        value
            .and_then(|value| NaiveDate::parse_from_str(value, "%Y-%m-%d").ok())
            .map(Self)
    }

    pub(crate) fn day(self) -> String {
        self.0.to_string()
    }

    /// Something this device recorded at `at_ms` was known by the end of the
    /// cutoff day, on the user's calendar.
    pub(crate) fn recorded(self, at_ms: i64) -> bool {
        chrono::DateTime::from_timestamp_millis(at_ms)
            .is_some_and(|at| at.with_timezone(&chrono::Local).date_naive() <= self.0)
    }

    /// A publication time on or before the cutoff; undated never is.
    pub(crate) fn published(self, time: Option<&TimePoint>) -> bool {
        crate::knowledge::published_by(time, &self.day())
    }

    /// A date-like value (`2026-03-18`, `2026-03`) on or before the cutoff.
    pub(crate) fn dated(self, value: &str) -> bool {
        crate::knowledge::last_day(value).is_some_and(|day| day <= self.0)
    }
}

/// Memory an as-of turn left out, by id so repeated reads count once.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct LeftOut {
    pub facts: BTreeSet<String>,
    pub sources: BTreeSet<String>,
    pub notes: BTreeSet<String>,
    pub methods: BTreeSet<String>,
}

impl LeftOut {
    pub(crate) fn merge(&mut self, other: LeftOut) {
        self.facts.extend(other.facts);
        self.sources.extend(other.sources);
        self.notes.extend(other.notes);
        self.methods.extend(other.methods);
    }

    pub(crate) fn counts(&self) -> AsOfLeftOut {
        let count = |set: &BTreeSet<String>| u32::try_from(set.len()).unwrap_or(u32::MAX);
        AsOfLeftOut {
            facts: count(&self.facts),
            sources: count(&self.sources),
            notes: count(&self.notes),
            methods: count(&self.methods),
        }
    }
}

/// A saved capture was known by the cutoff when it was published by then, or,
/// with no publication date, when it is the user's own statement saved by then.
pub(crate) fn capture_known(capture: &PublishedCapture, cutoff: Cutoff) -> bool {
    match capture
        .publication_time
        .as_ref()
        .filter(|time| time.value.is_some())
    {
        Some(time) => cutoff.published(Some(time)),
        None => capture.authorship == Authorship::User && cutoff.recorded(capture.received_at_ms),
    }
}

/// A conversation message was known by the cutoff when it was written by then,
/// or answered under a cutoff no later than this one.
pub(crate) fn message_known(message: &crate::domain::ChatMessage, cutoff: Cutoff) -> bool {
    cutoff.recorded(message.created_at_ms)
        || Cutoff::parse(message.as_of.as_deref()).is_some_and(|earlier| earlier.0 <= cutoff.0)
}

/// A Journal entry as it stood at the cutoff: absent when written later, and
/// with a later outcome read as still pending.
pub(crate) fn journal_known(mut record: JournalRecord, cutoff: Cutoff) -> Option<JournalRecord> {
    if !cutoff.recorded(record.created_at_ms) {
        return None;
    }
    if record
        .outcome
        .as_ref()
        .is_some_and(|outcome| !cutoff.dated(&outcome.observed_at))
    {
        record.outcome = None;
    }
    Some(record)
}

fn source_captures_known(
    write: &KnowledgeWrite<'_>,
    loupist_id: &str,
    record_id: &str,
    revision_id: Option<&str>,
    span_id: Option<&str>,
    cutoff: Cutoff,
) -> crate::knowledge::KnowledgeResult<bool> {
    let record = write.owned_record(loupist_id, record_id)?;
    let Some(revision_id) = revision_id
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .or(record.revision_id)
    else {
        return Ok(false);
    };
    let revision = write.get_revision(loupist_id, &revision_id)?;
    let payload: SourceRecordPayload = serde_json::from_value(revision.payload)?;
    Ok(payload
        .source
        .captures
        .iter()
        .filter(|capture| {
            span_id.is_none_or(|span| {
                capture
                    .representations
                    .iter()
                    .any(|representation| representation.spans.iter().any(|s| s.span_id == span))
            })
        })
        .any(|capture| capture_known(capture, cutoff)))
}

/// Whether a saved Source (one span of it, when named) was known by the cutoff.
pub(in crate::commands) fn source_known(
    state: &AppState,
    loupist_id: &str,
    record_id: &str,
    revision_id: Option<&str>,
    span_id: Option<&str>,
    cutoff: Cutoff,
) -> Result<bool, CommandError> {
    with_loupist_knowledge(state, loupist_id, |tx, blobs| {
        source_captures_known(
            &KnowledgeWrite::bind(tx, blobs),
            loupist_id,
            record_id,
            revision_id,
            span_id,
            cutoff,
        )
    })
}

fn lens_current_known(
    write: &KnowledgeWrite<'_>,
    loupist_id: &str,
    record_id: &str,
    cutoff: Cutoff,
) -> crate::knowledge::KnowledgeResult<bool> {
    Ok(cutoff.recorded(write.read_page(loupist_id, record_id)?.created_at_ms))
}

/// Whether a page existed by the cutoff. Sections filed later are hidden by the read.
pub(in crate::commands) fn lens_known(
    state: &AppState,
    loupist_id: &str,
    record_id: &str,
    cutoff: Cutoff,
) -> Result<bool, CommandError> {
    with_loupist_knowledge(state, loupist_id, |tx, blobs| {
        lens_current_known(
            &KnowledgeWrite::bind(tx, blobs),
            loupist_id,
            record_id,
            cutoff,
        )
    })
}

/// Relationships of a World revision published after the cutoff (or undated).
pub(in crate::commands) fn world_later_claims(
    state: &AppState,
    loupist_id: &str,
    record_id: &str,
    revision_id: Option<&str>,
    cutoff: Cutoff,
) -> Result<Vec<String>, CommandError> {
    with_loupist_knowledge(state, loupist_id, |tx, blobs| {
        let write = KnowledgeWrite::bind(tx, blobs);
        let record = write.owned_record(loupist_id, record_id)?;
        let Some(revision_id) = revision_id
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
            .or(record.revision_id)
        else {
            return Ok(Vec::new());
        };
        let body: WorldSnapshotBody =
            serde_json::from_value(write.get_revision(loupist_id, &revision_id)?.payload)?;
        Ok(body
            .claims
            .iter()
            .filter(|claim| !cutoff.published(claim.publication_time.as_ref()))
            .map(|claim| claim.id.clone())
            .collect())
    })
}

/// Evidence spans of a World revision whose Source was known by the cutoff.
/// A span that cannot be resolved to a retained Source is not known.
fn known_spans(
    write: &KnowledgeWrite<'_>,
    loupist_id: &str,
    world_record_id: &str,
    world_revision_id: &str,
    wanted: BTreeSet<String>,
    cutoff: Cutoff,
) -> crate::knowledge::KnowledgeResult<BTreeSet<String>> {
    let wanted: Vec<String> = wanted.into_iter().collect();
    let mut captures: BTreeMap<(String, String), Vec<PublishedCapture>> = BTreeMap::new();
    let mut known = BTreeSet::new();
    for chunk in wanted.chunks(256) {
        let Ok(targets) =
            write.world_source_targets(loupist_id, world_record_id, world_revision_id, chunk)
        else {
            continue;
        };
        for (span, target) in targets {
            let key = (target.record_id.clone(), target.revision_id.clone());
            if !captures.contains_key(&key) {
                let payload: SourceRecordPayload = serde_json::from_value(
                    write.get_revision(loupist_id, &target.revision_id)?.payload,
                )?;
                captures.insert(key.clone(), payload.source.captures);
            }
            let dated = captures[&key]
                .iter()
                .filter(|capture| {
                    capture.representations.iter().any(|representation| {
                        representation.spans.iter().any(|s| s.span_id == span)
                    })
                })
                .any(|capture| capture_known(capture, cutoff));
            if dated {
                known.insert(span);
            }
        }
    }
    Ok(known)
}

/// A relationship as it stood at the cutoff: an invalidation or expiry dated
/// later (or undated) is not shown, and evidence from Sources published later
/// is left out. `false` when none of its evidence was known by then.
fn claim_as_of(
    invalidations: [&mut Option<crate::knowledge::TimePoint>; 2],
    evidence: &mut Vec<crate::knowledge::EvidenceBinding>,
    spans: &BTreeSet<String>,
    cutoff: Cutoff,
) -> bool {
    for time in invalidations {
        if !time
            .as_ref()
            .is_some_and(|time| cutoff.published(Some(time)))
        {
            *time = None;
        }
    }
    let had = !evidence.is_empty();
    evidence.retain(|binding| spans.contains(&binding.span_id));
    !had || !evidence.is_empty()
}

/// A World view under a cutoff (`world_read`): relationships show their
/// status as of then, a relationship whose evidence was all published later is
/// left out, and names only it connected go with it (`keep` is the focus).
pub(in crate::commands) fn world_view_as_of(
    write: &KnowledgeWrite<'_>,
    loupist_id: &str,
    view: &mut crate::knowledge::WorldGraphView,
    keep: Option<&str>,
    cutoff: Cutoff,
) -> crate::knowledge::KnowledgeResult<()> {
    let wanted = view
        .edges
        .iter()
        .flat_map(|edge| edge.claim.evidence.iter().map(|e| e.span_id.clone()))
        .collect();
    let spans = known_spans(
        write,
        loupist_id,
        &view.record_id,
        &view.revision_id,
        wanted,
        cutoff,
    )?;
    let mut dropped_ends = BTreeSet::new();
    view.edges.retain_mut(|edge| {
        let claim = &mut edge.claim;
        let kept = claim_as_of(
            [&mut claim.invalid_at, &mut claim.expired_at],
            &mut claim.evidence,
            &spans,
            cutoff,
        );
        if !kept {
            dropped_ends.extend(edge.endpoints.iter().map(|end| end.node_id.clone()));
        }
        kept
    });
    let ends: BTreeSet<&str> = view
        .edges
        .iter()
        .flat_map(|edge| edge.endpoints.iter().map(|end| end.node_id.as_str()))
        .chain(keep)
        .collect();
    let orphaned: BTreeSet<String> = dropped_ends
        .into_iter()
        .filter(|id| !ends.contains(id.as_str()))
        .collect();
    view.nodes.retain(|node| !orphaned.contains(&node.id));
    Ok(())
}

/// What a World revision knew by the cutoff: its dated relationships, the
/// names they connect, evidence spans whose Source was known by then, and
/// annotations dated by then (every Source they cite was).
struct WorldKnown {
    claims: BTreeSet<String>,
    nodes: BTreeSet<String>,
    all_claims: BTreeSet<String>,
    spans: BTreeSet<String>,
    annotations: BTreeMap<String, bool>,
}

fn world_known(
    write: &KnowledgeWrite<'_>,
    loupist_id: &str,
    revision_id: &str,
    cutoff: Cutoff,
) -> crate::knowledge::KnowledgeResult<WorldKnown> {
    let revision = write.get_revision(loupist_id, revision_id)?;
    let body: WorldSnapshotBody = serde_json::from_value(revision.payload)?;
    let claims: BTreeSet<String> = body
        .claims
        .iter()
        .filter(|claim| cutoff.published(claim.publication_time.as_ref()))
        .map(|claim| claim.id.clone())
        .collect();
    let nodes = body
        .claims
        .iter()
        .filter(|claim| claims.contains(&claim.id))
        .flat_map(|claim| claim.endpoints.iter().map(|end| end.node_id.clone()))
        .collect();
    let wanted = body
        .claims
        .iter()
        .filter(|claim| claims.contains(&claim.id))
        .flat_map(|claim| claim.evidence.iter().map(|e| e.span_id.clone()))
        .chain(
            body.annotations
                .iter()
                .flat_map(|annotation| annotation.source_span_ids.iter().cloned()),
        )
        .collect();
    let spans = known_spans(
        write,
        loupist_id,
        &revision.record_id,
        revision_id,
        wanted,
        cutoff,
    )?;
    let annotations = body
        .annotations
        .iter()
        .map(|annotation| {
            let dated = !annotation.source_span_ids.is_empty()
                && annotation
                    .source_span_ids
                    .iter()
                    .all(|span| spans.contains(span));
            (annotation.topic_id.clone(), dated)
        })
        .collect();
    Ok(WorldKnown {
        claims,
        nodes,
        all_claims: body.claims.iter().map(|claim| claim.id.clone()).collect(),
        spans,
        annotations,
    })
}

/// The claim id of a graph view edge (Graphiti's `adapter_id`, or `id`).
fn view_edge_id(edge: &Value) -> Option<&str> {
    edge.get("adapter_id")
        .or_else(|| edge.get("id"))
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
}

fn view_node_ids(edge: &Value) -> Vec<String> {
    edge.get("endpoints")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|end| end.get("node_id").or_else(|| end.get("nodeId")))
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

/// A graph search view (Graphiti's ranked relationships) under a cutoff:
/// only relationships published by then, as they stood then, and the names
/// they connect reduced to name and kind. Returns whether any relationship
/// remains, and the ids of those left out.
fn graph_view_as_of(view: &mut Value, known: &WorldKnown, cutoff: Cutoff) -> (bool, Vec<String>) {
    let inner = if view.get("view").is_some_and(Value::is_object) {
        &mut view["view"]
    } else {
        view
    };
    let mut removed = Vec::new();
    let mut ends = BTreeSet::new();
    let Some(edges) = inner.get_mut("edges").and_then(Value::as_array_mut) else {
        return (false, removed);
    };
    edges.retain_mut(|edge| {
        let Some(id) = view_edge_id(edge).map(str::to_owned) else {
            return false;
        };
        if !known.claims.contains(&id) {
            removed.push(id);
            return false;
        }
        let Some(claim) = edge.as_object_mut() else {
            return false;
        };
        for key in ["invalid_at", "invalidAt", "expired_at", "expiredAt"] {
            if let Some(time) = claim.get_mut(key) {
                let dated = time
                    .get("value")
                    .and_then(Value::as_str)
                    .is_some_and(|value| cutoff.dated(value));
                if !dated {
                    *time = Value::Null;
                }
            }
        }
        if let Some(evidence) = claim.get_mut("evidence").and_then(Value::as_array_mut) {
            let had = !evidence.is_empty();
            evidence.retain(|binding| {
                binding
                    .get("span_id")
                    .or_else(|| binding.get("spanId"))
                    .and_then(Value::as_str)
                    .is_some_and(|span| known.spans.contains(span))
            });
            if had && evidence.is_empty() {
                removed.push(id);
                return false;
            }
        }
        ends.extend(view_node_ids(edge));
        true
    });
    let any = !edges.is_empty();
    if let Some(nodes) = inner.get_mut("nodes").and_then(Value::as_array_mut) {
        nodes.retain(|node| {
            node.get("adapter_id")
                .or_else(|| node.get("id"))
                .and_then(Value::as_str)
                .is_some_and(|id| ends.contains(id))
        });
        // A name's summary may carry later facts.
        for node in nodes.iter_mut().filter_map(Value::as_object_mut) {
            for key in [
                "description",
                "summary",
                "aliases",
                "properties",
                "attributes",
            ] {
                node.remove(key);
            }
        }
    }
    (any, removed)
}

/// Removes search hits the cutoff leaves out and records each by kind.
/// Source hits need a span known by then; World hits a relationship published
/// by then (a name needs one such relationship, an annotation every Source it
/// cites, a hit without an id is left out); a graph view keeps only such
/// relationships; method hits a current version written by then; saved notes
/// a recording date by then, and their pointers to later notes only those
/// written by then.
pub(in crate::commands) fn filter_hits(
    state: &AppState,
    loupist_id: &str,
    hits: &mut Vec<Value>,
    cutoff: Cutoff,
    left: &mut LeftOut,
) -> Result<(), CommandError> {
    let keep = with_loupist_knowledge(state, loupist_id, |tx, blobs| {
        let write = KnowledgeWrite::bind(tx, blobs);
        let mut sources: BTreeMap<(String, String), bool> = BTreeMap::new();
        let mut lenses: BTreeMap<String, bool> = BTreeMap::new();
        let mut worlds: BTreeMap<String, WorldKnown> = BTreeMap::new();
        let mut keep = Vec::with_capacity(hits.len());
        let mut removed = LeftOut::default();
        for hit in hits.iter_mut() {
            let text = |key: &str| {
                hit.get(key)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned()
            };
            let record_id = text("recordId");
            let kept = match text("scope").as_str() {
                "source" if !record_id.is_empty() => {
                    let span = text("spanId");
                    let key = (record_id.clone(), span.clone());
                    let known = match sources.get(&key) {
                        Some(known) => *known,
                        None => {
                            let known = source_captures_known(
                                &write,
                                loupist_id,
                                &record_id,
                                Some(&text("revisionId")),
                                (!span.is_empty()).then_some(span.as_str()),
                                cutoff,
                            )?;
                            sources.insert(key, known);
                            known
                        }
                    };
                    if !known {
                        removed.sources.insert(record_id.clone());
                    }
                    known
                }
                "lens" if !record_id.is_empty() => {
                    let known = match lenses.get(&record_id) {
                        Some(known) => *known,
                        None => {
                            let known = lens_current_known(&write, loupist_id, &record_id, cutoff)?;
                            lenses.insert(record_id.clone(), known);
                            known
                        }
                    };
                    if !known {
                        removed.methods.insert(record_id.clone());
                    }
                    known
                }
                "world" => {
                    let revision_id = text("revisionId");
                    let object_id = text("objectId");
                    if revision_id.is_empty() {
                        false
                    } else {
                        if !worlds.contains_key(&revision_id) {
                            let known = world_known(&write, loupist_id, &revision_id, cutoff)?;
                            worlds.insert(revision_id.clone(), known);
                        }
                        let known = &worlds[&revision_id];
                        if let Some(view) = hit.get_mut("view") {
                            let (any, later) = graph_view_as_of(view, known, cutoff);
                            removed.facts.extend(later);
                            any
                        } else if object_id.is_empty() {
                            false
                        } else if known.all_claims.contains(&object_id) {
                            let kept = known.claims.contains(&object_id);
                            if !kept {
                                removed.facts.insert(object_id.clone());
                            }
                            kept
                        } else if let Some(dated) = known.annotations.get(&object_id) {
                            *dated
                        } else {
                            known.nodes.contains(&object_id)
                        }
                    }
                }
                "research" => {
                    let kept = hit
                        .get("recordedAtMs")
                        .and_then(Value::as_i64)
                        .is_none_or(|at| cutoff.recorded(at));
                    if !kept {
                        removed.notes.insert(text("researchId"));
                    }
                    if let Some(later) = hit.get_mut("laterNotes").and_then(Value::as_array_mut) {
                        later.retain(|note| {
                            note.get("updatedAtMs")
                                .and_then(Value::as_i64)
                                .is_some_and(|at| cutoff.recorded(at))
                        });
                    }
                    kept
                }
                _ => true,
            };
            keep.push(kept);
        }
        Ok((keep, removed))
    })?;
    let (keep, removed) = keep;
    let mut flags = keep.into_iter();
    hits.retain(|_| flags.next().unwrap_or(true));
    left.merge(removed);
    Ok(())
}

/// Pointers to later notes, keeping only notes written by the cutoff.
pub(crate) fn later_notes_known(notes: &mut Vec<crate::knowledge::LaterNote>, cutoff: Cutoff) {
    notes.retain(|note| cutoff.recorded(note.updated_at_ms));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::TimePrecision;

    fn time(value: &str) -> TimePoint {
        TimePoint {
            precision: TimePrecision::Date,
            value: Some(value.into()),
            approximate: false,
        }
    }

    #[test]
    fn cutoff_reads_dates_at_the_end_of_the_day() {
        let cutoff = Cutoff::parse(Some("2026-03-10")).unwrap();
        assert!(cutoff.published(Some(&time("2026-03-10"))));
        assert!(cutoff.published(Some(&time("2026-03-10T23:59:00Z"))));
        assert!(!cutoff.published(Some(&time("2026-03-11"))));
        assert!(!cutoff.published(None));
        // A month counts at its last day.
        assert!(!cutoff.dated("2026-03"));
        assert!(cutoff.dated("2026-02"));
        let noon = chrono::NaiveDate::from_ymd_opt(2026, 3, 10)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap()
            .and_local_timezone(chrono::Local)
            .unwrap()
            .timestamp_millis();
        assert!(cutoff.recorded(noon));
        assert!(!cutoff.recorded(noon + 86_400_000));
    }

    #[test]
    fn journal_entries_and_outcomes_keep_to_the_cutoff() {
        let cutoff = Cutoff::parse(Some("2026-03-20")).unwrap();
        let created = chrono::NaiveDate::from_ymd_opt(2026, 3, 5)
            .unwrap()
            .and_hms_opt(9, 0, 0)
            .unwrap()
            .and_local_timezone(chrono::Local)
            .unwrap()
            .timestamp_millis();
        let mut record: JournalRecord = serde_json::from_value(serde_json::json!({
            "journalId": "j1", "researchId": "r1", "loupistId": "l1",
            "judgment": "Revenue will beat ₩445bn", "reasoning": "",
            "inputs": {"sources": [], "worlds": [], "lenses": [], "earlier": []},
            "inputDigest": "d", "createdAtMs": created, "originChatId": null,
            "updatedAtMs": created, "archived": false,
            "outcome": {"observed": 452.3, "observedAt": "2026-04-02", "result": "came_true"}
        }))
        .unwrap();
        // A later check reads as still pending.
        assert!(journal_known(record.clone(), cutoff)
            .unwrap()
            .outcome
            .is_none());
        record.outcome.as_mut().unwrap().observed_at = "2026-03-18".into();
        assert!(journal_known(record.clone(), cutoff)
            .unwrap()
            .outcome
            .is_some());
        // An entry written after the cutoff is left out.
        assert!(journal_known(record, Cutoff::parse(Some("2026-03-04")).unwrap()).is_none());
    }
}
