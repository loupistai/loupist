//! Typed asset intent and saved-read replay without a model.
pub(crate) mod setup;
#[cfg(test)]
mod tests;
pub(crate) mod wire;

use std::collections::BTreeSet;

use rusqlite::{Transaction, TransactionBehavior};
use serde::Deserialize;
use serde_json::json;
use tauri::State;

use super::{map_store, now_ms, tool_executor::AppToolExecutor};
use crate::{
    app::{AppState, CommandError},
    broker::ToolPolicy,
    knowledge::{
        assets::{self, saved_read, ThesisUpdate},
        KnowledgeError, KnowledgeResult,
    },
    runs::maintenance::MaintenanceActivityKind,
    store::user_context,
};
use wire::*;

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

pub(super) fn admit(state: &AppState, loupist_id: &str) -> Result<(), CommandError> {
    state.ensure_loupist_available(loupist_id)?;
    state
        .store
        .get_loupist(loupist_id)
        .map_err(map_store)?
        .ok_or_else(|| CommandError::not_found("Loupist"))?;
    Ok(())
}

fn map_context_error(error: crate::store::StoreError) -> CommandError {
    match error {
        crate::store::StoreError::Invalid(message) => CommandError::invalid(message),
        other => map_store(other),
    }
}

pub(crate) fn changed() {
    crate::briefings::desktop::emit(crate::events::ASSETS_CHANGED, ());
}

pub(crate) fn follows_changed(state: &AppState) {
    state.followed.schedule(state);
    changed();
}

#[tauri::command(async)]
pub fn assets_list(
    state: State<'_, AppState>,
    request: AssetsListRequest,
) -> Result<Vec<AssetRow>, CommandError> {
    admit(&state, &request.loupist_id)?;
    state
        .store
        .assets_list(&request.loupist_id, true)
        .map_err(map_context_error)
}

#[tauri::command(async)]
pub fn asset_follow_name(
    state: State<'_, AppState>,
    request: AssetFollowNameRequest,
) -> Result<AssetRow, CommandError> {
    admit(&state, &request.loupist_id)?;
    let origin = if request
        .reference
        .as_ref()
        .is_some_and(|fields| fields.figi.is_some())
    {
        AssetReferenceOrigin::Openfigi
    } else {
        AssetReferenceOrigin::User
    };
    let asset = state
        .store
        .asset_follow_name(
            &request.loupist_id,
            &request.name,
            request.reference,
            origin,
            now_ms(),
        )
        .map_err(map_context_error)?;
    follows_changed(&state);
    state
        .price_setup
        .queue(&state, &request.loupist_id, &asset.asset_id);
    Ok(asset)
}

#[tauri::command(async)]
pub fn asset_follow(
    state: State<'_, AppState>,
    request: AssetIdRequest,
) -> Result<AssetRow, CommandError> {
    admit(&state, &request.loupist_id)?;
    let asset = state
        .store
        .set_asset_followed(&request.loupist_id, &request.asset_id, Some(now_ms()))
        .map_err(map_context_error)?
        .ok_or_else(|| CommandError::not_found("asset"))?;
    follows_changed(&state);
    state
        .price_setup
        .queue(&state, &request.loupist_id, &asset.asset_id);
    Ok(asset)
}

#[tauri::command(async)]
pub fn asset_unfollow(
    state: State<'_, AppState>,
    request: AssetIdRequest,
) -> Result<(), CommandError> {
    admit(&state, &request.loupist_id)?;
    state
        .store
        .set_asset_followed(&request.loupist_id, &request.asset_id, None)
        .map_err(map_context_error)?
        .ok_or_else(|| CommandError::not_found("asset"))?;
    follows_changed(&state);
    Ok(())
}

#[tauri::command]
pub async fn asset_lookup(
    state: State<'_, AppState>,
    request: AssetLookupRequest,
) -> Result<AssetLookupResult, CommandError> {
    admit(&state, &request.loupist_id)?;
    crate::finance_data::openfigi::search(&state, &request.loupist_id, &request.query)
        .await
        .map(Into::into)
}

fn copied_asset(page: assets::PageRecord) -> AssetRow {
    AssetRow {
        asset_id: page.asset_id,
        name: page.name,
        reference: None,
        followed_at_ms: None,
        created_at_ms: page.created_at_ms,
    }
}

