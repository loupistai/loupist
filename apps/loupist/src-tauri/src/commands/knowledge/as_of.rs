//! Saved Memory under an as-of cutoff (docs/knowledge/memory.md#memory-under-a-cutoff). A turn
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
    day_key, decide_pair, is_temporal_qualifier, reopening_qualifiers, witness_order, Assertion,
    AssertionSources, AssertionWitness, Authorship, Ending, GraphClaim, JournalRecord,
    KnowledgeWrite, Lapse, PairEffects, PairSelection, PublishedCapture, Qualifier, Reopened,
    TimePoint, TimePrecision, WorldSnapshotBody, CONFLICTS_WITH_CLAIM, END_BASIS,
    SUPERSEDED_BY_CLAIM, SUPERSEDED_BY_SPAN,
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

    /// The as-of date a request names: a calendar day from 1990-01-01 to
    /// today on this device. Chat turns and World's reads take it from here;
    /// a later day is refused with its own code, which the renderer names.
    pub(crate) fn requested(value: Option<&str>) -> Result<Option<Self>, CommandError> {
        let Some(value) = value else {
            return Ok(None);
        };
        let date = NaiveDate::parse_from_str(value, "%Y-%m-%d")
            .ok()
            .filter(|_| value.len() == 10)
            .ok_or_else(|| CommandError::invalid("as_of must be a date in YYYY-MM-DD format"))?;
        let earliest = NaiveDate::from_ymd_opt(1990, 1, 1).expect("valid date");
        if date < earliest {
            return Err(CommandError::invalid("as_of must be 1990-01-01 or later"));
        }
        if date > crate::local_time::today() {
            return Err(CommandError::new(
                crate::app::CommandErrorCode::AsOfFuture,
                "as_of must be today or earlier",
            ));
        }
        Ok(Some(Self(date)))
    }

    pub(crate) fn day(self) -> String {
        self.0.to_string()
    }

    /// Something this device recorded at `at_ms` was known by the end of the
    /// cutoff day, on the user's calendar.
    pub(crate) fn recorded(self, at_ms: i64) -> bool {
        crate::local_time::date_at_ms(at_ms).is_some_and(|day| day <= self.0)
    }

    /// A publication time on or before the cutoff; undated never is.
    pub(crate) fn published(self, time: Option<&TimePoint>) -> bool {
        crate::knowledge::published_by(time, &self.day())
    }

    /// A date-like value (`2026-03-18`, `2026-03`) on or before the cutoff.
    pub(crate) fn dated(self, value: &str) -> bool {
        crate::knowledge::last_day(value).is_some_and(|day| day <= self.0)
    }

    /// The last millisecond of the cutoff day on the user's clock.
    pub(crate) fn end_ms(self) -> i64 {
        let offset = *crate::local_time::now().offset();
        use chrono::TimeZone;
        self.0
            .and_hms_milli_opt(23, 59, 59, 999)
            .and_then(|moment| offset.from_local_datetime(&moment).single())
            .map_or(i64::MAX, |moment| moment.timestamp_millis())
    }

    fn reached(self, time: &TimePoint) -> bool {
        if time.precision == TimePrecision::Instant {
            return time
                .value
                .as_deref()
                .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
                .is_some_and(|instant| self.recorded(instant.timestamp_millis()));
        }
        self.published(Some(time))
    }
}

fn projected_end_fields(claim: &GraphClaim) -> (Option<bool>, Option<TimePoint>) {
    // Projection has already checked the ending against the local cutoff.
    let end = claim.invalid_at.clone();
    let ended = end.is_some() || claim.status != crate::knowledge::ClaimStatus::Active;
    (ended.then_some(true), end)
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

pub(in crate::commands) fn source_captures_known(
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
    if revision.record_id != record_id {
        return Ok(false);
    }
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

/// The one Lens rule under a cutoff, for reads, search, backlinks and the
/// automatic context: a page is known when its whole current head was written
/// by then. A principle applies as it is today.
pub(in crate::commands) fn lens_visible(
    write: &KnowledgeWrite<'_>,
    loupist_id: &str,
    record_id: &str,
    cutoff: Cutoff,
) -> crate::knowledge::KnowledgeResult<bool> {
    if write.lens_kind(loupist_id, record_id)? == crate::knowledge::lens_page::LensKind::Principle {
        return Ok(true);
    }
    let Some(revision_id) = write.owned_record(loupist_id, record_id)?.revision_id else {
        return Ok(false);
    };
    Ok(cutoff.recorded(write.get_revision(loupist_id, &revision_id)?.created_at_ms))
}

/// A Lens page a cutoff can read: one that existed by the end of that day. It
/// reads as it stood then (`read_lens_page_as_of`). A principle applies as
/// it is today.
pub(in crate::commands) fn lens_created_by(
    state: &AppState,
    loupist_id: &str,
    record_id: &str,
    cutoff: Cutoff,
) -> Result<bool, CommandError> {
    with_loupist_knowledge(state, loupist_id, |tx, blobs| {
        let write = KnowledgeWrite::bind(tx, blobs);
        if write.lens_kind(loupist_id, record_id)?
            == crate::knowledge::lens_page::LensKind::Principle
        {
            return Ok(true);
        }
        write.owned_record(loupist_id, record_id)?;
        let created: i64 = tx.query_row(
            "SELECT created_at_ms FROM knowledge_records WHERE record_id = ?1",
            [record_id],
            |row| row.get(0),
        )?;
        Ok(cutoff.recorded(created))
    })
}

/// [`lens_visible`] in its own transaction.
pub(in crate::commands) fn lens_known(
    state: &AppState,
    loupist_id: &str,
    record_id: &str,
    cutoff: Cutoff,
) -> Result<bool, CommandError> {
    with_loupist_knowledge(state, loupist_id, |tx, blobs| {
        lens_visible(
            &KnowledgeWrite::bind(tx, blobs),
            loupist_id,
            record_id,
            cutoff,
        )
    })
}

/// Claims of a World revision without evidence known by the cutoff.
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
        let known = world_known(&write, loupist_id, &revision_id, cutoff)?;
        Ok(known
            .all_claims
            .into_iter()
            .filter(|id| !known.claims.contains_key(id))
            .collect())
    })
}

/// Evidence spans of a World revision whose Source was known by the cutoff.
/// A span that cannot be resolved to a retained Source is not known.
#[derive(Clone)]
struct KnownWitness {
    source_record: String,
    publication: Option<TimePoint>,
}

fn known_spans(
    write: &KnowledgeWrite<'_>,
    loupist_id: &str,
    world_record_id: &str,
    world_revision_id: &str,
    wanted: BTreeSet<String>,
    cutoff: Cutoff,
) -> crate::knowledge::KnowledgeResult<BTreeMap<String, KnownWitness>> {
    resolved_spans(
        write,
        loupist_id,
        world_record_id,
        world_revision_id,
        wanted,
        Some(cutoff),
    )
}

/// Evidence spans resolved to their retained Source and its publication; with
/// a cutoff, only those whose Source was known by then.
fn resolved_spans(
    write: &KnowledgeWrite<'_>,
    loupist_id: &str,
    world_record_id: &str,
    world_revision_id: &str,
    wanted: BTreeSet<String>,
    cutoff: Option<Cutoff>,
) -> crate::knowledge::KnowledgeResult<BTreeMap<String, KnownWitness>> {
    let wanted: Vec<String> = wanted.into_iter().collect();
    let mut captures: BTreeMap<(String, String), Vec<PublishedCapture>> = BTreeMap::new();
    let mut known = BTreeMap::new();
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
                .find(|capture| cutoff.is_none_or(|cutoff| capture_known(capture, cutoff)));
            if let Some(capture) = dated {
                known.insert(
                    span,
                    KnownWitness {
                        source_record: target.record_id,
                        publication: capture
                            .publication_time
                            .clone()
                            .filter(|time| time.precision != TimePrecision::Unknown),
                    },
                );
            }
        }
    }
    Ok(known)
}

