//! Host read of the World change feed. Claim rows go through [`super::as_of::world_view_as_of`].
use std::collections::{BTreeMap, BTreeSet};

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::State;

use super::as_of::Cutoff;
use super::*;
use crate::app::{AppState, CommandError};
use crate::commands::assets::wire::SinceReadBlock;
use crate::knowledge::{
    world_changes::{self, AboutRef, Axis, ChangeKind, Collected, FeedQuery, Since},
    GraphClaim, GraphCoverage, GraphEdge, IndexingState, KnowledgeError, KnowledgeResult,
    KnowledgeWrite, QueryScan, TimePoint, WorldGraphView,
};

pub(in crate::commands) struct ChangeRead {
    pub body: Value,
    pub record_id: String,
    pub revision_id: String,
    pub revision_digest: String,
    pub view_json: String,
}

pub(in crate::commands) fn read_lineage(
    state: &AppState,
    loupist_id: &str,
    claim_id: &str,
    replay: Option<(&str, Cutoff)>,
) -> Result<Option<Value>, CommandError> {
    with_loupist_knowledge(state, loupist_id, |tx, blobs| {
        if let Some((revision_id, cutoff)) = replay {
            let write = KnowledgeWrite::bind(tx, blobs);
            return super::as_of::lineage_as_of(
                &write,
                tx,
                loupist_id,
                revision_id,
                claim_id,
                cutoff,
            );
        }
        world_changes::claim_lineage(tx, loupist_id, claim_id).map(Some)
    })
}

pub(in crate::commands) fn read_between(
    state: &AppState,
    loupist_id: &str,
    query: crate::knowledge::world_changes::BetweenQuery,
) -> Result<ChangeRead, CommandError> {
    with_loupist_knowledge(state, loupist_id, |tx, blobs| {
        let write = crate::knowledge::KnowledgeWrite::bind(tx, blobs);
        let body = crate::knowledge::world_changes::between(&write, &query)?;
        let (record_id, revision_id, revision_digest, claims) =
            crate::knowledge::world_changes::comparison_delivery(
                &write,
                loupist_id,
                &body,
                query.cutoff,
            )?;
        let view = json!({
            "recordId": record_id,
            "revisionId": revision_id,
            "revisionDigest": revision_digest,
            "edges": claims.iter().map(edge).collect::<Vec<_>>(),
        });
        Ok(ChangeRead {
            view_json: serde_json::to_string(&view)?,
            body,
            record_id,
            revision_id,
            revision_digest,
        })
    })
}

/// A followed asset in `about` scopes the feed by its page, which is made on
/// first use: a followed asset may not have one yet. An id that is not a
/// followed asset is refused as such.
fn ensure_about_pages(
    state: &AppState,
    loupist_id: &str,
    about: &[AboutRef],
) -> Result<(), CommandError> {
    for item in about {
        let AboutRef::Asset { id } = item else {
            continue;
        };
        if !crate::knowledge::assets::valid_asset_id(id) {
            continue;
        }
        let Some(asset) = state
            .store
            .asset_by_id(loupist_id, id)
            .map_err(super::map_knowledge_store)?
        else {
            return Err(CommandError::new(
                "invalid",
                format!("change_about_invalid: {id} is not an asset the user follows. Pass the id of a followed asset, as What the user follows lists it, or a World node id from world_search."),
            ));
        };
        crate::commands::assets::with_assets(state, loupist_id, true, |tx| {
            crate::knowledge::assets::ensure_asset_page(
                tx,
                loupist_id,
                &asset,
                crate::commands::now_ms(),
            )
        })?;
    }
    Ok(())
}