fn open_target(state: &AppState, request: &AssetOpenRequest) -> Result<AssetRow, CommandError> {
    if let Some(id) = &request.asset_id {
        if !assets::valid_asset_id(id) {
            return Err(CommandError::invalid("Invalid asset ID."));
        }
        if let Some(asset) = state
            .store
            .asset_by_id(&request.loupist_id, id)
            .map_err(map_context_error)?
        {
            return Ok(asset);
        }
        // A copied folder can retain its page without a catalog asset.
        if let Some(page) = with_assets(state, &request.loupist_id, false, |tx| {
            assets::get_page(tx, &request.loupist_id, id)
        })? {
            return Ok(copied_asset(page));
        }
    }
    if let Some(ticker) = &request.ticker {
        let rows = state
            .store
            .assets_list(&request.loupist_id, false)
            .map_err(map_context_error)?;
        match user_context::by_ticker(&rows, ticker).as_slice() {
            [one] => return Ok((*one).clone()),
            [] => {}
            candidates => {
                return Err(CommandError::with_detail(
                    "ambiguous_asset",
                    "Several assets match this ticker.",
                    json!({"candidates":candidates.iter().take(20).copied().collect::<Vec<_>>()}),
                ))
            }
        }
    }
    Err(CommandError::new(
        "unknown_asset",
        "The asset is not tracked.",
    ))
}

pub(crate) fn page_for(
    state: &AppState,
    request: AssetOpenRequest,
) -> Result<ThinAssetPageView, CommandError> {
    admit(state, &request.loupist_id)?;
    let asset = open_target(state, &request)?;
    let page = with_assets(state, &request.loupist_id, true, |tx| {
        assets::ensure_asset_page(tx, &request.loupist_id, &asset, now_ms())?;
        assets::page_view(tx, &request.loupist_id, asset)
    })?;
    Ok(with_price_setup(state, &request.loupist_id, page))
}

fn with_price_setup(
    state: &AppState,
    loupist: &str,
    mut page: ThinAssetPageView,
) -> ThinAssetPageView {
    page.price_setup_running = state.price_setup.is_running(loupist, &page.asset.asset_id);
    page
}

#[tauri::command(async)]
pub fn asset_page_get(
    state: State<'_, AppState>,
    request: AssetOpenRequest,
) -> Result<ThinAssetPageView, CommandError> {
    if let Some(entity_id) = &request.entity_id {
        if request.asset_id.is_some() || request.ticker.is_some() {
            return Err(CommandError::invalid("Supply one asset lookup key."));
        }
        return page_by_entity(&state, &request.loupist_id, entity_id);
    }
    let loupist = request.loupist_id.clone();
    let stored_only = request.stored_only.unwrap_or(false);
    let page = page_for(&state, request)?;
    if !stored_only && !page.price_setup_running && page.saved_read.is_some() {
        let app = state.inner().clone();
        let asset = page.asset.asset_id.clone();
        tauri::async_runtime::spawn(async move {
            let _ = replay_current(&app, &loupist, &asset, Some(900)).await;
        });
    }
    Ok(page)
}

fn page_by_entity(
    state: &AppState,
    loupist_id: &str,
    entity_id: &str,
) -> Result<ThinAssetPageView, CommandError> {
    admit(state, loupist_id)?;
    let catalog = state
        .store
        .assets_list(loupist_id, false)
        .map_err(map_context_error)?;
    let page = with_assets(state, loupist_id, false, |tx| {
        let pages = assets::list_pages(tx, loupist_id)?;
        let Some(world) = assets::world(tx, loupist_id)? else {
            return Err(KnowledgeError::NotFound);
        };
        for page in pages {
            let asset = catalog
                .iter()
                .find(|asset| asset.asset_id == page.asset_id)
                .cloned()
                .unwrap_or_else(|| copied_asset(page.clone()));
            if assets::match_entity(&world, &asset, &page).as_deref() == Some(entity_id) {
                let view = assets::stored_page_view(tx, loupist_id, asset)?;
                if view.read.is_some() {
                    return Ok(view);
                }
            }
        }
        Err(KnowledgeError::NotFound)
    })?;
    Ok(with_price_setup(state, loupist_id, page))
}

