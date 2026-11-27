//! Change feed over World episodes, growth and Lens section state.
//! Recorded time never uses a backfilled episode's `created_at_ms`.
use std::collections::{BTreeMap, BTreeSet};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::NaiveDate;
use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::model::world_episodes::{self, EpisodeChanges, EpisodeEnd};
use super::{
    last_day, published_by, GraphClaim, GraphNode, KnowledgeError, KnowledgeResult, TimePoint,
};
use crate::commands::assets::wire::AssetRow;
use crate::local_time::date_at_ms;

pub(crate) const DEFAULT_LIMIT: u32 = 30;
pub(crate) const MAX_LIMIT: u32 = 50;
pub(crate) const MAX_ABOUT: usize = 16;

const CURSOR_PREFIX: &str = "wch1.";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Axis {
    Recorded,
    Published,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ChangeKind {
    Added,
    Supported,
    Ended,
    Conflict,
    Disputed,
    Withdrawn,
    Lens,
}

impl ChangeKind {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "added" => Self::Added,
            "supported" => Self::Supported,
            "ended" => Self::Ended,
            "conflict" => Self::Conflict,
            "disputed" => Self::Disputed,
            "withdrawn" => Self::Withdrawn,
            "lens" => Self::Lens,
            _ => return None,
        })
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Supported => "supported",
            Self::Ended => "ended",
            Self::Conflict => "conflict",
            Self::Disputed => "disputed",
            Self::Withdrawn => "withdrawn",
            Self::Lens => "lens",
        }
    }

    fn rank(self) -> u8 {
        match self {
            Self::Added => 0,
            Self::Supported => 1,
            Self::Ended => 2,
            Self::Conflict => 3,
            Self::Disputed => 4,
            Self::Withdrawn => 5,
            Self::Lens => 6,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum AboutRef {
    Node { id: String },
    Asset { id: String },
    Lens { id: String },
}

/// Lower bound of a since-mode window. A cursor replaces the other fields.
#[derive(Clone, Debug)]
pub(crate) enum Since {
    Date(NaiveDate),
    /// `inclusive` is the previous-run start. A resume cursor is exclusive.
    Millis {
        ms: i64,
        inclusive: bool,
    },
    Cursor(String),
}

#[derive(Clone, Debug)]
pub(crate) struct FeedQuery {
    pub loupist_id: String,
    pub since: Since,
    pub axis: Axis,
    /// Set when the caller named `axis`. A cursor refuses the other axis.
    pub axis_explicit: bool,
    pub about: Vec<AboutRef>,
    pub kinds: Option<BTreeSet<ChangeKind>>,
    pub limit: u32,
    pub first_run: bool,
    pub cutoff: Option<NaiveDate>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct SortKey {
    time: i64,
    tie: String,
    kind: u8,
    claim: String,
    /// The other claim of a conflict. Two conflicts in one episode are not one row.
    partner: String,
}

#[derive(Clone, Debug)]
pub(crate) struct Ending {
    pub at: TimePoint,
    pub basis: String,
    pub by_claim_id: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct Draft {
    key: SortKey,
    kind: ChangeKind,
    recorded_at_ms: Option<i64>,
    episode_id: Option<String>,
    claim_id: String,
    ending: Option<Ending>,
    with_claim_id: Option<String>,
    source_record_id: String,
    source_revision_id: String,
    span_id: String,
    publication: Option<TimePoint>,
    lens_record_id: Option<String>,
    lens_title: Option<String>,
    section_key: Option<String>,
    section_state: Option<String>,
    section_claims: Vec<String>,
    pub(crate) hide: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Cursor {
    v: u8,
    axis: Axis,
    since_date: Option<String>,
    since_ms: Option<i64>,
    since_inclusive: bool,
    until_revision: String,
    until_ms: i64,
    about: Vec<AboutRef>,
    kinds: Option<Vec<ChangeKind>>,
    limit: u32,
    after_time: Option<i64>,
    after_tie: Option<String>,
    after_kind: Option<u8>,
    after_claim: Option<String>,
    #[serde(default)]
    after_with: Option<String>,
    resume: bool,
    first_run: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct Collected {
    pub claims: BTreeMap<String, GraphClaim>,
    pub nodes: BTreeMap<String, GraphNode>,
    pub drafts: Vec<Draft>,
    pub before_invalid: BTreeMap<String, bool>,
    pub original_ids: BTreeSet<String>,
    pending: u32,
    failed: u32,
    window_since: Value,
    until_revision: String,
    until_ms: i64,
    axis: Axis,
    about: Vec<AboutRef>,
    kinds: Option<BTreeSet<ChangeKind>>,
    limit: u32,
    first_run: bool,
    pub(crate) world_record_id: String,
    pub(crate) revision_id: String,
    pub(crate) revision_digest: String,
    since_date: Option<String>,
    since_ms: Option<i64>,
    since_inclusive: bool,
    cutoff: Option<NaiveDate>,
    /// Source record and revision for each evidence span on a displayed claim.
    span_sources: BTreeMap<String, (String, String)>,
}

pub(crate) struct FeedPage {
    pub body: Value,
    pub record_id: String,
    pub revision_id: String,
    pub revision_digest: String,
    pub claims: Vec<GraphClaim>,
}

struct Head {
    record_id: String,
    revision_id: String,
    revision_digest: String,
    created_at_ms: i64,
    claims: BTreeMap<String, GraphClaim>,
    nodes: BTreeMap<String, GraphNode>,
}

struct Hit {
    episode_id: String,
    source_record_id: String,
    source_revision_id: String,
    span_id: String,
    publication: Option<TimePoint>,
    changes: EpisodeChanges,
    backfilled: bool,
    created_at_ms: i64,
}

fn invalid(code: &'static str) -> KnowledgeError {
    KnowledgeError::Invalid(code)
}

#[cfg(test)]
pub(crate) fn error_code(error: &KnowledgeError) -> Option<&'static str> {
    let KnowledgeError::Invalid(code) = error else {
        return None;
    };
    matches!(
        *code,
        "cursor_axis_mismatch"
            | "since_after_cutoff"
            | "between_needs_from"
            | "change_cursor_invalid"
            | "change_limit_invalid"
            | "change_about_invalid"
            | "change_kinds_invalid"
            | "change_since_invalid"
    )
    .then_some(*code)
}

/// The live feed's rows; reads go through `collect_with`.
#[cfg(test)]
pub(crate) fn collect(tx: &Transaction<'_>, query: &FeedQuery) -> KnowledgeResult<Collected> {
    collect_with(tx, query, None)
}

/// The World head revision, if World has one.
pub(crate) fn world_head_revision(
    tx: &Transaction<'_>,
    loupist_id: &str,
) -> KnowledgeResult<Option<String>> {
    Ok(super::catalog::world_record(tx, loupist_id)?.and_then(|world| world.revision_id))
}

/// An ending that took effect after the reading that established it: the feed
/// shows it when its effective date arrives.
#[derive(Clone, Debug)]
pub(crate) struct ElapsedEnd {
    pub episode: world_episodes::WorldEpisode,
    pub end: EpisodeEnd,
    pub effective_ms: i64,
}

/// A Lens section as a replay follows it.
#[derive(Clone, Debug)]
pub(crate) struct ReplaySection {
    pub record_id: String,
    pub title: String,
    pub key: String,
    pub filed_at_ms: i64,
    pub claims: Vec<String>,
    /// Cited spans with their publication.
    pub spans: Vec<(String, Option<TimePoint>)>,
}

/// One Superseded or Contested transition of a section.
#[derive(Clone, Debug)]
pub(crate) struct LensTransition {
    pub record_id: String,
    pub title: String,
    pub key: String,
    pub at_ms: i64,
    pub contested: bool,
    pub claim_ids: Vec<String>,
    /// The filing's own supersede. The filing stores no dated event for it, so
    /// it sets the section's state but is not a change-feed row.
    pub filed: bool,
}

/// Lens section history under a cutoff. Before `from_ms` (the first reading of
/// a Source the cutoff does not know, or the end of the cutoff day) the stored
/// dated events are exact, after each section's filed state; from then on
/// `transitions`, replayed from the projection, decide.
#[derive(Clone, Debug, Default)]
pub(crate) struct LensReplay {
    pub from_ms: i64,
    pub transitions: Vec<LensTransition>,
}

impl LensReplay {
    /// A section's state at `until_ms`: the stored events before the first
    /// unknown reading, then the replayed transitions.
    pub(crate) fn state_at(
        &self,
        tx: &Transaction<'_>,
        record_id: &str,
        key: &str,
        until_ms: i64,
    ) -> KnowledgeResult<super::lens_page::SectionState> {
        use super::lens_page::SectionState;
        let ours = |transition: &&LensTransition| {
            transition.record_id == record_id
                && transition.key == key
                && transition.at_ms <= until_ms
        };
        // The state the section was filed with, which no stored event records.
        let filed = self
            .transitions
            .iter()
            .filter(ours)
            .rfind(|transition| transition.filed && transition.at_ms < self.from_ms)
            .map(|transition| {
                (
                    transition.at_ms,
                    SectionState::Superseded {
                        since_ms: transition.at_ms,
                        claim_ids: transition.claim_ids.clone(),
                    },
                )
            });
        let mut state = super::lens::state_from_events(
            tx,
            record_id,
            key,
            until_ms.min(self.from_ms.saturating_sub(1)),
            filed,
        )?;
        for transition in self
            .transitions
            .iter()
            .filter(ours)
            .filter(|transition| transition.at_ms >= self.from_ms)
        {
            state = apply_section_transition(
                state,
                transition.contested,
                transition.at_ms,
                transition.claim_ids.clone(),
            );
        }
        Ok(state)
    }
}

/// A section's state after a Superseded or Contested transition that names
/// the claims it newly attached, as `LensBody::contest` and the supersede
/// rule keep it: a second transition of the same kind keeps when it began
/// and adds its claims, and a Contested section stays Contested.
pub(crate) fn apply_section_transition(
    state: super::lens_page::SectionState,
    contested: bool,
    at_ms: i64,
    claim_ids: Vec<String>,
) -> super::lens_page::SectionState {
    let joined = |mut ids: Vec<String>| {
        ids.extend(claim_ids.iter().cloned());
        ids.sort();
        ids.dedup();
        ids
    };
    match (contested, state) {
        (
            true,
            super::lens_page::SectionState::Contested {
                since_ms,
                claim_ids: ids,
            },
        ) => super::lens_page::SectionState::Contested {
            since_ms,
            claim_ids: joined(ids),
        },
        (true, _) => super::lens_page::SectionState::Contested {
            since_ms: at_ms,
            claim_ids: joined(Vec::new()),
        },
        (false, contested @ super::lens_page::SectionState::Contested { .. }) => contested,
        (
            false,
            super::lens_page::SectionState::Superseded {
                since_ms,
                claim_ids: ids,
            },
        ) => super::lens_page::SectionState::Superseded {
            since_ms,
            claim_ids: joined(ids),
        },
        (false, super::lens_page::SectionState::Current) => {
            super::lens_page::SectionState::Superseded {
                since_ms: at_ms,
                claim_ids: joined(Vec::new()),
            }
        }
    }
}

/// The World a cutoff knows, as its readings changed it (`as_of::episodes_as_of`).
#[derive(Clone, Debug, Default)]
pub(crate) struct Replayed {
    /// Lens section history under the cutoff.
    pub lens: Option<LensReplay>,
    /// Each live episode's changes; `None` hides a reading of a Source the
    /// cutoff does not know. A missing entry (a legacy shell) keeps its own.
    pub episodes: BTreeMap<String, Option<EpisodeChanges>>,
    pub elapsed: Vec<ElapsedEnd>,
    /// Disputes and withdrawals, by event id, whose witness the cutoff does not
    /// know: no row.
    pub unknown_transitions: BTreeSet<String>,
}

/// [`collect`], with the readings and elapsed ends `replayed` gives under a
/// cutoff in place of the stored episode changes and end events.
pub(crate) fn collect_with(
    tx: &Transaction<'_>,
    query: &FeedQuery,
    replayed: Option<&Replayed>,
) -> KnowledgeResult<Collected> {
    let episodes = replayed.map(|replayed| &replayed.episodes);
    if !(1..=MAX_LIMIT).contains(&query.limit) {
        return Err(invalid("change_limit_invalid"));
    }
    if query.about.len() > MAX_ABOUT {
        return Err(invalid("change_about_invalid"));
    }
    let (resolved, position) = resolve_since(query)?;
    if let Some(cutoff) = query.cutoff {
        if resolved
            .since_date
            .as_deref()
            .and_then(|value| NaiveDate::parse_from_str(value, "%Y-%m-%d").ok())
            .is_some_and(|date| date > cutoff)
            || resolved
                .since_ms
                .and_then(date_at_ms)
                .is_some_and(|date| date > cutoff)
        {
            return Err(invalid("since_after_cutoff"));
        }
    }
    let head = load_head(tx, &query.loupist_id)?;
    // A fresh read and a resume both close on the head as it is now. A page
    // cursor keeps the head it started with, so a publication between pages
    // waits for the resume cursor.
    let (until_revision, until_ms) = if resolved.from_cursor && !resolved.resume {
        (resolved.until_revision.clone(), resolved.until_ms)
    } else if let Some(head) = &head {
        let last_end =
            super::world_followups::last_effective_end(tx, &query.loupist_id, &head.record_id)?;
        // A replay closes at the end of its day: what the World had recorded
        // by then, whenever its head was last written.
        let replay_end = query.cutoff.map_or(0, end_of_local_day_ms);
        (
            head.revision_id.clone(),
            head.created_at_ms
                .max(last_end.unwrap_or(0))
                .max(replay_end),
        )
    } else {
        (String::new(), 0)
    };
    let kinds = resolved.kinds.clone().or(query.kinds.clone());
    // A page that only carries the cursor keeps its scope. A caller that names
    // a scope, including a check whose assets changed, replaces it.
    let about = if resolved.from_cursor && query.about.is_empty() {
        resolved.about.clone()
    } else {
        query.about.clone()
    };
    let limit = if resolved.from_cursor {
        resolved.limit
    } else {
        query.limit
    };
    let axis = resolved.axis;
    let first_run = if resolved.from_cursor {
        resolved.first_run
    } else {
        query.first_run
    };
    let scope = scope_of(tx, &query.loupist_id, &about, head.as_ref())?;
    let mut drafts = Vec::new();
    if let Some(head) = &head {
        if wants(kinds.as_ref(), ChangeKind::Lens) || kinds.is_none() {
            drafts.extend(lens_drafts(
                tx,
                &query.loupist_id,
                head,
                &scope,
                &resolved,
                until_ms,
                query.cutoff,
                replayed.and_then(|replayed| replayed.lens.as_ref()),
            )?);
        }
        let hits = episode_hits(
            tx,
            &query.loupist_id,
            &head.record_id,
            axis,
            until_ms,
            &resolved,
        )?;
        for mut hit in hits {
            match episodes.and_then(|episodes| episodes.get(&hit.episode_id)) {
                Some(None) => continue,
                Some(Some(changes)) => hit.changes = changes.clone(),
                None => {}
            }
            push_episode_drafts(
                &mut drafts,
                &hit,
                head,
                &scope,
                kinds.as_ref(),
                axis,
                &resolved,
                query.cutoff,
            );
        }
        drafts.extend(status_drafts(
            tx,
            &query.loupist_id,
            head,
            &scope,
            kinds.as_ref(),
            axis,
            &resolved,
            until_ms,
            query.cutoff,
            replayed.map(|replayed| &replayed.unknown_transitions),
        )?);
        if wants(kinds.as_ref(), ChangeKind::Ended) {
            let mut elapsed = match replayed {
                Some(replayed) => {
                    elapsed_drafts(&replayed.elapsed, query, head, &scope, &resolved, until_ms)
                }
                None => effective_end_drafts(tx, query, head, &scope, &resolved, until_ms)?,
            };
            // A previously published episode can already carry the same end.
            // In that window prefer its passage and publication provenance.
            let announced: BTreeSet<_> = drafts
                .iter()
                .filter_map(|draft| {
                    let ending = draft.ending.as_ref()?;
                    Some((draft.claim_id.as_str(), ending.at.value.as_deref()))
                })
                .collect();
            elapsed.retain(|draft| {
                let date = draft
                    .ending
                    .as_ref()
                    .and_then(|end| end.at.value.as_deref());
                !announced.contains(&(draft.claim_id.as_str(), date))
            });
            drafts.extend(elapsed);
        }
    }
    if let Some(after) = &position {
        drafts.retain(|draft| draft.key > *after);
    }
    drafts.sort_by(|left, right| left.key.cmp(&right.key));
    let mut claims = BTreeMap::new();
    let mut nodes = BTreeMap::new();
    if let Some(head) = &head {
        let mut wanted = BTreeSet::new();
        for draft in &drafts {
            if draft.kind != ChangeKind::Lens {
                wanted.insert(draft.claim_id.clone());
            }
            if let Some(by) = draft
                .ending
                .as_ref()
                .and_then(|end| end.by_claim_id.clone())
            {
                wanted.insert(by);
            }
            if let Some(partner) = &draft.with_claim_id {
                wanted.insert(partner.clone());
            }
            wanted.extend(draft.section_claims.iter().cloned());
        }
        for id in wanted {
            if let Some(claim) = head.claims.get(&id) {
                claims.insert(id, claim.clone());
            }
        }
        nodes.clone_from(&head.nodes);
    }
    let (pending, failed) = extraction_counts(
        tx,
        &query.loupist_id,
        &scope,
        head.as_ref().map(|head| head.record_id.as_str()),
        query.cutoff,
    )?;
    let mut span_sources = BTreeMap::new();
    for claim in claims.values() {
        for evidence in &claim.evidence {
            if span_sources.contains_key(&evidence.span_id) {
                continue;
            }
            if let Some(source) = span_read(tx, &evidence.span_id, query.cutoff)? {
                span_sources.insert(evidence.span_id.clone(), source);
            }
        }
    }
    let before_invalid = claims
        .iter()
        .map(|(id, claim)| (id.clone(), claim.invalid_at.is_some()))
        .collect();
    let original_ids = claims.keys().cloned().collect();
    let window_since = match &resolved.since_date {
        Some(date) => Value::String(date.clone()),
        None => resolved
            .since_ms
            .map(|ms| Value::Number(ms.into()))
            .unwrap_or(Value::Null),
    };
    Ok(Collected {
        claims,
        nodes,
        drafts,
        before_invalid,
        original_ids,
        pending,
        failed,
        window_since,
        until_revision,
        until_ms,
        axis,
        about,
        kinds,
        limit,
        first_run,
        world_record_id: head
            .as_ref()
            .map(|head| head.record_id.clone())
            .unwrap_or_default(),
        revision_id: head
            .as_ref()
            .map(|head| head.revision_id.clone())
            .unwrap_or_default(),
        revision_digest: head
            .as_ref()
            .map(|head| head.revision_digest.clone())
            .unwrap_or_default(),
        since_date: resolved.since_date,
        since_ms: resolved.since_ms,
        since_inclusive: resolved.since_inclusive,
        cutoff: query.cutoff,
        span_sources,
    })
}

/// Under a cutoff an episode row stands only when its Source was known by then
/// (`known(record, revision, span)`, the projection's own test): a legacy
/// shell, or an elapsed end, that the replay did not already decide.
pub(crate) fn keep_known_episodes(
    collected: &mut Collected,
    mut known: impl FnMut(&str, &str, &str) -> KnowledgeResult<bool>,
) -> KnowledgeResult<()> {
    for draft in &mut collected.drafts {
        if draft.episode_id.is_none() || draft.source_record_id.is_empty() {
            continue;
        }
        if !known(
            &draft.source_record_id,
            &draft.source_revision_id,
            &draft.span_id,
        )? {
            draft.hide = true;
        }
    }
    Ok(())
}

/// An ending the cutoff projection cleared, or whose witness it dropped, is not a row.
pub(crate) fn conceal_endings(collected: &mut Collected) {
    let dropped: BTreeSet<&str> = collected
        .original_ids
        .iter()
        .map(String::as_str)
        .filter(|id| !collected.claims.contains_key(*id))
        .collect();
    for draft in &mut collected.drafts {
        if draft.kind == ChangeKind::Conflict {
            let partner = draft.with_claim_id.as_deref().unwrap_or("");
            let related = collected.claims.get(&draft.claim_id).is_some_and(|claim| {
                claim.qualifiers.iter().any(|qualifier| {
                    qualifier.key == super::CONFLICTS_WITH_CLAIM && qualifier.value == partner
                })
            });
            draft.hide = collected.cutoff.is_some()
                && (!collected.claims.contains_key(&draft.claim_id)
                    || !collected.claims.contains_key(partner)
                    || !related);
            continue;
        }
        if draft.kind != ChangeKind::Ended {
            continue;
        }
        let cleared = collected
            .before_invalid
            .get(&draft.claim_id)
            .copied()
            .unwrap_or(false)
            && collected
                .claims
                .get(&draft.claim_id)
                .is_some_and(|claim| claim.invalid_at.is_none());
        let witness = draft
            .ending
            .as_ref()
            .and_then(|ending| ending.by_claim_id.as_deref());
        draft.hide = cleared
            || dropped.contains(draft.claim_id.as_str())
            || witness.is_some_and(|id| dropped.contains(id));
    }
}

fn start_of_local_day_ms(day: NaiveDate) -> Option<i64> {
    let offset = *crate::local_time::now().offset();
    day.and_hms_opt(0, 0, 0)
        .and_then(|moment| moment.and_local_timezone(offset).single())
        .map(|moment| moment.timestamp_millis())
}

fn recorded_after_ms(resolved: &Resolved) -> i64 {
    if let Some(ms) = resolved.since_ms {
        return if resolved.since_inclusive {
            ms.saturating_sub(1)
        } else {
            ms
        };
    }
    resolved
        .since_date
        .as_deref()
        .and_then(|value| NaiveDate::parse_from_str(value, "%Y-%m-%d").ok())
        .and_then(start_of_local_day_ms)
        .map(|ms| ms.saturating_sub(1))
        .unwrap_or(-1)
}

fn end_of_local_day_ms(day: NaiveDate) -> i64 {
    let offset = *crate::local_time::now().offset();
    day.and_hms_milli_opt(23, 59, 59, 999)
        .and_then(|moment| moment.and_local_timezone(offset).single())
        .map(|moment| moment.timestamp_millis())
        .unwrap_or(0)
}

pub(crate) fn finish(collected: Collected) -> FeedPage {
    let mut kept = Vec::new();
    for draft in collected.drafts {
        if draft.hide {
            continue;
        }
        if draft.kind != ChangeKind::Lens && !collected.claims.contains_key(&draft.claim_id) {
            continue;
        }
        kept.push(draft);
    }
    let mut totals = serde_json::Map::new();
    for draft in &kept {
        let count = totals
            .entry(draft.kind.as_str().to_owned())
            .or_insert(json!(0));
        *count = json!(count.as_u64().unwrap_or(0) + 1);
    }
    let capped = kept.len() > collected.limit as usize;
    let page: Vec<Draft> = kept.into_iter().take(collected.limit as usize).collect();
    let next = if let Some(last) = page.last().filter(|_| capped) {
        encode_cursor(&Cursor {
            v: 1,
            axis: collected.axis,
            since_date: collected.since_date.clone(),
            since_ms: collected.since_ms,
            since_inclusive: collected.since_inclusive,
            until_revision: collected.until_revision.clone(),
            until_ms: collected.until_ms,
            about: collected.about.clone(),
            kinds: collected
                .kinds
                .as_ref()
                .map(|kinds| kinds.iter().copied().collect()),
            limit: collected.limit,
            after_time: Some(last.key.time),
            after_tie: Some(last.key.tie.clone()),
            after_kind: Some(last.key.kind),
            after_claim: Some(last.key.claim.clone()),
            after_with: Some(last.key.partner.clone()),
            resume: false,
            first_run: collected.first_run,
        })
    } else {
        encode_cursor(&Cursor {
            v: 1,
            axis: collected.axis,
            since_date: None,
            // A live resume starts after today's head. A replay resume starts
            // after the replay day, so the next earlier day is not skipped.
            since_ms: Some(
                collected
                    .cutoff
                    .map(end_of_local_day_ms)
                    .unwrap_or(collected.until_ms),
            ),
            since_inclusive: false,
            until_revision: collected.until_revision.clone(),
            until_ms: collected.until_ms,
            about: collected.about.clone(),
            kinds: collected
                .kinds
                .as_ref()
                .map(|kinds| kinds.iter().copied().collect()),
            limit: collected.limit,
            after_time: None,
            after_tie: None,
            after_kind: None,
            after_claim: None,
            after_with: None,
            resume: true,
            first_run: false,
        })
    };
    let mut changes = Vec::new();
    let mut lens = Vec::new();
    let mut page_claims = Vec::new();
    let mut seen = BTreeSet::new();
    for draft in &page {
        if draft.kind == ChangeKind::Lens {
            lens.push(json!({
                "recordId": draft.lens_record_id,
                "title": draft.lens_title,
                "sectionKey": draft.section_key,
                "state": draft.section_state,
                "recordedAtMs": draft.recorded_at_ms,
                "claimIds": draft.section_claims,
            }));
            continue;
        }
        let Some(claim) = collected.claims.get(&draft.claim_id) else {
            continue;
        };
        if seen.insert(claim.id.clone()) {
            page_claims.push(claim.clone());
        }
        let endpoints: Vec<Value> = claim
            .endpoints
            .iter()
            .map(|end| {
                json!({
                    "id": end.node_id,
                    "name": collected.nodes.get(&end.node_id).map(|node| node.name.as_str()).unwrap_or(""),
                    "role": end.role,
                })
            })
            .collect();
        let mut row = json!({
            "kind": draft.kind.as_str(),
            "recordedAtMs": draft.recorded_at_ms,
            "episodeId": draft.episode_id,
            "claimId": claim.id,
            "proposition": claim.proposition,
            "relation": claim.relation,
            "endpoints": endpoints,
            "validAt": claim.valid_at,
            "invalidAt": claim.invalid_at,
            "status": claim.status.as_str(),
            "source": {
                "title": "",
                "publicationTime": draft.publication,
                "readArgs": {
                    "scope": "source",
                    "recordId": draft.source_record_id,
                    "revisionId": draft.source_revision_id,
                    "spanId": draft.span_id,
                }
            },
            "evidence": evidence_reads(&collected.span_sources, claim),
        });
        if claim.evidence.len() > row["evidence"].as_array().map(Vec::len).unwrap_or(0) {
            row["evidenceUnresolved"] = json!(true);
        }
        if let Some(ending) = &draft.ending {
            row["ending"] = json!({
                "at": ending.at,
                "approximate": ending.at.approximate,
                "basis": ending.basis,
                "byClaimId": ending.by_claim_id,
            });
            // The figure the fact that ended this one states, and which way it moved.
            let by = ending
                .by_claim_id
                .as_deref()
                .or_else(|| claim.qualifier(super::SUPERSEDED_BY_CLAIM));
            if let Some(by) = by.and_then(|id| collected.claims.get(id)) {
                row["figures"] = json!(super::figure_check::figure_change(
                    &claim.proposition,
                    &by.proposition
                ));
            }
        }
        if let Some(with) = &draft.with_claim_id {
            row["withClaimId"] = json!(with);
        }
        changes.push(row);
    }
    let body = json!({
        "window": {
            "since": collected.window_since,
            "until": if collected.until_revision.is_empty() { Value::Null } else { Value::String(collected.until_revision.clone()) },
            "axis": match collected.axis { Axis::Recorded => "recorded", Axis::Published => "published" },
        },
        "pending": collected.pending,
        "failed": collected.failed,
        "totals": totals,
        "changes": changes,
        "lens": lens,
        "first_run": collected.first_run,
        "capped": capped,
        "next": next,
    });
    FeedPage {
        body,
        record_id: collected.world_record_id,
        revision_id: collected.revision_id,
        revision_digest: collected.revision_digest,
        claims: page_claims,
    }
}

struct Resolved {
    axis: Axis,
    since_date: Option<String>,
    since_ms: Option<i64>,
    since_inclusive: bool,
    until_revision: String,
    until_ms: i64,
    about: Vec<AboutRef>,
    kinds: Option<BTreeSet<ChangeKind>>,
    limit: u32,
    first_run: bool,
    resume: bool,
    from_cursor: bool,
}

fn resolve_since(query: &FeedQuery) -> KnowledgeResult<(Resolved, Option<SortKey>)> {
    match &query.since {
        Since::Cursor(token) => {
            let cursor = decode_cursor(token)?;
            if query.axis_explicit && query.axis != cursor.axis {
                return Err(invalid("cursor_axis_mismatch"));
            }
            if cursor.v != 1 || !(1..=MAX_LIMIT).contains(&cursor.limit) {
                return Err(invalid("change_cursor_invalid"));
            }
            let position = match (
                cursor.resume,
                cursor.after_time,
                cursor.after_tie,
                cursor.after_kind,
                cursor.after_claim,
            ) {
                (false, Some(time), Some(tie), Some(kind), Some(claim)) => Some(SortKey {
                    time,
                    tie,
                    kind,
                    claim,
                    partner: cursor.after_with.unwrap_or_default(),
                }),
                (true, None, None, None, None) => None,
                (false, None, None, None, None) => None,
                _ => return Err(invalid("change_cursor_invalid")),
            };
            let kinds = cursor
                .kinds
                .map(|kinds| kinds.into_iter().collect::<BTreeSet<_>>());
            let resolved = Resolved {
                axis: cursor.axis,
                since_date: if cursor.resume {
                    None
                } else {
                    cursor.since_date
                },
                since_ms: cursor.since_ms,
                since_inclusive: cursor.since_inclusive && !cursor.resume,
                until_revision: cursor.until_revision,
                until_ms: cursor.until_ms,
                about: cursor.about,
                kinds,
                limit: cursor.limit,
                first_run: cursor.first_run && !cursor.resume,
                resume: cursor.resume,
                from_cursor: true,
            };
            Ok((resolved, position))
        }
        Since::Date(date) => Ok((
            Resolved {
                axis: query.axis,
                since_date: Some(date.to_string()),
                since_ms: None,
                since_inclusive: true,
                until_revision: String::new(),
                until_ms: i64::MAX,
                about: query.about.clone(),
                kinds: query.kinds.clone(),
                limit: query.limit,
                first_run: query.first_run,
                resume: false,
                from_cursor: false,
            },
            None,
        )),
        Since::Millis { ms, inclusive } => Ok((
            Resolved {
                axis: query.axis,
                since_date: None,
                since_ms: Some(*ms),
                since_inclusive: *inclusive,
                until_revision: String::new(),
                until_ms: i64::MAX,
                about: query.about.clone(),
                kinds: query.kinds.clone(),
                limit: query.limit,
                first_run: query.first_run,
                resume: false,
                from_cursor: false,
            },
            None,
        )),
    }
}

fn decode_cursor(token: &str) -> KnowledgeResult<Cursor> {
    let rest = token
        .strip_prefix(CURSOR_PREFIX)
        .ok_or_else(|| invalid("change_cursor_invalid"))?;
    let bytes = URL_SAFE_NO_PAD
        .decode(rest)
        .map_err(|_| invalid("change_cursor_invalid"))?;
    serde_json::from_slice(&bytes).map_err(|_| invalid("change_cursor_invalid"))
}

fn encode_cursor(cursor: &Cursor) -> String {
    let bytes = serde_json::to_vec(cursor).unwrap_or_default();
    format!("{CURSOR_PREFIX}{}", URL_SAFE_NO_PAD.encode(bytes))
}

fn load_head(tx: &Transaction<'_>, loupist_id: &str) -> KnowledgeResult<Option<Head>> {
    let Some(world) = super::catalog::world_record(tx, loupist_id)? else {
        return Ok(None);
    };
    let Some(revision_id) = world.revision_id else {
        return Ok(None);
    };
    let stored = super::revision::load_revision(tx, &revision_id)?;
    let body: super::WorldSnapshotBody = super::canonical::from_json(&stored.payload_json)?;
    Ok(Some(Head {
        record_id: world.record_id,
        revision_id: stored.revision_id,
        revision_digest: stored.revision_digest,
        created_at_ms: stored.created_at_ms,
        claims: body
            .claims
            .into_iter()
            .map(|claim| (claim.id.clone(), claim))
            .collect(),
        nodes: body
            .nodes
            .into_iter()
            .map(|node| (node.id.clone(), node))
            .collect(),
    }))
}

struct Scope {
    whole: bool,
    nodes: BTreeSet<String>,
    claims: BTreeSet<String>,
    lenses: BTreeSet<String>,
    sources: BTreeSet<String>,
    names: Vec<String>,
}

impl Scope {
    fn matches_claim(&self, claim: &GraphClaim) -> bool {
        self.whole
            || self.claims.contains(&claim.id)
            || claim
                .endpoints
                .iter()
                .any(|end| self.nodes.contains(&end.node_id))
    }
}

fn scope_of(
    tx: &Transaction<'_>,
    loupist_id: &str,
    about: &[AboutRef],
    head: Option<&Head>,
) -> KnowledgeResult<Scope> {
    if about.is_empty() {
        return Ok(Scope {
            whole: true,
            nodes: BTreeSet::new(),
            claims: BTreeSet::new(),
            lenses: BTreeSet::new(),
            sources: BTreeSet::new(),
            names: Vec::new(),
        });
    }
    let mut nodes = BTreeSet::new();
    let mut claims = BTreeSet::new();
    let mut lenses = BTreeSet::new();
    let mut names = Vec::new();
    for item in about {
        match item {
            AboutRef::Node { id } => {
                if id.is_empty() || id.chars().count() > 128 {
                    return Err(invalid("change_about_invalid"));
                }
                nodes.insert(id.clone());
            }
            AboutRef::Asset { id } => {
                if !super::assets::valid_asset_id(id) {
                    return Err(invalid("change_about_invalid"));
                }
                let Some(page) = super::assets::get_page(tx, loupist_id, id)? else {
                    return Err(KnowledgeError::NotFound);
                };
                names.push(fold(&page.name));
                if let Some(entity) = page.entity_id {
                    if let Some(node) = head.and_then(|head| head.nodes.get(&entity)) {
                        names.push(fold(&node.name));
                        names.extend(node.aliases.iter().map(|alias| fold(alias)));
                    }
                    nodes.insert(entity);
                }
            }
            AboutRef::Lens { id } => {
                super::model::validate_knowledge_id(id, "rec")
                    .map_err(|_| invalid("change_about_invalid"))?;
                let page = super::lens::read_page(tx, loupist_id, id)?;
                lenses.insert(id.clone());
                for anchor in page.anchors {
                    if let Some(node) = anchor.node_id {
                        nodes.insert(node);
                    }
                }
                for section in page.body.sections() {
                    for reference in &section.basis.refs {
                        if let super::lens_page::BasisRef::Claim { claim_id } = reference {
                            claims.insert(claim_id.clone());
                        }
                    }
                }
            }
        }
    }
    let mut sources = BTreeSet::new();
    if let Some(head) = head {
        for claim in head.claims.values().filter(|claim| {
            nodes
                .iter()
                .any(|node| claim.endpoints.iter().any(|end| &end.node_id == node))
                || claims.contains(&claim.id)
        }) {
            for evidence in &claim.evidence {
                if let Some(source) = source_for_span(tx, loupist_id, &evidence.span_id)? {
                    sources.insert(source);
                }
            }
        }
    }
    Ok(Scope {
        whole: false,
        nodes,
        claims,
        lenses,
        sources,
        names,
    })
}

/// The Source that holds a claim's evidence span, and its current revision.
/// A status row has no event Source; authorship is this span.
fn span_read(
    tx: &Transaction<'_>,
    span_id: &str,
    cutoff: Option<NaiveDate>,
) -> KnowledgeResult<Option<(String, String)>> {
    if span_id.is_empty() {
        return Ok(None);
    }
    let record: Option<String> = tx
        .query_row(
            "SELECT record_id FROM knowledge_source_spans WHERE span_id = ?1",
            [span_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(record) = record.filter(|record| !record.is_empty()) else {
        return Ok(None);
    };
    let revision: Option<String> = tx
        .query_row(
            "SELECT revision_id FROM knowledge_record_heads WHERE record_id = ?1",
            [&record],
            |row| row.get(0),
        )
        .optional()?;
    let Some(revision) = revision.filter(|revision| !revision.is_empty()) else {
        return Ok(None);
    };
    // A later capture can move the Source's title and URL while an earlier
    // span of the same record stays known. That later identity is not a label
    // for a claim the other side still shows.
    if let Some(cutoff) = cutoff {
        let payload: Option<String> = tx
            .query_row(
                "SELECT payload_json FROM knowledge_record_revisions WHERE revision_id = ?1",
                [&revision],
                |row| row.get(0),
            )
            .optional()?;
        let known = payload
            .as_deref()
            .and_then(|payload| serde_json::from_str::<Value>(payload).ok())
            .and_then(|value| publication_day(&value))
            .is_some_and(|day| day <= cutoff);
        if !known {
            return Ok(None);
        }
    }
    Ok(Some((record, revision)))
}

fn evidence_reads(spans: &BTreeMap<String, (String, String)>, claim: &GraphClaim) -> Vec<Value> {
    claim
        .evidence
        .iter()
        .filter_map(|binding| {
            let (record, revision) = spans.get(&binding.span_id)?;
            Some(json!({
                "recordId": record,
                "revisionId": revision,
                "spanId": binding.span_id,
            }))
        })
        .collect()
}

fn source_for_span(
    tx: &Transaction<'_>,
    loupist_id: &str,
    span_id: &str,
) -> KnowledgeResult<Option<String>> {
    Ok(tx
        .query_row(
            "SELECT source_record_id FROM knowledge_world_episodes
             WHERE loupist_id = ?1 AND span_id = ?2
             ORDER BY created_at_ms DESC LIMIT 1",
            params![loupist_id, span_id],
            |row| row.get(0),
        )
        .optional()?)
}

fn wants(kinds: Option<&BTreeSet<ChangeKind>>, kind: ChangeKind) -> bool {
    kinds.is_none_or(|kinds| kinds.contains(&kind))
}

fn in_time(resolved: &Resolved, until_ms: i64, at_ms: i64, cutoff: Option<NaiveDate>) -> bool {
    if at_ms > until_ms {
        return false;
    }
    if let Some(cutoff) = cutoff {
        if !date_at_ms(at_ms).is_some_and(|day| day <= cutoff) {
            return false;
        }
    }
    if let Some(date) = resolved
        .since_date
        .as_deref()
        .and_then(|value| NaiveDate::parse_from_str(value, "%Y-%m-%d").ok())
    {
        if !date_at_ms(at_ms).is_some_and(|day| day >= date) {
            return false;
        }
    }
    if let Some(ms) = resolved.since_ms {
        if resolved.since_inclusive {
            if at_ms < ms {
                return false;
            }
        } else if at_ms <= ms {
            return false;
        }
    }
    true
}

/// A published-axis row sorts at the start of the user's day of publication.
fn publication_sort_ms(publication: Option<&TimePoint>) -> Option<i64> {
    publication
        .and_then(super::world_filter::publication_day)
        .and_then(|day| day.and_hms_opt(0, 0, 0))
        .map(|moment| moment.and_utc().timestamp_millis())
}

fn published_in(
    resolved: &Resolved,
    publication: Option<&TimePoint>,
    cutoff: Option<NaiveDate>,
) -> bool {
    let Some(publication) = publication else {
        return false;
    };
    if let Some(cutoff) = cutoff {
        if !published_by(Some(publication), &cutoff.to_string()) {
            return false;
        }
    }
    let Some(day) = super::world_filter::publication_day(publication) else {
        return false;
    };
    if let Some(since) = resolved
        .since_date
        .as_deref()
        .and_then(|value| NaiveDate::parse_from_str(value, "%Y-%m-%d").ok())
    {
        if day < since {
            return false;
        }
    }
    if let Some(ms) = resolved.since_ms {
        let Some(start) = date_at_ms(ms) else {
            return false;
        };
        if resolved.since_inclusive {
            if day < start {
                return false;
            }
        } else if day <= start {
            return false;
        }
    }
    true
}

fn episode_hits(
    tx: &Transaction<'_>,
    loupist_id: &str,
    world_id: &str,
    axis: Axis,
    until_ms: i64,
    resolved: &Resolved,
) -> KnowledgeResult<Vec<Hit>> {
    let removed = super::memory_controls::load(tx, loupist_id)?.removed;
    if axis == Axis::Recorded {
        // The window is in the query. A row outside it is not read.
        let mut hits = Vec::new();
        for episode in world_episodes::recorded_window(
            tx,
            loupist_id,
            world_id,
            recorded_after_ms(resolved),
            until_ms,
        )? {
            if removed.contains(&episode.source_record_id) {
                continue;
            }
            if !in_time(resolved, until_ms, episode.created_at_ms, None) {
                continue;
            }
            hits.push(Hit {
                episode_id: episode.episode_id,
                source_record_id: episode.source_record_id,
                source_revision_id: episode.source_revision_id,
                span_id: episode.span_id,
                publication: episode.publication_time,
                changes: episode.changes,
                backfilled: episode.backfilled,
                created_at_ms: episode.created_at_ms,
            });
        }
        return Ok(hits);
    }
    let mut statement = tx.prepare(
        "SELECT episode_id, source_record_id, source_revision_id, span_id, publication_json,
                changes_json, backfilled, created_at_ms
         FROM knowledge_world_episodes
         WHERE loupist_id = ?1 AND world_record_id = ?2
           AND (backfilled = 1 OR created_at_ms <= ?3)
         ORDER BY episode_id",
    )?;
    let rows = statement.query_map(params![loupist_id, world_id, until_ms], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, bool>(6)?,
            row.get::<_, i64>(7)?,
        ))
    })?;
    let mut hits = Vec::new();
    for row in rows {
        let (episode_id, source, revision, span, publication, changes, backfilled, created_at_ms) =
            row?;
        if removed.contains(&source) {
            continue;
        }
        // A live publication after the frozen head is the next window. A backfill
        // shell's created_at_ms is the upgrade, not the publication.
        if !backfilled && created_at_ms > until_ms {
            continue;
        }
        let publication = publication
            .map(|value| serde_json::from_str(&value))
            .transpose()?;
        let changes = serde_json::from_str(&changes)?;
        hits.push(Hit {
            episode_id,
            source_record_id: source,
            source_revision_id: revision,
            span_id: span,
            publication,
            changes,
            backfilled,
            created_at_ms,
        });
    }
    Ok(hits)
}

#[allow(clippy::too_many_arguments)]
fn push_episode_drafts(
    drafts: &mut Vec<Draft>,
    hit: &Hit,
    head: &Head,
    scope: &Scope,
    kinds: Option<&BTreeSet<ChangeKind>>,
    axis: Axis,
    resolved: &Resolved,
    cutoff: Option<NaiveDate>,
) {
    if axis == Axis::Recorded {
        if cutoff
            .is_some_and(|cutoff| !date_at_ms(hit.created_at_ms).is_some_and(|day| day <= cutoff))
        {
            return;
        }
    } else if !published_in(resolved, hit.publication.as_ref(), cutoff) {
        return;
    }
    let recorded_at_ms = (!hit.backfilled).then_some(hit.created_at_ms);
    let time = if axis == Axis::Recorded {
        hit.created_at_ms
    } else {
        publication_sort_ms(hit.publication.as_ref()).unwrap_or(0)
    };
    let mut add =
        |kind: ChangeKind, claim_id: &str, ending: Option<Ending>, with: Option<String>| {
            if !wants(kinds, kind) {
                return;
            }
            let Some(claim) = head.claims.get(claim_id) else {
                return;
            };
            if !scope.matches_claim(claim) {
                return;
            }
            drafts.push(Draft {
                key: SortKey {
                    time,
                    tie: hit.episode_id.clone(),
                    kind: kind.rank(),
                    claim: claim_id.to_owned(),
                    partner: with.clone().unwrap_or_default(),
                },
                kind,
                recorded_at_ms,
                episode_id: Some(hit.episode_id.clone()),
                claim_id: claim_id.to_owned(),
                ending,
                with_claim_id: with,
                source_record_id: hit.source_record_id.clone(),
                source_revision_id: hit.source_revision_id.clone(),
                span_id: hit.span_id.clone(),
                publication: hit.publication.clone(),
                lens_record_id: None,
                lens_title: None,
                section_key: None,
                section_state: None,
                section_claims: Vec::new(),
                hide: false,
            });
        };
    for id in &hit.changes.created {
        add(ChangeKind::Added, id, None, None);
    }
    for id in &hit.changes.supported {
        add(ChangeKind::Supported, id, None, None);
    }
    for end in &hit.changes.ended {
        add(ChangeKind::Ended, &end.claim, Some(ending_of(end)), None);
    }
    for conflict in &hit.changes.conflicts {
        add(
            ChangeKind::Conflict,
            &conflict.claim,
            None,
            Some(conflict.with.clone()),
        );
    }
}

fn ending_of(end: &EpisodeEnd) -> Ending {
    let basis = match end.basis {
        world_episodes::EndBasis::Stated => "stated",
        world_episodes::EndBasis::Reference => "reference",
    };
    Ending {
        at: end.end.clone(),
        basis: basis.to_owned(),
        by_claim_id: end.by.clone(),
    }
}

/// Ended rows for endings that took effect after their reading, as the
/// replay under a cutoff dates them.
fn elapsed_drafts(
    elapsed: &[ElapsedEnd],
    query: &FeedQuery,
    head: &Head,
    scope: &Scope,
    resolved: &Resolved,
    until_ms: i64,
) -> Vec<Draft> {
    let mut drafts = Vec::new();
    for item in elapsed {
        let Some(claim) = head.claims.get(&item.end.claim) else {
            continue;
        };
        if !scope.matches_claim(claim) {
            continue;
        }
        let time = match resolved.axis {
            Axis::Recorded => {
                if !in_time(resolved, until_ms, item.effective_ms, query.cutoff) {
                    continue;
                }
                item.effective_ms
            }
            Axis::Published => {
                if !published_in(
                    resolved,
                    item.episode.publication_time.as_ref(),
                    query.cutoff,
                ) {
                    continue;
                }
                publication_sort_ms(item.episode.publication_time.as_ref()).unwrap_or(0)
            }
        };
        drafts.push(Draft {
            key: SortKey {
                time,
                tie: format!("elapsed:{}", item.episode.episode_id),
                kind: ChangeKind::Ended.rank(),
                claim: item.end.claim.clone(),
                partner: String::new(),
            },
            kind: ChangeKind::Ended,
            recorded_at_ms: Some(item.effective_ms),
            episode_id: Some(item.episode.episode_id.clone()),
            ending: Some(ending_of(&item.end)),
            claim_id: item.end.claim.clone(),
            with_claim_id: None,
            source_record_id: item.episode.source_record_id.clone(),
            source_revision_id: item.episode.source_revision_id.clone(),
            span_id: item.episode.span_id.clone(),
            publication: item.episode.publication_time.clone(),
            lens_record_id: None,
            lens_title: None,
            section_key: None,
            section_state: None,
            section_claims: Vec::new(),
            hide: false,
        });
    }
    drafts
}

fn effective_end_drafts(
    tx: &Transaction<'_>,
    query: &FeedQuery,
    head: &Head,
    scope: &Scope,
    resolved: &Resolved,
    until_ms: i64,
) -> KnowledgeResult<Vec<Draft>> {
    let mut statement = tx.prepare(
        "SELECT event_id, at_ms, origin -> 'effectiveEnd', origin ->> 'spanId'
         FROM knowledge_growth_events
         WHERE loupist_id = ?1 AND record_id = ?2 AND kind = 'claim_ended'
           AND at_ms <= ?3 AND origin -> 'effectiveEnd' IS NOT NULL",
    )?;
    let rows = statement.query_map(params![query.loupist_id, head.record_id, until_ms], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
        ))
    })?;
    let removed = super::memory_controls::load(tx, &query.loupist_id)?.removed;
    let mut drafts = Vec::new();
    for row in rows {
        let (event_id, at_ms, end, span) = row?;
        if resolved.axis == Axis::Recorded && !in_time(resolved, until_ms, at_ms, query.cutoff) {
            continue;
        }
        let end: EpisodeEnd = serde_json::from_str(&end)?;
        if head
            .claims
            .get(&end.claim)
            .is_none_or(|claim| !scope.matches_claim(claim))
        {
            continue;
        }
        let source = tx
            .query_row(
                "SELECT episode_id, source_record_id, source_revision_id, publication_json
             FROM knowledge_world_episodes
             WHERE loupist_id = ?1 AND world_record_id = ?2 AND span_id = ?3
             ORDER BY created_at_ms DESC, episode_id DESC LIMIT 1",
                params![query.loupist_id, head.record_id, span],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()?;
        let (episode, source_record_id, source_revision_id, publication) =
            source.unwrap_or_default();
        if removed.contains(&source_record_id) {
            continue;
        }
        let publication: Option<TimePoint> = publication
            .map(|raw| serde_json::from_str(&raw))
            .transpose()?;
        let time = match resolved.axis {
            Axis::Recorded => at_ms,
            Axis::Published => {
                if query
                    .cutoff
                    .is_some_and(|cutoff| !date_at_ms(at_ms).is_some_and(|day| day <= cutoff))
                    || !published_in(resolved, publication.as_ref(), query.cutoff)
                {
                    continue;
                }
                publication_sort_ms(publication.as_ref()).unwrap_or(0)
            }
        };
        drafts.push(Draft {
            key: SortKey {
                time,
                tie: event_id,
                kind: ChangeKind::Ended.rank(),
                claim: end.claim.clone(),
                partner: String::new(),
            },
            kind: ChangeKind::Ended,
            recorded_at_ms: Some(at_ms),
            episode_id: (!episode.is_empty()).then_some(episode),
            ending: Some(ending_of(&end)),
            claim_id: end.claim,
            with_claim_id: None,
            source_record_id,
            source_revision_id,
            span_id: span.unwrap_or_default(),
            publication,
            lens_record_id: None,
            lens_title: None,
            section_key: None,
            section_state: None,
            section_claims: Vec::new(),
            hide: false,
        });
    }
    Ok(drafts)
}

#[allow(clippy::too_many_arguments)]
fn status_drafts(
    tx: &Transaction<'_>,
    loupist_id: &str,
    head: &Head,
    scope: &Scope,
    kinds: Option<&BTreeSet<ChangeKind>>,
    axis: Axis,
    resolved: &Resolved,
    until_ms: i64,
    cutoff: Option<NaiveDate>,
    unknown: Option<&BTreeSet<String>>,
) -> KnowledgeResult<Vec<Draft>> {
    if !wants(kinds, ChangeKind::Disputed) && !wants(kinds, ChangeKind::Withdrawn) {
        return Ok(Vec::new());
    }
    let mut statement = tx.prepare(
        "SELECT event_id, at_ms, kind, subject_id, publication_json FROM knowledge_growth_events
         WHERE loupist_id = ?1 AND kind IN ('claim_disputed', 'claim_withdrawn')
           AND record_id = ?2
         ORDER BY at_ms, event_id",
    )?;
    let rows = statement.query_map(params![loupist_id, head.record_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<String>>(4)?,
        ))
    })?;
    let mut drafts = Vec::new();
    for row in rows {
        let (event_id, at_ms, kind, subject, publication) = row?;
        let Some(claim_id) = subject else {
            continue;
        };
        if unknown.is_some_and(|unknown| unknown.contains(&event_id)) {
            continue;
        }
        let kind = if kind == "claim_disputed" {
            ChangeKind::Disputed
        } else {
            ChangeKind::Withdrawn
        };
        if !wants(kinds, kind) {
            continue;
        }
        let Some(claim) = head.claims.get(&claim_id) else {
            continue;
        };
        if !scope.matches_claim(claim) {
            continue;
        }
        // The transition is dated by the witness that wrote it, or by the
        // event itself. The claim's own publication is when it was added.
        let transition = event_transition(tx, loupist_id, publication.as_deref(), at_ms)?;
        if axis == Axis::Published {
            if !published_in(resolved, Some(&transition), cutoff) {
                continue;
            }
        } else if !in_time(resolved, until_ms, at_ms, cutoff) {
            continue;
        }
        let time = if axis == Axis::Recorded {
            at_ms
        } else {
            publication_sort_ms(Some(&transition)).unwrap_or(at_ms)
        };
        drafts.push(Draft {
            // Disputes and withdrawals of one reading come in claim and kind
            // order, not in the order of their random event ids; the event id
            // keeps two rows apart.
            key: SortKey {
                time,
                tie: format!("status:{claim_id}"),
                kind: kind.rank(),
                claim: claim_id.clone(),
                partner: event_id,
            },
            kind,
            recorded_at_ms: Some(at_ms),
            episode_id: None,
            claim_id,
            ending: None,
            with_claim_id: None,
            source_record_id: String::new(),
            source_revision_id: String::new(),
            span_id: String::new(),
            publication: Some(transition),
            lens_record_id: None,
            lens_title: None,
            section_key: None,
            section_state: None,
            section_claims: Vec::new(),
            hide: false,
        });
    }
    Ok(drafts)
}