/// A relationship as it stood at the cutoff. Its end requires a known
/// witness and an effective date within the cutoff.
pub(crate) fn claim_as_of(
    claim: &mut GraphClaim,
    spans: &BTreeSet<String>,
    visible_claims: &BTreeSet<String>,
    cutoff: Cutoff,
) {
    claim
        .evidence
        .retain(|binding| spans.contains(&binding.span_id));
    let witness_known = claim.qualifier(SUPERSEDED_BY_SPAN).map_or_else(
        || {
            claim
                .expired_at
                .as_ref()
                .is_none_or(|time| cutoff.reached(time))
        },
        |span| spans.contains(span),
    );
    let ending_visible = witness_known
        && claim
            .invalid_at
            .as_ref()
            .is_some_and(|time| cutoff.reached(time));
    if !ending_visible {
        claim.invalid_at = None;
    }
    if !ending_visible
        || claim
            .expired_at
            .as_ref()
            .is_some_and(|at| !cutoff.reached(at))
    {
        claim.expired_at = None;
    }
    claim
        .qualifiers
        .retain(|qualifier| match qualifier.key.as_str() {
            SUPERSEDED_BY_SPAN | END_BASIS => ending_visible,
            SUPERSEDED_BY_CLAIM => ending_visible && visible_claims.contains(&qualifier.value),
            CONFLICTS_WITH_CLAIM => visible_claims.contains(&qualifier.value),
            _ => true,
        });
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
    let known = world_claims_as_of(write, loupist_id, &view.revision_id, cutoff)?;
    let mut dropped_ends = BTreeSet::new();
    view.edges.retain_mut(|edge| {
        if let Some(projected) = known.get(&edge.claim.id) {
            edge.claim = projected.clone();
            edge.status = projected.status;
            (edge.ended, edge.end) = projected_end_fields(projected);
            true
        } else {
            dropped_ends.extend(edge.endpoints.iter().map(|end| end.node_id.clone()));
            false
        }
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
    claims: BTreeMap<String, GraphClaim>,
    nodes: BTreeSet<String>,
    all_claims: BTreeSet<String>,
    annotations: BTreeMap<String, bool>,
}

pub(crate) fn world_claims_as_of(
    write: &KnowledgeWrite<'_>,
    loupist_id: &str,
    revision_id: &str,
    cutoff: Cutoff,
) -> crate::knowledge::KnowledgeResult<BTreeMap<String, GraphClaim>> {
    Ok(world_known(write, loupist_id, revision_id, cutoff)?.claims)
}

/// World as the cutoff knows it, for the renderer's reads under World's As of
/// date: the one projection ([`world_known`]) as a snapshot the unchanged map
/// code reads, and what it left out.
pub(crate) struct WorldAsOf {
    pub body: WorldSnapshotBody,
    pub report: crate::knowledge::WorldAsOfReport,
}

pub(crate) fn world_as_of(
    write: &KnowledgeWrite<'_>,
    loupist_id: &str,
    revision_id: &str,
    cutoff: Cutoff,
) -> crate::knowledge::KnowledgeResult<WorldAsOf> {
    let revision = write.get_revision(loupist_id, revision_id)?;
    let body: WorldSnapshotBody = serde_json::from_value(revision.payload)?;
    let mut known = world_known(write, loupist_id, revision_id, cutoff)?;
    let report = world_left_out(
        write,
        loupist_id,
        &revision.record_id,
        revision_id,
        &body,
        &known,
        cutoff,
    )?;
    let claims = body
        .claims
        .iter()
        .filter_map(|claim| known.claims.remove(&claim.id))
        .collect();
    // A name's summary may carry later facts, as in a World read under a cutoff.
    let nodes = body
        .nodes
        .into_iter()
        .filter(|node| known.nodes.contains(&node.id))
        .map(|mut node| {
            node.description.clear();
            node.aliases.clear();
            node.properties.clear();
            node
        })
        .collect();
    let annotations = body
        .annotations
        .into_iter()
        .filter(|annotation| known.annotations.get(&annotation.topic_id) == Some(&true))
        .collect();
    Ok(WorldAsOf {
        body: WorldSnapshotBody {
            nodes,
            claims,
            annotations,
            relation_vocabulary: Vec::new(),
            temporal: Default::default(),
        },
        report,
    })
}

/// What a World read under the cutoff leaves out: the relationships with no
/// evidence known by then, and the Sources World cites none of whose evidence
/// was.
pub(crate) fn world_as_of_report(
    write: &KnowledgeWrite<'_>,
    loupist_id: &str,
    revision_id: &str,
    cutoff: Cutoff,
) -> crate::knowledge::KnowledgeResult<crate::knowledge::WorldAsOfReport> {
    let revision = write.get_revision(loupist_id, revision_id)?;
    let body: WorldSnapshotBody = serde_json::from_value(revision.payload)?;
    let known = world_known(write, loupist_id, revision_id, cutoff)?;
    world_left_out(
        write,
        loupist_id,
        &revision.record_id,
        revision_id,
        &body,
        &known,
        cutoff,
    )
}

fn world_left_out(
    write: &KnowledgeWrite<'_>,
    loupist_id: &str,
    world_record_id: &str,
    revision_id: &str,
    body: &WorldSnapshotBody,
    known: &WorldKnown,
    cutoff: Cutoff,
) -> crate::knowledge::KnowledgeResult<crate::knowledge::WorldAsOfReport> {
    let later = body
        .claims
        .iter()
        .filter(|claim| !known.claims.contains_key(&claim.id))
        .count();
    let spans: BTreeSet<String> = body
        .claims
        .iter()
        .flat_map(|claim| claim.evidence.iter().map(|binding| binding.span_id.clone()))
        .collect();
    let all = resolved_spans(
        write,
        loupist_id,
        world_record_id,
        revision_id,
        spans.clone(),
        None,
    )?;
    let dated: BTreeSet<String> = known_spans(
        write,
        loupist_id,
        world_record_id,
        revision_id,
        spans,
        cutoff,
    )?
    .into_values()
    .map(|witness| witness.source_record)
    .collect();
    let sources: BTreeSet<&str> = all
        .values()
        .map(|witness| witness.source_record.as_str())
        .filter(|record| !dated.contains(*record))
        .collect();
    let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    Ok(crate::knowledge::WorldAsOfReport {
        facts: count(later),
        sources: count(sources.len()),
    })
}

/// After a reset takes a Source's spans out of World, every remaining claim's
/// ending and conflicts as the pair rule decides them without those spans: an
/// ending or conflict link that only the forgotten Source justified goes, as
/// it does in a replay. The same rule (`decide_pair`, `ending_order`) the
/// pipeline's `recompute_endings` applies.
pub(crate) fn recompute_after_removal(
    write: &KnowledgeWrite<'_>,
    loupist_id: &str,
    world_record_id: &str,
    world_revision_id: &str,
    body: &mut WorldSnapshotBody,
    removed: &BTreeSet<String>,
) -> crate::knowledge::KnowledgeResult<()> {
    let wanted = body
        .claims
        .iter()
        .flat_map(|claim| claim.evidence.iter().map(|binding| binding.span_id.clone()))
        .chain(body.temporal.witness_spans().map(str::to_owned))
        .filter(|span| !removed.contains(span))
        .collect();
    let known = resolved_spans(
        write,
        loupist_id,
        world_record_id,
        world_revision_id,
        wanted,
        None,
    )?;
    let present: BTreeSet<String> = body.claims.iter().map(|claim| claim.id.clone()).collect();
    let mut temporal = temporal_view(body, &present, &known);
    for claim in &mut body.claims {
        if let Some(view) = temporal.remove(&claim.id) {
            view.apply(claim);
        }
        claim.qualifiers.retain(|qualifier| {
            qualifier.key != CONFLICTS_WITH_CLAIM || present.contains(&qualifier.value)
        });
        // Its assertion time is the earliest publication among the Sources
        // still asserting it (master plan D2-1a).
        if !claim.evidence.is_empty() {
            claim.publication_time = claim
                .evidence
                .iter()
                .filter_map(|binding| known.get(&binding.span_id))
                .filter_map(|witness| witness.publication.clone())
                .min_by(|a, b| {
                    a.compare_at_common_precision(b)
                        .unwrap_or_else(|| a.value.cmp(&b.value))
                });
        }
    }
    Ok(())
}

/// What one claim shows after some readings, under the cutoff's projection.
#[derive(Clone, Debug, Default)]
pub(crate) struct Shown {
    pub end: Option<crate::knowledge::world_changes::ShownEnd>,
    pub conflicts: BTreeSet<String>,
    pub evidence: BTreeSet<String>,
    pub publication: Option<TimePoint>,
}

/// The World a cutoff knows, replayed reading by reading: the one projection
/// ([`world_claims_as_of`]) limited to the spans read by a moment. Its last
/// state is the World read under the cutoff; each earlier one is the World
/// that had read only those Sources then. The feed's rows and a claim's
/// lineage under a cutoff come from these states, not from the episodes the
/// full World recorded, whose changes depended on Sources read in between.
pub(crate) struct Replay {
    body: WorldSnapshotBody,
    known: BTreeMap<String, KnownWitness>,
    first_read: BTreeMap<String, i64>,
    cutoff: Cutoff,
}

impl Replay {
    pub(crate) fn load(
        write: &KnowledgeWrite<'_>,
        loupist_id: &str,
        revision_id: &str,
        cutoff: Cutoff,
    ) -> crate::knowledge::KnowledgeResult<Self> {
        let revision = write.get_revision(loupist_id, revision_id)?;
        let body: WorldSnapshotBody = serde_json::from_value(revision.payload)?;
        let wanted = body
            .claims
            .iter()
            .flat_map(|claim| {
                claim
                    .evidence
                    .iter()
                    .map(|e| e.span_id.as_str())
                    .chain(claim.qualifier(SUPERSEDED_BY_SPAN))
            })
            .chain(body.temporal.witness_spans())
            .map(str::to_owned)
            .collect();
        let known = known_spans(
            write,
            loupist_id,
            &revision.record_id,
            revision_id,
            wanted,
            cutoff,
        )?;
        let first_read = write.first_episode_ms_by_span(loupist_id, &revision.record_id)?;
        Ok(Self {
            body,
            known,
            first_read,
            cutoff,
        })
    }

    /// A span the cutoff knows and this Loupist had read by `until_ms`. A span
    /// with no live episode (a legacy shell) counts as read throughout.
    fn read_by(&self, span: &str, until_ms: i64) -> bool {
        self.known.contains_key(span) && self.first_read.get(span).is_none_or(|at| *at <= until_ms)
    }

    fn publication_of(&self, span: &str) -> Option<&TimePoint> {
        self.known.get(span)?.publication.as_ref()
    }

    /// For each of `claims`, the first reading after which every later
    /// reading still shows the ending the last one shows.
    fn recorded_since(&self, claims: &BTreeSet<String>) -> BTreeMap<String, i64> {
        let moments: BTreeSet<i64> = self
            .known
            .keys()
            .filter_map(|span| self.first_read.get(span).copied())
            .collect();
        let last = self.at(i64::MAX);
        let shown_end = |shown: &BTreeMap<String, Shown>, id: &str| {
            shown
                .get(id)
                .and_then(|shown| shown.end.as_ref())
                .map(|end| (end.end.clone(), end.span.clone()))
        };
        let mut since = BTreeMap::new();
        for moment in moments {
            let shown = self.at(moment);
            for id in claims {
                let end = shown_end(&shown, id);
                if end.is_some() && end == shown_end(&last, id) {
                    since.entry(id.clone()).or_insert(moment);
                } else {
                    since.remove(id);
                }
            }
        }
        since
    }

    /// Every claim visible after the readings up to `until_ms`.
    pub(crate) fn at(&self, until_ms: i64) -> BTreeMap<String, Shown> {
        let known: BTreeMap<String, KnownWitness> = self
            .known
            .iter()
            .filter(|(span, _)| self.read_by(span, until_ms))
            .map(|(span, witness)| (span.clone(), witness.clone()))
            .collect();
        let spans: BTreeSet<String> = known.keys().cloned().collect();
        let visible: BTreeSet<String> = self
            .body
            .claims
            .iter()
            .filter(|claim| {
                claim
                    .evidence
                    .iter()
                    .any(|binding| spans.contains(&binding.span_id))
            })
            .map(|claim| claim.id.clone())
            .collect();
        let mut temporal = temporal_view(&self.body, &visible, &known);
        let mut shown = BTreeMap::new();
        for claim in self
            .body
            .claims
            .iter()
            .filter(|claim| visible.contains(&claim.id))
        {
            let mut claim = claim.clone();
            if let Some(temporal) = temporal.remove(&claim.id) {
                temporal.apply(&mut claim);
            }
            claim_as_of(&mut claim, &spans, &visible, self.cutoff);
            let end =
                claim
                    .invalid_at
                    .clone()
                    .map(|end| crate::knowledge::world_changes::ShownEnd {
                        end,
                        by: claim.qualifier(SUPERSEDED_BY_CLAIM).map(str::to_owned),
                        basis: claim.qualifier(END_BASIS).unwrap_or("stated").to_owned(),
                        span: claim.qualifier(SUPERSEDED_BY_SPAN).map(str::to_owned),
                    });
            let publication = claim
                .evidence
                .iter()
                .filter_map(|binding| known.get(&binding.span_id))
                .filter_map(|witness| witness.publication.clone())
                .min_by(|a, b| {
                    a.compare_at_common_precision(b)
                        .unwrap_or_else(|| a.value.cmp(&b.value))
                });
            shown.insert(
                claim.id.clone(),
                Shown {
                    end,
                    conflicts: claim
                        .qualifiers
                        .iter()
                        .filter(|qualifier| qualifier.key == CONFLICTS_WITH_CLAIM)
                        .map(|qualifier| qualifier.value.clone())
                        .collect(),
                    evidence: claim
                        .evidence
                        .iter()
                        .map(|binding| binding.span_id.clone())
                        .collect(),
                    publication,
                },
            );
        }
        shown
    }
}

/// Each episode as the World that read only what the cutoff knows wrote it:
/// `None` for a reading of a Source the cutoff does not know, otherwise the
/// claims it added, supported, ended, reopened and set in conflict, from the
/// replayed states before and after it (the same comparison a publication's
/// diff makes). Episodes come in reading order, with their shown dates.
pub(crate) fn episodes_as_of(
    write: &KnowledgeWrite<'_>,
    tx: &rusqlite::Transaction<'_>,
    loupist_id: &str,
    revision_id: &str,
    cutoff: Cutoff,
) -> crate::knowledge::KnowledgeResult<(Replay, crate::knowledge::world_changes::Replayed)> {
    let (replay, readings) = readings_as_of(write, tx, loupist_id, revision_id, cutoff)?;
    let mut replayed = crate::knowledge::world_changes::Replayed::default();
    // An ending a reading established before it took effect: dated when it does,
    // once the cutoff's projection still shows it at the end.
    let mut pending: BTreeMap<String, crate::knowledge::world_changes::ElapsedEnd> =
        BTreeMap::new();
    let last = replay.at(i64::MAX);
    for (episode, known, ends) in &readings {
        replayed.episodes.insert(
            episode.episode_id.clone(),
            known.then(|| episode.changes.clone()),
        );
        for (claim, end) in ends {
            pending.insert(
                claim.clone(),
                crate::knowledge::world_changes::ElapsedEnd {
                    episode: episode.clone(),
                    end: end.clone(),
                    effective_ms: effective_ms(&end.end).unwrap_or(episode.created_at_ms),
                },
            );
        }
    }
    for (claim, item) in pending {
        if last
            .get(&claim)
            .and_then(|shown| shown.end.as_ref())
            .is_some_and(|end| end.end == item.end.end && end.by == item.end.by)
        {
            replayed.elapsed.push(item);
        }
    }
    // A dispute or withdrawal whose witness the cutoff does not know is no row.
    let mut statement = tx.prepare(
        "SELECT event_id, at_ms, publication_json FROM knowledge_growth_events
         WHERE loupist_id = ?1 AND kind IN ('claim_disputed', 'claim_withdrawn')",
    )?;
    let transitions = statement
        .query_map([loupist_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    for (event_id, at_ms, publication) in transitions {
        if !transition_known(write, loupist_id, publication.as_deref(), at_ms, cutoff) {
            replayed.unknown_transitions.insert(event_id);
        }
    }
    let world_record_id = write.get_revision(loupist_id, revision_id)?.record_id;
    let removed = removed_readings(write, tx, loupist_id, &world_record_id, cutoff)?;
    replayed.lens = lens_replay(write, tx, loupist_id, cutoff, &replay, &readings, &removed)?;
    Ok((replay, replayed))
}

/// Lens section history under the cutoff (`None` when the head page rolled
/// back is it): the state each section was filed
/// with, then the transitions at each seal after the first reading the cutoff
/// does not know (none when it came after the end of the cutoff day), by the
/// rule the publication follow-ups apply: a section whose cited claim falls (disputed,
/// withdrawn or removed) becomes Contested (Contested wins); one whose cited claim newly ends, and
/// that cites neither its ender, its witness span nor anything published at or
/// after the witness, becomes Superseded.
#[allow(clippy::type_complexity)]
fn lens_replay(
    write: &KnowledgeWrite<'_>,
    tx: &rusqlite::Transaction<'_>,
    loupist_id: &str,
    cutoff: Cutoff,
    replay: &Replay,
    readings: &[(
        crate::knowledge::world_episodes::WorldEpisode,
        bool,
        Vec<(String, crate::knowledge::world_episodes::EpisodeEnd)>,
    )],
    removed: &[(i64, bool)],
) -> crate::knowledge::KnowledgeResult<Option<crate::knowledge::world_changes::LensReplay>> {
    use crate::knowledge::lens_page::SectionState;
    use crate::knowledge::world_changes::{LensReplay, LensTransition};
    let end = cutoff.end_ms();
    // Every reading's seal, known or not, with a removed Source's among them.
    let all: Vec<(i64, bool)> = readings
        .iter()
        .map(|(episode, known, _)| (episode.created_at_ms, *known))
        .chain(removed.iter().copied())
        .collect();
    // Until the first reading the cutoff does not know, the World then is the
    // World the cutoff knows, and the stored events are its Lens history.
    let from_ms = all
        .iter()
        .filter(|(at_ms, known)| !known && *at_ms <= end)
        .map(|(at_ms, _)| *at_ms)
        .min();
    let from_ms = match from_ms {
        Some(from_ms) => from_ms,
        None => {
            // A later restatement (master plan 9.10) can return a section to
            // Current, and that return is no dated event. When a reading by the
            // cutoff withdrew an ending, the page as it stood is the head page
            // rolled back (`read_lens_page_as_of` without a replay); otherwise
            // the dated events, after each section's filed state, are exact.
            let reopen_by_end = readings.iter().any(|(episode, _, _)| {
                episode.created_at_ms <= end && !episode.changes.reopened.is_empty()
            });
            if reopen_by_end {
                return Ok(None);
            }
            end.saturating_add(1)
        }
    };
    let sections = write.lens_sections_for_replay(loupist_id, end)?;
    // A filing runs only the supersede rule, on the World as it stood then, and
    // stores no event for it: the state each earlier section was filed with.
    let mut filed = Vec::new();
    for section in sections
        .iter()
        .filter(|section| section.filed_at_ms < from_ms)
    {
        let then = replay.at(section.filed_at_ms);
        let stale = stale_claims(section, &then, None, section.filed_at_ms, replay);
        if !stale.is_empty() {
            filed.push(LensTransition {
                record_id: section.record_id.clone(),
                title: section.title.clone(),
                key: section.key.clone(),
                at_ms: section.filed_at_ms,
                contested: false,
                claim_ids: stale,
                filed: true,
            });
        }
    }
    let stored = LensReplay {
        from_ms,
        transitions: filed,
    };
    let mut states = Vec::with_capacity(sections.len());
    for section in &sections {
        states.push(stored.state_at(tx, &section.record_id, &section.key, from_ms - 1)?);
    }
    // The follow-ups run at a seal: the known readings. An end that takes
    // effect between two seals is met at the next one, as the seal's own
    // reconciliation of effective ends meets it.
    let mut seals: BTreeSet<i64> = all
        .iter()
        .filter(|(at_ms, known)| *known && *at_ms >= from_ms && *at_ms <= end)
        .map(|(at_ms, _)| *at_ms)
        .collect();
    let mut moments = seals.clone();
    moments.extend(
        sections
            .iter()
            .map(|section| section.filed_at_ms)
            .filter(|at| *at >= from_ms && *at <= end),
    );
    // Claims that fell (disputed, withdrawn or removed) at each moment, by a
    // transition whose witness the cutoff knows: what contests a section.
    let mut fallen: BTreeMap<i64, BTreeSet<String>> = BTreeMap::new();
    {
        let mut statement = tx.prepare(
            "SELECT at_ms, subject_id, publication_json FROM knowledge_growth_events
             WHERE loupist_id = ?1 AND kind IN ('claim_disputed', 'claim_withdrawn')
               AND at_ms >= ?2 AND at_ms <= ?3 AND subject_id IS NOT NULL",
        )?;
        let rows = statement
            .query_map(rusqlite::params![loupist_id, from_ms, end], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        for (at_ms, claim, publication) in rows {
            if transition_known(write, loupist_id, publication.as_deref(), at_ms, cutoff) {
                fallen.entry(at_ms).or_default().insert(claim);
            }
        }
    }
    moments.extend(fallen.keys().copied());
    seals.extend(fallen.keys().copied());
    // When each claim first fell by a transition the cutoff knows: a claim no
    // longer active has no effective end to meet.
    let mut fell_at: BTreeMap<String, i64> = BTreeMap::new();
    {
        let mut statement = tx.prepare(
            "SELECT at_ms, subject_id, publication_json FROM knowledge_growth_events
             WHERE loupist_id = ?1 AND kind IN ('claim_disputed', 'claim_withdrawn')
               AND subject_id IS NOT NULL",
        )?;
        let rows = statement
            .query_map(rusqlite::params![loupist_id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        for (at_ms, claim, publication) in rows {
            if transition_known(write, loupist_id, publication.as_deref(), at_ms, cutoff) {
                let first = fell_at.entry(claim).or_insert(at_ms);
                *first = (*first).min(at_ms);
            }
        }
    }
    let mut transitions = stored.transitions;
    // Newly reached is since the last seal: before `from_ms`, the last known
    // reading the stored history ran.
    let mut last_seal = all
        .iter()
        .filter(|(at_ms, known)| *known && *at_ms < from_ms)
        .map(|(at_ms, _)| *at_ms)
        .max()
        .unwrap_or(-1);
    let mut before = replay.at(last_seal);
    for at_ms in moments {
        let after = replay.at(at_ms);
        let seal = seals.contains(&at_ms);
        for (section, state) in sections.iter().zip(states.iter_mut()) {
            if section.filed_at_ms > at_ms {
                continue;
            }
            let filed_now = section.filed_at_ms == at_ms;
            if !seal && !filed_now {
                continue;
            }
            // Ended by now and not at the last seal (a filing has no last seal).
            let stale = stale_claims(
                section,
                &after,
                (!filed_now).then_some((&before, last_seal)),
                at_ms,
                replay,
            );
            // A seal first meets the ends that took effect since the last one,
            // of claims still active (`reconcile_effective_ends`), then contests
            // for the claims that fell, then supersedes for the ends this
            // reading stated, unless contested. A filing runs only the last.
            let (elapsed, stated): (Vec<String>, Vec<String>) = stale.into_iter().partition(|id| {
                !filed_now && before.get(id).is_some_and(|shown| shown.end.is_some())
            });
            let elapsed: Vec<String> = elapsed
                .into_iter()
                .filter(|id| fell_at.get(id).is_none_or(|fell| *fell >= at_ms))
                .collect();
            let supersede = |state: &mut SectionState,
                             transitions: &mut Vec<LensTransition>,
                             claims: Vec<String>| {
                if matches!(state, SectionState::Contested { .. }) {
                    return;
                }
                let claims = newly_attached(state, false, claims);
                if claims.is_empty() {
                    return;
                }
                *state = crate::knowledge::world_changes::apply_section_transition(
                    std::mem::replace(state, SectionState::Current),
                    false,
                    at_ms,
                    claims.clone(),
                );
                transitions.push(LensTransition {
                    record_id: section.record_id.clone(),
                    title: section.title.clone(),
                    key: section.key.clone(),
                    at_ms,
                    contested: false,
                    claim_ids: claims,
                    filed: filed_now,
                });
            };
            supersede(state, &mut transitions, elapsed);
            // A cited claim that falls contests the section.
            let conflicted: Vec<String> = section
                .claims
                .iter()
                .filter(|id| {
                    !filed_now
                        && fallen
                            .get(&at_ms)
                            .is_some_and(|claims| claims.contains(*id))
                })
                .cloned()
                .collect();
            // A transition names only the claims it newly attaches, so a later
            // claim in the section does not repeat an earlier claim's alert.
            let conflicted = newly_attached(state, true, conflicted);
            if !conflicted.is_empty() {
                *state = crate::knowledge::world_changes::apply_section_transition(
                    std::mem::replace(state, SectionState::Current),
                    true,
                    at_ms,
                    conflicted.clone(),
                );
                transitions.push(LensTransition {
                    record_id: section.record_id.clone(),
                    title: section.title.clone(),
                    key: section.key.clone(),
                    at_ms,
                    contested: true,
                    claim_ids: conflicted,
                    filed: false,
                });
            }
            supersede(state, &mut transitions, stated);
        }
        if seal {
            before = after;
            last_seal = at_ms;
        }
    }
    Ok(Some(LensReplay {
        from_ms,
        transitions,
    }))
}

/// Of `claims`, those a Contested (or Superseded) transition would newly
/// attach to a section in `state`.
fn newly_attached(
    state: &crate::knowledge::lens_page::SectionState,
    contested: bool,
    claims: Vec<String>,
) -> Vec<String> {
    use crate::knowledge::lens_page::SectionState;
    let attached: &[String] = match (contested, state) {
        (true, SectionState::Contested { claim_ids, .. })
        | (false, SectionState::Superseded { claim_ids, .. }) => claim_ids,
        _ => &[],
    };
    claims
        .into_iter()
        .filter(|id| !attached.contains(id))
        .collect()
}

/// The claims a section cites that have ended by `at_ms` in `after`, newly
/// since `before` (the World at the last seal, and when; a filing has none),
/// and that the section does not answer: it cites neither the ender, the
/// witness span nor anything published after the witness.
fn stale_claims(
    section: &crate::knowledge::world_changes::ReplaySection,
    after: &BTreeMap<String, Shown>,
    before: Option<(&BTreeMap<String, Shown>, i64)>,
    at_ms: i64,
    replay: &Replay,
) -> Vec<String> {
    let ended = |shown: Option<&Shown>, at_ms: i64| {
        let at = crate::knowledge::TimePoint::from_unix_ms(at_ms);
        shown
            .and_then(|shown| shown.end.as_ref())
            .filter(|end| end.end.at_or_before(&at))
            .cloned()
    };
    section
        .claims
        .iter()
        .filter(|id| {
            let Some(end) = ended(after.get(*id), at_ms) else {
                return false;
            };
            if before.is_some_and(|(before, then)| ended(before.get(*id), then).is_some()) {
                return false;
            }
            let witnessed = end
                .span
                .as_deref()
                .and_then(|span| replay.publication_of(span));
            let answered = end
                .by
                .as_ref()
                .is_some_and(|by| section.claims.contains(by))
                || end
                    .span
                    .as_ref()
                    .is_some_and(|span| section.spans.iter().any(|(cited, _)| cited == span))
                || witnessed.is_some_and(|witnessed| {
                    section
                        .claims
                        .iter()
                        .filter(|other| *other != *id)
                        .filter_map(|other| {
                            after
                                .get(other)
                                .and_then(|shown| shown.publication.as_ref())
                        })
                        .chain(
                            section
                                .spans
                                .iter()
                                .filter_map(|(_, published)| published.as_ref()),
                        )
                        .any(|published| {
                            witnessed.compare_at_common_precision(published)
                                == Some(std::cmp::Ordering::Less)
                        })
                });
            !answered
        })
        .cloned()
        .collect()
}

/// The first moment an end counts as reached on the user's calendar: an
/// instant at itself, a date at the start of that day, a month or year at its start.
fn effective_ms(end: &crate::knowledge::TimePoint) -> Option<i64> {
    let value = end.value.as_deref()?;
    if end.precision == TimePrecision::Instant {
        return chrono::DateTime::parse_from_rfc3339(value)
            .ok()
            .map(|at| at.timestamp_millis());
    }
    let day = match end.precision {
        TimePrecision::Month => {
            NaiveDate::parse_from_str(&format!("{}-01", value.get(..7)?), "%Y-%m-%d").ok()?
        }
        TimePrecision::Year => NaiveDate::from_ymd_opt(value.get(..4)?.parse().ok()?, 1, 1)?,
        _ => NaiveDate::parse_from_str(value.get(..10)?, "%Y-%m-%d").ok()?,
    };
    let offset = *crate::local_time::now().offset();
    use chrono::TimeZone;
    offset
        .from_local_datetime(&day.and_hms_opt(0, 0, 0)?)
        .single()
        .map(|at| at.timestamp_millis())
}

/// The readings in order, each with whether the cutoff knows its Source, its
/// replayed changes, and the ends it established that took effect later.
#[allow(clippy::type_complexity)]
fn readings_as_of(
    write: &KnowledgeWrite<'_>,
    tx: &rusqlite::Transaction<'_>,
    loupist_id: &str,
    revision_id: &str,
    cutoff: Cutoff,
) -> crate::knowledge::KnowledgeResult<(
    Replay,
    Vec<(
        crate::knowledge::world_episodes::WorldEpisode,
        bool,
        Vec<(String, crate::knowledge::world_episodes::EpisodeEnd)>,
    )>,
)> {
    use crate::knowledge::world_episodes::{EndBasis, EpisodeChanges, EpisodeConflict, EpisodeEnd};
    let replay = Replay::load(write, loupist_id, revision_id, cutoff)?;
    let world_record_id = write.get_revision(loupist_id, revision_id)?.record_id;
    let mut known_sources: BTreeMap<(String, String), bool> = BTreeMap::new();
    let mut before: BTreeMap<String, Shown> = BTreeMap::new();
    let mut out = Vec::new();
    let mut unknown_read = false;
    let removed_unknown = removed_readings(write, tx, loupist_id, &world_record_id, cutoff)?
        .into_iter()
        .filter(|(_, known)| !known)
        .map(|(at_ms, _)| at_ms)
        .min();
    for mut episode in crate::knowledge::world_episodes::recorded_window(
        tx,
        loupist_id,
        &world_record_id,
        -1,
        i64::MAX,
    )? {
        // A reading is known when its Source was known by the cutoff.
        let key = (episode.source_record_id.clone(), episode.span_id.clone());
        let known = match known_sources.get(&key) {
            Some(known) => *known,
            None => {
                let shown = source_captures_known(
                    write,
                    loupist_id,
                    &episode.source_record_id,
                    None,
                    Some(&episode.span_id),
                    cutoff,
                )
                .unwrap_or(false);
                known_sources.insert(key, shown);
                shown
            }
        };
        if !known {
            unknown_read = true;
            out.push((episode, false, Vec::new()));
            continue;
        }
        if removed_unknown.is_some_and(|at_ms| at_ms < episode.created_at_ms) {
            unknown_read = true;
        }
        let after = replay.at(episode.created_at_ms);
        if !unknown_read {
            // Nothing the cutoff does not know was read before this: the
            // World then is the full World then, and its diff is the stored one.
            let at = crate::knowledge::TimePoint::from_unix_ms(episode.created_at_ms);
            let later = pending_ends(&before, &after, &at);
            before = after;
            out.push((episode, true, later));
            continue;
        }
        let at = crate::knowledge::TimePoint::from_unix_ms(episode.created_at_ms);
        let ended_by = |shown: Option<&Shown>| {
            shown
                .and_then(|shown| shown.end.as_ref())
                .filter(|end| end.end.at_or_before(&at))
                .cloned()
        };
        let mut changes = EpisodeChanges::default();
        let mut later = Vec::new();
        for (id, shown) in &after {
            let earlier = before.get(id);
            if shown.evidence.contains(&episode.span_id) {
                match earlier {
                    None => changes.created.push(id.clone()),
                    Some(earlier) if !earlier.evidence.contains(&episode.span_id) => {
                        changes.supported.push(id.clone())
                    }
                    Some(_) => {}
                }
            }
            if let Some(end) = ended_by(Some(shown)) {
                if ended_by(earlier).is_none() {
                    changes.ended.push(EpisodeEnd {
                        claim: id.clone(),
                        by: end.by.clone(),
                        end: end.end.clone(),
                        basis: if end.basis == "reference" {
                            EndBasis::Reference
                        } else {
                            EndBasis::Stated
                        },
                    });
                }
            } else if ended_by(earlier).is_some() {
                changes.reopened.push(id.clone());
            }

            for partner in &shown.conflicts {
                if earlier.is_none_or(|earlier| !earlier.conflicts.contains(partner)) {
                    changes.conflicts.push(EpisodeConflict {
                        claim: id.clone(),
                        with: partner.clone(),
                    });
                }
            }
        }
        later.extend(pending_ends(&before, &after, &at));
        episode.changes = changes;
        before = after;
        out.push((episode, true, later));
    }
    Ok((replay, out))
}

/// Every reading of a Source a Memory reset removed, with whether the cutoff
/// knows that Source. Episode reads leave them out, yet the stored diffs after
/// such a reading are those of a World that read it, and its seal ran the
/// follow-ups.
fn removed_readings(
    write: &KnowledgeWrite<'_>,
    tx: &rusqlite::Transaction<'_>,
    loupist_id: &str,
    world_record_id: &str,
    cutoff: Cutoff,
) -> crate::knowledge::KnowledgeResult<Vec<(i64, bool)>> {
    let controls = crate::knowledge::memory_controls::load(tx, loupist_id)?;
    let mut statement = tx.prepare(
        "SELECT created_at_ms FROM knowledge_world_episodes
         WHERE loupist_id = ?1 AND world_record_id = ?2 AND backfilled = 0
           AND source_record_id = ?3",
    )?;
    let mut out = Vec::new();
    for record_id in &controls.removed {
        let times = statement
            .query_map(
                rusqlite::params![loupist_id, world_record_id, record_id],
                |row| row.get::<_, i64>(0),
            )?
            .collect::<Result<Vec<_>, _>>()?;
        if times.is_empty() {
            continue;
        }
        let known = source_captures_known(write, loupist_id, record_id, None, None, cutoff)
            .unwrap_or(false);
        out.extend(times.into_iter().map(|at_ms| (at_ms, known)));
    }
    Ok(out)
}

/// Whether the cutoff knows a dispute or withdrawal: by its witness Source,
/// as every reader decides a Source, or by the day it was recorded when no
/// Source wrote it. `publication` is the growth event's.
pub(crate) fn transition_known(
    write: &KnowledgeWrite<'_>,
    loupist_id: &str,
    publication: Option<&str>,
    at_ms: i64,
    cutoff: Cutoff,
) -> bool {
    let witness = publication
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        .and_then(|value| {
            value
                .pointer("/source/recordId")
                .and_then(Value::as_str)
                .map(str::to_owned)
        });
    match witness {
        Some(record) => {
            source_captures_known(write, loupist_id, &record, None, None, cutoff).unwrap_or(false)
        }
        None => cutoff.recorded(at_ms),
    }
}

/// Ends a reading established that take effect only after it.
fn pending_ends(
    before: &BTreeMap<String, Shown>,
    after: &BTreeMap<String, Shown>,
    at: &crate::knowledge::TimePoint,
) -> Vec<(String, crate::knowledge::world_episodes::EpisodeEnd)> {
    use crate::knowledge::world_episodes::{EndBasis, EpisodeEnd};
    let mut later = Vec::new();
    for (id, shown) in after {
        let Some(end) = shown.end.as_ref().filter(|end| !end.end.at_or_before(at)) else {
            continue;
        };
        let new = before
            .get(id)
            .and_then(|earlier| earlier.end.as_ref())
            .is_none_or(|old| old.end != end.end || old.by != end.by);
        if new {
            later.push((
                id.clone(),
                EpisodeEnd {
                    claim: id.clone(),
                    by: end.by.clone(),
                    end: end.end.clone(),
                    basis: if end.basis == "reference" {
                        EndBasis::Reference
                    } else {
                        EndBasis::Stated
                    },
                },
            ));
        }
    }
    later
}

/// Lineage under a cutoff: the readings the cutoff knows, in order, and after
/// each reading that can move the claim's ending, the same projection the World
/// read uses, limited to what had been read by then. That is the history of a
/// World that read only those Sources.
pub(crate) fn lineage_as_of(
    write: &KnowledgeWrite<'_>,
    tx: &rusqlite::Transaction<'_>,
    loupist_id: &str,
    revision_id: &str,
    claim_id: &str,
    cutoff: Cutoff,
) -> crate::knowledge::KnowledgeResult<Option<Value>> {
    let (replay, episodes) = readings_as_of(write, tx, loupist_id, revision_id, cutoff)?;
    if !replay.at(i64::MAX).contains_key(claim_id) {
        return Ok(None);
    }
    let mut shown_episodes = Vec::new();
    for (mut episode, known, _) in episodes {
        if !known {
            continue;
        }
        // The date the line returns is the capture's current publication
        // (a paste correction can move May back to January). The episode's
        // stored date is not that date, and a later stored date must not
        // enter an earlier history.
        let Ok(source) = write.owned_record(loupist_id, &episode.source_record_id) else {
            continue;
        };
        let Some(source_revision_id) = source.revision_id else {
            continue;
        };
        let shown = crate::knowledge::world_changes::span_publication(
            tx,
            &source_revision_id,
            &episode.span_id,
        )?;
        let returned = shown.or(episode.publication_time.clone());
        if returned
            .as_ref()
            .is_some_and(|time| time.value.is_some() && !cutoff.published(Some(time)))
        {
            continue;
        }
        episode.source_revision_id = source_revision_id;
        episode.publication_time = returned;
        shown_episodes.push(episode);
    }
    // Every known reading can move an ending: pass them all.
    let moves: BTreeSet<String> = shown_episodes
        .iter()
        .map(|episode| episode.episode_id.clone())
        .collect();
    let lineage = crate::knowledge::world_changes::claim_lineage_replayed(
        tx,
        claim_id,
        &shown_episodes,
        &moves,
        |until_ms| {
            Ok(replay
                .at(until_ms)
                .into_iter()
                .filter_map(|(id, shown)| shown.end.map(|end| (id, end)))
                .collect())
        },
    )?;
    Ok(Some(lineage))
}

#[derive(Default)]
struct TemporalView {
    ending: Option<Ending>,
    conflicts: BTreeSet<String>,
    reopened: Option<Reopened>,
    lapsed: Vec<Lapse>,
}

impl TemporalView {
    fn apply(self, claim: &mut GraphClaim) {
        let unchanged_end = self.ending.as_ref().is_some_and(|ending| {
            claim.invalid_at.as_ref() == Some(&ending.time)
                && claim.qualifier(SUPERSEDED_BY_SPAN) == Some(ending.span.as_str())
        });
        if !unchanged_end {
            claim.expired_at = None;
        }
        claim
            .qualifiers
            .retain(|item| !is_temporal_qualifier(&item.key));
        let mut qualifiers = Vec::new();
        let push = |items: &mut Vec<Qualifier>, key: &str, value: String| {
            items.push(Qualifier {
                key: key.into(),
                value,
            });
        };
        claim.invalid_at = self.ending.map(|ending| {
            let basis = match ending.basis {
                crate::knowledge::world_episodes::EndBasis::Reference => "reference",
                crate::knowledge::world_episodes::EndBasis::Stated => "stated",
            };
            push(&mut qualifiers, END_BASIS, basis.into());
            push(&mut qualifiers, SUPERSEDED_BY_SPAN, ending.span);
            if let Some(by) = ending.by {
                push(&mut qualifiers, SUPERSEDED_BY_CLAIM, by);
            }
            ending.time
        });
        for partner in self.conflicts.into_iter().take(4) {
            push(&mut qualifiers, CONFLICTS_WITH_CLAIM, partner);
        }
        for (key, value) in reopening_qualifiers(self.reopened.as_ref(), &self.lapsed) {
            push(&mut qualifiers, key, value);
        }
        claim.qualifiers.truncate(16 - qualifiers.len());
        claim.qualifiers.extend(qualifiers);
    }
}

/// Re-evaluate retained resolver selections among the assertions known that
/// day: each claim's assertions are only the spans in `spans`, its publication
/// days only theirs, and a pair's witnesses only those `spans` holds, so the
/// pair rule sees what a World that read only those Sources would store. A
/// later filing may have displaced an ending that was visible at the cutoff.
fn temporal_view(
    body: &WorldSnapshotBody,
    visible: &BTreeSet<String>,
    spans: &BTreeMap<String, KnownWitness>,
) -> BTreeMap<String, TemporalView> {
    let mut effects = PairEffects::<String>::default();
    let mut viewed = BTreeSet::<String>::new();
    let assertions: BTreeMap<_, _> = body
        .claims
        .iter()
        .filter(|claim| visible.contains(&claim.id))
        .filter_map(|claim| {
            let stored = body.temporal.assertions.get(&claim.id)?;
            let [left, right] = claim.endpoints.as_slice() else {
                return None;
            };
            let mut sources = AssertionSources {
                stated: stored
                    .stated
                    .clone()
                    .filter(|stated| spans.contains_key(&stated.witness.span)),
                ..Default::default()
            };
            for span in claim
                .evidence
                .iter()
                .map(|item| item.span_id.as_str())
                .chain(stored.witness.iter().map(|witness| witness.span.as_str()))
            {
                if let Some(witness) = spans.get(span) {
                    sources.source_ids.insert(witness.source_record.clone());
                    // An older claim stored no days: its witness's day alone.
                    if let Some(day) = witness
                        .publication
                        .as_ref()
                        .filter(|_| !stored.days.is_empty())
                    {
                        sources.add_day(day);
                    }
                    let candidate = AssertionWitness {
                        span: span.into(),
                        publication: witness.publication.clone(),
                    };
                    if sources
                        .witness
                        .as_ref()
                        .is_none_or(|old| witness_order(&candidate, old).is_lt())
                    {
                        sources.witness = Some(candidate);
                    }
                }
            }
            // Evidence keeps the earliest and the latest spans (32 in all): a
            // stored day between the days still read is one of the dropped.
            if claim.evidence.len() >= 32 {
                if let Some(last) = sources.days.last().cloned() {
                    for day in &stored.days {
                        if day_key(day) <= day_key(&last) {
                            sources.add_day(day);
                        }
                    }
                }
            }
            if stored.stated.is_some() {
                viewed.insert(claim.id.clone());
                if let Some(stated) = &sources.stated {
                    effects.consider(&claim.id, Ending::stated(stated));
                }
            }
            Some((
                claim.id.as_str(),
                (claim, [&left.node_id[..], &right.node_id[..]], sources),
            ))
        })
        .collect();
    for link in &body.temporal.links {
        for id in [&link.left, &link.right] {
            if assertions.contains_key(id.as_str()) {
                viewed.insert(id.clone());
            }
        }
        let known = |witness: &Option<AssertionWitness>| {
            witness
                .as_ref()
                .is_some_and(|witness| spans.contains_key(&witness.span))
        };
        if !known(&link.selected_by) {
            continue;
        }
        let Some((left, left_endpoints, left_sources)) = assertions.get(link.left.as_str()) else {
            continue;
        };
        let Some((right, right_endpoints, right_sources)) = assertions.get(link.right.as_str())
        else {
            continue;
        };
        fn assertion<'a>(
            claim: &'a GraphClaim,
            endpoints: [&'a str; 2],
            assertions: &'a AssertionSources,
        ) -> Assertion<'a> {
            Assertion {
                relation: &claim.relation,
                endpoints,
                valid_at: claim.valid_at.as_ref(),
                assertions,
            }
        }
        let reaffirmed_by: Vec<_> = link
            .reaffirmed_by
            .iter()
            .filter(|witness| spans.contains_key(&witness.span))
            .cloned()
            .collect();
        let outcome = decide_pair(
            assertion(left, *left_endpoints, left_sources),
            assertion(right, *right_endpoints, right_sources),
            PairSelection {
                same_assertion: link.same_assertion && known(&link.same_assertion_by),
                reaffirmed_by: &reaffirmed_by,
            },
        );
        effects.record(
            (&link.left, &link.left, left_sources),
            (&link.right, &link.right, right_sources),
            outcome,
        );
    }
    let endings = std::mem::take(&mut effects.endings);
    effects.drop_witnessed_conflicts(
        |id| endings.get(id),
        |id| assertions.get(id.as_str()).map(|(_, _, sources)| sources),
    );
    viewed
        .into_iter()
        .map(|id| {
            let (reopened, lapsed) = effects.take_reopening(&id);
            let view = TemporalView {
                ending: endings.get(&id).cloned(),
                conflicts: effects.conflicts.remove(&id).unwrap_or_default(),
                reopened,
                lapsed,
            };
            (id, view)
        })
        .collect()
}

fn world_known(
    write: &KnowledgeWrite<'_>,
    loupist_id: &str,
    revision_id: &str,
    cutoff: Cutoff,
) -> crate::knowledge::KnowledgeResult<WorldKnown> {
    let revision = write.get_revision(loupist_id, revision_id)?;
    let body: WorldSnapshotBody = serde_json::from_value(revision.payload)?;
    let wanted = body
        .claims
        .iter()
        .flat_map(|claim| {
            claim
                .evidence
                .iter()
                .map(|e| e.span_id.as_str())
                .chain(claim.qualifier(SUPERSEDED_BY_SPAN))
        })
        .chain(body.temporal.witness_spans())
        .map(str::to_owned)
        .chain(
            body.annotations
                .iter()
                .flat_map(|annotation| annotation.source_span_ids.iter().cloned()),
        )
        .collect();
    let known = known_spans(
        write,
        loupist_id,
        &revision.record_id,
        revision_id,
        wanted,
        cutoff,
    )?;
    let spans: BTreeSet<_> = known.keys().cloned().collect();
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
    let all_claims = body.claims.iter().map(|claim| claim.id.clone()).collect();
    let visible_claims: BTreeSet<_> = body
        .claims
        .iter()
        .filter(|claim| {
            claim
                .evidence
                .iter()
                .any(|binding| spans.contains(&binding.span_id))
        })
        .map(|claim| claim.id.clone())
        .collect();
    let mut temporal = temporal_view(&body, &visible_claims, &known);
    let first_read = write.first_episode_ms_by_span(loupist_id, &revision.record_id)?;
    let replay_body = body.clone();
    let mut moved = BTreeSet::new();
    let claims: BTreeMap<_, _> = body
        .claims
        .into_iter()
        .filter(|claim| visible_claims.contains(&claim.id))
        .map(|mut claim| {
            if let Some(temporal) = temporal.remove(&claim.id) {
                let stored = (claim.invalid_at.clone(), claim.expired_at.clone());
                temporal.apply(&mut claim);
                if claim.invalid_at.is_some()
                    && claim.expired_at.is_none()
                    && stored.0 != claim.invalid_at
                {
                    moved.insert(claim.id.clone());
                }
            }
            claim_as_of(&mut claim, &spans, &visible_claims, cutoff);
            // Who asserted it and when this Loupist first read it come from
            // the evidence known then, not from a Source read later or one a
            // Memory reset removed (whose label the stored claim may keep).
            if let Some(first) = claim.evidence.first() {
                claim.attribution = first.attribution.clone();
            }
            if let Some(read) = claim
                .evidence
                .iter()
                .filter_map(|binding| first_read.get(&binding.span_id))
                .min()
            {
                claim.recorded_at_ms = Some(*read);
            }
            // Its assertion time is the earliest publication among the known
            // Sources that assert it (master plan D2-1a), none when all are undated.
            claim.publication_time = claim
                .evidence
                .iter()
                .filter_map(|binding| known.get(&binding.span_id))
                .filter_map(|witness| witness.publication.clone())
                .min_by(|a, b| {
                    a.compare_at_common_precision(b)
                        .unwrap_or_else(|| a.value.cmp(&b.value))
                });
            (claim.id.clone(), claim)
        })
        .collect();
    let mut claims = claims;
    // An ending the stored World later moved or withdrew (a restatement,
    // master plan 9.10) was recorded when the readings known by the cutoff
    // first showed it, as the World that read only them recorded it.
    moved.retain(|id| {
        claims
            .get(id)
            .is_some_and(|claim| claim.invalid_at.is_some())
    });
    if !moved.is_empty() {
        let replay = Replay {
            body: replay_body,
            known: known.clone(),
            first_read: first_read.clone(),
            cutoff,
        };
        for (id, at_ms) in replay.recorded_since(&moved) {
            if let Some(claim) = claims.get_mut(&id) {
                let at = TimePoint::from_unix_ms(at_ms);
                if cutoff.reached(&at) {
                    // In the form the seal stores an instant.
                    claim.expired_at =
                        Some(crate::knowledge::validate_time_point(&at).unwrap_or(at));
                }
            }
        }
    }
    for claim in claims.values_mut() {
        // A later withdrawal or dispute is not the status of an earlier row.
        // End flags are derived from this status, so it has to be projected first.
        if let Some(status) =
            write.status_known_by(loupist_id, &claim.id, &mut |publication, at_ms| {
                transition_known(write, loupist_id, publication, at_ms, cutoff)
            })?
        {
            claim.status = status;
        }
    }
    let nodes = claims
        .values()
        .flat_map(|claim| claim.endpoints.iter().map(|end| end.node_id.clone()))
        .collect();
    Ok(WorldKnown {
        claims,
        nodes,
        all_claims,
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
fn graph_view_as_of(view: &mut Value, known: &WorldKnown) -> (bool, Vec<String>) {
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
        let Some(projected) = known.claims.get(&id) else {
            removed.push(id);
            return false;
        };
        let Some(claim) = edge.as_object_mut() else {
            return false;
        };
        project_end_mark(claim, projected);
        claim.insert(
            "status".into(),
            serde_json::json!(projected.status.as_str()),
        );
        if let Some(nested) = claim.get_mut("claim") {
            *nested = serde_json::json!(projected);
        }
        for (key, time) in [
            ("invalid_at", &projected.invalid_at),
            ("invalidAt", &projected.invalid_at),
            ("expired_at", &projected.expired_at),
            ("expiredAt", &projected.expired_at),
        ] {
            if let Some(field) = claim.get_mut(key) {
                *field = serde_json::json!(time);
            }
        }
        match &projected.invalid_at {
            Some(end) => {
                if let Some(approximate) = claim.get_mut("approximate") {
                    *approximate = Value::Bool(end.approximate);
                }
            }
            None => {
                claim.remove("approximate");
            }
        }
        if let Some(qualifiers) = claim.get_mut("qualifiers") {
            *qualifiers = serde_json::json!(projected.qualifiers);
        }
        if let Some(evidence) = claim.get_mut("evidence").and_then(Value::as_array_mut) {
            evidence.retain(|binding| {
                binding
                    .get("span_id")
                    .or_else(|| binding.get("spanId"))
                    .and_then(Value::as_str)
                    .is_some_and(|span| {
                        projected.evidence.iter().any(|known| known.span_id == span)
                    })
            });
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

fn project_end_mark(object: &mut serde_json::Map<String, Value>, claim: &GraphClaim) {
    let (ended, end) = projected_end_fields(claim);
    if ended.is_some() {
        object.insert("ended".into(), Value::Bool(true));
        object.insert("end".into(), serde_json::json!(end));
    } else {
        object.remove("ended");
        object.remove("end");
    }
}

/// Removes search hits the cutoff leaves out and records each by kind.
/// Source hits need a span known by then; World hits a relationship published
/// by then (a name needs one such relationship, an annotation every Source it
/// cites, a hit without an id is left out); a graph view keeps only such
/// relationships; Lens hits a current version written by then; saved notes
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
                            let known = lens_visible(&write, loupist_id, &record_id, cutoff)?;
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
                            let (any, later) = graph_view_as_of(view, known);
                            removed.facts.extend(later);
                            any
                        } else if object_id.is_empty() {
                            false
                        } else if known.all_claims.contains(&object_id) {
                            if let Some(projected) = known.claims.get(&object_id) {
                                if let Some(hit) = hit.as_object_mut() {
                                    project_end_mark(hit, projected);
                                }
                                true
                            } else {
                                removed.facts.insert(object_id.clone());
                                false
                            }
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
    use crate::knowledge::Qualifier;

    fn time(value: &str) -> TimePoint {
        TimePoint {
            precision: TimePrecision::Date,
            value: Some(value.into()),
            approximate: false,
        }
    }

    #[test]
    fn a_known_end_needs_both_its_witness_and_its_effective_date() {
        for notice in ["2026-03-10", "2026-05-01"] {
            for end in ["2026-03-15", "2026-06-30"] {
                for expiry in [None, Some("2026-03-15"), Some("2026-06-30")] {
                    for tagged in [false, true] {
                        for partner_known in [false, true] {
                            check_cutoffs(notice, end, expiry, tagged, partner_known);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_later_stated_end_preserves_the_earlier_reference_end_at_a_cutoff() {
        use crate::knowledge::world_episodes::EndBasis;
        use crate::knowledge::{AssertionLink, AssertionWitness, WorldTemporal};
        let claim = |id: &str, start: Option<&str>| -> GraphClaim {
            serde_json::from_value(serde_json::json!({
                "id": id, "proposition": id, "attribution":"publisher", "relation":"LEADS",
                "endpoints":[{"nodeId":id,"role":"source"},{"nodeId":"company","role":"target"}],
                "qualifiers":[], "status":"active", "validAt":start.map(time),
                "evidence":[{"spanId":id,"role":"extracted_from","attribution":"publisher"}],
            }))
            .unwrap()
        };
        let mut sources = BTreeMap::new();
        let mut assertions = BTreeMap::new();
        for (id, published) in [
            ("a", "2026-01-01"),
            ("b", "2026-02-01"),
            ("c", "2026-03-01"),
        ] {
            sources.insert(
                id.to_owned(),
                KnownWitness {
                    source_record: id.to_owned(),
                    publication: Some(time(published)),
                },
            );
            assertions.insert(
                id.to_owned(),
                AssertionSources {
                    source_ids: BTreeSet::from([id.to_owned()]),
                    witness: Some(AssertionWitness {
                        span: id.into(),
                        publication: Some(time(published)),
                    }),
                    stated: None,
                    ..Default::default()
                },
            );
        }
        let body = WorldSnapshotBody {
            nodes: vec![],
            claims: vec![
                claim("a", Some("2026-01-01")),
                claim("b", None),
                claim("c", Some("2026-01-15")),
            ],
            annotations: vec![],
            relation_vocabulary: vec![],
            temporal: WorldTemporal {
                links: vec![
                    AssertionLink {
                        left: "a".into(),
                        right: "b".into(),
                        same_assertion: true,
                        selected_by: assertions["b"].witness.clone(),
                        same_assertion_by: assertions["b"].witness.clone(),
                        reaffirmed_by: vec![],
                    },
                    AssertionLink {
                        left: "a".into(),
                        right: "c".into(),
                        same_assertion: true,
                        selected_by: assertions["c"].witness.clone(),
                        same_assertion_by: assertions["c"].witness.clone(),
                        reaffirmed_by: vec![],
                    },
                ],
                assertions,
            },
        };
        for (cutoff, date, by, basis) in [
            ("2026-01-20", None, None, None),
            (
                "2026-02-20",
                Some("2026-02-01"),
                Some("b"),
                Some(EndBasis::Reference),
            ),
            (
                "2026-03-20",
                Some("2026-01-15"),
                Some("c"),
                Some(EndBasis::Stated),
            ),
        ] {
            let at = Cutoff::parse(Some(cutoff)).unwrap();
            let known: BTreeMap<_, _> = sources
                .iter()
                .filter(|(_, witness)| at.published(witness.publication.as_ref()))
                .map(|(id, witness)| (id.clone(), witness.clone()))
                .collect();
            let visible = known.keys().cloned().collect();
            let views = temporal_view(&body, &visible, &known);
            let end = views.get("a").and_then(|view| view.ending.as_ref());
            assert_eq!(end.and_then(|end| end.time.value.as_deref()), date);
            assert_eq!(end.and_then(|end| end.by.as_deref()), by);
            assert_eq!(end.map(|end| end.basis), basis);
        }

        // In both cases a later restatement is the evidence for the resolver
        // decision. Both endpoint claims already had older, visible evidence.
        let mut body = body;
        let late = body.temporal.assertions["c"].witness.clone();
        body.temporal.links.truncate(1);
        body.temporal.links[0].same_assertion_by = late.clone();
        let visible = BTreeSet::from(["a".into(), "b".into()]);
        let mut known = sources.clone();
        known.remove("c");
        let upgraded = temporal_view(&body, &visible, &known);
        assert!(upgraded["a"].ending.is_none());
        assert!(upgraded["a"].conflicts.contains("b"));
        body.temporal.links[0].selected_by = late;
        let selected = temporal_view(&body, &visible, &known);
        assert!(selected["a"].ending.is_none());
        assert!(selected["a"].conflicts.is_empty());
        let later = temporal_view(&body, &visible, &sources);
        assert_eq!(later["a"].ending.as_ref().unwrap().by.as_deref(), Some("b"));
    }

    /// T-RESTATE under a cutoff (master plan 9.10): X in January, Y in
    /// February, X restated in March by a Source that marked the pair again.
    /// February knows X ended by Y; April knows Y ended in March and X current
    /// again, with the gap; without the March witness April knows a conflict.
    #[test]
    fn a_restated_claim_under_a_cutoff_uses_the_days_and_witnesses_known_then() {
        use crate::knowledge::world_episodes::EndBasis;
        use crate::knowledge::{AssertionLink, AssertionWitness, WorldTemporal, REOPENED_AT};
        let claim = |id: &str, spans: &[&str]| -> GraphClaim {
            let evidence: Vec<_> = spans
                .iter()
                .map(|span| serde_json::json!({"spanId":span,"role":"extracted_from","attribution":"publisher"}))
                .collect();
            serde_json::from_value(serde_json::json!({
                "id": id, "proposition": id, "attribution":"publisher", "relation":"CFO_OF",
                "endpoints":[{"nodeId":format!("person-{id}"),"role":"source"},{"nodeId":"company","role":"target"}],
                "qualifiers":[], "status":"active", "evidence": evidence,
            }))
            .unwrap()
        };
        let witness = |span: &str, day: &str| AssertionWitness {
            span: span.into(),
            publication: Some(time(day)),
        };
        let spans: BTreeMap<String, KnownWitness> = [
            ("spn_x_jan", "2026-01-12"),
            ("spn_y_feb", "2026-02-09"),
            ("spn_x_mar", "2026-03-16"),
        ]
        .into_iter()
        .map(|(span, day)| {
            (
                span.to_owned(),
                KnownWitness {
                    source_record: format!("rec_{span}"),
                    publication: Some(time(day)),
                },
            )
        })
        .collect();
        let sources = |spans: &[&str], days: &[&str]| AssertionSources {
            source_ids: spans.iter().map(|span| format!("rec_{span}")).collect(),
            witness: Some(witness(spans[0], days[0])),
            days: days.iter().map(|day| time(day)).collect(),
            ..Default::default()
        };
        let with_witness = |reaffirmed: Vec<AssertionWitness>| WorldSnapshotBody {
            nodes: vec![],
            claims: vec![
                claim("x", &["spn_x_jan", "spn_x_mar"]),
                claim("y", &["spn_y_feb"]),
            ],
            annotations: vec![],
            relation_vocabulary: vec![],
            temporal: WorldTemporal {
                links: vec![AssertionLink {
                    left: "x".into(),
                    right: "y".into(),
                    same_assertion: true,
                    selected_by: Some(witness("spn_y_feb", "2026-02-09")),
                    same_assertion_by: Some(witness("spn_y_feb", "2026-02-09")),
                    reaffirmed_by: reaffirmed,
                }],
                assertions: BTreeMap::from([
                    (
                        "x".to_owned(),
                        sources(&["spn_x_jan", "spn_x_mar"], &["2026-01-12", "2026-03-16"]),
                    ),
                    ("y".to_owned(), sources(&["spn_y_feb"], &["2026-02-09"])),
                ]),
            },
        };
        let view = |body: &WorldSnapshotBody, cutoff: &str| {
            let at = Cutoff::parse(Some(cutoff)).unwrap();
            let known: BTreeMap<_, _> = spans
                .iter()
                .filter(|(_, witness)| at.published(witness.publication.as_ref()))
                .map(|(id, witness)| (id.clone(), witness.clone()))
                .collect();
            let visible = BTreeSet::from(["x".to_owned(), "y".to_owned()]);
            temporal_view(body, &visible, &known)
        };
        let witnessed = with_witness(vec![
            witness("spn_y_feb", "2026-02-09"),
            witness("spn_x_mar", "2026-03-16"),
        ]);

        let february = view(&witnessed, "2026-02-20");
        let ended = february["x"]
            .ending
            .as_ref()
            .expect("February: X ended by Y");
        assert_eq!(ended.by.as_deref(), Some("y"));
        assert_eq!(ended.basis, EndBasis::Reference);
        assert_eq!(ended.time.value.as_deref(), Some("2026-02-09"));
        assert!(february["y"].ending.is_none());
        assert!(february["x"].reopened.is_none());

        let mut april = view(&witnessed, "2026-04-01");
        assert!(april["x"].ending.is_none(), "April: X is current again");
        let y = april["y"].ending.as_ref().expect("April: Y ended in March");
        assert_eq!(y.time.value.as_deref(), Some("2026-03-16"));
        assert!(y.time.approximate);
        assert_eq!(y.span, "spn_x_mar");
        assert!(april["x"].conflicts.is_empty() && april["y"].conflicts.is_empty());
        let reopened = april["x"].reopened.as_ref().expect("X reopened");
        assert_eq!(reopened.by_span, "spn_x_mar");
        assert_eq!(april["x"].lapsed.len(), 1);
        let mut x = witnessed.claims[0].clone();
        april.remove("x").unwrap().apply(&mut x);
        assert_eq!(x.qualifier(REOPENED_AT), Some("2026-03-16"));
        assert_eq!(x.qualifier("lapsed"), Some("2026-02-09/2026-03-16"));
        assert!(x.invalid_at.is_none());

        // The current view reads what April read: no Source after March.
        let today = view(&witnessed, "2026-11-27");
        assert!(today["x"].ending.is_none());
        assert_eq!(
            today["y"].ending,
            view(&witnessed, "2026-04-01")["y"].ending
        );

        // Without the March Source's witness the restatement is a conflict.
        let unwitnessed = with_witness(vec![witness("spn_y_feb", "2026-02-09")]);
        let april = view(&unwitnessed, "2026-04-01");
        assert!(april["x"].ending.is_none() && april["y"].ending.is_none());
        assert!(
            april["x"].conflicts.contains("y"),
            "conflict without the witness"
        );
        assert!(april["x"].reopened.is_none());
        // February is the same either way: the witness was not known then.
        assert_eq!(
            view(&unwitnessed, "2026-02-20")["x"].ending,
            view(&witnessed, "2026-02-20")["x"].ending
        );
    }

    fn check_cutoffs(
        notice: &str,
        end: &str,
        expiry: Option<&str>,
        tagged: bool,
        partner_known: bool,
    ) {
        let mut claim: GraphClaim = serde_json::from_value(serde_json::json!({
            "id": "cfo", "proposition": "Kim serves as CFO",
            "attribution": "prior report", "relation": "CFO", "endpoints": [], "status": "active",
            "evidence": [
                {"spanId":"prior", "role":"extracted_from", "attribution":"prior report"},
                {"spanId":"later-support", "role":"extracted_from", "attribution":"later report"}
            ],
            "qualifiers": [],
        }))
        .unwrap();
        claim.invalid_at = Some(TimePoint {
            approximate: true,
            ..time(end)
        });
        claim.expired_at = expiry.map(time);
        let qualifier = |key: &str, value: &str| Qualifier {
            key: key.into(),
            value: value.into(),
        };
        claim.qualifiers.extend([
            qualifier(END_BASIS, "reference"),
            qualifier(SUPERSEDED_BY_CLAIM, "next"),
            qualifier(CONFLICTS_WITH_CLAIM, "partner"),
        ]);
        if tagged {
            claim
                .qualifiers
                .push(qualifier(SUPERSEDED_BY_SPAN, "notice"));
        }
        let raw = serde_json::json!({"edges":[{
            "adapter_id": claim.id,
            "invalid_at": claim.invalid_at,
            "expired_at": claim.expired_at,
            "qualifiers": claim.qualifiers,
            "evidence": claim.evidence.iter().map(|binding| serde_json::json!({
                "span_id":binding.span_id, "role":binding.role, "attribution":binding.attribution
            })).collect::<Vec<_>>()
        }]});
        let mut visible = BTreeSet::from(["cfo".into()]);
        if partner_known {
            visible.insert("partner".into());
        }
        let start = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        for day in start.iter_days().take(126) {
            let cutoff = Cutoff(day);
            let date = cutoff.day();
            let mut spans = BTreeSet::from(["prior".into()]);
            if cutoff.published(Some(&time(notice))) {
                spans.insert("notice".into());
            }
            let mut projected = claim.clone();
            claim_as_of(&mut projected, &spans, &visible, cutoff);
            let show_end = end <= date.as_str()
                && if tagged {
                    notice <= date.as_str()
                } else {
                    expiry.is_none_or(|expiry| expiry <= date.as_str())
                };
            assert_eq!(projected.invalid_at.is_some(), show_end, "{date}");
            assert_eq!(projected.is_current(&time(&date)), !show_end, "{date}");
            assert_eq!(
                projected.expired_at,
                expiry
                    .filter(|expiry| show_end && *expiry <= date.as_str())
                    .map(time)
            );
            assert_eq!(
                projected.qualifiers.iter().any(|q| q.key == END_BASIS),
                show_end
            );
            assert_eq!(
                projected
                    .qualifiers
                    .iter()
                    .any(|q| q.key == CONFLICTS_WITH_CLAIM),
                partner_known
            );
            assert_eq!(projected.evidence.len(), 1);
            let known = WorldKnown {
                claims: BTreeMap::from([(projected.id.clone(), projected.clone())]),
                nodes: BTreeSet::new(),
                all_claims: BTreeSet::new(),
                annotations: BTreeMap::new(),
            };
            let mut view = raw.clone();
            assert!(graph_view_as_of(&mut view, &known).0);
            let result = &view["edges"][0];
            assert_eq!(
                result["invalid_at"],
                serde_json::json!(projected.invalid_at)
            );
            assert_eq!(
                result["expired_at"],
                serde_json::json!(projected.expired_at)
            );
            assert_eq!(
                result["qualifiers"],
                serde_json::json!(projected.qualifiers)
            );
            assert_eq!(
                result["evidence"][0]["span_id"],
                projected.evidence[0].span_id
            );
            assert_eq!(
                result["evidence"].as_array().unwrap().len(),
                projected.evidence.len()
            );
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