#[tauri::command]
pub async fn asset_price_check(
    state: State<'_, AppState>,
    request: AssetIdRequest,
) -> Result<ThinAssetPageView, CommandError> {
    admit(&state, &request.loupist_id)?;
    let open = || AssetOpenRequest {
        loupist_id: request.loupist_id.clone(),
        asset_id: Some(request.asset_id.clone()),
        ticker: None,
        entity_id: None,
        stored_only: None,
    };
    let page = page_for(&state, open())?;
    if page.asset.followed_at_ms.is_none() || page.price_setup_running {
        return Ok(page);
    }
    if page
        .saved_read
        .as_ref()
        .is_none_or(|read| matches!(read.state, PriceReadState::None | PriceReadState::Failed))
    {
        setup::launch(
            &state,
            &request.loupist_id,
            &[request.asset_id.clone()].into_iter().collect(),
            true,
        )
        .await?;
    } else {
        replay_current(&state, &request.loupist_id, &request.asset_id, None).await?;
    }
    page_for(&state, open())
}

#[tauri::command]
pub async fn asset_price_series(
    state: State<'_, AppState>,
    request: AssetPriceSeriesRequest,
) -> Result<Vec<AssetPricePoint>, CommandError> {
    admit(&state, &request.loupist_id)?;
    let loupist = &request.loupist_id;
    let asset = &request.asset_id;
    let stored = if request.stored_only.unwrap_or(false) {
        None
    } else {
        with_assets(&state, loupist, false, |tx| {
            saved_read::load(tx, loupist, asset)
        })?
    };
    if let Some((stored, call)) = stored
        .as_ref()
        .and_then(|stored| stored.read.history.as_ref().map(|call| (stored, call)))
    {
        if let Ok((payload, _, _, _)) = replay(&state, loupist, call, Some(900)).await {
            if let Ok(points) = saved_read::history(&payload, &call.fields) {
                with_assets(&state, loupist, true, |tx| {
                    saved_read::settle_history(tx, loupist, asset, stored, &points)
                })?;
            }
        }
    }
    with_assets(&state, loupist, false, |tx| {
        saved_read::points(tx, loupist, asset)
    })
}

#[tauri::command(async)]
pub fn watchlist_quotes(
    state: State<'_, AppState>,
    request: AssetsListRequest,
) -> Result<Vec<StoredAssetQuote>, CommandError> {
    admit(&state, &request.loupist_id)?;
    let rows = state
        .store
        .assets_list(&request.loupist_id, true)
        .map_err(map_context_error)?;
    with_assets(&state, &request.loupist_id, false, |tx| {
        let mut prices = Vec::new();
        for asset in rows {
            if let Some(price) = saved_read::load(tx, &request.loupist_id, &asset.asset_id)?
                .and_then(|stored| stored.price)
            {
                prices.push(StoredAssetQuote {
                    asset_id: asset.asset_id,
                    price,
                });
            }
        }
        Ok(prices)
    })
}

pub(crate) async fn replay_current(
    state: &AppState,
    loupist: &str,
    asset: &str,
    max_age: Option<u64>,
) -> Result<(), CommandError> {
    let Some(stored) = with_assets(state, loupist, false, |tx| {
        saved_read::load(tx, loupist, asset)
    })?
    else {
        return Ok(());
    };
    let listing = state
        .store
        .asset_by_id(loupist, asset)
        .map_err(map_context_error)?
        .and_then(|row| row.reference)
        .map(|reference| reference.fields);
    let result = match replay(state, loupist, &stored.read.current, max_age).await {
        Ok((payload, provider, source_url, dataset)) => saved_read::price(
            &payload,
            &stored.read.current.fields,
            provider,
            source_url,
            dataset,
        )
        .map(|mut price| {
            // The same completion as the save, so an older read without a
            // currency gains one on its next repeat.
            saved_read::complete_currency(
                &mut price,
                stored.read.quoted_currency.as_deref(),
                listing.as_ref(),
            );
            price
        })
        .map_err(|_| PriceReadState::Failed),
        Err(status) => Err(status),
    };
    if with_assets(state, loupist, true, |tx| {
        saved_read::settle(tx, loupist, asset, &stored, result)
    })? {
        changed();
    }
    Ok(())
}