pub(in crate::commands) fn read_changes(
    state: &AppState,
    loupist_id: &str,
    query: FeedQuery,
) -> Result<ChangeRead, CommandError> {
    ensure_about_pages(state, loupist_id, &query.about)?;
    with_loupist_knowledge(state, loupist_id, |tx, blobs| {
        if query.cutoff.is_none() {
            crate::knowledge::reconcile_due_endings(tx, loupist_id, crate::commands::now_ms())?;
        }
        // Under a cutoff each reading's changes are those of the World that
        // read only what the cutoff knows (`as_of::episodes_as_of`).
        let replayed = match (
            query.cutoff,
            world_changes::world_head_revision(tx, loupist_id)?,
        ) {
            (Some(day), Some(revision_id)) => {
                let write = KnowledgeWrite::bind(tx, blobs);
                Some(
                    super::as_of::episodes_as_of(
                        &write,
                        tx,
                        loupist_id,
                        &revision_id,
                        Cutoff(day),
                    )?
                    .1,
                )
            }
            _ => None,
        };
        let mut collected = world_changes::collect_with(tx, &query, replayed.as_ref())?;
        if let Some(day) = query.cutoff {
            let write = KnowledgeWrite::bind(tx, blobs);
            project(&write, loupist_id, &mut collected, Cutoff(day))?;
            let mut known = BTreeMap::new();
            world_changes::keep_known_episodes(&mut collected, |record, revision, span| {
                let key = (record.to_owned(), revision.to_owned(), span.to_owned());
                if let Some(known) = known.get(&key) {
                    return Ok(*known);
                }
                let shown = super::as_of::source_captures_known(
                    &write,
                    loupist_id,
                    record,
                    (!revision.is_empty()).then_some(revision),
                    (!span.is_empty()).then_some(span),
                    Cutoff(day),
                )
                .unwrap_or(false);
                known.insert(key, shown);
                Ok(shown)
            })?;
            world_changes::conceal_endings(&mut collected);
        }
        let mut page = world_changes::finish(collected);
        world_changes::fill_source_titles(tx, &mut page.body)?;
        let view = json!({
            "recordId": page.record_id,
            "revisionId": page.revision_id,
            "revisionDigest": page.revision_digest,
            "edges": page.claims.iter().map(edge).collect::<Vec<_>>(),
        });
        Ok(ChangeRead {
            view_json: serde_json::to_string(&view)?,
            body: page.body,
            record_id: page.record_id,
            revision_id: page.revision_id,
            revision_digest: page.revision_digest,
        })
    })
}

fn edge(claim: &GraphClaim) -> Value {
    let (ended, end) = world_changes::ended_fields(claim);
    let mut row = json!({
        "id": claim.id,
        "relation": claim.relation,
        "endpoints": claim.endpoints,
        "status": claim.status.as_str(),
        "claim": claim,
    });
    if ended {
        row["ended"] = json!(true);
        row["end"] = json!(end);
    }
    row
}

fn project(
    write: &KnowledgeWrite<'_>,
    loupist_id: &str,
    collected: &mut Collected,
    cutoff: Cutoff,
) -> crate::knowledge::KnowledgeResult<()> {
    if collected.claims.is_empty() || collected.revision_id.is_empty() {
        return Ok(());
    }
    let mut view = WorldGraphView {
        source_targets: Default::default(),
        record_id: collected.world_record_id.clone(),
        revision_id: collected.revision_id.clone(),
        revision_digest: collected.revision_digest.clone(),
        nodes: Vec::new(),
        edges: collected
            .claims
            .values()
            .map(|claim| {
                let (ended, end) = world_changes::ended_fields(claim);
                GraphEdge {
                    id: claim.id.clone(),
                    relation: claim.relation.clone(),
                    endpoints: claim.endpoints.clone(),
                    status: claim.status,
                    claim: claim.clone(),
                    ended: ended.then_some(true),
                    end,
                }
            })
            .collect(),
        next_cursor: None,
        coverage: GraphCoverage::SealedSnapshot,
        indexing_state: IndexingState::SnapshotOnly,
        truncated: false,
        scan: QueryScan {
            strategy: "change_feed".into(),
            nodes_scanned: 0,
            claims_scanned: u32::try_from(collected.claims.len()).unwrap_or(u32::MAX),
        },
    };
    super::as_of::world_view_as_of(write, loupist_id, &mut view, None, cutoff)?;
    collected.claims = view
        .edges
        .into_iter()
        .map(|edge| (edge.claim.id.clone(), edge.claim))
        .collect();
    Ok(())
}

