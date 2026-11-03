//! What the user follows reaches each Loupist's Map through "What you follow"
//! Sources and ordinary World jobs: at creation, after a burst of follows, and
//! when a Loupist is opened.
use super::{
    admit, admit_mutation, graph_settings_view, load_graph_settings, with_loupist_knowledge,
};
use crate::{
    app::{AppState, CommandError},
    knowledge::{FollowedAsset, FollowedState, FollowedStatus},
    store::LoupistStore,
};
use serde::Deserialize;
use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tauri::State;

/// A burst of follows (several symbols, an import) settles before anything is written.
const QUIET: Duration = Duration::from_secs(3);

fn current_followed(state: &AppState) -> Result<Vec<FollowedAsset>, CommandError> {
    let context = state
        .store
        .map_context()
        .map_err(crate::commands::map_store)?;
    let asset = |item: crate::store::user_context::MapContextItem| {
        FollowedAsset::new(&item.symbol, &item.market, item.display_name.as_deref())
    };
    Ok(crate::knowledge::followed_assets(
        context.watchlist.into_iter().map(asset),
        context.holdings.into_iter().map(asset),
    ))
}

/// Brings one Loupist's "What you follow" Sources up to date with the watchlist
/// and holdings: assets no earlier one lists get a new Source (names, symbols and
/// markets; holdings by name only), an anchor each and an ordinary World job.
/// Without a connected model the Source waits and is queued when one connects.
/// Returns how many World jobs were queued now.
pub(crate) fn sync_followed_assets(
    state: &AppState,
    loupist_id: &str,
) -> Result<usize, CommandError> {
    let assets = current_followed(state)?;
    if assets.is_empty() {
        return Ok(0);
    }
    let _activity = admit_mutation(state, loupist_id)?;
    let at_ms = crate::commands::now_ms();
    let queued = with_loupist_knowledge(state, loupist_id, |tx, blobs| {
        crate::knowledge::KnowledgeWrite::bind(tx, blobs).sync_followed(loupist_id, &assets, at_ms)
    })?;
    if queued > 0 {
        state.graph.wake();
    }
    Ok(queued)
}

/// Every Loupist that can be opened now; one that cannot catches up when it is opened.
fn sync_every_loupist(state: &AppState) {
    let Ok(loupists) = state.store.list_loupists() else {
        return;
    };
    for loupist in loupists {
        if sync_followed_assets(state, &loupist.id).is_err() {
            eprintln!(
                "Loupist could not add what the user follows to {}",
                loupist.id
            );
        }
    }
}

/// Follows and imports schedule one write per burst: each change restarts the
/// wait, and only the wait that ends last writes.
#[derive(Clone, Default)]
pub(crate) struct FollowedSync {
    generation: Arc<AtomicU64>,
}

impl FollowedSync {
    pub(crate) fn schedule(&self, state: &AppState) {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let latest = self.generation.clone();
        let state = state.clone();
        let wait = async move {
            tokio::time::sleep(QUIET).await;
            if latest.load(Ordering::SeqCst) == generation {
                sync_every_loupist(&state);
            }
        };
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => drop(runtime.spawn(wait)),
            Err(_) => drop(tauri::async_runtime::spawn(wait)),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FollowedStatusRequest {
    pub loupist_id: String,
}

/// Where reading what the user follows stands for one Loupist. A queued read
/// that World processing cannot start now (no model, a provider to reconnect,
/// recovery) is `waiting`.
pub(crate) fn followed_status(
    state: &AppState,
    loupist_id: &str,
) -> Result<FollowedStatus, CommandError> {
    admit(state, loupist_id)?;
    let mut status = with_loupist_knowledge(state, loupist_id, |tx, blobs| {
        crate::knowledge::KnowledgeWrite::bind(tx, blobs).followed_status(loupist_id)
    })?;
    if status.state == FollowedState::Queued {
        let settings = load_graph_settings(state, loupist_id)?;
        let view = graph_settings_view(state, loupist_id, &settings)?;
        status.state = queued_state(&view.readiness.status);
    }
    Ok(status)
}

/// A queued read by World processing readiness. A missing runtime is a broken
/// install, not something to wait for; the screen says a read is taking long.
fn queued_state(readiness: &str) -> FollowedState {
    match readiness {
        "ready" | "runtime_unavailable" => FollowedState::Queued,
        "disabled" | "paused" => FollowedState::Off,
        _ => FollowedState::Waiting,
    }
}

#[tauri::command]
pub fn knowledge_followed_status(
    request: FollowedStatusRequest,
    state: State<'_, AppState>,
) -> Result<FollowedStatus, CommandError> {
    followed_status(state.inner(), &request.loupist_id)
}

#[cfg(test)]
mod tests;