impl<'a> super::KnowledgeWrite<'a> {
    /// The transaction this write is bound to, for a read that also replays
    /// episodes (`as_of::episodes_as_of`).
    pub(crate) fn transaction(&self) -> &'a Transaction<'a> {
        self.tx
    }
}

impl super::KnowledgeWrite<'_> {
    /// When this Loupist first read each span into the World: the earliest live
    /// episode of it. A backfilled shell's time is the upgrade, not a reading.
    pub(crate) fn first_episode_ms_by_span(
        &self,
        loupist_id: &str,
        world_record_id: &str,
    ) -> KnowledgeResult<BTreeMap<String, i64>> {
        let mut statement = self.tx.prepare(
            "SELECT span_id, MIN(created_at_ms) FROM knowledge_world_episodes
             WHERE loupist_id = ?1 AND world_record_id = ?2 AND backfilled = 0
             GROUP BY span_id",
        )?;
        let rows = statement.query_map(params![loupist_id, world_record_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Status reached by replaying, in order, the dispute and withdrawal events
    /// `known` admits (the cutoff's rule, from the event's publication and time).
    /// `None` means no such event was recorded, so the stored status stands.
    /// A later transition does not erase one the cutoff had already reached.
    pub(crate) fn status_known_by(
        &self,
        loupist_id: &str,
        claim_id: &str,
        known: &mut dyn FnMut(Option<&str>, i64) -> bool,
    ) -> KnowledgeResult<Option<super::ClaimStatus>> {
        let mut statement = self.tx.prepare(
            "SELECT at_ms, kind, publication_json FROM knowledge_growth_events
             WHERE loupist_id = ?1 AND subject_id = ?2
               AND kind IN ('claim_disputed', 'claim_withdrawn')
             ORDER BY at_ms, event_id",
        )?;
        let rows = statement.query_map(params![loupist_id, claim_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })?;
        let mut saw = false;
        let mut status = super::ClaimStatus::Active;
        for row in rows {
            let (at_ms, kind, publication) = row?;
            saw = true;
            if known(publication.as_deref(), at_ms) {
                status = if kind == "claim_disputed" {
                    super::ClaimStatus::Disputed
                } else {
                    super::ClaimStatus::Withdrawn
                };
            }
        }
        Ok(saw.then_some(status))
    }
}

/// When the status change was published: its witness Source, or the event day.
fn event_transition(
    tx: &Transaction<'_>,
    loupist_id: &str,
    publication: Option<&str>,
    at_ms: i64,
) -> KnowledgeResult<TimePoint> {
    if let Some(raw) = publication {
        if let Ok(value) = serde_json::from_str::<Value>(raw) {
            if let Some(record) = value.pointer("/source/recordId").and_then(Value::as_str) {
                if let Ok(meta) = source_meta(tx, loupist_id, record) {
                    if let Some(day) = meta.published {
                        return Ok(TimePoint {
                            precision: super::TimePrecision::Date,
                            value: Some(day.to_string()),
                            approximate: false,
                        });
                    }
                }
            }
        }
    }
    Ok(TimePoint {
        precision: super::TimePrecision::Date,
        value: date_at_ms(at_ms).map(|day| day.to_string()),
        approximate: false,
    })
}

#[allow(clippy::too_many_arguments)]
fn lens_drafts(
    tx: &Transaction<'_>,
    loupist_id: &str,
    head: &Head,
    scope: &Scope,
    resolved: &Resolved,
    until_ms: i64,
    cutoff: Option<NaiveDate>,
    replayed: Option<&LensReplay>,
) -> KnowledgeResult<Vec<Draft>> {
    if cutoff.is_some() {
        let mut drafts = lens_transitions(tx, loupist_id, head, scope, resolved, until_ms, cutoff)?;
        if let Some(replayed) = replayed {
            drafts.retain(|draft| draft.key.time < replayed.from_ms);
            // A filing's own supersede stores no event, so it is no row.
            for transition in replayed
                .transitions
                .iter()
                .filter(|transition| !transition.filed)
            {
                if !in_time(resolved, until_ms, transition.at_ms, cutoff) {
                    continue;
                }
                let in_scope = scope.whole
                    || scope.lenses.contains(&transition.record_id)
                    || transition.claim_ids.iter().any(|id| {
                        scope.claims.contains(id)
                            || head
                                .claims
                                .get(id)
                                .is_some_and(|claim| scope.matches_claim(claim))
                    });
                if !in_scope {
                    continue;
                }
                drafts.push(Draft {
                    key: SortKey {
                        time: transition.at_ms,
                        tie: transition.record_id.clone(),
                        kind: ChangeKind::Lens.rank(),
                        claim: transition.key.clone(),
                        // One section can be superseded and contested at one seal.
                        partner: if transition.contested {
                            "contested".to_owned()
                        } else {
                            "superseded".to_owned()
                        },
                    },
                    kind: ChangeKind::Lens,
                    recorded_at_ms: Some(transition.at_ms),
                    episode_id: None,
                    claim_id: transition.key.clone(),
                    ending: None,
                    with_claim_id: None,
                    source_record_id: String::new(),
                    source_revision_id: String::new(),
                    span_id: String::new(),
                    publication: None,
                    lens_record_id: Some(transition.record_id.clone()),
                    lens_title: Some(transition.title.clone()),
                    section_key: Some(transition.key.clone()),
                    section_state: Some(
                        if transition.contested {
                            "contested"
                        } else {
                            "superseded"
                        }
                        .to_owned(),
                    ),
                    section_claims: transition.claim_ids.clone(),
                    hide: false,
                });
            }
        }
        return Ok(drafts);
    }
    let entries = super::lens::index_entries(tx, loupist_id)?;
    let mut drafts = Vec::new();
    for entry in entries {
        if entry.kind == super::lens_page::LensKind::Principle {
            continue;
        }
        let page = super::lens::read_page(tx, loupist_id, &entry.record_id)?;
        let stored = super::revision::load_revision(tx, &page.revision_id)?;
        let body = super::lens_page::LensBody::from_payload(
            entry.kind,
            super::canonical::from_json(&stored.payload_json)?,
        )?;
        let Some(sections) = body.page().map(|page| page.sections.clone()) else {
            continue;
        };
        for section in sections {
            let (state, since_ms, claim_ids) = match section.state {
                super::lens_page::SectionState::Superseded {
                    since_ms,
                    claim_ids,
                } => ("superseded", since_ms, claim_ids),
                super::lens_page::SectionState::Contested {
                    since_ms,
                    claim_ids,
                } => ("contested", since_ms, claim_ids),
                super::lens_page::SectionState::Current => continue,
            };
            if !scope.whole
                && !scope.lenses.contains(&entry.record_id)
                && !claim_ids.iter().any(|id| {
                    scope.claims.contains(id)
                        || head
                            .claims
                            .get(id)
                            .is_some_and(|claim| scope.matches_claim(claim))
                })
            {
                continue;
            }
            if !in_time(resolved, until_ms, since_ms, cutoff) {
                continue;
            }
            drafts.push(Draft {
                key: SortKey {
                    time: since_ms,
                    tie: entry.record_id.clone(),
                    kind: ChangeKind::Lens.rank(),
                    claim: section.key.as_str().to_owned(),
                    partner: state.to_owned(),
                },
                kind: ChangeKind::Lens,
                recorded_at_ms: Some(since_ms),
                episode_id: None,
                claim_id: section.key.as_str().to_owned(),
                ending: None,
                with_claim_id: None,
                source_record_id: String::new(),
                source_revision_id: String::new(),
                span_id: String::new(),
                publication: None,
                lens_record_id: Some(entry.record_id.clone()),
                lens_title: Some(entry.title.clone()),
                section_key: Some(section.key.as_str().to_owned()),
                section_state: Some(state.to_owned()),
                section_claims: claim_ids,
                hide: false,
            });
        }
    }
    Ok(drafts)
}

/// Superseded and contested sections as they were dated, including one a later
/// repair has put back. The live section state is not the replay.
fn lens_transitions(
    tx: &Transaction<'_>,
    loupist_id: &str,
    head: &Head,
    scope: &Scope,
    resolved: &Resolved,
    until_ms: i64,
    cutoff: Option<NaiveDate>,
) -> KnowledgeResult<Vec<Draft>> {
    let mut statement = tx.prepare(
        "SELECT at_ms, kind, record_id, subject_id, after_summary, origin
         FROM knowledge_growth_events
         WHERE loupist_id = ?1 AND kind IN ('lens_superseded', 'lens_contested')
         ORDER BY at_ms, event_id",
    )?;
    let rows = statement.query_map(params![loupist_id], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(5)?,
        ))
    })?;
    let mut drafts = Vec::new();
    for row in rows {
        let (at_ms, kind, record_id, subject, after, origin) = row?;
        let Some(record_id) = record_id else {
            continue;
        };
        if !in_time(resolved, until_ms, at_ms, cutoff) {
            continue;
        }
        let parsed = origin
            .as_deref()
            .and_then(|raw| serde_json::from_str::<Value>(raw).ok());
        // The display summary is capped at 240 characters, so later section
        // keys live in the stored claim map, not in that summary.
        let keys = lens_transition_keys(subject.as_deref(), after.as_deref(), parsed.as_ref());
        if keys.is_empty() {
            continue;
        }
        let state = if kind == "lens_contested" {
            "contested"
        } else {
            "superseded"
        };
        let title = super::lens::read_page(tx, loupist_id, &record_id)
            .ok()
            .map(|page| page.title);
        let Some(title) = title else {
            continue;
        };
        let flat_ids = parsed
            .as_ref()
            .and_then(|value| value.get("claimIds").cloned())
            .and_then(|value| serde_json::from_value::<Vec<String>>(value).ok())
            .unwrap_or_default();
        for key in keys {
            let claim_ids = parsed
                .as_ref()
                .and_then(|value| value.get("sections"))
                .and_then(|sections| sections.get(&key))
                .cloned()
                .and_then(|value| serde_json::from_value::<Vec<String>>(value).ok())
                .unwrap_or_else(|| flat_ids.clone());
            let in_scope = scope.whole
                || scope.lenses.contains(&record_id)
                || claim_ids.iter().any(|id| {
                    scope.claims.contains(id)
                        || head
                            .claims
                            .get(id)
                            .is_some_and(|claim| scope.matches_claim(claim))
                });
            if !in_scope {
                continue;
            }
            drafts.push(Draft {
                key: SortKey {
                    time: at_ms,
                    tie: record_id.clone(),
                    kind: ChangeKind::Lens.rank(),
                    claim: key.clone(),
                    // One section can be superseded and contested at one seal.
                    partner: state.to_owned(),
                },
                kind: ChangeKind::Lens,
                recorded_at_ms: Some(at_ms),
                episode_id: None,
                claim_id: key.clone(),
                ending: None,
                with_claim_id: None,
                source_record_id: String::new(),
                source_revision_id: String::new(),
                span_id: String::new(),
                publication: None,
                lens_record_id: Some(record_id.clone()),
                lens_title: Some(title.clone()),
                section_key: Some(key),
                section_state: Some(state.to_owned()),
                section_claims: claim_ids,
                hide: false,
            });
        }
    }
    Ok(drafts)
}