/// Bridge read of the change feed. `between.asset` picks the latest report and the one before it.
#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorldChangesRequest {
    pub loupist_id: String,
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub since: Option<String>,
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub axis: Option<String>,
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub about: Option<Vec<AboutInput>>,
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub kinds: Option<Vec<String>>,
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub limit: Option<u32>,
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub between: Option<BetweenInput>,
    /// A `since` read of World as of a calendar day (`YYYY-MM-DD`, today or
    /// earlier), as `WorldGraphQuery.asOf` reads it: the rows of the World
    /// that had read only what was published by then.
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub as_of: Option<String>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(untagged)]
pub enum AboutInput {
    Id(String),
    Ref(AboutRefInput),
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AboutRefInput {
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub node: Option<String>,
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub asset: Option<String>,
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub lens: Option<String>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BetweenInput {
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub asset: Option<String>,
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub to: Option<String>,
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub from: Option<String>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct WorldChangesView {
    pub mode: String,
    /// Null when the asset has no earlier report of the same company and form.
    pub pair: Option<ChangePair>,
    pub from: Option<ChangeSource>,
    pub to: Option<ChangeSource>,
    pub ended_by_to: Vec<ChangeClaim>,
    pub new_in_to: Vec<ChangeClaim>,
    pub restated: Vec<ChangeClaim>,
    pub not_restated: Vec<ChangeClaim>,
    pub conflicts: Vec<ChangeConflict>,
    #[cfg_attr(test, ts(type = "unknown"))]
    pub window: Option<Value>,
    pub pending: u32,
    pub failed: u32,
    #[cfg_attr(test, ts(type = "unknown[]"))]
    pub changes: Vec<Value>,
    #[cfg_attr(test, ts(type = "unknown[]"))]
    pub lens: Vec<Value>,
    pub first_run: bool,
    pub capped: bool,
    pub next: Option<String>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ChangePair {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ChangeSource {
    pub record_id: String,
    pub title: String,
    pub publication_time: Option<String>,
    pub episodes: u32,
    /// True while World is still reading the newer report.
    pub pending: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ChangeClaim {
    pub claim_id: String,
    pub proposition: String,
    pub relation: String,
    pub status: String,
    pub valid_at: Option<TimePoint>,
    pub invalid_at: Option<TimePoint>,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ChangeConflict {
    pub claim_id: String,
    pub with_claim_id: String,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClaimHistoryRequest {
    pub loupist_id: String,
    pub claim_id: String,
    /// The claim's history in World as of a calendar day (`YYYY-MM-DD`,
    /// today or earlier), as `WorldGraphQuery.asOf` reads it.
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub as_of: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ClaimHistory {
    #[cfg_attr(test, ts(type = "unknown"))]
    pub created: Option<Value>,
    #[cfg_attr(test, ts(type = "unknown[]"))]
    pub supported: Vec<Value>,
    #[cfg_attr(test, ts(type = "unknown"))]
    pub ended: Option<Value>,
    /// Endings a later reopen replaced. The current relationship is `ended`.
    #[serde(default)]
    #[cfg_attr(test, ts(type = "unknown[]"))]
    pub endings: Vec<Value>,
    pub ended_claims: Vec<String>,
    pub ended_by_claim_id: Option<String>,
}

#[tauri::command(async)]
pub fn knowledge_world_changes(
    request: WorldChangesRequest,
    state: State<'_, AppState>,
) -> Result<WorldChangesView, CommandError> {
    read_world_changes(state.inner(), request)
}

#[tauri::command(async)]
pub fn knowledge_world_claim_history(
    request: ClaimHistoryRequest,
    state: State<'_, AppState>,
) -> Result<ClaimHistory, CommandError> {
    claim_history(state.inner(), request)
}

pub(in crate::commands) fn claim_history(
    state: &AppState,
    request: ClaimHistoryRequest,
) -> Result<ClaimHistory, CommandError> {
    super::admit(state, &request.loupist_id)?;
    let cutoff = Cutoff::requested(request.as_of.as_deref())?;
    let head = match cutoff {
        Some(_) => super::with_loupist_knowledge(state, &request.loupist_id, |tx, _| {
            world_changes::world_head_revision(tx, &request.loupist_id)
        })?,
        None => None,
    };
    let replay = head.as_deref().zip(cutoff);
    let value = match read_lineage(state, &request.loupist_id, &request.claim_id, replay)? {
        Some(value) => value,
        // Under a date, a claim World did not hold then has no history.
        None if cutoff.is_some() => return Err(CommandError::not_found("claim")),
        None => return Err(CommandError::internal("Could not read the claim history.")),
    };
    serde_json::from_value(value)
        .map_err(|_| CommandError::internal("Could not read the claim history."))
}

pub(in crate::commands) fn since_read_block(
    state: &AppState,
    loupist_id: &str,
    asset_id: &str,
) -> Result<SinceReadBlock, CommandError> {
    let moved_at = super::with_loupist_knowledge(state, loupist_id, |tx, _blobs| {
        let moved: Option<i64> = tx.query_row(
            "SELECT MAX(at_ms) FROM knowledge_asset_moves WHERE loupist_id = ?1 AND asset_id = ?2",
            rusqlite::params![loupist_id, asset_id],
            |row| row.get(0),
        )?;
        Ok(moved)
    })?;
    asset_since_read(state, loupist_id, asset_id, moved_at.unwrap_or(0))
}

/// The asset feed block shared by the desktop bridge and the agent asset page:
/// up to eight changes after `since_ms`, the Lens sections that became
/// Superseded or Contested beside them (not behind them on the same page),
/// full totals and what is still pending.
pub(in crate::commands) fn asset_since_read(
    state: &AppState,
    loupist_id: &str,
    asset_id: &str,
    since_ms: i64,
) -> Result<SinceReadBlock, CommandError> {
    let feed = |kinds: BTreeSet<ChangeKind>| {
        read_changes(
            state,
            loupist_id,
            FeedQuery {
                loupist_id: loupist_id.to_owned(),
                since: Since::Millis {
                    ms: since_ms,
                    inclusive: false,
                },
                axis: Axis::Recorded,
                axis_explicit: true,
                about: vec![AboutRef::Asset {
                    id: asset_id.to_owned(),
                }],
                kinds: Some(kinds),
                limit: 8,
                first_run: false,
                cutoff: None,
            },
        )
    };
    let claims = feed(BTreeSet::from([
        ChangeKind::Added,
        ChangeKind::Supported,
        ChangeKind::Ended,
        ChangeKind::Conflict,
        ChangeKind::Disputed,
        ChangeKind::Withdrawn,
    ]))?
    .body;
    let lens = feed(BTreeSet::from([ChangeKind::Lens]))?.body;
    let mut counts = BTreeMap::new();
    for body in [&claims, &lens] {
        if let Some(object) = body["totals"].as_object() {
            for (kind, count) in object {
                if let Some(count) = count.as_u64().and_then(|count| u32::try_from(count).ok()) {
                    counts.insert(kind.clone(), count);
                }
            }
        }
    }
    Ok(SinceReadBlock {
        counts,
        changes: claims["changes"].as_array().cloned().unwrap_or_default(),
        lens: lens["lens"].as_array().cloned().unwrap_or_default(),
        pending: claims["pending"].as_u64().unwrap_or(0) as u32,
        failed: claims["failed"].as_u64().unwrap_or(0) as u32,
        capped: claims["capped"].as_bool().unwrap_or(false)
            || lens["capped"].as_bool().unwrap_or(false),
        next: claims["next"].as_str().unwrap_or("").to_owned(),
    })
}

pub(in crate::commands) fn read_world_changes(
    state: &AppState,
    request: WorldChangesRequest,
) -> Result<WorldChangesView, CommandError> {
    super::admit(state, &request.loupist_id)?;
    let cutoff = Cutoff::requested(request.as_of.as_deref())?;
    if request.between.is_some() && cutoff.is_some() {
        return Err(CommandError::new("invalid", "asOf applies to a since read"));
    }
    if request.between.is_some() && request.since.is_some() {
        return Err(CommandError::new(
            "invalid",
            "pass since or between, not both",
        ));
    }
    if let Some(between) = request.between {
        return read_between_request(state, &request.loupist_id, between);
    }
    let Some(since_text) = request.since.as_deref() else {
        return Err(CommandError::new("invalid", "pass since or between"));
    };
    read_since_request(state, &request, since_text, cutoff)
}

fn read_between_request(
    state: &AppState,
    loupist_id: &str,
    between: BetweenInput,
) -> Result<WorldChangesView, CommandError> {
    if between.asset.is_some() && between.to.is_some() {
        return Err(CommandError::new(
            "invalid",
            "pass an asset or a source, not both",
        ));
    }
    let asset = if let Some(asset_id) = between.asset.as_deref() {
        if !crate::knowledge::assets::valid_asset_id(asset_id) {
            return Err(super::map_knowledge(KnowledgeError::Invalid("asset_id")));
        }
        // Asset identity is in the app catalog. Resolve it before opening
        // the Loupist database, which owns the reports and World links.
        Some(
            state
                .store
                .asset_by_id(loupist_id, asset_id)
                .map_err(super::map_knowledge_store)?
                .ok_or_else(|| super::map_knowledge(KnowledgeError::NotFound))?,
        )
    } else {
        None
    };
    super::with_loupist_knowledge(state, loupist_id, |tx, blobs| {
        let write = crate::knowledge::KnowledgeWrite::bind(tx, blobs);
        if let Some(asset) = asset.as_ref() {
            let Some(pair) = world_changes::pair_for_asset(tx, loupist_id, asset)? else {
                return Ok(empty_between());
            };
            let body = world_changes::between(
                &write,
                &world_changes::BetweenQuery {
                    loupist_id: loupist_id.to_owned(),
                    to: pair.to_id.clone(),
                    from: Some(pair.from_id.clone()),
                    cutoff: None,
                },
            )?;
            return view_from_between(&body, &pair.from_id, &pair.to_id, pair.pending);
        }
        let Some(to) = between.to.clone() else {
            return Err(KnowledgeError::Invalid("between_needs_from"));
        };
        let body = world_changes::between(
            &write,
            &world_changes::BetweenQuery {
                loupist_id: loupist_id.to_owned(),
                to: to.clone(),
                from: between.from.clone(),
                cutoff: None,
            },
        )?;
        let pending = world_changes::source_is_pending(tx, loupist_id, &to)?;
        let from_id = body["from"]["recordId"].as_str().unwrap_or("").to_owned();
        view_from_between(&body, &from_id, &to, pending)
    })
}

fn read_since_request(
    state: &AppState,
    request: &WorldChangesRequest,
    since_text: &str,
    cutoff: Option<Cutoff>,
) -> Result<WorldChangesView, CommandError> {
    if since_text == "previous_run" {
        return Err(CommandError::new(
            "since_previous_run_unavailable",
            "since_previous_run_unavailable: this turn has no previous run start",
        ));
    }
    let since = if since_text.starts_with("wch1.") {
        Since::Cursor(since_text.to_owned())
    } else if let Ok(date) = NaiveDate::parse_from_str(since_text, "%Y-%m-%d") {
        Since::Date(date)
    } else {
        return Err(CommandError::new(
            "invalid",
            "a YYYY-MM-DD date or a next cursor",
        ));
    };
    let axis_explicit = request.axis.is_some();
    let axis = match request.axis.as_deref() {
        None => Axis::Recorded,
        Some("recorded") => Axis::Recorded,
        Some("published") => Axis::Published,
        Some(_) => {
            return Err(CommandError::new("invalid", "recorded or published"));
        }
    };
    let about_input = request.about.as_deref().unwrap_or(&[]);
    if about_input.len() > world_changes::MAX_ABOUT {
        return Err(CommandError::new("invalid", "change_about_invalid"));
    }
    let about = about_input
        .iter()
        .map(parse_about)
        .collect::<Result<Vec<_>, _>>()?;
    let kind_names = request.kinds.as_deref().unwrap_or(&[]);
    let kinds = if kind_names.is_empty() {
        None
    } else {
        let mut set = BTreeSet::new();
        for name in kind_names {
            let kind = ChangeKind::parse(name).ok_or_else(|| {
                CommandError::new(
                    "invalid",
                    "added, supported, ended, conflict, disputed, withdrawn, or lens",
                )
            })?;
            set.insert(kind);
        }
        Some(set)
    };
    let limit = match request.limit {
        None => world_changes::DEFAULT_LIMIT,
        Some(limit) if (1..=world_changes::MAX_LIMIT).contains(&limit) => limit,
        Some(_) => return Err(CommandError::new("invalid", "change_limit_invalid")),
    };
    let read = read_changes(
        state,
        &request.loupist_id,
        FeedQuery {
            loupist_id: request.loupist_id.clone(),
            since,
            axis,
            axis_explicit,
            about,
            kinds,
            limit,
            first_run: false,
            cutoff: cutoff.map(|cutoff| cutoff.0),
        },
    )?;
    let body = read.body;
    Ok(WorldChangesView {
        mode: "since".into(),
        pair: None,
        from: None,
        to: None,
        ended_by_to: Vec::new(),
        new_in_to: Vec::new(),
        restated: Vec::new(),
        not_restated: Vec::new(),
        conflicts: Vec::new(),
        window: body.get("window").cloned(),
        pending: u32_field(&body, "pending"),
        failed: u32_field(&body, "failed"),
        changes: body["changes"].as_array().cloned().unwrap_or_default(),
        lens: body["lens"].as_array().cloned().unwrap_or_default(),
        first_run: body["first_run"].as_bool().unwrap_or(false),
        capped: body["capped"].as_bool().unwrap_or(false),
        next: body["next"].as_str().map(str::to_owned),
    })
}

fn parse_about(input: &AboutInput) -> Result<AboutRef, CommandError> {
    match input {
        AboutInput::Id(id) if id.starts_with("ast_") => Ok(AboutRef::Asset { id: id.clone() }),
        AboutInput::Id(id) if id.starts_with("rec_") => Ok(AboutRef::Lens { id: id.clone() }),
        AboutInput::Id(id) => Ok(AboutRef::Node { id: id.clone() }),
        AboutInput::Ref(reference) => match (
            reference.node.clone(),
            reference.asset.clone(),
            reference.lens.clone(),
        ) {
            (Some(id), None, None) => Ok(AboutRef::Node { id }),
            (None, Some(id), None) => Ok(AboutRef::Asset { id }),
            (None, None, Some(id)) => Ok(AboutRef::Lens { id }),
            _ => Err(CommandError::new(
                "invalid",
                "a node id, an asset id, or a Lens record id",
            )),
        },
    }
}

fn view_from_between(
    body: &Value,
    from_id: &str,
    to_id: &str,
    pending: bool,
) -> KnowledgeResult<WorldChangesView> {
    let mut view = empty_between();
    view.pair = Some(ChangePair {
        from: from_id.to_owned(),
        to: to_id.to_owned(),
    });
    view.from = Some(change_source(&body["from"], false)?);
    view.to = Some(change_source(&body["to"], pending)?);
    view.ended_by_to = change_claims(&body["ended_by_to"])?;
    view.new_in_to = change_claims(&body["new_in_to"])?;
    view.restated = change_claims(&body["restated"])?;
    view.not_restated = change_claims(&body["not_restated"])?;
    view.conflicts = serde_json::from_value(body["conflicts"].clone())
        .map_err(|_| KnowledgeError::Invalid("change_feed"))?;
    Ok(view)
}

fn change_source(value: &Value, pending: bool) -> KnowledgeResult<ChangeSource> {
    Ok(ChangeSource {
        record_id: value["recordId"]
            .as_str()
            .ok_or(KnowledgeError::Invalid("change_feed"))?
            .to_owned(),
        title: value["title"].as_str().unwrap_or("").to_owned(),
        publication_time: value["publicationTime"].as_str().map(str::to_owned),
        episodes: u32_field(value, "episodes"),
        pending,
    })
}

fn change_claims(value: &Value) -> KnowledgeResult<Vec<ChangeClaim>> {
    serde_json::from_value(value.clone()).map_err(|_| KnowledgeError::Invalid("change_feed"))
}

fn u32_field(value: &Value, key: &str) -> u32 {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|number| u32::try_from(number).ok())
        .unwrap_or(0)
}

fn empty_between() -> WorldChangesView {
    WorldChangesView {
        mode: "between".into(),
        pair: None,
        from: None,
        to: None,
        ended_by_to: Vec::new(),
        new_in_to: Vec::new(),
        restated: Vec::new(),
        not_restated: Vec::new(),
        conflicts: Vec::new(),
        window: None,
        pending: 0,
        failed: 0,
        changes: Vec::new(),
        lens: Vec::new(),
        first_run: false,
        capped: false,
        next: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::assets::wire::{AssetReferenceFields, AssetReferenceOrigin};
    use crate::commands::tests::{profile, seed_profile};
    use crate::knowledge::{
        Authorship, Coverage, RepresentationRole, RetainMaterial, RetainNewSource, Selector,
        SourceClass, TimePrecision,
    };

    #[test]
    fn report_comparison_uses_the_catalog_asset_in_a_folder_loupist() {
        let temp = tempfile::tempdir().unwrap();
        let state = AppState::for_folder_test(temp.path().join("app"));
        let id = "loupist-a";
        seed_profile(
            &state.store,
            &profile(id, &temp.path().join("workspace"), 1),
        );
        let asset = state
            .store
            .asset_follow_name(
                id,
                "Lamppost Test Works",
                Some(AssetReferenceFields {
                    ticker: Some("LMP".into()),
                    exchange: Some("US".into()),
                    ..Default::default()
                }),
                AssetReferenceOrigin::User,
                2,
            )
            .unwrap();
        let (older, newer) = with_loupist_knowledge(&state, id, |tx, blobs| {
            let count: i64 = tx.query_row("SELECT COUNT(*) FROM assets", [], |row| row.get(0))?;
            assert_eq!(count, 0);
            let write = KnowledgeWrite::bind(tx, blobs);
            let retain = |title: &str, day: &str| {
                let bytes = br#"{"company":{"symbol":"LMP"},"form":"10-Q"}"#.to_vec();
                write.retain_new_source(RetainNewSource {
                    origin: None,
                    loupist_id: id.into(),
                    at_ms: 3,
                    title: title.into(),
                    source_class: SourceClass::PublisherDocument,
                    origin_locator: format!("fixture:{title}"),
                    provider_identity: "fixture".into(),
                    authorship: Authorship::Publisher,
                    coverage: Coverage::IncludedFull,
                    origin_chat_id: None,
                    request_ref: None,
                    declared_digest: None,
                    publication_time: Some(TimePoint {
                        precision: TimePrecision::Date,
                        value: Some(day.into()),
                        approximate: false,
                    }),
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
                })
            };
            Ok((
                retain("Q2", "2026-06-01")?.record_id,
                retain("Q3", "2026-09-01")?.record_id,
            ))
        })
        .unwrap();

        let comparison = read_world_changes(
            &state,
            serde_json::from_value(json!({
                "loupistId": id,
                "between": {"asset": asset.asset_id},
            }))
            .unwrap(),
        )
        .unwrap();
        let pair = comparison.pair.unwrap();
        assert_eq!((pair.from, pair.to), (older, newer));
    }
}