async fn replay(
    state: &AppState,
    loupist: &str,
    call: &SavedReadCall,
    max_age: Option<u64>,
) -> Result<
    (
        serde_json::Value,
        String,
        Option<String>,
        crate::datasets::DatasetRef,
    ),
    PriceReadState,
> {
    let capability_ids = super::capture_chat_connector_authority(state, loupist)
        .map_err(|_| PriceReadState::NeedsConnection)?
        .capability_ids
        .into_iter()
        .collect();
    let executor = AppToolExecutor::for_host(state, loupist, capability_ids);
    let home = crate::chat::home::ensure_home_thread(state, loupist)
        .map_err(|_| PriceReadState::Failed)?;
    let policy = ToolPolicy {
        loupist_id: loupist.into(),
        session_id: home.id,
        use_memory: false,
        propose_memory_updates: false,
        memory_proposal_budget: 0,
        as_of: None,
        display_currency: None,
    };
    let result = executor.replay_price_call(&policy, call, max_age).await;
    executor.host_shutdown().await;
    result
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssetEntitySetRequest {
    pub loupist_id: String,
    pub asset_id: String,
    #[cfg_attr(test, ts(type = "null"))]
    pub entity_id: serde_json::Value,
}

#[tauri::command(async)]
pub fn asset_entity_set(
    state: State<'_, AppState>,
    request: AssetEntitySetRequest,
) -> Result<ThinAssetPageView, CommandError> {
    admit(&state, &request.loupist_id)?;
    if !request.entity_id.is_null() {
        return Err(CommandError::invalid("entityId must be null."));
    }
    let _activity = state
        .maintenance
        .admit_kind(MaintenanceActivityKind::MemoryMutation)?;
    let asset = open_target(
        &state,
        &AssetOpenRequest {
            loupist_id: request.loupist_id.clone(),
            asset_id: Some(request.asset_id),
            ticker: None,
            entity_id: None,
            stored_only: None,
        },
    )?;
    with_assets(&state, &request.loupist_id, true, |tx| {
        assets::reject_entity(tx, &request.loupist_id, &asset.asset_id, now_ms())?;
        assets::page_view(tx, &request.loupist_id, asset)
    })
    .map(|page| with_price_setup(&state, &request.loupist_id, page))
    .inspect(|_| changed())
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssetVerdictUndoRequest {
    pub loupist_id: String,
    pub asset_id: String,
    pub move_id: String,
}

#[tauri::command(async)]
pub fn asset_verdict_undo(
    state: State<'_, AppState>,
    request: AssetVerdictUndoRequest,
) -> Result<ThinAssetPageView, CommandError> {
    admit(&state, &request.loupist_id)?;
    let _activity = state
        .maintenance
        .admit_kind(MaintenanceActivityKind::MemoryMutation)?;
    let asset = open_target(
        &state,
        &AssetOpenRequest {
            loupist_id: request.loupist_id.clone(),
            asset_id: Some(request.asset_id),
            ticker: None,
            entity_id: None,
            stored_only: None,
        },
    )?;
    with_assets(&state, &request.loupist_id, true, |tx| {
        assets::undo(
            tx,
            &request.loupist_id,
            &asset.asset_id,
            &request.move_id,
            now_ms(),
        )?;
        assets::page_view(tx, &request.loupist_id, asset)
    })
    .map(|page| with_price_setup(&state, &request.loupist_id, page))
    .inspect(|_| changed())
}

#[tauri::command(async)]
pub fn asset_page_seen(
    state: State<'_, AppState>,
    request: AssetIdRequest,
) -> Result<(), CommandError> {
    admit(&state, &request.loupist_id)?;
    with_assets(&state, &request.loupist_id, true, |tx| {
        assets::mark_seen(tx, &request.loupist_id, &request.asset_id, now_ms()).map(|_| ())
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

pub(crate) fn commit_turn(
    tx: &Transaction<'_>,
    loupist_id: &str,
    chat_id: &str,
    message_id: &str,
    at_ms: i64,
    theses: &[ThesisUpdate],
) -> KnowledgeResult<TurnAssets> {
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