fn lens_transition_keys(
    subject: Option<&str>,
    after: Option<&str>,
    origin: Option<&Value>,
) -> Vec<String> {
    if let Some(sections) = origin
        .and_then(|value| value.get("sections"))
        .and_then(Value::as_object)
    {
        if !sections.is_empty() {
            return sections.keys().cloned().collect();
        }
    }
    if let Some(key) = subject.map(str::trim).filter(|key| !key.is_empty()) {
        return vec![key.to_owned()];
    }
    after
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .map(str::to_owned)
        .collect()
}

fn extraction_counts(
    tx: &Transaction<'_>,
    loupist_id: &str,
    scope: &Scope,
    world_id: Option<&str>,
    cutoff: Option<NaiveDate>,
) -> KnowledgeResult<(u32, u32)> {
    let Some(world_id) = world_id else {
        return Ok((0, 0));
    };
    let mut statement = tx.prepare(
        "SELECT status, payload_json, created_at_ms FROM knowledge_graph_outbox
         WHERE loupist_id = ?1 AND world_record_id = ?2
         ORDER BY created_at_ms, job_id",
    )?;
    let rows = statement.query_map(params![loupist_id, world_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;
    let mut by_source: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for row in rows {
        let (status, payload, created_at_ms) = row?;
        if cutoff.is_some_and(|cutoff| date_at_ms(created_at_ms).is_none_or(|day| day > cutoff)) {
            continue;
        }
        let value: Value = serde_json::from_str(&payload).unwrap_or(Value::Null);
        let source = value
            .pointer("/sourceBinding/sourceRecordId")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        if source.is_empty() {
            continue;
        }
        let named = !scope.names.is_empty() && source_names_match(tx, &source, &scope.names)?;
        if !(scope.whole || scope.sources.contains(&source) || named) {
            continue;
        }
        // Later jobs for the same chunk replace an earlier attempt. A retry
        // that completed does not leave the old failure counted. Chunks of one
        // span share its span id, so the chunk index keeps them apart.
        let span = value
            .pointer("/sourceBinding/spanId")
            .and_then(Value::as_str)
            .unwrap_or("source");
        let chunk_index = value
            .pointer("/chunkIndex")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let chunk = format!("{span}:{chunk_index}");
        by_source.entry(source).or_default().insert(chunk, status);
    }
    let mut pending = 0u32;
    let mut failed = 0u32;
    for chunks in by_source.values() {
        let running = chunks
            .values()
            .any(|status| status == super::graph_jobs::JOB_RUNNING);
        let queued = chunks
            .values()
            .any(|status| status == super::graph_jobs::JOB_QUEUED);
        // A failed chunk parks the chunks after it, so the Source is stopped,
        // not still pending, whatever state a later chunk is in.
        let stopped = chunks.values().any(|status| {
            status == super::graph_jobs::JOB_FAILED_EXTRACTION
                || status == super::graph_jobs::JOB_FAILED_BEFORE
                || status == super::graph_jobs::JOB_RECONCILE
                || status == super::graph_jobs::JOB_CANCELLED
        });
        if stopped {
            failed = failed.saturating_add(1);
        } else if running || queued {
            pending = pending.saturating_add(1);
        }
    }
    Ok((pending, failed))
}

fn source_names_match(
    tx: &Transaction<'_>,
    record_id: &str,
    names: &[String],
) -> KnowledgeResult<bool> {
    let payload: Option<String> = tx
        .query_row(
            "SELECT payload_json FROM knowledge_record_revisions WHERE record_id = ?1",
            [record_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(payload) = payload else {
        return Ok(false);
    };
    let value: Value = serde_json::from_str(&payload)?;
    let mut companies = BTreeSet::new();
    collect_companies(&value, &mut companies, 0);
    Ok(names.iter().any(|name| companies.contains(name)))
}

fn collect_companies(value: &Value, companies: &mut BTreeSet<String>, depth: usize) {
    if depth > 12 {
        return;
    }
    let Value::Object(object) = value else {
        if let Value::Array(items) = value {
            for item in items {
                collect_companies(item, companies, depth + 1);
            }
        }
        return;
    };
    for key in [
        "entity_id",
        "entityId",
        "corp_code",
        "cik",
        "symbol",
        "ticker",
        "stock_code",
    ] {
        if let Some(text) = object.get(key).and_then(Value::as_str) {
            companies.insert(fold(text));
        }
    }
    if let Some(company) = object.get("company") {
        if let Some(text) = company.as_str() {
            companies.insert(fold(text));
        }
        if let Some(company) = company.as_object() {
            for key in ["name", "symbol", "ticker", "corp_code", "cik", "stock_code"] {
                if let Some(text) = company.get(key).and_then(Value::as_str) {
                    companies.insert(fold(text));
                }
            }
        }
    }
    for child in object.values() {
        collect_companies(child, companies, depth + 1);
    }
}

fn fold(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii() {
                ch.to_ascii_lowercase()
            } else {
                ch
            }
        })
        .collect()
}

#[derive(Clone, Debug)]
pub(crate) struct BetweenQuery {
    pub loupist_id: String,
    pub to: String,
    pub from: Option<String>,
    pub cutoff: Option<NaiveDate>,
}

/// Claim-level comparison of two Sources. `not_restated` is never an ending.
pub(crate) fn between(
    write: &super::KnowledgeWrite<'_>,
    query: &BetweenQuery,
) -> KnowledgeResult<Value> {
    let tx = write.tx;
    let to = source_meta(tx, &query.loupist_id, &query.to)?;
    // An unseen target's company and form are not a pairing key. Its title
    // and locator stay out of the comparison as well.
    let to_visible = source_known(&to, query.cutoff);
    let from = match &query.from {
        Some(id) => source_meta(tx, &query.loupist_id, id)?,
        None if !to_visible => return Err(invalid("between_needs_from")),
        None => earlier_pair(tx, &query.loupist_id, &to, query.cutoff)?
            .ok_or_else(|| invalid("between_needs_from"))?,
    };
    let mut to_eps = world_episodes::by_source(tx, &query.loupist_id, &to.record_id)?;
    let mut from_eps = world_episodes::by_source(tx, &query.loupist_id, &from.record_id)?;
    // A replay date is the last day a report is known. A later filing, and any
    // episode published after that day, is not part of the comparison.
    if !source_known(&to, query.cutoff) {
        to_eps.clear();
    }
    if !source_known(&from, query.cutoff) {
        from_eps.clear();
    }
    to_eps.retain(|episode| episode_known(episode, query.cutoff));
    from_eps.retain(|episode| episode_known(episode, query.cutoff));
    // Under a cutoff each reading's changes are those of the World that read
    // only what the cutoff knows, as in the feed.
    if let (Some(cutoff), Some(revision_id)) =
        (query.cutoff, world_head_revision(tx, &query.loupist_id)?)
    {
        let (_, replayed) = crate::commands::knowledge::as_of::episodes_as_of(
            write,
            tx,
            &query.loupist_id,
            &revision_id,
            crate::commands::knowledge::as_of::Cutoff(cutoff),
        )?;
        let replayed: BTreeMap<String, Option<EpisodeChanges>> =
            replayed.episodes.into_iter().collect();
        for episodes in [&mut to_eps, &mut from_eps] {
            episodes.retain_mut(|episode| match replayed.get(&episode.episode_id) {
                Some(Some(changes)) => {
                    episode.changes = changes.clone();
                    true
                }
                Some(None) => false,
                None => true,
            });
        }
    }
    let mut to_set = claim_set(&to_eps);
    let mut from_set = claim_set(&from_eps);
    let mut ended: BTreeSet<String> = to_eps
        .iter()
        .flat_map(|episode| episode.changes.ended.iter().map(|end| end.claim.clone()))
        .filter(|id| from_set.contains(id))
        .collect();
    let head = load_head(tx, &query.loupist_id)?;
    let projected = match (query.cutoff, head.as_ref()) {
        (Some(cutoff), Some(head)) if !head.revision_id.is_empty() => {
            Some(crate::commands::knowledge::as_of::world_claims_as_of(
                write,
                &query.loupist_id,
                &head.revision_id,
                crate::commands::knowledge::as_of::Cutoff(cutoff),
            )?)
        }
        _ => None,
    };
    // Groups are classified only from claims the cutoff still shows. A missing
    // id is not filled from the live head: its remaining evidence may be later.
    if let Some(projected) = &projected {
        let known = |id: &String| projected.contains_key(id);
        to_set.retain(known);
        from_set.retain(known);
        ended.retain(known);
    }
    let mut evidence_error = None;
    let mut show = |ids: BTreeSet<String>| -> Vec<Value> {
        ids.into_iter()
            .filter_map(|id| {
                let claim = if let Some(projected) = &projected {
                    projected.get(&id)?
                } else {
                    head.as_ref().and_then(|head| head.claims.get(&id))?
                };
                let mut invalid_at = claim.invalid_at.clone();
                if projected.is_none() {
                    if let Some(cutoff) = query.cutoff {
                        if !published_by(invalid_at.as_ref(), &cutoff.to_string()) {
                            invalid_at = None;
                        }
                    }
                }
                let mut evidence = Vec::new();
                let mut unresolved = false;
                for binding in &claim.evidence {
                    match span_read(tx, &binding.span_id, query.cutoff) {
                        Ok(Some((record, revision))) => evidence.push(json!({
                            "recordId": record,
                            "revisionId": revision,
                            "spanId": binding.span_id,
                        })),
                        Ok(None) => unresolved = true,
                        Err(error) => {
                            unresolved = true;
                            if evidence_error.is_none() {
                                evidence_error = Some(error);
                            }
                        }
                    }
                }
                let mut row = json!({
                    "claimId": claim.id,
                    "proposition": claim.proposition,
                    "relation": claim.relation,
                    "status": claim.status.as_str(),
                    "validAt": claim.valid_at,
                    "invalidAt": invalid_at,
                    "evidence": evidence,
                });
                if unresolved {
                    row["evidenceUnresolved"] = json!(true);
                }
                Some(row)
            })
            .collect()
    };
    let shared = |claim: &GraphClaim| {
        let subjects: BTreeSet<&str> = from_set
            .iter()
            .filter_map(|id| head.as_ref().and_then(|head| head.claims.get(id)))
            .flat_map(|claim| claim.endpoints.iter().map(|end| end.node_id.as_str()))
            .collect();
        claim
            .endpoints
            .iter()
            .any(|end| subjects.contains(end.node_id.as_str()))
    };
    let mut ended_ids = ended.clone();
    ended_ids.retain(|id| {
        head.as_ref()
            .and_then(|head| head.claims.get(id))
            .is_none_or(shared)
    });
    if let Some(projected) = &projected {
        // An ending the projection rejected does not take a visible claim out
        // of restated or not_restated. Those groups are classified after this.
        ended_ids.retain(|id| {
            projected.get(id).is_some_and(|claim| {
                claim.invalid_at.is_some()
                    && (claim
                        .qualifier(super::SUPERSEDED_BY_SPAN)
                        .is_some_and(|span| to_eps.iter().any(|episode| episode.span_id == span))
                        || claim
                            .qualifier(super::SUPERSEDED_BY_CLAIM)
                            .is_some_and(|by| to_set.contains(by)))
            })
        });
    }
    let mut new_ids = BTreeSet::new();
    let mut restated = BTreeSet::new();
    let mut not_restated = BTreeSet::new();
    for id in &to_set {
        let Some(claim) = head.as_ref().and_then(|head| head.claims.get(id)) else {
            continue;
        };
        if !shared(claim) && !from_set.is_empty() {
            continue;
        }
        if ended_ids.contains(id) {
            continue;
        }
        if from_set.contains(id) {
            restated.insert(id.clone());
        } else if to_eps
            .iter()
            .any(|episode| episode.changes.created.iter().any(|created| created == id))
        {
            new_ids.insert(id.clone());
        }
    }
    for id in &from_set {
        if ended_ids.contains(id) || to_set.contains(id) {
            continue;
        }
        not_restated.insert(id.clone());
    }
    let conflicts: Vec<Value> = to_eps
        .iter()
        .flat_map(|episode| episode.changes.conflicts.iter())
        .filter(|conflict| {
            if let Some(projected) = &projected {
                let Some(claim) = projected.get(&conflict.claim) else {
                    return false;
                };
                return projected.contains_key(&conflict.with)
                    && claim.qualifiers.iter().any(|qualifier| {
                        qualifier.key == super::CONFLICTS_WITH_CLAIM
                            && qualifier.value == conflict.with
                    });
            }
            head.as_ref()
                .and_then(|head| head.claims.get(&conflict.claim))
                .is_some_and(shared)
        })
        .map(|conflict| json!({"claimId": conflict.claim, "withClaimId": conflict.with}))
        .collect();
    let mut capped = false;
    let ended_by_to = bound_rows(show(ended_ids), &mut capped);
    let new_in_to = bound_rows(show(new_ids), &mut capped);
    let restated = bound_rows(show(restated), &mut capped);
    let not_restated = bound_rows(show(not_restated), &mut capped);
    let conflicts = bound_rows(conflicts, &mut capped);
    if let Some(error) = evidence_error {
        return Err(error);
    }
    Ok(json!({
        "from": source_side(&from, from_eps.len(), source_known(&from, query.cutoff)),
        "to": source_side(&to, to_eps.len(), source_known(&to, query.cutoff)),
        "ended_by_to": ended_by_to,
        "new_in_to": new_in_to,
        "restated": restated,
        "not_restated": not_restated,
        "conflicts": conflicts,
        "capped": capped,
    }))
}

/// The World revision and the bounded claims a comparison may be cited for.
/// Under a cutoff the claims are the same projection the rows were drawn from.
pub(crate) fn comparison_delivery(
    write: &super::KnowledgeWrite<'_>,
    loupist_id: &str,
    body: &Value,
    cutoff: Option<NaiveDate>,
) -> KnowledgeResult<(String, String, String, Vec<GraphClaim>)> {
    let tx = write.tx;
    let Some(head) = load_head(tx, loupist_id)? else {
        return Ok((String::new(), String::new(), String::new(), Vec::new()));
    };
    let projected = match cutoff {
        Some(day) if !head.revision_id.is_empty() => {
            Some(crate::commands::knowledge::as_of::world_claims_as_of(
                write,
                loupist_id,
                &head.revision_id,
                crate::commands::knowledge::as_of::Cutoff(day),
            )?)
        }
        _ => None,
    };
    let mut ids = BTreeSet::new();
    for key in ["ended_by_to", "new_in_to", "restated", "not_restated"] {
        for row in body[key].as_array().into_iter().flatten() {
            if let Some(id) = row["claimId"].as_str() {
                ids.insert(id.to_owned());
            }
        }
    }
    for row in body["conflicts"].as_array().into_iter().flatten() {
        for key in ["claimId", "withClaimId"] {
            if let Some(id) = row[key].as_str() {
                ids.insert(id.to_owned());
            }
        }
    }
    let claims = ids
        .into_iter()
        .filter_map(|id| {
            if let Some(projected) = &projected {
                projected.get(&id).cloned()
            } else {
                head.claims.get(&id).cloned()
            }
        })
        .collect();
    Ok((
        head.record_id,
        head.revision_id,
        head.revision_digest,
        claims,
    ))
}

fn source_known(meta: &SourceMeta, cutoff: Option<NaiveDate>) -> bool {
    match (cutoff, meta.published) {
        (None, _) => true,
        (Some(cutoff), Some(published)) => published <= cutoff,
        (Some(_), None) => false,
    }
}

fn episode_known(episode: &world_episodes::WorldEpisode, cutoff: Option<NaiveDate>) -> bool {
    let Some(cutoff) = cutoff else {
        return true;
    };
    published_by(episode.publication_time.as_ref(), &cutoff.to_string())
}

fn bound_rows(mut rows: Vec<Value>, capped: &mut bool) -> Vec<Value> {
    if rows.len() > MAX_LIMIT as usize {
        *capped = true;
        rows.truncate(MAX_LIMIT as usize);
    }
    rows
}

#[derive(Clone)]
struct SourceMeta {
    record_id: String,
    title: String,
    published: Option<NaiveDate>,
    companies: BTreeSet<String>,
    forms: BTreeSet<String>,
}

fn source_side(meta: &SourceMeta, episodes: usize, visible: bool) -> Value {
    json!({
        "recordId": meta.record_id,
        "title": visible.then(|| meta.title.clone()),
        "publicationTime": visible.then(|| meta.published.map(|day| day.to_string())).flatten(),
        "episodes": episodes,
    })
}

fn source_meta(
    tx: &Transaction<'_>,
    loupist_id: &str,
    record_id: &str,
) -> KnowledgeResult<SourceMeta> {
    if super::memory_controls::load(tx, loupist_id)?
        .removed
        .contains(record_id)
    {
        return Err(KnowledgeError::NotFound);
    }
    let title: String = tx
        .query_row(
            "SELECT title FROM knowledge_records WHERE record_id = ?1 AND owner_loupist_id = ?2 AND kind = 'source'",
            params![record_id, loupist_id],
            |row| row.get(0),
        )
        .optional()?
        .ok_or(KnowledgeError::NotFound)?;
    let payload: String = tx.query_row(
        "SELECT v.payload_json FROM knowledge_record_heads h
         JOIN knowledge_record_revisions v ON v.revision_id = h.revision_id
         WHERE h.record_id = ?1",
        [record_id],
        |row| row.get(0),
    )?;
    let value: Value = serde_json::from_str(&payload)?;
    let mut companies = BTreeSet::new();
    let mut forms = BTreeSet::new();
    collect_declared(&value, &mut companies, &mut forms, 0);
    let mut bodies =
        tx.prepare("SELECT body FROM knowledge_search_documents WHERE record_id = ?1")?;
    let rows = bodies.query_map([record_id], |row| row.get::<_, String>(0))?;
    for row in rows {
        if let Ok(body) = serde_json::from_str::<Value>(&row?) {
            collect_declared(&body, &mut companies, &mut forms, 0);
        }
    }
    Ok(SourceMeta {
        record_id: record_id.to_owned(),
        title,
        published: publication_day(&value),
        companies,
        forms,
    })
}

fn earlier_pair(
    tx: &Transaction<'_>,
    loupist_id: &str,
    to: &SourceMeta,
    cutoff: Option<NaiveDate>,
) -> KnowledgeResult<Option<SourceMeta>> {
    if to.companies.is_empty() || to.forms.is_empty() {
        return Ok(None);
    }
    let mut statement = tx.prepare(
        "SELECT r.record_id FROM knowledge_records r
         WHERE r.owner_loupist_id = ?1 AND r.kind = 'source' AND r.record_id <> ?2
         ORDER BY r.created_at_ms, r.record_id",
    )?;
    let ids = statement
        .query_map(params![loupist_id, to.record_id], |row| {
            row.get::<_, String>(0)
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut best: Option<SourceMeta> = None;
    for id in ids {
        let meta = match source_meta(tx, loupist_id, &id) {
            Ok(meta) => meta,
            Err(KnowledgeError::NotFound) => continue,
            Err(error) => return Err(error),
        };
        if !source_known(&meta, cutoff) {
            continue;
        }
        let same = meta.companies.intersection(&to.companies).next().is_some()
            && meta.forms.intersection(&to.forms).next().is_some();
        let earlier = match (meta.published, to.published) {
            (Some(left), Some(right)) => left < right,
            _ => false,
        };
        if same
            && earlier
            && best
                .as_ref()
                .is_none_or(|current| meta.published > current.published)
        {
            best = Some(meta);
        }
    }
    Ok(best)
}

/// The latest report for an asset and the report before it, same company and form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AssetPair {
    pub from_id: String,
    pub to_id: String,
    pub pending: bool,
}

pub(crate) fn pair_for_asset(
    tx: &Transaction<'_>,
    loupist_id: &str,
    asset: &AssetRow,
) -> KnowledgeResult<Option<AssetPair>> {
    let names = asset_identity(tx, loupist_id, asset)?;
    if names.is_empty() {
        return Ok(None);
    }
    let mut statement = tx.prepare(
        "SELECT record_id FROM knowledge_records
         WHERE owner_loupist_id = ?1 AND kind = 'source'
         ORDER BY created_at_ms, record_id",
    )?;
    let ids = statement
        .query_map(params![loupist_id], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut filings = Vec::new();
    for id in ids {
        let meta = match source_meta(tx, loupist_id, &id) {
            Ok(meta) => meta,
            Err(KnowledgeError::NotFound) => continue,
            Err(error) => return Err(error),
        };
        let Some(published) = meta.published else {
            continue;
        };
        if meta.forms.is_empty() || meta.companies.is_disjoint(&names) {
            continue;
        }
        filings.push(Filing {
            record_id: meta.record_id,
            published,
            companies: meta.companies,
            forms: meta.forms,
        });
    }
    filings.sort_by(|left, right| {
        right
            .published
            .cmp(&left.published)
            .then(right.record_id.cmp(&left.record_id))
    });
    for index in 0..filings.len() {
        let to_day = filings[index].published;
        let to_id = filings[index].record_id.clone();
        let companies = filings[index].companies.clone();
        let forms = filings[index].forms.clone();
        let from = filings[index + 1..]
            .iter()
            .filter(|other| {
                other.published < to_day
                    && !other.companies.is_disjoint(&companies)
                    && !other.forms.is_disjoint(&forms)
                    && !other.companies.is_disjoint(&names)
            })
            .max_by(|left, right| {
                left.published
                    .cmp(&right.published)
                    .then(left.record_id.cmp(&right.record_id))
            });
        let Some(from) = from else {
            continue;
        };
        let pending = source_is_pending(tx, loupist_id, &to_id)?;
        return Ok(Some(AssetPair {
            from_id: from.record_id.clone(),
            to_id,
            pending,
        }));
    }
    Ok(None)
}

struct Filing {
    record_id: String,
    published: NaiveDate,
    companies: BTreeSet<String>,
    forms: BTreeSet<String>,
}

/// True while a World read of this Source is queued or running.
pub(crate) fn source_is_pending(
    tx: &Transaction<'_>,
    loupist_id: &str,
    source_id: &str,
) -> KnowledgeResult<bool> {
    let mut statement = tx
        .prepare("SELECT status, payload_json FROM knowledge_graph_outbox WHERE loupist_id = ?1")?;
    let rows = statement.query_map(params![loupist_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (status, payload) = row?;
        if status != super::graph_jobs::JOB_QUEUED && status != super::graph_jobs::JOB_RUNNING {
            continue;
        }
        let value: Value = serde_json::from_str(&payload).unwrap_or(Value::Null);
        if value
            .pointer("/sourceBinding/sourceRecordId")
            .and_then(Value::as_str)
            == Some(source_id)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn asset_identity(
    tx: &Transaction<'_>,
    loupist_id: &str,
    asset: &AssetRow,
) -> KnowledgeResult<BTreeSet<String>> {
    let mut names = BTreeSet::new();
    push_name(&mut names, &asset.name);
    if let Some(reference) = &asset.reference {
        for code in [
            reference.fields.ticker.as_deref(),
            reference.fields.figi.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            push_name(&mut names, code);
        }
    }
    let entity_id: Option<String> = match tx.query_row(
        "SELECT entity_id FROM knowledge_asset_pages WHERE loupist_id = ?1 AND asset_id = ?2",
        params![loupist_id, asset.asset_id],
        |row| row.get(0),
    ) {
        Ok(id) => id,
        Err(rusqlite::Error::QueryReturnedNoRows) => None,
        Err(error) => return Err(error.into()),
    };
    if let Some(entity_id) = entity_id {
        if let Some(head) = load_head(tx, loupist_id)? {
            if let Some(node) = head.nodes.get(&entity_id) {
                push_name(&mut names, &node.name);
                for alias in &node.aliases {
                    push_name(&mut names, alias);
                }
            }
        }
    }
    Ok(names)
}

fn push_name(names: &mut BTreeSet<String>, value: &str) {
    let folded = fold(value.trim());
    if !folded.is_empty() {
        names.insert(folded);
    }
}

fn claim_set(episodes: &[world_episodes::WorldEpisode]) -> BTreeSet<String> {
    episodes
        .iter()
        .flat_map(|episode| {
            episode
                .changes
                .created
                .iter()
                .chain(episode.changes.supported.iter())
                .cloned()
        })
        .collect()
}

fn collect_declared(
    value: &Value,
    companies: &mut BTreeSet<String>,
    forms: &mut BTreeSet<String>,
    depth: usize,
) {
    if depth > 12 {
        return;
    }
    match value {
        Value::Object(object) => {
            collect_companies(value, companies, depth);
            for key in ["form", "document_type", "documentType"] {
                if let Some(text) = object.get(key).and_then(Value::as_str) {
                    forms.insert(fold(text));
                }
            }
            for child in object.values() {
                collect_declared(child, companies, forms, depth + 1);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_declared(item, companies, forms, depth + 1);
            }
        }
        _ => {}
    }
}

fn publication_day(value: &Value) -> Option<NaiveDate> {
    let mut best = None;
    fn walk(value: &Value, best: &mut Option<NaiveDate>, depth: usize) {
        if depth > 12 {
            return;
        }
        match value {
            Value::Object(object) => {
                for key in ["publicationTime", "publication_time"] {
                    // The same day `published_by` reads: an instant on the
                    // user's calendar, a date as written.
                    let day = object.get(key).and_then(|time| {
                        match serde_json::from_value::<TimePoint>(time.clone()) {
                            Ok(time) => super::world_filter::publication_day(&time),
                            Err(_) => time.get("value").and_then(Value::as_str).and_then(last_day),
                        }
                    });
                    if let Some(day) = day {
                        if best.is_none_or(|current| day > current) {
                            *best = Some(day);
                        }
                    }
                }
                for child in object.values() {
                    walk(child, best, depth + 1);
                }
            }
            Value::Array(items) => {
                for item in items {
                    walk(item, best, depth + 1);
                }
            }
            _ => {}
        }
    }
    walk(value, &mut best, 0);
    best
}

/// Lineage of one claim from its episodes: what created it, what supported it,
/// what ended it, what it ended, and the claim that ended it.
/// The live lineage. Under a cutoff it is `claim_lineage_replayed`.
pub(crate) fn claim_lineage(
    tx: &Transaction<'_>,
    loupist_id: &str,
    claim_id: &str,
) -> KnowledgeResult<Value> {
    let episodes = world_episodes::for_claim(tx, loupist_id, claim_id)?;
    let mut created = None;
    let mut supported = Vec::new();
    let mut ended = None;
    let mut endings = Vec::new();
    let mut ended_claims = BTreeSet::new();
    let mut ended_rows = BTreeMap::new();
    let mut ended_by = None;
    for episode in episodes {
        let row = episode_line(tx, &episode)?;
        if episode.changes.reopened.iter().any(|id| id == claim_id) {
            if let Some(previous) = ended.take() {
                endings.push(previous);
            }
            ended_by = None;
        }
        for reopened in &episode.changes.reopened {
            if ended_claims.remove(reopened) {
                if let Some(prior) = ended_rows.remove(reopened) {
                    endings.push(prior);
                }
            }
        }
        if episode.changes.created.iter().any(|id| id == claim_id) && created.is_none() {
            created = Some(row.clone());
        }
        if episode.changes.supported.iter().any(|id| id == claim_id) {
            supported.push(row.clone());
        }
        if let Some(end) = episode
            .changes
            .ended
            .iter()
            .find(|end| end.claim == claim_id)
        {
            ended_by = end.by.clone();
            let mut row = row.clone();
            row["end"] = json!(end.end);
            row["basis"] = json!(match end.basis {
                world_episodes::EndBasis::Stated => "stated",
                world_episodes::EndBasis::Reference => "reference",
            });
            ended = Some(row);
        }
        for end in &episode.changes.ended {
            if end.by.as_deref() == Some(claim_id) {
                ended_claims.insert(end.claim.clone());
                let mut prior = row.clone();
                prior["claimId"] = json!(end.claim);
                prior["end"] = json!(end.end);
                ended_rows.insert(end.claim.clone(), prior);
            }
        }
    }
    Ok(json!({
        "created": created,
        "supported": supported,
        "ended": ended,
        "endings": endings,
        "endedClaims": ended_claims.into_iter().collect::<Vec<_>>(),
        "endedByClaimId": ended_by,
    }))
}

/// A claim's ending as a projection shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ShownEnd {
    pub end: TimePoint,
    pub by: Option<String>,
    pub basis: String,
    /// The span that established it, when tagged.
    pub span: Option<String>,
}

/// Lineage under a cutoff, replayed from the one projection. `episodes` are the
/// readings the cutoff knows, in reading order, with their shown dates; after
/// each one in `moves` (a reading that can change the claim's ending),
/// `state_at(ms)` is the projection of the World after the readings up to
/// `ms`: each claim's shown ending. Rows follow the live lineage: an ending
/// becomes `ended` where it first shows, a reopen moves it to `endings`.
pub(crate) fn claim_lineage_replayed(
    tx: &Transaction<'_>,
    claim_id: &str,
    episodes: &[world_episodes::WorldEpisode],
    moves: &BTreeSet<String>,
    mut state_at: impl FnMut(i64) -> KnowledgeResult<BTreeMap<String, ShownEnd>>,
) -> KnowledgeResult<Value> {
    let mut created = None;
    let mut supported = Vec::new();
    let mut ended: Option<Value> = None;
    let mut ended_by: Option<String> = None;
    let mut endings = Vec::new();
    let mut ended_rows: BTreeMap<String, Value> = BTreeMap::new();
    let mut shown: BTreeMap<String, ShownEnd> = BTreeMap::new();
    for episode in episodes {
        let row = episode_line(tx, episode)?;
        let next = if moves.contains(&episode.episode_id) {
            Some(state_at(episode.created_at_ms)?)
        } else {
            None
        };
        if let Some(next) = &next {
            // Reopened here: it showed an end before this reading and none after.
            if shown.contains_key(claim_id) && !next.contains_key(claim_id) {
                if let Some(previous) = ended.take() {
                    endings.push(previous);
                }
                ended_by = None;
            }
            let reopened: Vec<String> = ended_rows
                .keys()
                .filter(|id| !next.contains_key(*id))
                .cloned()
                .collect();
            for id in reopened {
                if let Some(prior) = ended_rows.remove(&id) {
                    endings.push(prior);
                }
            }
        }
        let touches = episode.changes.created.iter().any(|id| id == claim_id)
            || episode.changes.supported.iter().any(|id| id == claim_id);
        if touches {
            if created.is_none() {
                created = Some(row.clone());
            } else {
                supported.push(row.clone());
            }
        }
        if let Some(next) = next {
            if let Some(end) = next.get(claim_id).filter(|_| !shown.contains_key(claim_id)) {
                let mut line = row.clone();
                line["end"] = json!(end.end);
                line["basis"] = json!(end.basis);
                ended = Some(line);
                ended_by = end.by.clone();
            }
            for (id, end) in &next {
                if end.by.as_deref() == Some(claim_id) && !shown.contains_key(id) {
                    let mut prior = row.clone();
                    prior["claimId"] = json!(id);
                    prior["end"] = json!(end.end);
                    ended_rows.insert(id.clone(), prior);
                }
            }
            shown = next;
        }
    }
    Ok(json!({
        "created": created,
        "supported": supported,
        "ended": ended,
        "endings": endings,
        "endedClaims": ended_rows.keys().cloned().collect::<Vec<_>>(),
        "endedByClaimId": ended_by,
    }))
}

/// Publication stored on the Source revision for one span. A later correction
/// of that capture replaces the date an episode row still carries.
pub(crate) fn span_publication(
    tx: &Transaction<'_>,
    revision_id: &str,
    span_id: &str,
) -> KnowledgeResult<Option<TimePoint>> {
    if revision_id.is_empty() || span_id.is_empty() {
        return Ok(None);
    }
    let payload: Option<String> = tx
        .query_row(
            "SELECT payload_json FROM knowledge_record_revisions WHERE revision_id = ?1",
            [revision_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(payload) = payload else {
        return Ok(None);
    };
    let Ok(value) = serde_json::from_str::<Value>(&payload) else {
        return Ok(None);
    };
    let Some(captures) = value.pointer("/source/captures").and_then(Value::as_array) else {
        return Ok(None);
    };
    for capture in captures {
        let holds = capture
            .pointer("/representations")
            .and_then(Value::as_array)
            .is_some_and(|representations| {
                representations.iter().any(|representation| {
                    representation
                        .pointer("/spans")
                        .and_then(Value::as_array)
                        .is_some_and(|spans| {
                            spans.iter().any(|span| {
                                span.get("spanId").and_then(Value::as_str) == Some(span_id)
                            })
                        })
                })
            });
        if !holds {
            continue;
        }
        let publication = capture
            .get("publicationTime")
            .cloned()
            .map(serde_json::from_value)
            .transpose()?;
        return Ok(publication);
    }
    Ok(None)
}

fn episode_line(
    tx: &Transaction<'_>,
    episode: &world_episodes::WorldEpisode,
) -> KnowledgeResult<Value> {
    let title: Option<String> = tx
        .query_row(
            "SELECT title FROM knowledge_records WHERE record_id = ?1",
            [&episode.source_record_id],
            |row| row.get(0),
        )
        .optional()?;
    let publication = span_publication(tx, &episode.source_revision_id, &episode.span_id)?
        .or(episode.publication_time.clone());
    Ok(json!({
        "episodeId": episode.episode_id,
        "sourceTitle": title,
        "publicationTime": publication,
        "referenceBasis": episode.reference_basis.map(|basis| match basis {
            world_episodes::ReferenceBasis::Published => "published",
            world_episodes::ReferenceBasis::Received => "received",
        }),
        "readArgs": {
            "scope": "source",
            "recordId": episode.source_record_id,
            "revisionId": episode.source_revision_id,
            "spanId": episode.span_id,
        }
    }))
}

/// `(ended, end)` for today. A current claim is `(false, None)`.
pub(crate) fn ended_fields(claim: &GraphClaim) -> (bool, Option<TimePoint>) {
    let today = crate::local_time::today().to_string();
    let now = TimePoint {
        precision: super::TimePrecision::Date,
        value: Some(today),
        approximate: false,
    };
    if claim.is_current(&now) {
        (false, None)
    } else {
        (true, claim.invalid_at.clone())
    }
}

/// A claim the cutoff day has already passed is ended, with its end.
pub(crate) fn ended_mark(claim: &GraphClaim) -> Option<Value> {
    let (ended, end) = ended_fields(claim);
    if !ended {
        return None;
    }
    Some(json!({"ended": true, "end": end}))
}

/// Titles are filled when the page is rendered so a removed Source stays blank.
pub(crate) fn fill_source_titles(tx: &Transaction<'_>, body: &mut Value) -> KnowledgeResult<()> {
    let Some(changes) = body.get_mut("changes").and_then(Value::as_array_mut) else {
        return Ok(());
    };
    for change in changes {
        let Some(record_id) = change
            .pointer("/source/readArgs/recordId")
            .and_then(Value::as_str)
        else {
            continue;
        };
        if record_id.is_empty() {
            continue;
        }
        let title: Option<String> = tx
            .query_row(
                "SELECT title FROM knowledge_records WHERE record_id = ?1",
                [record_id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(title) = title {
            change["source"]["title"] = Value::String(title);
        }
    }
    Ok(())
}

#[cfg(any(test, feature = "e2e"))]
impl super::KnowledgeWrite<'_> {
    pub(crate) fn insert_world_episode(
        &self,
        episode: &world_episodes::WorldEpisode,
    ) -> KnowledgeResult<()> {
        world_episodes::insert(self.tx, episode)
    }

    /// Fixture and tests: one section becomes Superseded and publishes a revision.
    pub(crate) fn supersede_section(
        &self,
        loupist_id: &str,
        record_id: &str,
        section_key: &str,
        claim_ids: Vec<String>,
        at_ms: i64,
    ) -> KnowledgeResult<()> {
        let record = super::authority::require_record(self.tx, loupist_id, record_id)?;
        let kind = super::lens::lens_kind(self.tx, loupist_id, record_id)?;
        let revision_id = record
            .head_revision_id
            .clone()
            .ok_or(KnowledgeError::NotFound)?;
        let stored = super::revision::load_revision(self.tx, &revision_id)?;
        let mut body = super::lens_page::LensBody::from_payload(
            kind,
            super::canonical::from_json(&stored.payload_json)?,
        )?;
        let key = super::lens_page::SectionKey::new(section_key)?;
        let page = match &mut body {
            super::lens_page::LensBody::Perspective(page)
            | super::lens_page::LensBody::Method(page)
            | super::lens_page::LensBody::Subject(page)
            | super::lens_page::LensBody::Idea(page, _) => page,
            super::lens_page::LensBody::Principle(_) => {
                return Err(KnowledgeError::Invalid("a principle has no sections"))
            }
        };
        let section = page
            .sections
            .iter_mut()
            .find(|section| section.key == key)
            .ok_or(KnowledgeError::NotFound)?;
        section.state = super::lens_page::SectionState::Superseded {
            since_ms: at_ms,
            claim_ids: claim_ids.clone(),
        };
        section.updated_at_ms = at_ms;
        let payload = body.to_payload()?;
        let published = super::revision::publish_revision(
            self.tx,
            &record,
            loupist_id,
            Some(&revision_id),
            &payload,
            &[],
            at_ms,
        )?;
        super::search_index::index_payload(
            self.tx,
            None,
            record_id,
            &published.revision_id,
            super::RecordKind::Lens,
            &record.title,
            &payload,
        )?;
        self.tx.execute(
            "UPDATE knowledge_lens_metadata SET updated_at_ms = ?1 WHERE record_id = ?2",
            params![at_ms, record_id],
        )?;
        let mut event = super::growth::GrowthEvent::new(
            loupist_id,
            record_id,
            at_ms,
            "lens_superseded",
            "processing",
            &record.title,
        );
        event.subject_id = Some(section_key.to_owned());
        event.origin = Some(json!({"claimIds": claim_ids}));
        event.insert(self.tx)?;
        Ok(())
    }

    /// Tests: one Source publication through the path the extraction worker
    /// uses, so its episode, growth events and Lens follow-ups are the ones
    /// production writes (the cutoff and taint oracles ingest through it).
    #[cfg(test)]
    pub(crate) fn publish_world_from_source(
        &self,
        request: super::SealWorld,
        source: &super::RetainedSource,
        title: &str,
        episode: world_episodes::WorldEpisode,
        temporal: super::WorldTemporal,
    ) -> KnowledgeResult<super::WorldRevision> {
        super::world::seal_world_from_source(
            self.tx,
            request,
            None,
            None,
            Some(super::growth::WorldGrowthSource {
                record_id: source.record_id.clone(),
                revision_id: source.revision_id.clone(),
                source_id: source.source_id.clone(),
                title: title.to_owned(),
            }),
            &[],
            episode,
            Some(temporal),
        )
    }

    /// Fixture: put a superseded section back. The earlier transition stays.
    /// Only the unit tests call this. The e2e seed uses `supersede_section`.
    #[cfg(test)]
    pub(crate) fn restore_section(
        &self,
        loupist_id: &str,
        record_id: &str,
        section_key: &str,
        at_ms: i64,
    ) -> KnowledgeResult<()> {
        let record = super::authority::require_record(self.tx, loupist_id, record_id)?;
        let kind = super::lens::lens_kind(self.tx, loupist_id, record_id)?;
        let revision_id = record
            .head_revision_id
            .clone()
            .ok_or(KnowledgeError::NotFound)?;
        let stored = super::revision::load_revision(self.tx, &revision_id)?;
        let mut body = super::lens_page::LensBody::from_payload(
            kind,
            super::canonical::from_json(&stored.payload_json)?,
        )?;
        let key = super::lens_page::SectionKey::new(section_key)?;
        let page = match &mut body {
            super::lens_page::LensBody::Perspective(page)
            | super::lens_page::LensBody::Method(page)
            | super::lens_page::LensBody::Subject(page)
            | super::lens_page::LensBody::Idea(page, _) => page,
            super::lens_page::LensBody::Principle(_) => {
                return Err(KnowledgeError::Invalid("a principle has no sections"))
            }
        };
        let section = page
            .sections
            .iter_mut()
            .find(|section| section.key == key)
            .ok_or(KnowledgeError::NotFound)?;
        section.state = super::lens_page::SectionState::Current;
        section.updated_at_ms = at_ms;
        let payload = body.to_payload()?;
        super::revision::publish_revision(
            self.tx,
            &record,
            loupist_id,
            Some(&revision_id),
            &payload,
            &[],
            at_ms,
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::assets::wire::{
        AssetReference, AssetReferenceFields, AssetReferenceOrigin,
    };
    use crate::knowledge::model::world_episodes::{
        EndBasis, EpisodeConflict, EpisodeEnd, EpisodeModel, ReferenceBasis, WorldEpisode,
    };
    use crate::knowledge::{
        Authorship, BindingRole, ClaimEndpoint, ClaimStatus, Coverage, CreateRecord,
        DependencyRole, EvidenceBinding, EvidenceRevision, GraphNode, KnowledgeStore, RecordKind,
        RepresentationRole, RetainMaterial, RetainNewSource, SealWorld, Selector, SourceClass,
        TimePrecision,
    };

    fn day(value: &str) -> TimePoint {
        TimePoint {
            precision: TimePrecision::Date,
            value: Some(value.into()),
            approximate: false,
        }
    }

    fn claim(id: &str, span: &str) -> GraphClaim {
        GraphClaim {
            id: id.into(),
            proposition: format!("proposition {id}"),
            attribution: "fixture".into(),
            relation: "reports".into(),
            endpoints: vec![ClaimEndpoint {
                node_id: "node-a".into(),
                role: "subject".into(),
            }],
            qualifiers: Vec::new(),
            evidence: vec![EvidenceBinding {
                span_id: span.into(),
                role: BindingRole::ExtractedFrom,
                attribution: "fixture".into(),
                figure_check: None,
                passage: None,
            }],
            status: ClaimStatus::Active,
            recorded_at_ms: Some(1_000),
            publication_time: Some(day("2024-01-01")),
            effective_time: None,
            valid_at: None,
            invalid_at: None,
            expired_at: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn episode(
        id: &str,
        world: &str,
        source: &str,
        revision: &str,
        span: &str,
        at: i64,
        changes: EpisodeChanges,
        published: &str,
    ) -> WorldEpisode {
        WorldEpisode {
            episode_id: id.into(),
            loupist_id: "owner".into(),
            world_record_id: world.into(),
            job_id: format!("job-{id}"),
            sealed_revision_id: format!("rev-{id}"),
            previous_revision_id: None,
            source_record_id: source.into(),
            source_revision_id: revision.into(),
            span_id: span.into(),
            chunk_index: Some(0),
            chunk_count: Some(1),
            char_start: Some(0),
            char_end: Some(4),
            reference_time: Some(published.into()),
            reference_basis: Some(ReferenceBasis::Published),
            publication_time: Some(day(published)),
            model: Some(EpisodeModel {
                model_profile: "fixture".into(),
                thinking_level: "low".into(),
            }),
            changes,
            backfilled: false,
            created_at_ms: at,
        }
    }

    fn query(since: Since, axis: Axis, limit: u32) -> FeedQuery {
        FeedQuery {
            loupist_id: "owner".into(),
            since,
            axis,
            axis_explicit: true,
            about: Vec::new(),
            kinds: None,
            limit,
            first_run: false,
            cutoff: None,
        }
    }

    fn ids(body: &Value) -> Vec<String> {
        body["changes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["claimId"].as_str().unwrap().to_owned())
            .collect()
    }

    #[test]
    fn pages_keep_every_row_once_when_publications_arrive_between_them() {
        let dir = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(dir.path().join("knowledge.sqlite")).unwrap();
        let retained = store
            .retain_new_source(RetainNewSource {
                origin: None,
                loupist_id: "owner".into(),
                at_ms: 1,
                title: "Filing".into(),
                source_class: SourceClass::PublisherDocument,
                origin_locator: "fixture:filing".into(),
                provider_identity: "fixture".into(),
                authorship: Authorship::Publisher,
                coverage: Coverage::IncludedFull,
                origin_chat_id: None,
                request_ref: None,
                declared_digest: None,
                material: Some(RetainMaterial {
                    media_type: "text/plain".into(),
                    role: RepresentationRole::Original,
                    document_id: "filing".into(),
                    parser_version: None,
                    parent_representation_id: None,
                    input_span_ids: Vec::new(),
                    selector: Selector::ByteRange { start: 0, end: 4 },
                    context_note: "filing".into(),
                    bytes: b"text".to_vec(),
                }),
                publication_time: Some(day("2024-06-01")),
            })
            .unwrap();
        let span = retained.span_id.clone().unwrap();
        let claims = ["c1", "c2", "c3", "c4", "c5", "c-new"]
            .into_iter()
            .map(|id| claim(id, &span))
            .collect();
        let (world_id, revision_id) = store
            .transact(|write| {
                let record = write.create_record(CreateRecord {
                    loupist_id: "owner".into(),
                    kind: RecordKind::World,
                    title: "World".into(),
                    at_ms: 1,
                })?;
                let sealed = write.seal_world(SealWorld {
                    loupist_id: "owner".into(),
                    record_id: record.record_id.clone(),
                    expected_revision_id: None,
                    at_ms: 5_000,
                    nodes: vec![GraphNode {
                        id: "node-a".into(),
                        name: "Node A".into(),
                        aliases: Vec::new(),
                        description: String::new(),
                        kind: "company".into(),
                        source_role: "subject".into(),
                        properties: BTreeMap::new(),
                    }],
                    claims,
                    annotations: Vec::new(),
                    dependencies: vec![EvidenceRevision {
                        record_id: retained.record_id.clone(),
                        revision_digest: retained.revision_digest.clone(),
                        role: DependencyRole::WorldSource,
                    }],
                })?;
                for (id, at) in [("c1", 1_000), ("c2", 2_000), ("c3", 3_000), ("c4", 4_000)] {
                    write.insert_world_episode(&episode(
                        id,
                        &record.record_id,
                        &retained.record_id,
                        &retained.revision_id,
                        &span,
                        at,
                        EpisodeChanges {
                            created: vec![id.into()],
                            ..EpisodeChanges::default()
                        },
                        "2024-06-01",
                    ))?;
                }
                Ok((record.record_id, sealed.revision_id))
            })
            .unwrap();
        let mut request = query(
            Since::Millis {
                ms: 0,
                inclusive: true,
            },
            Axis::Recorded,
            2,
        );
        let first = store
            .transact(|write| {
                let collected = collect(write.tx, &request)?;
                let mut body = finish(collected).body;
                fill_source_titles(write.tx, &mut body)?;
                Ok(body)
            })
            .unwrap();
        assert_eq!(ids(&first), ["c1", "c2"]);
        assert_eq!(first["capped"], true);
        assert_eq!(first["changes"][0]["source"]["title"], "Filing");
        store
            .transact(|write| {
                write.insert_world_episode(&episode(
                    "c5",
                    &world_id,
                    &retained.record_id,
                    &retained.revision_id,
                    &span,
                    2_500,
                    EpisodeChanges {
                        created: vec!["c5".into()],
                        ..EpisodeChanges::default()
                    },
                    "2024-06-01",
                ))
            })
            .unwrap();
        request.since = Since::Cursor(first["next"].as_str().unwrap().to_owned());
        let second = store
            .transact(|write| Ok(finish(collect(write.tx, &request)?).body))
            .unwrap();
        assert_eq!(ids(&second), ["c5", "c3"]);
        request.since = Since::Cursor(second["next"].as_str().unwrap().to_owned());
        let third = store
            .transact(|write| Ok(finish(collect(write.tx, &request)?).body))
            .unwrap();
        assert_eq!(ids(&third), ["c4"]);
        assert_eq!(third["capped"], false);
        let seen = [ids(&first), ids(&second), ids(&third)].concat();
        assert_eq!(seen, ["c1", "c2", "c5", "c3", "c4"]);
        store
            .transact(|write| {
                write.seal_world(SealWorld {
                    loupist_id: "owner".into(),
                    record_id: world_id.clone(),
                    expected_revision_id: Some(revision_id),
                    at_ms: 9_000,
                    nodes: vec![GraphNode {
                        id: "node-a".into(),
                        name: "Node A".into(),
                        aliases: Vec::new(),
                        description: String::new(),
                        kind: "company".into(),
                        source_role: "subject".into(),
                        properties: BTreeMap::new(),
                    }],
                    claims: ["c1", "c2", "c3", "c4", "c5", "c-new"]
                        .into_iter()
                        .map(|id| claim(id, &span))
                        .collect(),
                    annotations: Vec::new(),
                    dependencies: vec![EvidenceRevision {
                        record_id: retained.record_id.clone(),
                        revision_digest: retained.revision_digest,
                        role: DependencyRole::WorldSource,
                    }],
                })?;
                write.insert_world_episode(&episode(
                    "c-new",
                    &world_id,
                    &retained.record_id,
                    &retained.revision_id,
                    &span,
                    9_000,
                    EpisodeChanges {
                        created: vec!["c-new".into()],
                        ..EpisodeChanges::default()
                    },
                    "2024-07-01",
                ))
            })
            .unwrap();
        request.since = Since::Cursor(third["next"].as_str().unwrap().to_owned());
        let resumed = store
            .transact(|write| Ok(finish(collect(write.tx, &request)?).body))
            .unwrap();
        assert_eq!(ids(&resumed), ["c-new"]);
        request.since = Since::Cursor(resumed["next"].as_str().unwrap().to_owned());
        let again = store
            .transact(|write| Ok(finish(collect(write.tx, &request)?).body))
            .unwrap();
        assert!(ids(&again).is_empty());
        let _ = world_id;
    }

    #[test]
    fn bounds_axis_cursor_and_backfill_time() {
        let dir = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(dir.path().join("knowledge.sqlite")).unwrap();
        let retained = store
            .retain_new_source(RetainNewSource {
                origin: None,
                loupist_id: "owner".into(),
                at_ms: 1,
                title: "Filing".into(),
                source_class: SourceClass::PublisherDocument,
                origin_locator: "fixture:filing".into(),
                provider_identity: "fixture".into(),
                authorship: Authorship::Publisher,
                coverage: Coverage::IncludedFull,
                origin_chat_id: None,
                request_ref: None,
                declared_digest: None,
                material: Some(RetainMaterial {
                    media_type: "text/plain".into(),
                    role: RepresentationRole::Original,
                    document_id: "filing".into(),
                    parser_version: None,
                    parent_representation_id: None,
                    input_span_ids: Vec::new(),
                    selector: Selector::ByteRange { start: 0, end: 4 },
                    context_note: "filing".into(),
                    bytes: b"text".to_vec(),
                }),
                publication_time: Some(day("2020-01-01")),
            })
            .unwrap();
        let span = retained.span_id.clone().unwrap();
        let mut kept = claim("kept", &span);
        kept.publication_time = Some(day("2024-03-01"));
        let world_id = store
            .transact(|write| {
                let record = write.create_record(CreateRecord {
                    loupist_id: "owner".into(),
                    kind: RecordKind::World,
                    title: "World".into(),
                    at_ms: 1,
                })?;
                write.seal_world(SealWorld {
                    loupist_id: "owner".into(),
                    record_id: record.record_id.clone(),
                    expected_revision_id: None,
                    at_ms: 5_000,
                    nodes: vec![GraphNode {
                        id: "node-a".into(),
                        name: "Node A".into(),
                        aliases: Vec::new(),
                        description: String::new(),
                        kind: "company".into(),
                        source_role: "subject".into(),
                        properties: BTreeMap::new(),
                    }],
                    claims: vec![kept],
                    annotations: Vec::new(),
                    dependencies: vec![EvidenceRevision {
                        record_id: retained.record_id.clone(),
                        revision_digest: retained.revision_digest.clone(),
                        role: DependencyRole::WorldSource,
                    }],
                })?;
                write.insert_world_episode(&episode(
                    "live",
                    &record.record_id,
                    &retained.record_id,
                    &retained.revision_id,
                    &span,
                    4_000,
                    EpisodeChanges {
                        created: vec!["kept".into()],
                        ended: vec![EpisodeEnd {
                            claim: "kept".into(),
                            by: Some("ender".into()),
                            end: day("2024-03-01"),
                            basis: EndBasis::Stated,
                        }],
                        conflicts: vec![EpisodeConflict {
                            claim: "kept".into(),
                            with: "other".into(),
                        }],
                        ..EpisodeChanges::default()
                    },
                    "2024-03-01",
                ))?;
                write.tx.execute(
                    "INSERT INTO knowledge_world_episodes (
                        episode_id, loupist_id, world_record_id, job_id, sealed_revision_id,
                        source_record_id, source_revision_id, span_id, publication_json,
                        changes_json, backfilled, created_at_ms
                     ) VALUES (
                        'eps_backfill', 'owner', ?1, 'job-backfill', 'rev-backfill',
                        ?2, ?3, ?4, ?5, '{\"created\":[\"kept\"]}', 1, 9000000000000
                     )",
                    params![
                        record.record_id,
                        retained.record_id,
                        retained.revision_id,
                        span,
                        serde_json::to_string(&day("2020-01-01")).unwrap(),
                    ],
                )?;
                write.tx.execute(
                    "INSERT INTO knowledge_graph_outbox (
                        job_id, loupist_id, world_record_id, world_revision_id, status,
                        payload_json, payload_digest, created_at_ms
                     ) VALUES
                     ('job-q', 'owner', ?1, 'rev', 'queued', '{\"sourceBinding\":{\"sourceRecordId\":\"rec-q\"}}', 'd', 1),
                     ('job-f', 'owner', ?1, 'rev', 'failed_before_mutation', '{\"sourceBinding\":{\"sourceRecordId\":\"rec-f\"}}', 'd', 2)",
                    params![record.record_id],
                )?;
                Ok(record.record_id)
            })
            .unwrap();
        let cases = [
            (
                "limit zero",
                query(
                    Since::Millis {
                        ms: 0,
                        inclusive: true,
                    },
                    Axis::Recorded,
                    0,
                ),
                true,
            ),
            (
                "limit past fifty",
                query(
                    Since::Millis {
                        ms: 0,
                        inclusive: true,
                    },
                    Axis::Recorded,
                    51,
                ),
                true,
            ),
        ];
        for (name, request, rejects) in cases {
            let error = store
                .transact(|write| collect(write.tx, &request))
                .expect_err(name);
            assert_eq!(error_code(&error), Some("change_limit_invalid"), "{name}");
            assert!(rejects);
        }
        let mut too_many = query(
            Since::Millis {
                ms: 0,
                inclusive: true,
            },
            Axis::Recorded,
            30,
        );
        too_many.about = (0..17)
            .map(|index| AboutRef::Node {
                id: format!("n{index}"),
            })
            .collect();
        let error = store
            .transact(|write| collect(write.tx, &too_many))
            .unwrap_err();
        assert_eq!(error_code(&error), Some("change_about_invalid"));
        let page = store
            .transact(|write| {
                Ok(finish(collect(
                    write.tx,
                    &query(
                        Since::Millis {
                            ms: 0,
                            inclusive: true,
                        },
                        Axis::Recorded,
                        30,
                    ),
                )?)
                .body)
            })
            .unwrap();
        assert_eq!(ids(&page), ["kept", "kept", "kept"]);
        assert_eq!(page["pending"], 1);
        assert_eq!(page["failed"], 1);
        let future = NaiveDate::from_ymd_opt(2026, 11, 25)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp_millis();
        store
            .transact(|write| {
                write.tx.execute(
                    "INSERT INTO knowledge_graph_outbox (
                        job_id, loupist_id, world_record_id, world_revision_id, status,
                        payload_json, payload_digest, created_at_ms
                     ) VALUES (
                        'job-later', 'owner', ?1, 'rev', 'queued',
                        '{\"sourceBinding\":{\"sourceRecordId\":\"rec-later\"}}', 'd', ?2
                     )",
                    params![&world_id, future],
                )?;
                Ok(())
            })
            .unwrap();
        let mut historical = query(
            Since::Millis {
                ms: 0,
                inclusive: true,
            },
            Axis::Recorded,
            30,
        );
        historical.cutoff = Some(NaiveDate::from_ymd_opt(2024, 1, 1).unwrap());
        let replay = store
            .transact(|write| Ok(finish(collect(write.tx, &historical)?).body))
            .unwrap();
        assert_eq!(
            replay["pending"], 1,
            "a job queued after the replay date does not count"
        );
        assert!(page["next"].as_str().unwrap().starts_with(CURSOR_PREFIX));
        let recorded_later = store
            .transact(|write| {
                Ok(finish(collect(
                    write.tx,
                    &query(
                        Since::Date(NaiveDate::from_ymd_opt(2024, 6, 1).unwrap()),
                        Axis::Recorded,
                        30,
                    ),
                )?)
                .body)
            })
            .unwrap();
        assert!(
            ids(&recorded_later).is_empty(),
            "backfill created_at_ms is not when it was learned"
        );
        let published_old = store
            .transact(|write| {
                Ok(finish(collect(
                    write.tx,
                    &query(
                        Since::Date(NaiveDate::from_ymd_opt(2020, 1, 1).unwrap()),
                        Axis::Published,
                        30,
                    ),
                )?)
                .body)
            })
            .unwrap();
        let backfill = published_old["changes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["episodeId"] == "eps_backfill")
            .unwrap();
        assert!(backfill["recordedAtMs"].is_null());
        let published_after = store
            .transact(|write| {
                Ok(finish(collect(
                    write.tx,
                    &query(
                        Since::Date(NaiveDate::from_ymd_opt(2021, 1, 1).unwrap()),
                        Axis::Published,
                        30,
                    ),
                )?)
                .body)
            })
            .unwrap();
        assert!(published_after["changes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["episodeId"] != "eps_backfill"));
        let mut other_axis = query(
            Since::Cursor(page["next"].as_str().unwrap().to_owned()),
            Axis::Published,
            30,
        );
        other_axis.axis_explicit = true;
        let error = store
            .transact(|write| collect(write.tx, &other_axis))
            .unwrap_err();
        assert_eq!(error_code(&error), Some("cursor_axis_mismatch"));
        let lens = store
            .file_method(
                "owner",
                "Change page",
                "The supply ended.",
                &crate::knowledge::test_support::turn_origin("chat", "msg"),
                4_000,
            )
            .unwrap();
        store
            .transact(|write| {
                write.supersede_section(
                    "owner",
                    &lens.record_id,
                    "steps",
                    vec!["kept".into()],
                    4_500,
                )
            })
            .unwrap();
        let with_lens = store
            .transact(|write| {
                Ok(finish(collect(
                    write.tx,
                    &query(
                        Since::Millis {
                            ms: 0,
                            inclusive: true,
                        },
                        Axis::Recorded,
                        30,
                    ),
                )?)
                .body)
            })
            .unwrap();
        assert!(with_lens["lens"].as_array().unwrap().iter().any(|row| {
            row["state"] == "superseded"
                && row["sectionKey"] == "steps"
                && row["recordedAtMs"].as_i64() == Some(4_500)
        }));
        let _ = world_id;
    }

    #[test]
    fn a_replay_resume_keeps_the_days_after_the_previous_cutoff() {
        let dir = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(dir.path().join("knowledge.sqlite")).unwrap();
        let bytes = b"text".to_vec();
        let retained = store
            .retain_new_source(RetainNewSource {
                origin: None,
                loupist_id: "owner".into(),
                at_ms: 1,
                title: "Filing".into(),
                source_class: SourceClass::PublisherDocument,
                origin_locator: "fixture:replay".into(),
                provider_identity: "fixture".into(),
                authorship: Authorship::Publisher,
                coverage: Coverage::IncludedFull,
                origin_chat_id: None,
                request_ref: None,
                declared_digest: None,
                material: Some(RetainMaterial {
                    media_type: "text/plain".into(),
                    role: RepresentationRole::Original,
                    document_id: "replay".into(),
                    parser_version: None,
                    parent_representation_id: None,
                    input_span_ids: Vec::new(),
                    selector: Selector::ByteRange { start: 0, end: 4 },
                    context_note: "replay".into(),
                    bytes,
                }),
                publication_time: Some(day("2026-11-10")),
            })
            .unwrap();
        let span = retained.span_id.clone().unwrap();
        let head = NaiveDate::from_ymd_opt(2026, 11, 25)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp_millis();
        let read = |since, cutoff: NaiveDate| {
            let mut request = query(since, Axis::Published, 30);
            request.cutoff = Some(cutoff);
            store
                .transact(|write| Ok(finish(collect(write.tx, &request)?).body))
                .unwrap()
        };
        store
            .transact(|write| {
                let record = write.create_record(CreateRecord {
                    loupist_id: "owner".into(),
                    kind: RecordKind::World,
                    title: "World".into(),
                    at_ms: 1,
                })?;
                write.seal_world(SealWorld {
                    loupist_id: "owner".into(),
                    record_id: record.record_id.clone(),
                    expected_revision_id: None,
                    at_ms: head,
                    nodes: vec![GraphNode {
                        id: "node-a".into(),
                        name: "Node A".into(),
                        aliases: Vec::new(),
                        description: String::new(),
                        kind: "company".into(),
                        source_role: "subject".into(),
                        properties: BTreeMap::new(),
                    }],
                    claims: vec![claim("guidance", &span)],
                    annotations: Vec::new(),
                    dependencies: vec![EvidenceRevision {
                        record_id: retained.record_id.clone(),
                        revision_digest: retained.revision_digest.clone(),
                        role: DependencyRole::WorldSource,
                    }],
                })?;
                write.insert_world_episode(&episode(
                    "later",
                    &record.record_id,
                    &retained.record_id,
                    &retained.revision_id,
                    &span,
                    head,
                    EpisodeChanges {
                        ended: vec![EpisodeEnd {
                            claim: "guidance".into(),
                            by: None,
                            end: day("2026-11-10"),
                            basis: EndBasis::Stated,
                        }],
                        ..EpisodeChanges::default()
                    },
                    "2026-11-10",
                ))?;
                Ok(())
            })
            .unwrap();
        let early = read(
            Since::Date(NaiveDate::from_ymd_opt(2026, 11, 1).unwrap()),
            NaiveDate::from_ymd_opt(2026, 11, 5).unwrap(),
        );
        assert!(early["changes"].as_array().unwrap().is_empty(), "{early}");
        let cursor = early["next"].as_str().unwrap().to_owned();
        let on_the_day = read(
            Since::Cursor(cursor),
            NaiveDate::from_ymd_opt(2026, 11, 10).unwrap(),
        );
        assert_eq!(
            on_the_day["changes"][0]["claimId"], "guidance",
            "{on_the_day}"
        );
    }

    #[test]
    fn a_recorded_page_does_not_read_episodes_outside_its_window() {
        let dir = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(dir.path().join("knowledge.sqlite")).unwrap();
        let bytes = b"text".to_vec();
        let retained = store
            .retain_new_source(RetainNewSource {
                origin: None,
                loupist_id: "owner".into(),
                at_ms: 1,
                title: "Filing".into(),
                source_class: SourceClass::PublisherDocument,
                origin_locator: "fixture:window".into(),
                provider_identity: "fixture".into(),
                authorship: Authorship::Publisher,
                coverage: Coverage::IncludedFull,
                origin_chat_id: None,
                request_ref: None,
                declared_digest: None,
                material: Some(RetainMaterial {
                    media_type: "text/plain".into(),
                    role: RepresentationRole::Original,
                    document_id: "window".into(),
                    parser_version: None,
                    parent_representation_id: None,
                    input_span_ids: Vec::new(),
                    selector: Selector::ByteRange { start: 0, end: 4 },
                    context_note: "window".into(),
                    bytes,
                }),
                publication_time: Some(day("2026-06-01")),
            })
            .unwrap();
        let span = retained.span_id.clone().unwrap();
        let at = NaiveDate::from_ymd_opt(2026, 6, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp_millis();
        let page = store
            .transact(|write| {
                let record = write.create_record(CreateRecord {
                    loupist_id: "owner".into(),
                    kind: RecordKind::World,
                    title: "World".into(),
                    at_ms: 1,
                })?;
                write.seal_world(SealWorld {
                    loupist_id: "owner".into(),
                    record_id: record.record_id.clone(),
                    expected_revision_id: None,
                    at_ms: at + 86_400_000,
                    nodes: vec![GraphNode {
                        id: "node-a".into(),
                        name: "Node A".into(),
                        aliases: Vec::new(),
                        description: String::new(),
                        kind: "company".into(),
                        source_role: "subject".into(),
                        properties: BTreeMap::new(),
                    }],
                    claims: vec![claim("kept", &span)],
                    annotations: Vec::new(),
                    dependencies: vec![EvidenceRevision {
                        record_id: retained.record_id.clone(),
                        revision_digest: retained.revision_digest.clone(),
                        role: DependencyRole::WorldSource,
                    }],
                })?;
                write.tx.execute(
                    "INSERT INTO knowledge_world_episodes (
                        episode_id, loupist_id, world_record_id, job_id, sealed_revision_id,
                        source_record_id, source_revision_id, span_id,
                        chunk_index, chunk_count, char_start, char_end,
                        reference_time, reference_basis, publication_json, model_json,
                        changes_json, backfilled, created_at_ms
                     ) VALUES (
                        'poison', 'owner', ?1, 'job-poison', 'rev-poison',
                        ?2, ?3, ?4,
                        0, 1, 0, 4,
                        '2020-01-01', 'published', ?5, '{\"model_profile\":\"fixture\",\"thinking_level\":\"low\"}',
                        'not-json', 0, 1
                     )",
                    params![
                        record.record_id,
                        retained.record_id,
                        retained.revision_id,
                        span,
                        serde_json::to_string(&day("2020-01-01")).unwrap(),
                    ],
                )?;
                write.insert_world_episode(&episode(
                    "kept",
                    &record.record_id,
                    &retained.record_id,
                    &retained.revision_id,
                    &span,
                    at,
                    EpisodeChanges {
                        created: vec!["kept".into()],
                        ..EpisodeChanges::default()
                    },
                    "2026-06-01",
                ))?;
                let body = finish(collect(
                    write.tx,
                    &query(
                        Since::Date(NaiveDate::from_ymd_opt(2026, 1, 1).unwrap()),
                        Axis::Recorded,
                        30,
                    ),
                )?)
                .body;
                Ok(body)
            })
            .unwrap();
        assert_eq!(page["changes"][0]["claimId"], "kept", "{page}");
    }

    #[test]
    fn a_cursor_uses_the_scope_the_caller_names_now() {
        let dir = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(dir.path().join("knowledge.sqlite")).unwrap();
        let bytes = b"text".to_vec();
        let retained = store
            .retain_new_source(RetainNewSource {
                origin: None,
                loupist_id: "owner".into(),
                at_ms: 1,
                title: "Filing".into(),
                source_class: SourceClass::PublisherDocument,
                origin_locator: "fixture:scope".into(),
                provider_identity: "fixture".into(),
                authorship: Authorship::Publisher,
                coverage: Coverage::IncludedFull,
                origin_chat_id: None,
                request_ref: None,
                declared_digest: None,
                material: Some(RetainMaterial {
                    media_type: "text/plain".into(),
                    role: RepresentationRole::Original,
                    document_id: "scope".into(),
                    parser_version: None,
                    parent_representation_id: None,
                    input_span_ids: Vec::new(),
                    selector: Selector::ByteRange { start: 0, end: 4 },
                    context_note: "scope".into(),
                    bytes,
                }),
                publication_time: Some(day("2026-01-01")),
            })
            .unwrap();
        let span = retained.span_id.clone().unwrap();
        store
            .transact(|write| {
                let record = write.create_record(CreateRecord {
                    loupist_id: "owner".into(),
                    kind: RecordKind::World,
                    title: "World".into(),
                    at_ms: 1,
                })?;
                write.seal_world(SealWorld {
                    loupist_id: "owner".into(),
                    record_id: record.record_id.clone(),
                    expected_revision_id: None,
                    at_ms: NaiveDate::from_ymd_opt(2026, 1, 4)
                        .unwrap()
                        .and_hms_opt(12, 0, 0)
                        .unwrap()
                        .and_utc()
                        .timestamp_millis(),
                    nodes: vec![
                        GraphNode {
                            id: "node-a".into(),
                            name: "Kept".into(),
                            aliases: Vec::new(),
                            description: String::new(),
                            kind: "company".into(),
                            source_role: "subject".into(),
                            properties: BTreeMap::new(),
                        },
                        GraphNode {
                            id: "node-b".into(),
                            name: "Other".into(),
                            aliases: Vec::new(),
                            description: String::new(),
                            kind: "company".into(),
                            source_role: "subject".into(),
                            properties: BTreeMap::new(),
                        },
                    ],
                    claims: vec![claim("guidance", &span), claim("other", &span)],
                    annotations: Vec::new(),
                    dependencies: vec![EvidenceRevision {
                        record_id: retained.record_id.clone(),
                        revision_digest: retained.revision_digest.clone(),
                        role: DependencyRole::WorldSource,
                    }],
                })?;
                let noon = |day: u32| {
                    NaiveDate::from_ymd_opt(2026, 1, day)
                        .unwrap()
                        .and_hms_opt(12, 0, 0)
                        .unwrap()
                        .and_utc()
                        .timestamp_millis()
                };
                for (id, claim_id, at) in [
                    ("ended", "guidance", noon(2)),
                    ("ended-2", "other", noon(3)),
                ] {
                    write.insert_world_episode(&episode(
                        id,
                        &record.record_id,
                        &retained.record_id,
                        &retained.revision_id,
                        &span,
                        at,
                        EpisodeChanges {
                            ended: vec![EpisodeEnd {
                                claim: claim_id.into(),
                                by: None,
                                end: day("2026-01-01"),
                                basis: EndBasis::Stated,
                            }],
                            ..EpisodeChanges::default()
                        },
                        "2026-01-01",
                    ))?;
                }
                let mut first = query(
                    Since::Date(NaiveDate::from_ymd_opt(2026, 1, 1).unwrap()),
                    Axis::Recorded,
                    1,
                );
                first.about = vec![AboutRef::Node {
                    id: "node-a".into(),
                }];
                let page = finish(collect(write.tx, &first)?).body;
                assert_eq!(page["capped"], true, "{page}");
                assert_eq!(page["changes"].as_array().unwrap().len(), 1);
                let mut narrowed = query(
                    Since::Cursor(page["next"].as_str().unwrap().to_owned()),
                    Axis::Recorded,
                    30,
                );
                narrowed.about = vec![AboutRef::Node {
                    id: "node-b".into(),
                }];
                narrowed.axis_explicit = true;
                let again = finish(collect(write.tx, &narrowed)?).body;
                assert!(again["changes"].as_array().unwrap().is_empty(), "{again}");
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn between_caps_a_comparison_at_the_feed_limit() {
        let dir = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(dir.path().join("knowledge.sqlite")).unwrap();
        let retain = |title: &str, published: &str| {
            let bytes =
                serde_json::to_vec(&json!({"company":{"symbol":"LMP"},"form":"10-Q"})).unwrap();
            store
                .retain_new_source(RetainNewSource {
                    origin: None,
                    loupist_id: "owner".into(),
                    at_ms: 1,
                    title: title.into(),
                    source_class: SourceClass::PublisherDocument,
                    origin_locator: format!("fixture:{title}"),
                    provider_identity: "fixture".into(),
                    authorship: Authorship::Publisher,
                    coverage: Coverage::IncludedFull,
                    origin_chat_id: None,
                    request_ref: None,
                    declared_digest: None,
                    material: Some(RetainMaterial {
                        media_type: "text/plain".into(),
                        role: RepresentationRole::Original,
                        document_id: title.into(),
                        parser_version: None,
                        parent_representation_id: None,
                        input_span_ids: Vec::new(),
                        selector: Selector::ByteRange {
                            start: 0,
                            end: bytes.len() as u64,
                        },
                        context_note: title.into(),
                        bytes,
                    }),
                    publication_time: Some(day(published)),
                })
                .unwrap()
        };
        let older = retain("q1", "2026-03-01");
        let newer = retain("q2", "2026-06-01");
        let span = newer.span_id.clone().unwrap();
        let ids: Vec<String> = (0..=MAX_LIMIT).map(|index| format!("c{index}")).collect();
        let claims: Vec<_> = ids.iter().map(|id| claim(id, &span)).collect();
        let page = store
            .transact(|write| {
                let record = write.create_record(CreateRecord {
                    loupist_id: "owner".into(),
                    kind: RecordKind::World,
                    title: "World".into(),
                    at_ms: 1,
                })?;
                write.seal_world(SealWorld {
                    loupist_id: "owner".into(),
                    record_id: record.record_id.clone(),
                    expected_revision_id: None,
                    at_ms: 5_000,
                    nodes: vec![GraphNode {
                        id: "node-a".into(),
                        name: "Node A".into(),
                        aliases: Vec::new(),
                        description: String::new(),
                        kind: "company".into(),
                        source_role: "subject".into(),
                        properties: BTreeMap::new(),
                    }],
                    claims,
                    annotations: Vec::new(),
                    dependencies: vec![
                        EvidenceRevision {
                            record_id: older.record_id.clone(),
                            revision_digest: older.revision_digest.clone(),
                            role: DependencyRole::WorldSource,
                        },
                        EvidenceRevision {
                            record_id: newer.record_id.clone(),
                            revision_digest: newer.revision_digest.clone(),
                            role: DependencyRole::WorldSource,
                        },
                    ],
                })?;
                write.insert_world_episode(&episode(
                    "many",
                    &record.record_id,
                    &newer.record_id,
                    &newer.revision_id,
                    &span,
                    2_000,
                    EpisodeChanges {
                        created: ids,
                        ..EpisodeChanges::default()
                    },
                    "2026-06-01",
                ))?;
                between(
                    write,
                    &BetweenQuery {
                        loupist_id: "owner".into(),
                        to: newer.record_id.clone(),
                        from: Some(older.record_id),
                        cutoff: None,
                    },
                )
            })
            .unwrap();
        assert_eq!(page["capped"], true);
        assert_eq!(
            page["new_in_to"].as_array().unwrap().len(),
            MAX_LIMIT as usize
        );
    }

    #[test]
    fn between_pairs_claims_and_refuses_an_unpaired_source() {
        let dir = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(dir.path().join("knowledge.sqlite")).unwrap();
        let retain = |title: &str, published: &str, company: &str, form: &str| {
            let bytes =
                serde_json::to_vec(&json!({"company":{"symbol": company},"form": form})).unwrap();
            let end = bytes.len() as u64;
            let retained = store
                .retain_new_source(RetainNewSource {
                    origin: None,
                    loupist_id: "owner".into(),
                    at_ms: 1,
                    title: title.into(),
                    source_class: SourceClass::PublisherDocument,
                    origin_locator: format!("fixture:{title}"),
                    provider_identity: "fixture".into(),
                    authorship: Authorship::Publisher,
                    coverage: Coverage::IncludedFull,
                    origin_chat_id: None,
                    request_ref: None,
                    declared_digest: None,
                    material: Some(RetainMaterial {
                        media_type: "text/plain".into(),
                        role: RepresentationRole::Original,
                        document_id: title.into(),
                        parser_version: None,
                        parent_representation_id: None,
                        input_span_ids: Vec::new(),
                        selector: Selector::ByteRange { start: 0, end },
                        context_note: title.into(),
                        bytes,
                    }),
                    publication_time: Some(day(published)),
                })
                .unwrap();
            retained
        };
        let older = retain("q2", "2026-06-01", "LMP", "10-Q");
        let newer = retain("q3", "2026-09-01", "LMP", "10-Q");
        let alone = retain("annual", "2026-12-01", "OTHER", "10-K");
        let span = older.span_id.clone().unwrap();
        store
            .transact(|write| {
                let record = write.create_record(CreateRecord {
                    loupist_id: "owner".into(),
                    kind: RecordKind::World,
                    title: "World".into(),
                    at_ms: 1,
                })?;
                let claims = ["old-kept", "old-ended", "old-silent", "new-claim"]
                    .into_iter()
                    .map(|id| claim(id, &span))
                    .collect();
                write.seal_world(SealWorld {
                    loupist_id: "owner".into(),
                    record_id: record.record_id.clone(),
                    expected_revision_id: None,
                    at_ms: 5_000,
                    nodes: vec![GraphNode {
                        id: "node-a".into(),
                        name: "Node A".into(),
                        aliases: Vec::new(),
                        description: String::new(),
                        kind: "company".into(),
                        source_role: "subject".into(),
                        properties: BTreeMap::new(),
                    }],
                    claims,
                    annotations: Vec::new(),
                    dependencies: vec![
                        EvidenceRevision {
                            record_id: older.record_id.clone(),
                            revision_digest: older.revision_digest.clone(),
                            role: DependencyRole::WorldSource,
                        },
                        EvidenceRevision {
                            record_id: newer.record_id.clone(),
                            revision_digest: newer.revision_digest.clone(),
                            role: DependencyRole::WorldSource,
                        },
                    ],
                })?;
                write.insert_world_episode(&episode(
                    "from",
                    &record.record_id,
                    &older.record_id,
                    &older.revision_id,
                    &span,
                    1_000,
                    EpisodeChanges {
                        created: vec!["old-kept".into(), "old-ended".into(), "old-silent".into()],
                        ..EpisodeChanges::default()
                    },
                    "2026-06-01",
                ))?;
                write.insert_world_episode(&episode(
                    "to",
                    &record.record_id,
                    &newer.record_id,
                    &newer.revision_id,
                    &span,
                    2_000,
                    EpisodeChanges {
                        created: vec!["new-claim".into()],
                        supported: vec!["old-kept".into()],
                        ended: vec![EpisodeEnd {
                            claim: "old-ended".into(),
                            by: Some("new-claim".into()),
                            end: day("2026-09-01"),
                            basis: EndBasis::Stated,
                        }],
                        ..EpisodeChanges::default()
                    },
                    "2026-09-01",
                ))?;
                let paired = between(
                    write,
                    &BetweenQuery {
                        loupist_id: "owner".into(),
                        to: newer.record_id.clone(),
                        from: None,
                        cutoff: None,
                    },
                )?;
                assert_eq!(paired["from"]["recordId"], older.record_id);
                assert_eq!(paired["capped"], false);
                assert_eq!(paired["ended_by_to"][0]["claimId"], "old-ended");
                assert_eq!(paired["new_in_to"][0]["claimId"], "new-claim");
                let replay = between(
                    write,
                    &BetweenQuery {
                        loupist_id: "owner".into(),
                        to: newer.record_id.clone(),
                        from: Some(older.record_id.clone()),
                        cutoff: Some(NaiveDate::from_ymd_opt(2026, 7, 1).unwrap()),
                    },
                )?;
                assert_eq!(replay["to"]["episodes"], 0, "{replay}");
                assert!(replay["to"]["title"].is_null(), "{replay}");
                assert!(replay["to"]["publicationTime"].is_null(), "{replay}");
                assert!(
                    replay["new_in_to"].as_array().unwrap().is_empty(),
                    "{replay}"
                );
                assert!(
                    replay["ended_by_to"].as_array().unwrap().is_empty(),
                    "{replay}"
                );
                assert_eq!(paired["restated"][0]["claimId"], "old-kept");
                assert_eq!(paired["not_restated"][0]["claimId"], "old-silent");
                let missing = between(
                    write,
                    &BetweenQuery {
                        loupist_id: "owner".into(),
                        to: alone.record_id,
                        from: None,
                        cutoff: None,
                    },
                )
                .unwrap_err();
                assert_eq!(error_code(&missing), Some("between_needs_from"));
                let lineage = claim_lineage(write.tx, "owner", "old-ended")?;
                assert_eq!(lineage["created"]["episodeId"], "from");
                assert_eq!(lineage["ended"]["episodeId"], "to");
                assert_eq!(lineage["endedByClaimId"], "new-claim");
                write.insert_world_episode(&episode(
                    "reopen",
                    &record.record_id,
                    &newer.record_id,
                    &newer.revision_id,
                    &span,
                    3_000,
                    EpisodeChanges {
                        reopened: vec!["old-ended".into()],
                        ..EpisodeChanges::default()
                    },
                    "2026-10-01",
                ))?;
                let reopened = claim_lineage(write.tx, "owner", "old-ended")?;
                assert!(reopened["ended"].is_null(), "{reopened}");
                assert!(reopened["endedByClaimId"].is_null(), "{reopened}");
                assert_eq!(reopened["endings"][0]["episodeId"], "to");
                let mut controls = crate::knowledge::memory_controls::load(write.tx, "owner")?;
                controls.removed.insert(older.record_id.clone());
                crate::knowledge::memory_controls::save(write.tx, "owner", &controls)?;
                let hidden = between(
                    write,
                    &BetweenQuery {
                        loupist_id: "owner".into(),
                        to: newer.record_id,
                        from: Some(older.record_id),
                        cutoff: None,
                    },
                )
                .unwrap_err();
                assert!(matches!(hidden, KnowledgeError::NotFound));
                Ok(())
            })
            .unwrap();
    }

    fn filing(
        store: &KnowledgeStore,
        title: &str,
        published: &str,
        company: &str,
        form: &str,
    ) -> String {
        let bytes =
            serde_json::to_vec(&json!({"company": {"symbol": company}, "form": form})).unwrap();
        let end = bytes.len() as u64;
        store
            .retain_new_source(RetainNewSource {
                origin: None,
                loupist_id: "owner".into(),
                at_ms: 1,
                title: title.into(),
                source_class: SourceClass::PublisherDocument,
                origin_locator: format!("fixture:{title}"),
                provider_identity: "fixture".into(),
                authorship: Authorship::Publisher,
                coverage: Coverage::IncludedFull,
                origin_chat_id: None,
                request_ref: None,
                declared_digest: None,
                material: Some(RetainMaterial {
                    media_type: "text/plain".into(),
                    role: RepresentationRole::Original,
                    document_id: title.into(),
                    parser_version: None,
                    parent_representation_id: None,
                    input_span_ids: Vec::new(),
                    selector: Selector::ByteRange { start: 0, end },
                    context_note: title.into(),
                    bytes,
                }),
                publication_time: Some(day(published)),
            })
            .unwrap()
            .record_id
    }

    fn asset(asset_id: &str, name: &str, ticker: &str) -> AssetRow {
        AssetRow {
            asset_id: asset_id.into(),
            name: name.into(),
            reference: Some(AssetReference {
                fields: AssetReferenceFields {
                    ticker: Some(ticker.into()),
                    ..Default::default()
                },
                origin: AssetReferenceOrigin::User,
            }),
            followed_at_ms: Some(1),
            created_at_ms: 1,
        }
    }

    #[test]
    fn an_asset_picks_the_latest_report_and_the_one_before_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(dir.path().join("knowledge.sqlite")).unwrap();
        let older = filing(&store, "q2", "2026-06-01", "LMP", "10-Q");
        let newer = filing(&store, "q3", "2026-09-01", "LMP", "10-Q");
        let solo = filing(&store, "solo", "2026-08-01", "SOLO", "10-Q");
        let queued_old = filing(&store, "que-old", "2026-03-01", "QUE", "10-Q");
        let queued_new = filing(&store, "que-new", "2026-11-01", "QUE", "10-Q");
        let holding = asset("ast_aaaaaaaaaaaaaaaa", "Holding", "LMP");
        let solo_asset = asset("ast_bbbbbbbbbbbbbbbb", "Solo", "SOLO");
        let queued_asset = asset("ast_cccccccccccccccc", "Queued", "QUE");
        store
            .transact(|write| {
                let linked: Option<Option<String>> = write
                    .tx
                    .query_row(
                        "SELECT entity_id FROM knowledge_asset_pages WHERE asset_id = 'ast_aaaaaaaaaaaaaaaa'",
                        [],
                        |row| row.get(0),
                    )
                    .optional()?;
                assert!(linked.flatten().is_none(), "the ticker matches with no linked World name");
                let pair = pair_for_asset(write.tx, "owner", &holding)?.unwrap();
                assert_eq!(pair.from_id, older);
                assert_eq!(pair.to_id, newer);
                assert!(!pair.pending);
                assert!(pair_for_asset(write.tx, "owner", &solo_asset)?.is_none());
                let _ = solo;
                write.tx.execute(
                    "INSERT INTO knowledge_graph_outbox (
                        job_id, loupist_id, world_record_id, world_revision_id, status,
                        payload_json, payload_digest, created_at_ms
                     ) VALUES (
                        'job-que', 'owner', 'rec_world', 'rev', 'queued', ?1, 'd', 3
                     )",
                    params![json!({"sourceBinding": {"sourceRecordId": queued_new}}).to_string()],
                )?;
                let queued = pair_for_asset(write.tx, "owner", &queued_asset)?.unwrap();
                assert_eq!(queued.from_id, queued_old);
                assert_eq!(queued.to_id, queued_new);
                assert!(queued.pending);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn a_short_page_still_reports_the_window_total() {
        let dir = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(dir.path().join("knowledge.sqlite")).unwrap();
        let bytes = b"notes".to_vec();
        let retained = store
            .retain_new_source(RetainNewSource {
                origin: None,
                loupist_id: "owner".into(),
                at_ms: 1,
                title: "notes".into(),
                source_class: SourceClass::PublisherDocument,
                origin_locator: "fixture:totals".into(),
                provider_identity: "fixture".into(),
                authorship: Authorship::Publisher,
                coverage: Coverage::IncludedFull,
                origin_chat_id: None,
                request_ref: None,
                declared_digest: None,
                material: Some(RetainMaterial {
                    media_type: "text/plain".into(),
                    role: RepresentationRole::Original,
                    document_id: "notes".into(),
                    parser_version: None,
                    parent_representation_id: None,
                    input_span_ids: Vec::new(),
                    selector: Selector::ByteRange {
                        start: 0,
                        end: bytes.len() as u64,
                    },
                    context_note: "notes".into(),
                    bytes,
                }),
                publication_time: Some(day("2026-01-01")),
            })
            .unwrap();
        let span = retained.span_id.unwrap();
        let body = store
            .transact(|write| {
                let record = write.create_record(CreateRecord {
                    loupist_id: "owner".into(),
                    kind: RecordKind::World,
                    title: "World".into(),
                    at_ms: 1,
                })?;
                let claims = ["c1", "c2", "c3"]
                    .into_iter()
                    .map(|id| claim(id, &span))
                    .collect();
                write.seal_world(SealWorld {
                    loupist_id: "owner".into(),
                    record_id: record.record_id.clone(),
                    expected_revision_id: None,
                    at_ms: 5_000,
                    nodes: vec![GraphNode {
                        id: "node-a".into(),
                        name: "Node A".into(),
                        aliases: Vec::new(),
                        description: String::new(),
                        kind: "company".into(),
                        source_role: "subject".into(),
                        properties: BTreeMap::new(),
                    }],
                    claims,
                    annotations: Vec::new(),
                    dependencies: vec![EvidenceRevision {
                        record_id: retained.record_id.clone(),
                        revision_digest: retained.revision_digest,
                        role: DependencyRole::WorldSource,
                    }],
                })?;
                write.insert_world_episode(&episode(
                    "all",
                    &record.record_id,
                    &retained.record_id,
                    &retained.revision_id,
                    &span,
                    1_000,
                    EpisodeChanges {
                        created: vec!["c1".into(), "c2".into(), "c3".into()],
                        ..EpisodeChanges::default()
                    },
                    "2026-01-01",
                ))?;
                write.tx.execute(
                    "INSERT INTO knowledge_graph_outbox (
                        job_id, loupist_id, world_record_id, world_revision_id, status,
                        payload_json, payload_digest, created_at_ms
                     ) VALUES ('job-wait', 'owner', ?1, 'rev', 'queued', ?2, 'd', 1)",
                    params![
                        record.record_id,
                        json!({"sourceBinding": {"sourceRecordId": retained.record_id}})
                            .to_string()
                    ],
                )?;
                Ok(finish(collect(
                    write.tx,
                    &query(
                        Since::Millis {
                            ms: 0,
                            inclusive: true,
                        },
                        Axis::Recorded,
                        1,
                    ),
                )?)
                .body)
            })
            .unwrap();
        assert_eq!(body["changes"].as_array().unwrap().len(), 1);
        assert_eq!(body["totals"]["added"], 3, "{body}");
        assert_eq!(body["pending"], 1);
    }

    #[test]
    fn an_april_replay_does_not_return_an_ending_established_in_may() {
        let dir = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(dir.path().join("knowledge.sqlite")).unwrap();
        let retain = |title: &str, published: &str| {
            let bytes = format!(r#"{{"company":{{"symbol":"LMP"}},"form":"10-Q","id":"{title}"}}"#)
                .into_bytes();
            let end = bytes.len() as u64;
            store
                .retain_new_source(RetainNewSource {
                    origin: None,
                    loupist_id: "owner".into(),
                    at_ms: 1,
                    title: title.into(),
                    source_class: SourceClass::PublisherDocument,
                    origin_locator: format!("fixture:{title}"),
                    provider_identity: "fixture".into(),
                    authorship: Authorship::Publisher,
                    coverage: Coverage::IncludedFull,
                    origin_chat_id: None,
                    request_ref: None,
                    declared_digest: None,
                    material: Some(RetainMaterial {
                        media_type: "text/plain".into(),
                        role: RepresentationRole::Original,
                        document_id: title.into(),
                        parser_version: None,
                        parent_representation_id: None,
                        input_span_ids: Vec::new(),
                        selector: Selector::ByteRange { start: 0, end },
                        context_note: title.into(),
                        bytes,
                    }),
                    publication_time: Some(day(published)),
                })
                .unwrap()
        };
        let march = retain("march", "2026-03-01");
        let april = retain("april", "2026-04-01");
        let june = retain("june", "2026-06-01");
        let march_span = march.span_id.clone().unwrap();
        let april_span = april.span_id.clone().unwrap();
        let june_span = june.span_id.clone().unwrap();
        let body = store
            .transact(|write| {
                let record = write.create_record(CreateRecord {
                    loupist_id: "owner".into(),
                    kind: RecordKind::World,
                    title: "World".into(),
                    at_ms: 1,
                })?;
                let mut ended = claim("old-ended", &march_span);
                ended.invalid_at = Some(day("2026-03-15"));
                ended.qualifiers = vec![crate::knowledge::Qualifier {
                    key: crate::knowledge::SUPERSEDED_BY_SPAN.into(),
                    value: june_span,
                }];
                write.seal_world(SealWorld {
                    loupist_id: "owner".into(),
                    record_id: record.record_id.clone(),
                    expected_revision_id: None,
                    at_ms: 5_000,
                    nodes: vec![GraphNode {
                        id: "node-a".into(),
                        name: "Node A".into(),
                        aliases: Vec::new(),
                        description: String::new(),
                        kind: "company".into(),
                        source_role: "subject".into(),
                        properties: BTreeMap::new(),
                    }],
                    claims: vec![ended],
                    annotations: Vec::new(),
                    dependencies: vec![
                        EvidenceRevision {
                            record_id: march.record_id.clone(),
                            revision_digest: march.revision_digest.clone(),
                            role: DependencyRole::WorldSource,
                        },
                        EvidenceRevision {
                            record_id: april.record_id.clone(),
                            revision_digest: april.revision_digest.clone(),
                            role: DependencyRole::WorldSource,
                        },
                        EvidenceRevision {
                            record_id: june.record_id.clone(),
                            revision_digest: june.revision_digest,
                            role: DependencyRole::WorldSource,
                        },
                    ],
                })?;
                write.insert_world_episode(&episode(
                    "from",
                    &record.record_id,
                    &march.record_id,
                    &march.revision_id,
                    &march_span,
                    1_000,
                    EpisodeChanges {
                        created: vec!["old-ended".into()],
                        ..EpisodeChanges::default()
                    },
                    "2026-03-01",
                ))?;
                write.insert_world_episode(&episode(
                    "to",
                    &record.record_id,
                    &april.record_id,
                    &april.revision_id,
                    &april_span,
                    2_000,
                    EpisodeChanges {
                        ended: vec![EpisodeEnd {
                            claim: "old-ended".into(),
                            by: None,
                            end: day("2026-03-15"),
                            basis: EndBasis::Stated,
                        }],
                        ..EpisodeChanges::default()
                    },
                    "2026-04-01",
                ))?;
                between(
                    write,
                    &BetweenQuery {
                        loupist_id: "owner".into(),
                        to: april.record_id,
                        from: Some(march.record_id),
                        cutoff: Some(NaiveDate::from_ymd_opt(2026, 4, 30).unwrap()),
                    },
                )
            })
            .unwrap();
        assert!(body["ended_by_to"].as_array().unwrap().is_empty(), "{body}");
    }

    #[test]
    fn a_repaired_section_stays_in_the_replay_that_saw_it_superseded() {
        let dir = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(dir.path().join("knowledge.sqlite")).unwrap();
        let yesterday = chrono::DateTime::parse_from_rfc3339("2026-04-01T12:00:00Z")
            .unwrap()
            .timestamp_millis();
        let today = chrono::DateTime::parse_from_rfc3339("2026-04-02T12:00:00Z")
            .unwrap()
            .timestamp_millis();
        let later = chrono::DateTime::parse_from_rfc3339("2026-04-03T12:00:00Z")
            .unwrap()
            .timestamp_millis();
        store
            .transact(|write| {
                let record = write.create_record(CreateRecord {
                    loupist_id: "owner".into(),
                    kind: RecordKind::World,
                    title: "World".into(),
                    at_ms: 1,
                })?;
                write.seal_world(SealWorld {
                    loupist_id: "owner".into(),
                    record_id: record.record_id,
                    expected_revision_id: None,
                    at_ms: later,
                    nodes: Vec::new(),
                    claims: Vec::new(),
                    annotations: Vec::new(),
                    dependencies: Vec::new(),
                })
            })
            .unwrap();
        let lens = store
            .file_method(
                "owner",
                "Change page",
                "The supply ended.",
                &crate::knowledge::test_support::turn_origin("chat", "msg"),
                yesterday - 1,
            )
            .unwrap();
        store
            .transact(|write| {
                write.supersede_section(
                    "owner",
                    &lens.record_id,
                    "steps",
                    vec!["kept".into()],
                    yesterday,
                )?;
                write.restore_section("owner", &lens.record_id, "steps", today)
            })
            .unwrap();
        let replay = store
            .transact(|write| {
                let mut request = query(
                    Since::Millis {
                        ms: 0,
                        inclusive: true,
                    },
                    Axis::Recorded,
                    30,
                );
                request.cutoff = Some(NaiveDate::from_ymd_opt(2026, 4, 1).unwrap());
                Ok(finish(collect(write.tx, &request)?).body)
            })
            .unwrap();
        assert!(
            replay["lens"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| { row["state"] == "superseded" && row["sectionKey"] == "steps" }),
            "{replay}"
        );
        let live = store
            .transact(|write| {
                Ok(finish(collect(
                    write.tx,
                    &query(
                        Since::Millis {
                            ms: 0,
                            inclusive: true,
                        },
                        Axis::Recorded,
                        30,
                    ),
                )?)
                .body)
            })
            .unwrap();
        assert!(live["lens"].as_array().unwrap().is_empty(), "{live}");
    }

    #[test]
    fn a_later_withdrawal_is_not_in_an_earlier_published_month() {
        let dir = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(dir.path().join("knowledge.sqlite")).unwrap();
        let bytes = b"notes".to_vec();
        let retained = store
            .retain_new_source(RetainNewSource {
                origin: None,
                loupist_id: "owner".into(),
                at_ms: 1,
                title: "january".into(),
                source_class: SourceClass::PublisherDocument,
                origin_locator: "fixture:january".into(),
                provider_identity: "fixture".into(),
                authorship: Authorship::Publisher,
                coverage: Coverage::IncludedFull,
                origin_chat_id: None,
                request_ref: None,
                declared_digest: None,
                material: Some(RetainMaterial {
                    media_type: "text/plain".into(),
                    role: RepresentationRole::Original,
                    document_id: "january".into(),
                    parser_version: None,
                    parent_representation_id: None,
                    input_span_ids: Vec::new(),
                    selector: Selector::ByteRange {
                        start: 0,
                        end: bytes.len() as u64,
                    },
                    context_note: "january".into(),
                    bytes,
                }),
                publication_time: Some(day("2026-01-15")),
            })
            .unwrap();
        let span = retained.span_id.unwrap();
        let read = |since: &str, cutoff: Option<NaiveDate>| {
            let mut request = query(Since::Date(since.parse().unwrap()), Axis::Published, 30);
            request.cutoff = cutoff;
            request.kinds = Some(BTreeSet::from([ChangeKind::Withdrawn]));
            store
                .transact(|write| Ok(finish(collect(write.tx, &request)?).body))
                .unwrap()
        };
        store
            .transact(|write| {
                let record = write.create_record(CreateRecord {
                    loupist_id: "owner".into(),
                    kind: RecordKind::World,
                    title: "World".into(),
                    at_ms: 1,
                })?;
                let mut old = claim("old", &span);
                old.publication_time = Some(day("2026-01-15"));
                write.seal_world(SealWorld {
                    loupist_id: "owner".into(),
                    record_id: record.record_id.clone(),
                    expected_revision_id: None,
                    at_ms: 2,
                    nodes: vec![GraphNode {
                        id: "node-a".into(),
                        name: "Node A".into(),
                        aliases: Vec::new(),
                        description: String::new(),
                        kind: "company".into(),
                        source_role: "subject".into(),
                        properties: BTreeMap::new(),
                    }],
                    claims: vec![old],
                    annotations: Vec::new(),
                    dependencies: vec![EvidenceRevision {
                        record_id: retained.record_id.clone(),
                        revision_digest: retained.revision_digest,
                        role: DependencyRole::WorldSource,
                    }],
                })?;
                let mut event = crate::knowledge::growth::GrowthEvent::new(
                    "owner",
                    &record.record_id,
                    chrono::DateTime::parse_from_rfc3339("2026-05-15T12:00:00Z")
                        .unwrap()
                        .timestamp_millis(),
                    "claim_withdrawn",
                    "processing",
                    "withdrew",
                );
                event.subject_id = Some("old".into());
                event.insert(write.tx)?;
                Ok(())
            })
            .unwrap();
        let february = read(
            "2026-01-01",
            Some(NaiveDate::from_ymd_opt(2026, 2, 28).unwrap()),
        );
        assert!(
            february["changes"].as_array().unwrap().is_empty(),
            "{february}"
        );
        let may = read("2026-05-01", None);
        assert_eq!(may["changes"][0]["kind"], "withdrawn", "{may}");
    }

    #[test]
    fn a_malformed_chunk_counts_as_a_failed_source() {
        let dir = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(dir.path().join("knowledge.sqlite")).unwrap();
        let bytes = b"notes".to_vec();
        let retained = store
            .retain_new_source(RetainNewSource {
                origin: None,
                loupist_id: "owner".into(),
                at_ms: 1,
                title: "notes".into(),
                source_class: SourceClass::PublisherDocument,
                origin_locator: "fixture:notes".into(),
                provider_identity: "fixture".into(),
                authorship: Authorship::Publisher,
                coverage: Coverage::IncludedFull,
                origin_chat_id: None,
                request_ref: None,
                declared_digest: None,
                material: Some(RetainMaterial {
                    media_type: "text/plain".into(),
                    role: RepresentationRole::Original,
                    document_id: "notes".into(),
                    parser_version: None,
                    parent_representation_id: None,
                    input_span_ids: Vec::new(),
                    selector: Selector::ByteRange {
                        start: 0,
                        end: bytes.len() as u64,
                    },
                    context_note: "notes".into(),
                    bytes,
                }),
                publication_time: Some(day("2026-01-01")),
            })
            .unwrap();
        let body = store
            .transact(|write| {
                let record = write.create_record(CreateRecord {
                    loupist_id: "owner".into(),
                    kind: RecordKind::World,
                    title: "World".into(),
                    at_ms: 1,
                })?;
                write.seal_world(SealWorld {
                    loupist_id: "owner".into(),
                    record_id: record.record_id.clone(),
                    expected_revision_id: None,
                    at_ms: 2,
                    nodes: Vec::new(),
                    claims: Vec::new(),
                    annotations: Vec::new(),
                    dependencies: Vec::new(),
                })?;
                let payload = |span: &str| {
                    json!({"sourceBinding": {"sourceRecordId": retained.record_id, "spanId": span}})
                        .to_string()
                };
                write.tx.execute(
                    "INSERT INTO knowledge_graph_outbox (
                        job_id, loupist_id, world_record_id, world_revision_id, status,
                        payload_json, payload_digest, created_at_ms
                     ) VALUES
                     ('job-done', 'owner', ?1, 'rev', 'completed', ?2, 'd', 1),
                     ('job-bad', 'owner', ?1, 'rev', 'failed_extraction', ?3, 'd', 2),
                     ('job-tail', 'owner', ?1, 'rev', 'queued', ?4, 'd', 3)",
                    params![
                        record.record_id,
                        payload("span-a"),
                        payload("span-b"),
                        payload("span-c")
                    ],
                )?;
                Ok(finish(collect(
                    write.tx,
                    &query(
                        Since::Millis {
                            ms: 0,
                            inclusive: true,
                        },
                        Axis::Recorded,
                        30,
                    ),
                )?)
                .body)
            })
            .unwrap();
        assert_eq!(body["failed"], 1, "{body}");
        assert_eq!(body["pending"], 0);
    }

    #[test]
    fn a_paused_follower_does_not_erase_the_malformed_chunk() {
        let dir = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(dir.path().join("knowledge.sqlite")).unwrap();
        let bytes = b"notes".to_vec();
        let retained = store
            .retain_new_source(RetainNewSource {
                origin: None,
                loupist_id: "owner".into(),
                at_ms: 1,
                title: "notes".into(),
                source_class: SourceClass::PublisherDocument,
                origin_locator: "fixture:chunks".into(),
                provider_identity: "fixture".into(),
                authorship: Authorship::Publisher,
                coverage: Coverage::IncludedFull,
                origin_chat_id: None,
                request_ref: None,
                declared_digest: None,
                material: Some(RetainMaterial {
                    media_type: "text/plain".into(),
                    role: RepresentationRole::Original,
                    document_id: "chunks".into(),
                    parser_version: None,
                    parent_representation_id: None,
                    input_span_ids: Vec::new(),
                    selector: Selector::ByteRange {
                        start: 0,
                        end: bytes.len() as u64,
                    },
                    context_note: "notes".into(),
                    bytes,
                }),
                publication_time: Some(day("2026-01-01")),
            })
            .unwrap();
        let body = store
            .transact(|write| {
                let record = write.create_record(CreateRecord {
                    loupist_id: "owner".into(),
                    kind: RecordKind::World,
                    title: "World".into(),
                    at_ms: 1,
                })?;
                write.seal_world(SealWorld {
                    loupist_id: "owner".into(),
                    record_id: record.record_id.clone(),
                    expected_revision_id: None,
                    at_ms: 2,
                    nodes: Vec::new(),
                    claims: Vec::new(),
                    annotations: Vec::new(),
                    dependencies: Vec::new(),
                })?;
                let payload = |index: u32| {
                    json!({
                        "sourceBinding": {
                            "sourceRecordId": retained.record_id,
                            "spanId": "span-shared"
                        },
                        "chunkIndex": index
                    })
                    .to_string()
                };
                write.tx.execute(
                    "INSERT INTO knowledge_graph_outbox (
                        job_id, loupist_id, world_record_id, world_revision_id, status,
                        payload_json, payload_digest, created_at_ms
                     ) VALUES
                     ('job-bad', 'owner', ?1, 'rev', 'failed_extraction', ?2, 'd1', 1),
                     ('job-tail', 'owner', ?1, 'rev', 'paused_budget', ?3, 'd2', 2)",
                    params![record.record_id, payload(0), payload(1)],
                )?;
                Ok(finish(collect(
                    write.tx,
                    &query(
                        Since::Millis {
                            ms: 0,
                            inclusive: true,
                        },
                        Axis::Recorded,
                        30,
                    ),
                )?)
                .body)
            })
            .unwrap();
        assert_eq!(body["failed"], 1, "{body}");
        assert_eq!(body["pending"], 0, "{body}");
    }

    #[test]
    fn two_conflicts_in_one_episode_both_cross_a_page_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(dir.path().join("knowledge.sqlite")).unwrap();
        let bytes = br#"{"company":{"symbol":"LMP"},"form":"10-Q"}"#.to_vec();
        let retained = store
            .retain_new_source(RetainNewSource {
                origin: None,
                loupist_id: "owner".into(),
                at_ms: 1,
                title: "q2".into(),
                source_class: SourceClass::PublisherDocument,
                origin_locator: "fixture:q2".into(),
                provider_identity: "fixture".into(),
                authorship: Authorship::Publisher,
                coverage: Coverage::IncludedFull,
                origin_chat_id: None,
                request_ref: None,
                declared_digest: None,
                material: Some(RetainMaterial {
                    media_type: "text/plain".into(),
                    role: RepresentationRole::Original,
                    document_id: "q2".into(),
                    parser_version: None,
                    parent_representation_id: None,
                    input_span_ids: Vec::new(),
                    selector: Selector::ByteRange {
                        start: 0,
                        end: bytes.len() as u64,
                    },
                    context_note: "q2".into(),
                    bytes,
                }),
                publication_time: Some(day("2026-06-01")),
            })
            .unwrap();
        let span = retained.span_id.clone().unwrap();
        let page = store
            .transact(|write| {
                let record = write.create_record(CreateRecord {
                    loupist_id: "owner".into(),
                    kind: RecordKind::World,
                    title: "World".into(),
                    at_ms: 1,
                })?;
                let claims = ["kept", "other", "third"]
                    .into_iter()
                    .map(|id| claim(id, &span))
                    .collect();
                write.seal_world(SealWorld {
                    loupist_id: "owner".into(),
                    record_id: record.record_id.clone(),
                    expected_revision_id: None,
                    at_ms: 5_000,
                    nodes: vec![GraphNode {
                        id: "node-a".into(),
                        name: "Node A".into(),
                        aliases: Vec::new(),
                        description: String::new(),
                        kind: "company".into(),
                        source_role: "subject".into(),
                        properties: BTreeMap::new(),
                    }],
                    claims,
                    annotations: Vec::new(),
                    dependencies: vec![EvidenceRevision {
                        record_id: retained.record_id.clone(),
                        revision_digest: retained.revision_digest.clone(),
                        role: DependencyRole::WorldSource,
                    }],
                })?;
                write.insert_world_episode(&episode(
                    "both",
                    &record.record_id,
                    &retained.record_id,
                    &retained.revision_id,
                    &span,
                    2_000,
                    EpisodeChanges {
                        created: vec!["kept".into(), "other".into(), "third".into()],
                        conflicts: vec![
                            EpisodeConflict {
                                claim: "kept".into(),
                                with: "other".into(),
                            },
                            EpisodeConflict {
                                claim: "kept".into(),
                                with: "third".into(),
                            },
                        ],
                        ..EpisodeChanges::default()
                    },
                    "2026-06-01",
                ))?;
                let mut first = query(
                    Since::Millis {
                        ms: 0,
                        inclusive: true,
                    },
                    Axis::Recorded,
                    1,
                );
                first.kinds = Some(BTreeSet::from([ChangeKind::Conflict]));
                let page = finish(collect(write.tx, &first)?);
                let next = page.body["next"].as_str().unwrap().to_owned();
                let mut second = query(Since::Cursor(next), Axis::Recorded, 1);
                second.kinds = Some(BTreeSet::from([ChangeKind::Conflict]));
                let rest = finish(collect(write.tx, &second)?);
                let mut partners = vec![
                    page.body["changes"][0]["withClaimId"]
                        .as_str()
                        .unwrap()
                        .to_owned(),
                    rest.body["changes"][0]["withClaimId"]
                        .as_str()
                        .unwrap()
                        .to_owned(),
                ];
                partners.sort();
                Ok(partners)
            })
            .unwrap();
        assert_eq!(page, ["other", "third"]);
    }

    #[test]
    fn a_corrected_capture_date_is_what_april_history_returns() {
        let dir = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(dir.path().join("knowledge.sqlite")).unwrap();
        let bytes = b"notes".to_vec();
        let retained = store
            .retain_new_source(RetainNewSource {
                origin: None,
                loupist_id: "owner".into(),
                at_ms: 1,
                title: "corrected".into(),
                source_class: SourceClass::PublisherDocument,
                origin_locator: "fixture:corrected".into(),
                provider_identity: "fixture".into(),
                authorship: Authorship::Publisher,
                coverage: Coverage::IncludedFull,
                origin_chat_id: None,
                request_ref: None,
                declared_digest: None,
                material: Some(RetainMaterial {
                    media_type: "text/plain".into(),
                    role: RepresentationRole::Original,
                    document_id: "corrected".into(),
                    parser_version: None,
                    parent_representation_id: None,
                    input_span_ids: Vec::new(),
                    selector: Selector::ByteRange {
                        start: 0,
                        end: bytes.len() as u64,
                    },
                    context_note: "corrected".into(),
                    bytes,
                }),
                publication_time: Some(day("2026-01-15")),
            })
            .unwrap();
        let span = retained.span_id.clone().unwrap();
        let lineage = store
            .transact(|write| {
                let record = write.create_record(CreateRecord {
                    loupist_id: "owner".into(),
                    kind: RecordKind::World,
                    title: "World".into(),
                    at_ms: 1,
                })?;
                write.seal_world(SealWorld {
                    loupist_id: "owner".into(),
                    record_id: record.record_id.clone(),
                    expected_revision_id: None,
                    at_ms: 2,
                    nodes: vec![GraphNode {
                        id: "node-a".into(),
                        name: "Node A".into(),
                        aliases: Vec::new(),
                        description: String::new(),
                        kind: "company".into(),
                        source_role: "subject".into(),
                        properties: BTreeMap::new(),
                    }],
                    claims: vec![claim("old", &span)],
                    annotations: Vec::new(),
                    dependencies: vec![EvidenceRevision {
                        record_id: retained.record_id.clone(),
                        revision_digest: retained.revision_digest.clone(),
                        role: DependencyRole::WorldSource,
                    }],
                })?;
                write.insert_world_episode(&episode(
                    "stale",
                    &record.record_id,
                    &retained.record_id,
                    &retained.revision_id,
                    &span,
                    3,
                    EpisodeChanges {
                        created: vec!["old".into()],
                        ..EpisodeChanges::default()
                    },
                    "2026-05-01",
                ))?;
                let world_revision: String = write.tx.query_row(
                    "SELECT revision_id FROM knowledge_record_heads WHERE record_id = ?1",
                    [&record.record_id],
                    |row| row.get(0),
                )?;
                crate::commands::knowledge::as_of::lineage_as_of(
                    write,
                    write.tx,
                    "owner",
                    &world_revision,
                    "old",
                    crate::commands::knowledge::as_of::Cutoff(
                        NaiveDate::from_ymd_opt(2026, 4, 30).unwrap(),
                    ),
                )
            })
            .unwrap()
            .unwrap();
        assert_eq!(
            lineage["created"]["publicationTime"]["value"], "2026-01-15",
            "{lineage}"
        );
        assert!(!lineage.to_string().contains("2026-05-01"), "{lineage}");
    }

    #[test]
    fn an_unseen_target_is_not_paired_by_its_company_or_form() {
        let dir = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(dir.path().join("knowledge.sqlite")).unwrap();
        let older = filing(&store, "q2-visible", "2026-06-01", "LMP", "10-Q");
        let newer = filing(&store, "q3-hidden", "2026-09-01", "LMP", "10-Q");
        let error = store
            .transact(|write| {
                between(
                    write,
                    &BetweenQuery {
                        loupist_id: "owner".into(),
                        to: newer,
                        from: None,
                        cutoff: Some(NaiveDate::from_ymd_opt(2026, 7, 1).unwrap()),
                    },
                )
            })
            .unwrap_err();
        assert_eq!(error_code(&error), Some("between_needs_from"));
        let _ = older;
    }

    #[test]
    fn an_older_filing_does_not_return_a_claim_whose_evidence_is_wholly_later() {
        let dir = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(dir.path().join("knowledge.sqlite")).unwrap();
        let retain = |title: &str, published: &str| {
            let bytes = format!(r#"{{"company":{{"symbol":"LMP"}},"form":"10-Q","id":"{title}"}}"#)
                .into_bytes();
            let end = bytes.len() as u64;
            store
                .retain_new_source(RetainNewSource {
                    origin: None,
                    loupist_id: "owner".into(),
                    at_ms: 1,
                    title: title.into(),
                    source_class: SourceClass::PublisherDocument,
                    origin_locator: format!("fixture:{title}"),
                    provider_identity: "fixture".into(),
                    authorship: Authorship::Publisher,
                    coverage: Coverage::IncludedFull,
                    origin_chat_id: None,
                    request_ref: None,
                    declared_digest: None,
                    material: Some(RetainMaterial {
                        media_type: "text/plain".into(),
                        role: RepresentationRole::Original,
                        document_id: title.into(),
                        parser_version: None,
                        parent_representation_id: None,
                        input_span_ids: Vec::new(),
                        selector: Selector::ByteRange { start: 0, end },
                        context_note: title.into(),
                        bytes,
                    }),
                    publication_time: Some(day(published)),
                })
                .unwrap()
        };
        let march = retain("march", "2026-03-01");
        let april = retain("april", "2026-04-01");
        let may = retain("may", "2026-05-01");
        let may_span = may.span_id.clone().unwrap();
        let body = store
            .transact(|write| {
                let record = write.create_record(CreateRecord {
                    loupist_id: "owner".into(),
                    kind: RecordKind::World,
                    title: "World".into(),
                    at_ms: 1,
                })?;
                write.seal_world(SealWorld {
                    loupist_id: "owner".into(),
                    record_id: record.record_id.clone(),
                    expected_revision_id: None,
                    at_ms: 5_000,
                    nodes: vec![GraphNode {
                        id: "node-a".into(),
                        name: "Node A".into(),
                        aliases: Vec::new(),
                        description: String::new(),
                        kind: "company".into(),
                        source_role: "subject".into(),
                        properties: BTreeMap::new(),
                    }],
                    claims: vec![claim("later-only", &may_span)],
                    annotations: Vec::new(),
                    dependencies: vec![
                        EvidenceRevision {
                            record_id: march.record_id.clone(),
                            revision_digest: march.revision_digest.clone(),
                            role: DependencyRole::WorldSource,
                        },
                        EvidenceRevision {
                            record_id: april.record_id.clone(),
                            revision_digest: april.revision_digest.clone(),
                            role: DependencyRole::WorldSource,
                        },
                        EvidenceRevision {
                            record_id: may.record_id.clone(),
                            revision_digest: may.revision_digest.clone(),
                            role: DependencyRole::WorldSource,
                        },
                    ],
                })?;
                write.insert_world_episode(&episode(
                    "march",
                    &record.record_id,
                    &march.record_id,
                    &march.revision_id,
                    &march.span_id.clone().unwrap(),
                    1_000,
                    EpisodeChanges {
                        created: vec!["later-only".into()],
                        ..EpisodeChanges::default()
                    },
                    "2026-03-01",
                ))?;
                between(
                    write,
                    &BetweenQuery {
                        loupist_id: "owner".into(),
                        to: april.record_id,
                        from: Some(march.record_id),
                        cutoff: Some(NaiveDate::from_ymd_opt(2026, 4, 30).unwrap()),
                    },
                )
            })
            .unwrap();
        assert!(
            body["not_restated"].as_array().unwrap().is_empty(),
            "{body}"
        );
        assert!(!body.to_string().contains("later-only"), "{body}");
    }

    #[test]
    fn an_asset_replay_keeps_a_production_lens_alert() {
        use crate::knowledge::lens_page::{
            AnchorInput, Basis, BasisRef, FilingOrigin, LensOp, PageDelta, PageKind, PageTarget,
            SectionKey,
        };
        let dir = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(dir.path().join("knowledge.sqlite")).unwrap();
        let bytes = b"notes".to_vec();
        let retained = store
            .retain_new_source(RetainNewSource {
                origin: None,
                loupist_id: "owner".into(),
                at_ms: 1,
                title: "notes".into(),
                source_class: SourceClass::PublisherDocument,
                origin_locator: "fixture:notes".into(),
                provider_identity: "fixture".into(),
                authorship: Authorship::Publisher,
                coverage: Coverage::IncludedFull,
                origin_chat_id: None,
                request_ref: None,
                declared_digest: None,
                material: Some(RetainMaterial {
                    media_type: "text/plain".into(),
                    role: RepresentationRole::Original,
                    document_id: "notes".into(),
                    parser_version: None,
                    parent_representation_id: None,
                    input_span_ids: Vec::new(),
                    selector: Selector::ByteRange {
                        start: 0,
                        end: bytes.len() as u64,
                    },
                    context_note: "notes".into(),
                    bytes,
                }),
                publication_time: Some(day("2026-04-01")),
            })
            .unwrap();
        let span = retained.span_id.unwrap();
        let at = chrono::DateTime::parse_from_rfc3339("2026-04-01T12:00:00Z")
            .unwrap()
            .timestamp_millis();
        let body = store
            .transact(|write| {
                let record = write.create_record(CreateRecord {
                    loupist_id: "owner".into(),
                    kind: RecordKind::World,
                    title: "World".into(),
                    at_ms: 1,
                })?;
                write.seal_world(SealWorld {
                    loupist_id: "owner".into(),
                    record_id: record.record_id,
                    expected_revision_id: None,
                    at_ms: at,
                    nodes: vec![GraphNode {
                        id: "node-a".into(),
                        name: "Node A".into(),
                        aliases: Vec::new(),
                        description: String::new(),
                        kind: "company".into(),
                        source_role: "subject".into(),
                        properties: BTreeMap::new(),
                    }],
                    claims: vec![claim("kept", &span)],
                    annotations: Vec::new(),
                    dependencies: vec![EvidenceRevision {
                        record_id: retained.record_id.clone(),
                        revision_digest: retained.revision_digest.clone(),
                        role: DependencyRole::WorldSource,
                    }],
                })?;
                write.file(
                    "owner",
                    &FilingOrigin::User {
                        chat_id: None,
                        message_id: None,
                    },
                    vec![PageDelta {
                        target: PageTarget::Create {
                            kind: PageKind::Subject,
                            title: "Node A".into(),
                            anchors: vec![AnchorInput::Node {
                                node_id: "node-a".into(),
                            }],
                        },
                        ops: vec![LensOp::Put {
                            key: SectionKey::new("facts").unwrap(),
                            heading: "Facts".into(),
                            markdown: "Node A reports it.".into(),
                            basis: Basis {
                                refs: vec![BasisRef::Claim {
                                    claim_id: "kept".into(),
                                }],
                                uncertainty: None,
                            },
                            expected: None,
                        }],
                    }],
                    at - 1,
                )?;
                crate::knowledge::lens::contest_pages(
                    write.tx,
                    "owner",
                    &BTreeSet::from(["kept".into()]),
                    at,
                )?;
                let mut request = query(
                    Since::Millis {
                        ms: 0,
                        inclusive: true,
                    },
                    Axis::Recorded,
                    30,
                );
                request.cutoff = Some(NaiveDate::from_ymd_opt(2026, 4, 1).unwrap());
                request.about = vec![AboutRef::Node {
                    id: "node-a".into(),
                }];
                Ok(finish(collect(write.tx, &request)?).body)
            })
            .unwrap();
        assert!(
            body["lens"].as_array().unwrap().iter().any(|row| {
                row["state"] == "contested"
                    && row["sectionKey"] == "facts"
                    && row["claimIds"]
                        .as_array()
                        .is_some_and(|ids| ids.iter().any(|id| id == "kept"))
            }),
            "{body}"
        );
    }

    #[test]
    fn reopening_a_claim_clears_it_from_the_former_ender() {
        let dir = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(dir.path().join("knowledge.sqlite")).unwrap();
        let bytes = b"notes".to_vec();
        let retained = store
            .retain_new_source(RetainNewSource {
                origin: None,
                loupist_id: "owner".into(),
                at_ms: 1,
                title: "notes".into(),
                source_class: SourceClass::PublisherDocument,
                origin_locator: "fixture:notes".into(),
                provider_identity: "fixture".into(),
                authorship: Authorship::Publisher,
                coverage: Coverage::IncludedFull,
                origin_chat_id: None,
                request_ref: None,
                declared_digest: None,
                material: Some(RetainMaterial {
                    media_type: "text/plain".into(),
                    role: RepresentationRole::Original,
                    document_id: "notes".into(),
                    parser_version: None,
                    parent_representation_id: None,
                    input_span_ids: Vec::new(),
                    selector: Selector::ByteRange {
                        start: 0,
                        end: bytes.len() as u64,
                    },
                    context_note: "notes".into(),
                    bytes,
                }),
                publication_time: Some(day("2026-03-01")),
            })
            .unwrap();
        let span = retained.span_id.unwrap();
        let lineage = store
            .transact(|write| {
                let record = write.create_record(CreateRecord {
                    loupist_id: "owner".into(),
                    kind: RecordKind::World,
                    title: "World".into(),
                    at_ms: 1,
                })?;
                write.seal_world(SealWorld {
                    loupist_id: "owner".into(),
                    record_id: record.record_id.clone(),
                    expected_revision_id: None,
                    at_ms: 2,
                    nodes: vec![GraphNode {
                        id: "node-a".into(),
                        name: "Node A".into(),
                        aliases: Vec::new(),
                        description: String::new(),
                        kind: "company".into(),
                        source_role: "subject".into(),
                        properties: BTreeMap::new(),
                    }],
                    claims: vec![claim("subject", &span), claim("ender", &span)],
                    annotations: Vec::new(),
                    dependencies: vec![EvidenceRevision {
                        record_id: retained.record_id.clone(),
                        revision_digest: retained.revision_digest,
                        role: DependencyRole::WorldSource,
                    }],
                })?;
                write.insert_world_episode(&episode(
                    "end",
                    &record.record_id,
                    &retained.record_id,
                    &retained.revision_id,
                    &span,
                    1_000,
                    EpisodeChanges {
                        ended: vec![EpisodeEnd {
                            claim: "subject".into(),
                            by: Some("ender".into()),
                            end: day("2026-03-01"),
                            basis: EndBasis::Stated,
                        }],
                        ..EpisodeChanges::default()
                    },
                    "2026-03-01",
                ))?;
                write.insert_world_episode(&episode(
                    "reopen",
                    &record.record_id,
                    &retained.record_id,
                    &retained.revision_id,
                    &span,
                    2_000,
                    EpisodeChanges {
                        reopened: vec!["subject".into()],
                        ..EpisodeChanges::default()
                    },
                    "2026-04-01",
                ))?;
                claim_lineage(write.tx, "owner", "ender")
            })
            .unwrap();
        let ended: Vec<_> = lineage["endedClaims"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|id| id.as_str())
            .collect();
        assert!(ended.is_empty(), "{lineage}");
        assert_eq!(lineage["endings"][0]["episodeId"], "end", "{lineage}");
        assert_eq!(lineage["endings"][0]["claimId"], "subject");
    }

    #[test]
    fn an_april_comparison_receipt_does_not_keep_a_may_ending_or_span() {
        let dir = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(dir.path().join("knowledge.sqlite")).unwrap();
        let retain = |title: &str, published: &str| {
            let bytes = title.as_bytes().to_vec();
            let end = bytes.len() as u64;
            store
                .retain_new_source(RetainNewSource {
                    origin: None,
                    loupist_id: "owner".into(),
                    at_ms: 1,
                    title: title.into(),
                    source_class: SourceClass::PublisherDocument,
                    origin_locator: format!("fixture:{title}"),
                    provider_identity: "fixture".into(),
                    authorship: Authorship::Publisher,
                    coverage: Coverage::IncludedFull,
                    origin_chat_id: None,
                    request_ref: None,
                    declared_digest: None,
                    material: Some(RetainMaterial {
                        media_type: "text/plain".into(),
                        role: RepresentationRole::Original,
                        document_id: title.into(),
                        parser_version: None,
                        parent_representation_id: None,
                        input_span_ids: Vec::new(),
                        selector: Selector::ByteRange { start: 0, end },
                        context_note: title.into(),
                        bytes,
                    }),
                    publication_time: Some(day(published)),
                })
                .unwrap()
        };
        let march = retain("march", "2026-03-01");
        let april = retain("april", "2026-04-01");
        let may = retain("may", "2026-05-01");
        let march_span = march.span_id.clone().unwrap();
        let may_span = may.span_id.clone().unwrap();
        let claim = store
            .transact(|write| {
                let record = write.create_record(CreateRecord {
                    loupist_id: "owner".into(),
                    kind: RecordKind::World,
                    title: "World".into(),
                    at_ms: 1,
                })?;
                let mut kept = claim("kept", &march_span);
                kept.invalid_at = Some(day("2026-05-01"));
                kept.evidence.push(EvidenceBinding {
                    span_id: may_span.clone(),
                    role: BindingRole::ExtractedFrom,
                    attribution: "fixture".into(),
                    figure_check: None,
                    passage: None,
                });
                write.seal_world(SealWorld {
                    loupist_id: "owner".into(),
                    record_id: record.record_id.clone(),
                    expected_revision_id: None,
                    at_ms: 5_000,
                    nodes: vec![GraphNode {
                        id: "node-a".into(),
                        name: "Node A".into(),
                        aliases: Vec::new(),
                        description: String::new(),
                        kind: "company".into(),
                        source_role: "subject".into(),
                        properties: BTreeMap::new(),
                    }],
                    claims: vec![kept],
                    annotations: Vec::new(),
                    dependencies: vec![
                        EvidenceRevision {
                            record_id: march.record_id.clone(),
                            revision_digest: march.revision_digest.clone(),
                            role: DependencyRole::WorldSource,
                        },
                        EvidenceRevision {
                            record_id: april.record_id.clone(),
                            revision_digest: april.revision_digest.clone(),
                            role: DependencyRole::WorldSource,
                        },
                        EvidenceRevision {
                            record_id: may.record_id.clone(),
                            revision_digest: may.revision_digest.clone(),
                            role: DependencyRole::WorldSource,
                        },
                    ],
                })?;
                write.insert_world_episode(&episode(
                    "march",
                    &record.record_id,
                    &march.record_id,
                    &march.revision_id,
                    &march_span,
                    1_000,
                    EpisodeChanges {
                        created: vec!["kept".into()],
                        ..EpisodeChanges::default()
                    },
                    "2026-03-01",
                ))?;
                let cutoff = Some(NaiveDate::from_ymd_opt(2026, 4, 30).unwrap());
                let body = between(
                    write,
                    &BetweenQuery {
                        loupist_id: "owner".into(),
                        to: april.record_id,
                        from: Some(march.record_id),
                        cutoff,
                    },
                )?;
                let (_, _, _, claims) = comparison_delivery(write, "owner", &body, cutoff)?;
                Ok(claims.into_iter().find(|claim| claim.id == "kept"))
            })
            .unwrap()
            .unwrap();
        assert!(claim.invalid_at.is_none(), "{claim:?}");
        assert_eq!(
            claim
                .evidence
                .iter()
                .map(|binding| binding.span_id.as_str())
                .collect::<Vec<_>>(),
            vec![march_span.as_str()]
        );
    }

    #[test]
    fn a_later_source_identity_does_not_label_the_other_sides_claim() {
        let dir = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(dir.path().join("knowledge.sqlite")).unwrap();
        let retain = |title: &str, published: &str| {
            let bytes = title.as_bytes().to_vec();
            let end = bytes.len() as u64;
            store
                .retain_new_source(RetainNewSource {
                    origin: None,
                    loupist_id: "owner".into(),
                    at_ms: 1,
                    title: title.into(),
                    source_class: SourceClass::PublisherDocument,
                    origin_locator: format!("fixture:{title}"),
                    provider_identity: "fixture".into(),
                    authorship: Authorship::Publisher,
                    coverage: Coverage::IncludedFull,
                    origin_chat_id: None,
                    request_ref: None,
                    declared_digest: None,
                    material: Some(RetainMaterial {
                        media_type: "text/plain".into(),
                        role: RepresentationRole::Original,
                        document_id: title.into(),
                        parser_version: None,
                        parent_representation_id: None,
                        input_span_ids: Vec::new(),
                        selector: Selector::ByteRange { start: 0, end },
                        context_note: title.into(),
                        bytes,
                    }),
                    publication_time: Some(day(published)),
                })
                .unwrap()
        };
        let mixed = retain("q3-hidden", "2026-03-01");
        let visible = retain("q2-visible", "2026-06-01");
        let mixed_span = mixed.span_id.clone().unwrap();
        let visible_span = visible.span_id.clone().unwrap();
        let body = store
            .transact(|write| {
                let stored = super::super::revision::load_revision(write.tx, &mixed.revision_id)?;
                let mut payload: Value = serde_json::from_str(&stored.payload_json)?;
                let mut later = payload["source"]["captures"][0].clone();
                later["captureId"] = json!("cap_later_identity");
                later["publicationTime"] =
                    json!({"precision": "date", "value": "2026-09-01", "approximate": false});
                if let Some(representations) = later["representations"].as_array_mut() {
                    for representation in representations {
                        representation["representationId"] = json!("rep_later_identity");
                        representation["spans"] = json!([]);
                    }
                }
                payload["source"]["captures"]
                    .as_array_mut()
                    .unwrap()
                    .push(later);
                payload["source"]["title"] = json!("q3-hidden");
                payload["source"]["originLocator"] = json!("https://hidden.example/q3");
                let record =
                    super::super::authority::require_record(write.tx, "owner", &mixed.record_id)?;
                let published = super::super::revision::publish_revision(
                    write.tx,
                    &record,
                    "owner",
                    Some(&mixed.revision_id),
                    &payload,
                    &[],
                    40,
                )?;
                let world = write.create_record(CreateRecord {
                    loupist_id: "owner".into(),
                    kind: RecordKind::World,
                    title: "World".into(),
                    at_ms: 1,
                })?;
                let mut kept = claim("kept", &visible_span);
                kept.evidence.insert(
                    0,
                    EvidenceBinding {
                        span_id: mixed_span.clone(),
                        role: BindingRole::ExtractedFrom,
                        attribution: "fixture".into(),
                        figure_check: None,
                        passage: None,
                    },
                );
                write.seal_world(SealWorld {
                    loupist_id: "owner".into(),
                    record_id: world.record_id.clone(),
                    expected_revision_id: None,
                    at_ms: 5_000,
                    nodes: vec![GraphNode {
                        id: "node-a".into(),
                        name: "Node A".into(),
                        aliases: Vec::new(),
                        description: String::new(),
                        kind: "company".into(),
                        source_role: "subject".into(),
                        properties: BTreeMap::new(),
                    }],
                    claims: vec![kept],
                    annotations: Vec::new(),
                    dependencies: vec![
                        EvidenceRevision {
                            record_id: mixed.record_id.clone(),
                            revision_digest: published.revision_digest,
                            role: DependencyRole::WorldSource,
                        },
                        EvidenceRevision {
                            record_id: visible.record_id.clone(),
                            revision_digest: visible.revision_digest.clone(),
                            role: DependencyRole::WorldSource,
                        },
                    ],
                })?;
                write.insert_world_episode(&episode(
                    "from",
                    &world.record_id,
                    &visible.record_id,
                    &visible.revision_id,
                    &visible_span,
                    1_000,
                    EpisodeChanges {
                        created: vec!["kept".into()],
                        ..EpisodeChanges::default()
                    },
                    "2026-06-01",
                ))?;
                between(
                    write,
                    &BetweenQuery {
                        loupist_id: "owner".into(),
                        to: mixed.record_id.clone(),
                        from: Some(visible.record_id.clone()),
                        cutoff: Some(NaiveDate::from_ymd_opt(2026, 7, 1).unwrap()),
                    },
                )
            })
            .unwrap();
        let records: Vec<_> = body["not_restated"][0]["evidence"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|item| item["recordId"].as_str())
            .collect();
        assert!(records.contains(&visible.record_id.as_str()), "{body}");
        assert!(!records.contains(&mixed.record_id.as_str()), "{body}");
        assert!(!body.to_string().contains("hidden.example"), "{body}");
        assert!(!body.to_string().contains("q3-hidden"), "{body}");
    }

    #[test]
    fn a_rejected_ending_leaves_the_visible_claim_in_its_group() {
        let dir = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(dir.path().join("knowledge.sqlite")).unwrap();
        let retain = |title: &str, published: &str| {
            let bytes = title.as_bytes().to_vec();
            let end = bytes.len() as u64;
            store
                .retain_new_source(RetainNewSource {
                    origin: None,
                    loupist_id: "owner".into(),
                    at_ms: 1,
                    title: title.into(),
                    source_class: SourceClass::PublisherDocument,
                    origin_locator: format!("fixture:{title}"),
                    provider_identity: "fixture".into(),
                    authorship: Authorship::Publisher,
                    coverage: Coverage::IncludedFull,
                    origin_chat_id: None,
                    request_ref: None,
                    declared_digest: None,
                    material: Some(RetainMaterial {
                        media_type: "text/plain".into(),
                        role: RepresentationRole::Original,
                        document_id: title.into(),
                        parser_version: None,
                        parent_representation_id: None,
                        input_span_ids: Vec::new(),
                        selector: Selector::ByteRange { start: 0, end },
                        context_note: title.into(),
                        bytes,
                    }),
                    publication_time: Some(day(published)),
                })
                .unwrap()
        };
        let earlier = retain("earlier", "2026-03-01");
        let later = retain("later", "2026-03-15");
        let span = earlier.span_id.clone().unwrap();
        let body = store
            .transact(|write| {
                let record = write.create_record(CreateRecord {
                    loupist_id: "owner".into(),
                    kind: RecordKind::World,
                    title: "World".into(),
                    at_ms: 1,
                })?;
                let mut kept = claim("kept", &span);
                kept.invalid_at = Some(day("2026-05-01"));
                write.seal_world(SealWorld {
                    loupist_id: "owner".into(),
                    record_id: record.record_id.clone(),
                    expected_revision_id: None,
                    at_ms: 5_000,
                    nodes: vec![GraphNode {
                        id: "node-a".into(),
                        name: "Node A".into(),
                        aliases: Vec::new(),
                        description: String::new(),
                        kind: "company".into(),
                        source_role: "subject".into(),
                        properties: BTreeMap::new(),
                    }],
                    claims: vec![kept],
                    annotations: Vec::new(),
                    dependencies: vec![
                        EvidenceRevision {
                            record_id: earlier.record_id.clone(),
                            revision_digest: earlier.revision_digest.clone(),
                            role: DependencyRole::WorldSource,
                        },
                        EvidenceRevision {
                            record_id: later.record_id.clone(),
                            revision_digest: later.revision_digest.clone(),
                            role: DependencyRole::WorldSource,
                        },
                    ],
                })?;
                write.insert_world_episode(&episode(
                    "from",
                    &record.record_id,
                    &earlier.record_id,
                    &earlier.revision_id,
                    &span,
                    1_000,
                    EpisodeChanges {
                        created: vec!["kept".into()],
                        ..EpisodeChanges::default()
                    },
                    "2026-03-01",
                ))?;
                write.insert_world_episode(&episode(
                    "to",
                    &record.record_id,
                    &later.record_id,
                    &later.revision_id,
                    &later.span_id.clone().unwrap(),
                    2_000,
                    EpisodeChanges {
                        ended: vec![EpisodeEnd {
                            claim: "kept".into(),
                            by: None,
                            end: day("2026-05-01"),
                            basis: EndBasis::Stated,
                        }],
                        ..EpisodeChanges::default()
                    },
                    "2026-03-15",
                ))?;
                between(
                    write,
                    &BetweenQuery {
                        loupist_id: "owner".into(),
                        to: later.record_id,
                        from: Some(earlier.record_id),
                        cutoff: Some(NaiveDate::from_ymd_opt(2026, 4, 30).unwrap()),
                    },
                )
            })
            .unwrap();
        assert!(body["ended_by_to"].as_array().unwrap().is_empty(), "{body}");
        assert_eq!(body["not_restated"][0]["claimId"], "kept", "{body}");
    }

    #[test]
    fn a_truncated_section_summary_does_not_drop_a_later_alert() {
        let dir = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(dir.path().join("knowledge.sqlite")).unwrap();
        let bytes = b"notes".to_vec();
        let retained = store
            .retain_new_source(RetainNewSource {
                origin: None,
                loupist_id: "owner".into(),
                at_ms: 1,
                title: "notes".into(),
                source_class: SourceClass::PublisherDocument,
                origin_locator: "fixture:notes".into(),
                provider_identity: "fixture".into(),
                authorship: Authorship::Publisher,
                coverage: Coverage::IncludedFull,
                origin_chat_id: None,
                request_ref: None,
                declared_digest: None,
                material: Some(RetainMaterial {
                    media_type: "text/plain".into(),
                    role: RepresentationRole::Original,
                    document_id: "notes".into(),
                    parser_version: None,
                    parent_representation_id: None,
                    input_span_ids: Vec::new(),
                    selector: Selector::ByteRange {
                        start: 0,
                        end: bytes.len() as u64,
                    },
                    context_note: "notes".into(),
                    bytes,
                }),
                publication_time: Some(day("2026-04-01")),
            })
            .unwrap();
        let span = retained.span_id.unwrap();
        let at = chrono::DateTime::parse_from_rfc3339("2026-04-01T12:00:00Z")
            .unwrap()
            .timestamp_millis();
        let lens = store
            .file_method(
                "owner",
                "Long page",
                "Notes.",
                &crate::knowledge::test_support::turn_origin("chat", "msg"),
                at - 1,
            )
            .unwrap();
        let body = store
            .transact(|write| {
                let record = write.create_record(CreateRecord {
                    loupist_id: "owner".into(),
                    kind: RecordKind::World,
                    title: "World".into(),
                    at_ms: 1,
                })?;
                write.seal_world(SealWorld {
                    loupist_id: "owner".into(),
                    record_id: record.record_id,
                    expected_revision_id: None,
                    at_ms: at,
                    nodes: vec![GraphNode {
                        id: "node-a".into(),
                        name: "Node A".into(),
                        aliases: Vec::new(),
                        description: String::new(),
                        kind: "company".into(),
                        source_role: "subject".into(),
                        properties: BTreeMap::new(),
                    }],
                    claims: vec![claim("kept", &span)],
                    annotations: Vec::new(),
                    dependencies: vec![EvidenceRevision {
                        record_id: retained.record_id,
                        revision_digest: retained.revision_digest,
                        role: DependencyRole::WorldSource,
                    }],
                })?;
                write.tx.execute(
                    "INSERT INTO knowledge_growth_events (
                        event_id, loupist_id, at_ms, kind, record_id, actor, summary, after_summary, origin
                     ) VALUES (
                        'gev_truncated', 'owner', ?1, 'lens_superseded', ?2, 'processing', 'sections', 'early', ?3
                     )",
                    params![
                        at,
                        lens.record_id,
                        json!({"sections": {"early": [], "late-section": ["kept"]}}).to_string()
                    ],
                )?;
                let mut request = query(
                    Since::Millis {
                        ms: 0,
                        inclusive: true,
                    },
                    Axis::Recorded,
                    30,
                );
                request.cutoff = Some(NaiveDate::from_ymd_opt(2026, 4, 1).unwrap());
                request.about = vec![AboutRef::Node {
                    id: "node-a".into(),
                }];
                Ok(finish(collect(write.tx, &request)?).body)
            })
            .unwrap();
        assert!(
            body["lens"].as_array().unwrap().iter().any(|row| {
                row["sectionKey"] == "late-section"
                    && row["claimIds"]
                        .as_array()
                        .is_some_and(|ids| ids.iter().any(|id| id == "kept"))
            }),
            "{body}"
        );
    }

    #[test]
    fn unresolved_evidence_is_marked_instead_of_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(dir.path().join("knowledge.sqlite")).unwrap();
        let retain = |title: &str, published: &str| {
            let bytes = title.as_bytes().to_vec();
            let end = bytes.len() as u64;
            store
                .retain_new_source(RetainNewSource {
                    origin: None,
                    loupist_id: "owner".into(),
                    at_ms: 1,
                    title: title.into(),
                    source_class: SourceClass::PublisherDocument,
                    origin_locator: format!("fixture:{title}"),
                    provider_identity: "fixture".into(),
                    authorship: Authorship::Publisher,
                    coverage: Coverage::IncludedFull,
                    origin_chat_id: None,
                    request_ref: None,
                    declared_digest: None,
                    material: Some(RetainMaterial {
                        media_type: "text/plain".into(),
                        role: RepresentationRole::Original,
                        document_id: title.into(),
                        parser_version: None,
                        parent_representation_id: None,
                        input_span_ids: Vec::new(),
                        selector: Selector::ByteRange { start: 0, end },
                        context_note: title.into(),
                        bytes,
                    }),
                    publication_time: Some(day(published)),
                })
                .unwrap()
        };
        let mixed = retain("hidden-span", "2026-03-01");
        let visible = retain("visible-side", "2026-06-01");
        let mixed_span = mixed.span_id.clone().unwrap();
        let body = store
            .transact(|write| {
                let stored = super::super::revision::load_revision(write.tx, &mixed.revision_id)?;
                let mut payload: Value = serde_json::from_str(&stored.payload_json)?;
                let mut later = payload["source"]["captures"][0].clone();
                later["captureId"] = json!("cap_unresolved");
                later["publicationTime"] =
                    json!({"precision": "date", "value": "2026-09-01", "approximate": false});
                if let Some(representations) = later["representations"].as_array_mut() {
                    for representation in representations {
                        representation["representationId"] = json!("rep_unresolved");
                        representation["spans"] = json!([]);
                    }
                }
                payload["source"]["captures"]
                    .as_array_mut()
                    .unwrap()
                    .push(later);
                let record =
                    super::super::authority::require_record(write.tx, "owner", &mixed.record_id)?;
                let published = super::super::revision::publish_revision(
                    write.tx,
                    &record,
                    "owner",
                    Some(&mixed.revision_id),
                    &payload,
                    &[],
                    40,
                )?;
                let world = write.create_record(CreateRecord {
                    loupist_id: "owner".into(),
                    kind: RecordKind::World,
                    title: "World".into(),
                    at_ms: 1,
                })?;
                write.seal_world(SealWorld {
                    loupist_id: "owner".into(),
                    record_id: world.record_id.clone(),
                    expected_revision_id: None,
                    at_ms: 5_000,
                    nodes: vec![GraphNode {
                        id: "node-a".into(),
                        name: "Node A".into(),
                        aliases: Vec::new(),
                        description: String::new(),
                        kind: "company".into(),
                        source_role: "subject".into(),
                        properties: BTreeMap::new(),
                    }],
                    claims: vec![claim("kept", &mixed_span)],
                    annotations: Vec::new(),
                    dependencies: vec![
                        EvidenceRevision {
                            record_id: mixed.record_id.clone(),
                            revision_digest: published.revision_digest,
                            role: DependencyRole::WorldSource,
                        },
                        EvidenceRevision {
                            record_id: visible.record_id.clone(),
                            revision_digest: visible.revision_digest,
                            role: DependencyRole::WorldSource,
                        },
                    ],
                })?;
                write.insert_world_episode(&episode(
                    "from",
                    &world.record_id,
                    &visible.record_id,
                    &visible.revision_id,
                    &visible.span_id.unwrap(),
                    1_000,
                    EpisodeChanges {
                        created: vec!["kept".into()],
                        ..EpisodeChanges::default()
                    },
                    "2026-06-01",
                ))?;
                between(
                    write,
                    &BetweenQuery {
                        loupist_id: "owner".into(),
                        to: mixed.record_id,
                        from: Some(visible.record_id),
                        cutoff: Some(NaiveDate::from_ymd_opt(2026, 7, 1).unwrap()),
                    },
                )
            })
            .unwrap();
        assert_eq!(body["not_restated"][0]["evidence"], json!([]), "{body}");
        assert_eq!(
            body["not_restated"][0]["evidenceUnresolved"], true,
            "{body}"
        );
    }

    #[test]
    fn a_second_contested_claim_does_not_replay_the_first() {
        use crate::knowledge::lens_page::{
            Basis, BasisRef, FilingOrigin, LensOp, PageDelta, PageKind, PageTarget, SectionKey,
        };
        let dir = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(dir.path().join("knowledge.sqlite")).unwrap();
        let bytes = b"notes".to_vec();
        let retained = store
            .retain_new_source(RetainNewSource {
                origin: None,
                loupist_id: "owner".into(),
                at_ms: 1,
                title: "notes".into(),
                source_class: SourceClass::PublisherDocument,
                origin_locator: "fixture:notes".into(),
                provider_identity: "fixture".into(),
                authorship: Authorship::Publisher,
                coverage: Coverage::IncludedFull,
                origin_chat_id: None,
                request_ref: None,
                declared_digest: None,
                material: Some(RetainMaterial {
                    media_type: "text/plain".into(),
                    role: RepresentationRole::Original,
                    document_id: "notes".into(),
                    parser_version: None,
                    parent_representation_id: None,
                    input_span_ids: Vec::new(),
                    selector: Selector::ByteRange {
                        start: 0,
                        end: bytes.len() as u64,
                    },
                    context_note: "notes".into(),
                    bytes,
                }),
                publication_time: Some(day("2026-04-01")),
            })
            .unwrap();
        let span = retained.span_id.unwrap();
        store
            .transact(|write| {
                let record = write.create_record(CreateRecord {
                    loupist_id: "owner".into(),
                    kind: RecordKind::World,
                    title: "World".into(),
                    at_ms: 1,
                })?;
                write.seal_world(SealWorld {
                    loupist_id: "owner".into(),
                    record_id: record.record_id,
                    expected_revision_id: None,
                    at_ms: 2,
                    nodes: vec![GraphNode {
                        id: "node-a".into(),
                        name: "Node A".into(),
                        aliases: Vec::new(),
                        description: String::new(),
                        kind: "company".into(),
                        source_role: "subject".into(),
                        properties: BTreeMap::new(),
                    }],
                    claims: vec![claim("alpha", &span), claim("beta", &span)],
                    annotations: Vec::new(),
                    dependencies: vec![EvidenceRevision {
                        record_id: retained.record_id,
                        revision_digest: retained.revision_digest,
                        role: DependencyRole::WorldSource,
                    }],
                })
            })
            .unwrap();
        let origin = FilingOrigin::User {
            chat_id: None,
            message_id: None,
        };
        let cite = |ids: &[&str]| Basis {
            refs: ids
                .iter()
                .map(|id| BasisRef::Claim {
                    claim_id: (*id).to_owned(),
                })
                .collect(),
            uncertainty: None,
        };
        let filed = store
            .transact(|write| {
                write.file(
                    "owner",
                    &origin,
                    vec![PageDelta {
                        target: PageTarget::Create {
                            kind: PageKind::Method,
                            title: "Notes".into(),
                            anchors: Vec::new(),
                        },
                        ops: vec![LensOp::Put {
                            key: SectionKey::new("facts").unwrap(),
                            heading: "Facts".into(),
                            markdown: "Alpha and beta hold.".into(),
                            basis: cite(&["alpha", "beta"]),
                            expected: None,
                        }],
                    }],
                    1_000,
                )
            })
            .unwrap();
        let _ = filed;
        store
            .transact(|write| {
                super::super::lens::contest_pages(
                    write.tx,
                    "owner",
                    &BTreeSet::from(["alpha".to_owned()]),
                    2_000,
                )?;
                super::super::lens::contest_pages(
                    write.tx,
                    "owner",
                    &BTreeSet::from(["beta".to_owned()]),
                    3_000,
                )
            })
            .unwrap();
        let origin_json: String = store
            .transact(|write| {
                write
                    .tx
                    .query_row(
                        "SELECT origin FROM knowledge_growth_events
                         WHERE loupist_id = 'owner' AND kind = 'lens_contested'
                         ORDER BY at_ms DESC LIMIT 1",
                        [],
                        |row| row.get(0),
                    )
                    .map_err(super::super::KnowledgeError::from)
            })
            .unwrap();
        let value: Value = serde_json::from_str(&origin_json).unwrap();
        let ids = value["sections"]["facts"].as_array().unwrap();
        assert!(ids.iter().any(|id| id == "beta"), "{value}");
        assert!(!ids.iter().any(|id| id == "alpha"), "{value}");
    }
}
