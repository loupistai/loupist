//! Asset page commands and the host refresh. React submits typed intent; Rust
//! creates pages, runs the class's reads without a model, records dated
//! observations, keeps the read and its moves, and decides what is new.
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use rusqlite::{Transaction, TransactionBehavior};
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::State;

use super::{map_store, now_ms, tool_executor::AppToolExecutor};
use crate::{
    app::{AppState, CommandError},
    broker::ToolPolicy,
    knowledge::{
        assets::{
            self, observe, plan, AssetClass, AssetDelta, AssetIdentity, AssetPageView, ClassSource,
            NewObservation, ObservationOrigin, RefreshStatus, ThesisUpdate, ViewContext,
        },
        KnowledgeError, KnowledgeResult,
    },
    runs::maintenance::MaintenanceActivityKind,
    store::user_context::MapContext,
};

const DAY_MS: i64 = 86_400_000;
/// A briefing refreshes at most this many followed assets, skips assets checked
/// in the last 30 minutes, and stops after 90 seconds.
pub(crate) const BRIEFING_MAX_ASSETS: usize = 24;
pub(crate) const BRIEFING_BUDGET: Duration = Duration::from_secs(90);
const FRESH_MS: i64 = 30 * 60 * 1000;
/// Rows per asset in a briefing's context.
const BRIEFING_ROWS: usize = 3;

fn map_assets(error: KnowledgeError) -> CommandError {
    match error {
        KnowledgeError::NotFound => CommandError::not_found("asset page"),
        KnowledgeError::RevisionConflict { .. } => CommandError::conflict(error.to_string()),
        KnowledgeError::AssetField { code, expected }
        | KnowledgeError::JournalField { code, expected } => CommandError::new(code, expected),
        error => CommandError::internal(error.to_string()),
    }
}

