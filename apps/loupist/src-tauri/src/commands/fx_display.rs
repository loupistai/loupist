//! Rate snapshots for the requested Loupist's display currency.
use std::future::Future;

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;
use tauri::State;

use super::tool_executor::AppToolExecutor;
use crate::{
    app::{AppState, CommandError},
    broker::{BrokerError, ToolPolicy},
    finance_data::fx::FxDisplay,
};

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FxDisplayRequest {
    pub loupist_id: String,
}

#[tauri::command]
pub async fn fx_display(
    request: FxDisplayRequest,
    state: State<'_, AppState>,
) -> Result<FxDisplay, CommandError> {
    #[cfg(feature = "eval-harness")]
    let now = crate::eval_harness::clock::now();
    #[cfg(not(feature = "eval-harness"))]
    let now = Utc::now();
    read(&state, request, now, fetch_rates)
}

fn read<F, Fut>(
    state: &AppState,
    request: FxDisplayRequest,
    now: DateTime<Utc>,
    fetch: F,
) -> Result<FxDisplay, CommandError>
where
    F: FnOnce(AppState, String, Vec<String>) -> Fut + Send + 'static,
    Fut: Future<Output = Result<Value, BrokerError>> + Send + 'static,
{
    let loupist_id = request.loupist_id;
    super::require_text(&loupist_id, "Loupist", 512)?;
    state.ensure_loupist_available(&loupist_id)?;
    // Bind this workspace even if the global setting is unset or there is a cache hit.
    super::loupist_config_directory(state, &loupist_id)?;
    let appearance = crate::scoped_config::resolve(&state.artifacts.app_data_dir, None)?.appearance;
    let Some(currency) = appearance.display_currency else {
        return Ok(FxDisplay::Unset);
    };
    let capabilities = super::capture_chat_connector_authority(state, &loupist_id)?.capability_ids;
    let connected = capabilities.iter().any(|id| id == "openbb.platform");
    let app = state.clone();
    Ok(state
        .fx_reference
        .display(now, currency, connected, move || {
            fetch(app, loupist_id, capabilities)
        }))
}

async fn fetch_rates(
    state: AppState,
    loupist_id: String,
    capabilities: Vec<String>,
) -> Result<Value, BrokerError> {
    // Host admission uses the ordinary home Thread without recording a turn.
    let session_id = crate::chat::home::ensure_home_thread(&state, &loupist_id)
        .map_err(|_| BrokerError::Execution("FX display scope unavailable".into()))?
        .id;
    let mut executor =
        AppToolExecutor::for_host(&state, &loupist_id, capabilities.into_iter().collect());
    let policy = ToolPolicy {
        loupist_id,
        session_id,
        use_memory: false,
        propose_memory_updates: false,
        memory_proposal_budget: 0,
        as_of: None,
        display_currency: None,
    };
    executor.host_fx_read(&policy).await
}

#[cfg(all(test, unix))]
mod tests;