/// One transaction on the Loupist database; `write` takes the write lock first.
pub(crate) fn with_assets<T>(
    state: &AppState,
    loupist_id: &str,
    write: bool,
    body: impl FnOnce(&Transaction<'_>) -> KnowledgeResult<T>,
) -> Result<T, CommandError> {
    let mut connection = state.store.lock_loupist(loupist_id).map_err(map_store)?;
    let transaction = if write {
        connection.transaction_with_behavior(TransactionBehavior::Immediate)
    } else {
        connection.transaction()
    }
    .map_err(|error| CommandError::internal(error.to_string()))?;
    let value = body(&transaction).map_err(map_assets)?;
    if write {
        transaction
            .commit()
            .map_err(|error| CommandError::internal(error.to_string()))?;
    }
    Ok(value)
}

/// Watchlist and holdings identities (never quantities) and connection names.
/// Read before any Loupist transaction: the app database has its own lock.
pub(crate) fn view_context(state: &AppState) -> Result<ViewContext, CommandError> {
    let MapContext {
        watchlist,
        holdings,
        ..
    } = state.store.map_context().map_err(map_store)?;
    let connection_names = crate::marketplace::bundled_catalog()
        .map(|catalog| {
            catalog
                .entries
                .into_iter()
                .map(|entry| (entry.id, entry.name))
                .collect()
        })
        .unwrap_or_default();
    Ok(ViewContext {
        watchlist,
        holdings,
        connection_names,
        today: None,
    })
}

fn admit(state: &AppState, loupist_id: &str) -> Result<(), CommandError> {
    state.ensure_loupist_available(loupist_id)?;
    state
        .store
        .get_loupist(loupist_id)
        .map_err(map_store)?
        .ok_or_else(|| CommandError::not_found("Loupist"))?;
    Ok(())
}

/// The name the user follows an asset by, else the one the caller gave, else the symbol.
fn identity(
    context: &ViewContext,
    symbol: &str,
    market: &str,
    name: Option<&str>,
) -> AssetIdentity {
    let same = |item: &&crate::store::user_context::MapContextItem| {
        item.symbol.eq_ignore_ascii_case(symbol.trim())
            && item.market.eq_ignore_ascii_case(market.trim())
    };
    let followed = context
        .watchlist
        .iter()
        .find(same)
        .or_else(|| context.holdings.iter().find(same))
        .and_then(|item| item.display_name.clone());
    AssetIdentity {
        symbol: symbol.trim().to_owned(),
        market: market.trim().to_owned(),
        name: followed
            .or_else(|| {
                name.map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| symbol.trim().to_owned()),
    }
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssetPageGetRequest {
    pub loupist_id: String,
    /// An existing page; otherwise `symbol` and `market` name (and create) it.
    pub asset_id: Option<String>,
    pub symbol: Option<String>,
    pub market: Option<String>,
    pub name: Option<String>,
    /// The last visit, for "price since your last look"; 30 days when omitted.
    pub since_ms: Option<i64>,
}

fn since(value: Option<i64>) -> i64 {
    value.unwrap_or_else(|| now_ms() - 30 * DAY_MS)
}

/// Opens (and on the first visit creates) the page for one asset.
#[tauri::command]
pub fn asset_page_get(
    state: State<'_, AppState>,
    request: AssetPageGetRequest,
) -> Result<AssetPageView, CommandError> {
    admit(&state, &request.loupist_id)?;
    let context = view_context(&state)?;
    let loupist = request.loupist_id.clone();
    let target = match (&request.asset_id, &request.symbol, &request.market) {
        (Some(id), None, None) if assets::valid_asset_id(id) => Ok(id.clone()),
        (None, Some(symbol), Some(market)) => {
            Err(identity(&context, symbol, market, request.name.as_deref()))
        }
        _ => {
            return Err(CommandError::invalid(
                "an asset page needs assetId, or symbol and market",
            ))
        }
    };
    with_assets(&state, &loupist, true, |tx| {
        let asset_id = match &target {
            Ok(id) => id.clone(),
            Err(identity) => assets::ensure_page(tx, &loupist, identity, None, now_ms())?.asset_id,
        };
        sync_results(tx, &loupist, &asset_id)?;
        assets::page_view(tx, &loupist, &asset_id, since(request.since_ms), &context)
    })
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssetDeltaRequest {
    pub loupist_id: String,
    /// Every page when omitted.
    pub asset_id: Option<String>,
    pub since_ms: i64,
}

#[tauri::command]
pub fn asset_delta(
    state: State<'_, AppState>,
    request: AssetDeltaRequest,
) -> Result<Vec<AssetDelta>, CommandError> {
    admit(&state, &request.loupist_id)?;
    let loupist = request.loupist_id.clone();
    with_assets(&state, &loupist, false, |tx| {
        assets::deltas(tx, &loupist, request.asset_id.as_deref(), request.since_ms)
    })
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpcomingEventsRequest {
    pub loupist_id: String,
}

/// The morning message's event strip: results dates, dividend dates and
/// Journal checks in the next two weeks, soonest first. Read-only; no model.
#[tauri::command]
pub fn upcoming_events(
    state: State<'_, AppState>,
    request: UpcomingEventsRequest,
) -> Result<Vec<assets::upcoming::UpcomingEvent>, CommandError> {
    admit(&state, &request.loupist_id)?;
    let context = view_context(&state)?;
    let loupist = request.loupist_id.clone();
    with_assets(&state, &loupist, false, |tx| {
        assets::upcoming::upcoming(tx, &loupist, &context)
    })
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssetRefreshRequest {
    pub loupist_id: String,
    pub asset_id: String,
    pub since_ms: Option<i64>,
}

/// Check now: the class's reads without a model call.
#[tauri::command]
pub async fn asset_refresh(
    state: State<'_, AppState>,
    request: AssetRefreshRequest,
) -> Result<AssetPageView, CommandError> {
    admit(&state, &request.loupist_id)?;
    let _activity = state
        .maintenance
        .admit_kind(MaintenanceActivityKind::MemoryMutation)?;
    refresh_asset(
        &state,
        &request.loupist_id,
        &request.asset_id,
        ObservationOrigin::Refresh,
        None,
    )
    .await?;
    let context = view_context(&state)?;
    let loupist = request.loupist_id.clone();
    with_assets(&state, &loupist, true, |tx| {
        assets::page_view(
            tx,
            &loupist,
            &request.asset_id,
            since(request.since_ms),
            &context,
        )
    })
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssetClassSetRequest {
    pub loupist_id: String,
    pub asset_id: String,
    pub class: AssetClass,
    pub since_ms: Option<i64>,
}

#[tauri::command]
pub fn asset_class_set(
    state: State<'_, AppState>,
    request: AssetClassSetRequest,
) -> Result<AssetPageView, CommandError> {
    admit(&state, &request.loupist_id)?;
    let _activity = state
        .maintenance
        .admit_kind(MaintenanceActivityKind::MemoryMutation)?;
    let context = view_context(&state)?;
    let loupist = request.loupist_id.clone();
    with_assets(&state, &loupist, true, |tx| {
        assets::set_class(
            tx,
            &loupist,
            &request.asset_id,
            request.class,
            ClassSource::User,
        )?;
        assets::page_view(
            tx,
            &loupist,
            &request.asset_id,
            since(request.since_ms),
            &context,
        )
    })
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssetVerdictUndoRequest {
    pub loupist_id: String,
    pub asset_id: String,
    /// The move the user saw as the latest; a newer move makes this a conflict.
    pub move_id: String,
    pub since_ms: Option<i64>,
}

/// The user's Undo of the Loupist's latest move.
#[tauri::command]
pub fn asset_verdict_undo(
    state: State<'_, AppState>,
    request: AssetVerdictUndoRequest,
) -> Result<AssetPageView, CommandError> {
    admit(&state, &request.loupist_id)?;
    let _activity = state
        .maintenance
        .admit_kind(MaintenanceActivityKind::MemoryMutation)?;
    let context = view_context(&state)?;
    let loupist = request.loupist_id.clone();
    with_assets(&state, &loupist, true, |tx| {
        assets::undo(tx, &loupist, &request.asset_id, &request.move_id, now_ms())?;
        assets::page_view(
            tx,
            &loupist,
            &request.asset_id,
            since(request.since_ms),
            &context,
        )
    })
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssetPageSeenRequest {
    pub loupist_id: String,
    pub asset_id: String,
}

/// Opening the page acknowledges its crossed lines.
#[tauri::command]
pub fn asset_page_seen(
    state: State<'_, AppState>,
    request: AssetPageSeenRequest,
) -> Result<(), CommandError> {
    admit(&state, &request.loupist_id)?;
    let loupist = request.loupist_id.clone();
    with_assets(&state, &loupist, true, |tx| {
        assets::mark_seen(tx, &loupist, &request.asset_id, now_ms()).map(|_| ())
    })
}

/// Whether a capability grant covers a planned read. OpenBB reads need any
/// connection served by the bundled market-data runtime.
fn granted(
    read: &plan::PlannedRead,
    capabilities: &BTreeSet<String>,
    openbb: &BTreeSet<String>,
) -> bool {
    match read.server {
        Some(_) => capabilities.iter().any(|id| openbb.contains(id)),
        None => capabilities.contains(read.connection),
    }
}

fn openbb_entries() -> (BTreeSet<String>, BTreeMap<String, String>) {
    let mut ids = BTreeSet::new();
    let mut names = BTreeMap::new();
    if let Ok(catalog) = crate::marketplace::bundled_catalog() {
        for entry in catalog.entries {
            if entry.runtime.server_id.as_deref() == Some(plan::OPENBB) {
                ids.insert(entry.id.clone());
            }
            names.insert(entry.id, entry.name);
        }
    }
    (ids, names)
}

/// The scheduler's stop signal for a briefing's refresh; `None` for Check now.
pub(crate) type RefreshStop = Option<tokio::sync::watch::Receiver<bool>>;

fn stopped(stop: &RefreshStop) -> bool {
    stop.as_ref().is_some_and(|stop| *stop.borrow())
}

/// Resolves when the stop signal is raised; never when there is none.
async fn until_stopped(stop: &mut RefreshStop) {
    if let Some(stop) = stop {
        if stop.wait_for(|stopped| *stopped).await.is_ok() {
            return;
        }
    }
    std::future::pending::<()>().await
}

/// Runs one asset's planned reads with the Loupist's connection grants and
/// records what they returned. A first refresh is a baseline; identical rows
/// are ignored. Per-source failures are recorded on the page, never raised.
pub(crate) async fn refresh_asset(
    state: &AppState,
    loupist_id: &str,
    asset_id: &str,
    origin: ObservationOrigin,
    stop: RefreshStop,
) -> Result<usize, CommandError> {
    let capabilities: BTreeSet<String> =
        super::capture_chat_connector_authority(state, loupist_id)?
            .capability_ids
            .into_iter()
            .collect();
    let executor = AppToolExecutor::for_host(state, loupist_id, capabilities.clone());
    let refreshed = refresh_with(
        state,
        loupist_id,
        asset_id,
        origin,
        HostReads {
            executor: &executor,
            capabilities: &capabilities,
            max_age: Some(super::tool_executor::assets::HOST_MAX_AGE_SECONDS),
        },
        stop,
    )
    .await;
    executor.host_shutdown().await;
    refreshed
}

/// How a refresh reads: the executor, the Loupist's grants and the window in
/// which an identical read is reused instead of fetched again.
pub(in crate::commands) struct HostReads<'a> {
    pub(in crate::commands) executor: &'a AppToolExecutor,
    pub(in crate::commands) capabilities: &'a BTreeSet<String>,
    pub(in crate::commands) max_age: Option<u64>,
}

/// `refresh_asset` through given reads: the grant check, the first-refresh
/// fund lookup, per-source statuses and the observations. A raised stop
/// signal abandons the refresh between or during reads, unrecorded.
pub(in crate::commands) async fn refresh_with(
    state: &AppState,
    loupist_id: &str,
    asset_id: &str,
    origin: ObservationOrigin,
    reads_with: HostReads<'_>,
    mut stop: RefreshStop,
) -> Result<usize, CommandError> {
    let HostReads {
        executor,
        capabilities,
        max_age,
    } = reads_with;
    let page = with_assets(state, loupist_id, false, |tx| {
        assets::get_page(tx, loupist_id, asset_id)
    })?
    .ok_or_else(|| CommandError::not_found("asset page"))?;
    let now = chrono::Utc::now();
    let today = now.with_timezone(&chrono::Local).date_naive();
    let session_day = assets::class::session_day(&page.market, now);
    let lookup = page.class == AssetClass::Equity
        && page.class_source == ClassSource::Default
        && !assets::class::is_korean_market(&page.market);
    let reads = plan::reads(page.class, &page.symbol, &page.market, today, lookup);
    let (openbb, names) = openbb_entries();
    let status = |read: &plan::PlannedRead, status: &str| RefreshStatus {
        status: status.into(),
        connection: names
            .get(read.connection)
            .cloned()
            .unwrap_or_else(|| read.connection.to_owned()),
        at_ms: now_ms(),
    };
    if !reads
        .iter()
        .any(|read| granted(read, capabilities, &openbb))
    {
        // Nothing to read (a sources-only class) or no grant for any read.
        let statuses = reads
            .iter()
            .map(|read| (read.kind.to_owned(), status(read, "needs_connection")))
            .collect();
        with_assets(state, loupist_id, true, |tx| {
            assets::record_refresh(tx, loupist_id, asset_id, statuses, now_ms())
        })?;
        return Ok(0);
    }
    let home = crate::chat::home::ensure_home_thread(state, loupist_id)?;
    let policy = ToolPolicy {
        loupist_id: loupist_id.to_owned(),
        session_id: home.id,
        use_memory: false,
        propose_memory_updates: false,
        memory_proposal_budget: 0,
        as_of: None,
    };
    let mut statuses = BTreeMap::new();
    let mut found: Vec<NewObservation> = Vec::new();
    let mut class_lookup = None;
    let mut connected = None;
    for read in &reads {
        if stopped(&stop) {
            return Ok(0);
        }
        if !granted(read, capabilities, &openbb) {
            statuses.insert(read.kind.to_owned(), status(read, "needs_connection"));
            continue;
        }
        if let Some(server) = read.server {
            if connected.is_none() {
                let tools: Vec<&str> = reads
                    .iter()
                    .filter(|r| r.server.is_some())
                    .map(|r| r.tool)
                    .collect();
                connected = Some(tokio::select! {
                    outcome = executor.host_connect(&policy, server, &tools) => outcome.is_ok(),
                    _ = until_stopped(&mut stop) => return Ok(0),
                });
            }
            if connected != Some(true) {
                statuses.insert(read.kind.to_owned(), status(read, "failed"));
                continue;
            }
        }
        let outcome = tokio::select! {
            outcome = executor.host_read(&policy, read, max_age) => outcome,
            _ = until_stopped(&mut stop) => return Ok(0),
        };
        match outcome {
            Ok((payload, provider)) => {
                if read.tool == "etf_info" && lookup {
                    let fund = payload
                        .pointer("/structuredContent/results")
                        .and_then(Value::as_array)
                        .is_some_and(|rows| !rows.is_empty());
                    class_lookup = Some(if fund {
                        AssetClass::Fund
                    } else {
                        AssetClass::Equity
                    });
                }
                let dataset = payload
                    .get("dataset_ref")
                    .cloned()
                    .and_then(|value| serde_json::from_value(value).ok());
                let retrieved = payload
                    .get("fetched_at")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                found.extend(observe::extract(
                    &observe::ToolRead {
                        tool: read.tool,
                        provider: provider.as_deref(),
                        payload: &payload,
                        dataset,
                        retrieved_at: retrieved.as_deref(),
                        citation_url: None,
                        today,
                        session_day,
                    },
                    &page.symbol,
                ));
                statuses.insert(read.kind.to_owned(), status(read, "ok"));
            }
            Err(_) => {
                // A lookup that fails says nothing about the class.
                if read.tool != "etf_info" || !lookup {
                    statuses.insert(read.kind.to_owned(), status(read, "failed"));
                }
            }
        }
    }
    if page.class == AssetClass::Equity && assets::class::is_korean_market(&page.market) {
        let korea = KoreaReads {
            executor,
            capabilities,
            openbb: &openbb,
            policy: &policy,
            max_age,
            today,
            session_day,
        };
        let capital = korea
            .capital(state, loupist_id, &page, &found, &mut stop)
            .await;
        found.extend(capital?);
        match korea.flows(state, loupist_id, &page, &mut stop).await? {
            FlowsOutcome::NotDue => {}
            FlowsOutcome::NeedsConnection(read) => {
                statuses.insert("flows".into(), status(&read, "needs_connection"));
            }
            FlowsOutcome::Failed(read) => {
                statuses.insert("flows".into(), status(&read, "failed"));
            }
            FlowsOutcome::Read(read, observation) => {
                found.extend(observation.map(|observation| *observation));
                statuses.insert("flows".into(), status(&read, "ok"));
            }
        }
        if stopped(&stop) {
            return Ok(0);
        }
    }
    // A lookup read is not one of the page's checks.
    if lookup {
        statuses.retain(|kind, _| kind != "fees");
        found.retain(|observation| observation.kind != assets::ObservationKind::Fees);
    }
    with_assets(state, loupist_id, true, |tx| {
        if let Some(class) = class_lookup {
            assets::set_class(tx, loupist_id, asset_id, class, ClassSource::Lookup)?;
        }
        let added =
            assets::record_observations(tx, loupist_id, asset_id, found, origin, None, now_ms())?;
        assets::record_refresh(tx, loupist_id, asset_id, statuses, now_ms())?;
        sync_results(tx, loupist_id, asset_id)?;
        Ok(added)
    })
}

/// Capital decisions read at most per refresh, newest filings first.
const MAX_CAPITAL_READS: usize = 2;
/// Decisions the page words in plain terms; other 주요사항보고서 stay filing rows.
const CAPITAL_KINDS: [&str; 4] = [
    "rights_offering",
    "convertible_bonds",
    "bonds_with_warrants",
    "buyback",
];

/// The Korean reads beyond a refresh's four: capital decisions named by the
/// filings just read, and Korea Investment's investor flows once a day.
struct KoreaReads<'a> {
    executor: &'a AppToolExecutor,
    capabilities: &'a BTreeSet<String>,
    openbb: &'a BTreeSet<String>,
    policy: &'a ToolPolicy,
    max_age: Option<u64>,
    today: chrono::NaiveDate,
    session_day: chrono::NaiveDate,
}

enum FlowsOutcome {
    NotDue,
    NeedsConnection(plan::PlannedRead),
    Failed(plan::PlannedRead),
    Read(plan::PlannedRead, Option<Box<NewObservation>>),
}

impl KoreaReads<'_> {
    async fn read(
        &self,
        read: &plan::PlannedRead,
        stop: &mut RefreshStop,
    ) -> Option<Option<(Value, Option<String>)>> {
        tokio::select! {
            outcome = self.executor.host_read(self.policy, read, self.max_age) => Some(outcome.ok()),
            _ = until_stopped(stop) => None,
        }
    }

    fn tool_read<'p>(
        &self,
        read: &'p plan::PlannedRead,
        payload: &'p Value,
        provider: Option<&'p str>,
    ) -> observe::ToolRead<'p> {
        observe::ToolRead {
            tool: read.tool,
            provider,
            payload,
            dataset: payload
                .get("dataset_ref")
                .cloned()
                .and_then(|value| serde_json::from_value(value).ok()),
            retrieved_at: payload.get("fetched_at").and_then(Value::as_str),
            citation_url: None,
            today: self.today,
            session_day: self.session_day,
        }
    }

    /// Reads the decisions behind new 주요사항보고서 the filings read listed
    /// (rights offering, convertible bonds, bonds with warrants, buyback).
    async fn capital(
        &self,
        state: &AppState,
        loupist_id: &str,
        page: &assets::PageRecord,
        found: &[NewObservation],
        stop: &mut RefreshStop,
    ) -> Result<Vec<NewObservation>, CommandError> {
        let known: BTreeSet<String> = with_assets(state, loupist_id, false, |tx| {
            Ok(assets::observations(tx, loupist_id, &page.asset_id)?
                .into_iter()
                .filter(|item| item.kind == assets::ObservationKind::Capital)
                .map(|item| item.dedupe_key)
                .collect())
        })?;
        let mut wanted: Vec<(&str, chrono::NaiveDate, &'static str)> = found
            .iter()
            .filter(|item| item.kind == assets::ObservationKind::Filing)
            .filter_map(|item| {
                let id = item.facts["filingId"].as_str()?;
                let kind = crate::finance_data::dart_decision_kind(item.facts["title"].as_str()?)
                    .filter(|kind| CAPITAL_KINDS.contains(kind))?;
                let filed = observe::parse_day(&item.fact_date)?;
                (!known.contains(&format!("capital:{id}"))).then_some((id, filed, kind))
            })
            .collect();
        wanted.sort_by(|a, b| b.0.cmp(a.0));
        let mut out = Vec::new();
        for (id, filed, kind) in wanted.into_iter().take(MAX_CAPITAL_READS) {
            let read = plan::capital_read(&page.symbol, kind, filed);
            if !granted(&read, self.capabilities, self.openbb) {
                break;
            }
            let Some(outcome) = self.read(&read, stop).await else {
                return Ok(out);
            };
            let Some((payload, provider)) = outcome else {
                continue;
            };
            out.extend(
                observe::extract(
                    &self.tool_read(&read, &payload, provider.as_deref()),
                    &page.symbol,
                )
                .into_iter()
                .filter(|item| item.facts["filingId"] == id),
            );
        }
        Ok(out)
    }

    /// Investor flows, short selling and the lending balance, when Korea
    /// Investment is on and the day's flows are final and not yet read.
    async fn flows(
        &self,
        state: &AppState,
        loupist_id: &str,
        page: &assets::PageRecord,
        stop: &mut RefreshStop,
    ) -> Result<FlowsOutcome, CommandError> {
        let [investor, short, loan] = plan::flows_reads(&page.symbol, self.session_day);
        if !granted(&investor, self.capabilities, self.openbb) {
            return Ok(FlowsOutcome::NeedsConnection(investor));
        }
        let latest = with_assets(state, loupist_id, false, |tx| {
            Ok(assets::observations(tx, loupist_id, &page.asset_id)?
                .into_iter()
                .filter(|item| item.kind == assets::ObservationKind::InvestorFlows)
                .filter_map(|item| observe::parse_day(&item.fact_date))
                .max())
        })?;
        let now = chrono::Utc::now();
        let attempted = page.refresh.get("flows").map(|status| status.at_ms);
        if !assets::flows::due(latest, attempted, now) {
            return Ok(FlowsOutcome::NotDue);
        }
        let mut payloads = Vec::new();
        for read in [&investor, &short, &loan] {
            let Some(outcome) = self.read(read, stop).await else {
                return Ok(FlowsOutcome::NotDue);
            };
            payloads.push(outcome);
        }
        let Some((investor_payload, provider)) = payloads[0].take() else {
            return Ok(FlowsOutcome::Failed(investor));
        };
        let observation = {
            let reads = assets::flows::FlowReads {
                investor: self.tool_read(&investor, &investor_payload, provider.as_deref()),
                short: payloads[1].as_ref().map(|(payload, provider)| {
                    self.tool_read(&short, payload, provider.as_deref())
                }),
                loan: payloads[2]
                    .as_ref()
                    .map(|(payload, provider)| self.tool_read(&loan, payload, provider.as_deref())),
            };
            assets::flows::summary(&reads, assets::flows::last_final_session(now))
        };
        Ok(FlowsOutcome::Read(investor, observation.map(Box::new)))
    }
}

/// Keeps the asset's results events in step with what was just recorded
/// (dates, releases, final reports); no reads, no model.
fn sync_results(tx: &Transaction<'_>, loupist_id: &str, asset_id: &str) -> KnowledgeResult<()> {
    let Some(page) = assets::get_page(tx, loupist_id, asset_id)? else {
        return Ok(());
    };
    let today = crate::knowledge::earnings::dates::listing_today(&page.market, chrono::Utc::now());
    crate::knowledge::earnings::sync(tx, loupist_id, &page, today, now_ms())?;
    Ok(())
}

/// A briefing's refresh: pages for followed assets, then their reads, within
/// the budget, one asset at a time (each holding a Memory activity, so
/// maintenance can start between assets). Assets not reached in time, or
/// after the stop signal, are left "not checked" for this run.
pub(crate) async fn refresh_followed(
    state: &AppState,
    loupist_id: &str,
    budget: Duration,
    stop: RefreshStop,
) -> Result<usize, CommandError> {
    let started = Instant::now();
    if stopped(&stop) {
        return Ok(0);
    }
    let context = view_context(state)?;
    let identities: Vec<AssetIdentity> = followed(&context)
        .into_iter()
        .take(BRIEFING_MAX_ASSETS)
        .collect();
    let pages = with_assets(state, loupist_id, true, |tx| {
        identities
            .iter()
            .map(|identity| assets::ensure_page(tx, loupist_id, identity, None, now_ms()))
            .collect::<KnowledgeResult<Vec<_>>>()
    })?;
    let mut refreshed = 0;
    for page in pages {
        if started.elapsed() >= budget || stopped(&stop) {
            break;
        }
        if page
            .refreshed_at_ms
            .is_some_and(|at| now_ms() - at < FRESH_MS)
        {
            continue;
        }
        let Ok(_activity) = state
            .maintenance
            .admit_kind(MaintenanceActivityKind::MemoryMutation)
        else {
            break;
        };
        let remaining = budget.saturating_sub(started.elapsed());
        if tokio::time::timeout(
            remaining,
            refresh_asset(
                state,
                loupist_id,
                &page.asset_id,
                ObservationOrigin::Briefing,
                stop.clone(),
            ),
        )
        .await
        .is_ok_and(|result| result.is_ok())
        {
            refreshed += 1;
        }
    }
    Ok(refreshed)
}

/// Followed assets (watched, then held) as page identities. Names only.
pub(crate) fn followed(context: &ViewContext) -> Vec<AssetIdentity> {
    let watching = context.watchlist.iter().map(|item| {
        crate::knowledge::FollowedAsset::new(
            &item.symbol,
            &item.market,
            item.display_name.as_deref(),
        )
    });
    let holding = context.holdings.iter().map(|item| {
        crate::knowledge::FollowedAsset::new(
            &item.symbol,
            &item.market,
            item.display_name.as_deref(),
        )
    });
    crate::knowledge::followed_assets(watching, holding)
        .into_iter()
        .filter(|asset| !asset.market.is_empty())
        .map(|asset| AssetIdentity {
            symbol: asset.symbol,
            market: asset.market,
            name: asset.name,
        })
        .collect()
}

/// The briefing context: up to three rows per followed asset with a delta.
pub(crate) fn briefing_assets(
    state: &AppState,
    loupist_id: &str,
    since_ms: i64,
) -> Result<(Vec<AssetDelta>, Value), CommandError> {
    let mut deltas = with_assets(state, loupist_id, false, |tx| {
        assets::deltas(tx, loupist_id, None, since_ms)
    })?;
    deltas.retain(|delta| delta.rows.iter().any(|row| row.material));
    deltas.truncate(BRIEFING_MAX_ASSETS);
    for delta in &mut deltas {
        delta.rows.retain(|row| row.material);
        delta.rows.sort_by_key(|row| row_rank(&row.kind));
        delta.rows.truncate(BRIEFING_ROWS);
    }
    let prompt = json!(deltas
        .iter()
        .map(|delta| json!({
            "assetId": delta.asset_id,
            "name": delta.name,
            "symbol": delta.symbol,
            "market": delta.market,
            "read": delta.verdict.map(assets::Verdict::label),
            "rows": delta.rows.iter().map(|row| json!({"kind": row.kind, "factDate": row.fact_date, "facts": row.facts, "sections": row.sections, "sources": row.evidence.iter().map(|evidence| json!({"title": evidence.title, "url": evidence.url})).collect::<Vec<_>>()})).collect::<Vec<_>>(),
        }))
        .collect::<Vec<_>>());
    Ok((deltas, prompt))
}

fn row_rank(kind: &str) -> u8 {
    match kind {
        "thesis" => 0,
        "filing" => 1,
        "results" => 2,
        "earnings" | "dividend" => 3,
        "price" | "rates" => 4,
        _ => 5,
    }
}

/// Compares the filing sections a reply retained, after its commit. The pairs
/// are read and the counts written in short transactions; the comparison runs
/// with no lock held, so the folder stays available meanwhile.
pub(crate) fn align_turn_filings(
    state: &AppState,
    loupist_id: &str,
    message_id: &str,
    at_ms: i64,
) -> Result<usize, CommandError> {
    let pairs = with_assets(state, loupist_id, false, |tx| {
        assets::retained_filing_pairs(tx, loupist_id, message_id)
    })?;
    if pairs.is_empty() {
        return Ok(0);
    }
    let blobs = super::knowledge::knowledge_blobs_for_loupist(state, loupist_id)?;
    let compared: Vec<_> = pairs
        .into_iter()
        .filter_map(|pair| {
            let deltas = assets::compare_filing(&blobs, &pair).ok()?;
            (!deltas.is_empty()).then_some((pair, deltas))
        })
        .collect();
    if compared.is_empty() {
        return Ok(0);
    }
    with_assets(state, loupist_id, true, |tx| {
        for (pair, deltas) in &compared {
            assets::record_sections(
                tx,
                loupist_id,
                &pair.asset_id,
                &pair.dedupe_key,
                &pair.record_id,
                &pair.previous_record_id,
                deltas,
                at_ms,
            )?;
        }
        Ok(compared.len())
    })
}

/// What a turn's asset work did when the reply committed.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct TurnAssets {
    /// A line was crossed: the home Thread announces it.
    pub(crate) crossed: bool,
    /// Updates the tool accepted that no longer fit the committed page (the
    /// user undid a move or archived a reason meanwhile). Never applied; the
    /// run log and the receipt say so.
    pub(crate) unsaved: Vec<crate::domain::UnsavedThesis>,
}

impl TurnAssets {
    /// Run-log entries for updates that were not saved.
    pub(crate) fn trace_notes(&self, offset_ms: u64) -> Vec<crate::domain::ChatToolTraceEntry> {
        self.unsaved
            .iter()
            .map(|unsaved| crate::domain::ChatToolTraceEntry {
                call_id: None,
                name: "thesis_update".into(),
                component: Some(crate::domain::THESIS_COMMIT_COMPONENT.into()),
                args: Some(json!({"assetId": unsaved.asset_id})),
                start_offset_ms: offset_ms,
                duration_ms: 0,
                outcome: crate::domain::ToolTraceOutcome::Error,
                error: Some(json!({
                    "code": "thesis_not_saved",
                    "reason": unsaved.reason,
                    "assetId": unsaved.asset_id,
                    "name": unsaved.name,
                })),
                result_bytes: None,
            })
            .collect()
    }
}

/// Commits a completed turn's asset work in its transaction: staged thesis
/// updates (re-checked against the committed page; only the first per asset)
/// and observations from the turn's acknowledged reads.
pub(crate) fn commit_turn(
    tx: &Transaction<'_>,
    loupist_id: &str,
    chat_id: &str,
    message_id: &str,
    at_ms: i64,
    theses: &[ThesisUpdate],
    observed: &[(AssetIdentity, Vec<NewObservation>)],
) -> KnowledgeResult<TurnAssets> {
    for (identity, observations) in observed {
        let page = assets::ensure_page(tx, loupist_id, identity, None, at_ms)?;
        assets::record_observations(
            tx,
            loupist_id,
            &page.asset_id,
            observations.clone(),
            ObservationOrigin::Chat,
            Some((chat_id, message_id)),
            at_ms,
        )?;
    }
    let mut result = TurnAssets::default();
    let mut moved_assets = BTreeSet::new();
    for update in theses {
        // The tool refuses a second update per asset; the commit enforces it too.
        let refused = if !moved_assets.insert(update.asset_id.clone()) {
            Some("thesis_twice")
        } else {
            // The user may have undone or written something since the tool call;
            // an update that no longer fits is not forced.
            assets::check_update(tx, loupist_id, update)
                .err()
                .map(|error| error.code())
        };
        if let Some(reason) = refused {
            let name = assets::get_page(tx, loupist_id, &update.asset_id)?
                .map(|page| page.name)
                .unwrap_or_else(|| update.asset_id.clone());
            result.unsaved.push(crate::domain::UnsavedThesis {
                asset_id: update.asset_id.clone(),
                name,
                reason: reason.to_owned(),
            });
            continue;
        }
        let applied =
            assets::apply_update(tx, loupist_id, update, Some((chat_id, message_id)), at_ms)?;
        result.crossed |= applied
            .moved
            .as_ref()
            .is_some_and(|moved| moved.line.as_ref().is_some_and(|line| line.crossed));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The requests the renderer sends (its tests check them against the same
    /// file) are accepted by these commands, whose requests deny unknown fields.
    #[test]
    fn the_renderers_asset_requests_are_accepted() {
        let examples: BTreeMap<String, Vec<Value>> = serde_json::from_str(include_str!(
            "../../../src/contracts/assetRequests.example.json"
        ))
        .unwrap();
        fn accepts<T: serde::de::DeserializeOwned>(examples: &[Value]) {
            for example in examples {
                serde_json::from_value::<T>(example.clone())
                    .unwrap_or_else(|error| panic!("{example}: {error}"));
            }
        }
        for (command, requests) in &examples {
            match command.as_str() {
                "asset_page_get" => accepts::<AssetPageGetRequest>(requests),
                "asset_delta" => accepts::<AssetDeltaRequest>(requests),
                "asset_refresh" => accepts::<AssetRefreshRequest>(requests),
                "asset_class_set" => accepts::<AssetClassSetRequest>(requests),
                "asset_verdict_undo" => accepts::<AssetVerdictUndoRequest>(requests),
                "asset_page_seen" => accepts::<AssetPageSeenRequest>(requests),
                "upcoming_events" => accepts::<UpcomingEventsRequest>(requests),
                other => panic!("no asset command {other}"),
            }
        }
        assert_eq!(examples.len(), 7);
        // A field Rust does not know is refused, as the stale Undo revision was.
        assert!(serde_json::from_value::<AssetVerdictUndoRequest>(json!({
            "loupistId": "loupist-quality", "assetId": "ast_0123456789abcdef",
            "moveId": "amv_1", "expectedRevision": 4
        }))
        .is_err());
    }
}
