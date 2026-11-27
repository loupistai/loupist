//! `provider_quota_read`: the OpenRouter credit line, or null.
//!
//! Other providers, an account with no stored key, a key with no limit, and a
//! failed read are null. The key stays on the credential path the catalog
//! already uses and is not returned.

use serde::Deserialize;
use tauri::State;

use crate::app::{AppState, CommandError};
use crate::providers::openrouter_quota::OpenRouterQuota;

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct ProviderQuotaRequest {
    pub provider: String,
}

#[tauri::command(rename_all = "snake_case")]
pub async fn provider_quota_read(
    request: ProviderQuotaRequest,
    state: State<'_, AppState>,
) -> Result<Option<OpenRouterQuota>, CommandError> {
    if request.provider != "openrouter" {
        return Ok(None);
    }
    let agent = state.pi_agent_data_dir()?;
    let Some(key) = crate::providers::catalog::provider_api_key(&agent, "openrouter")? else {
        return Ok(None);
    };
    Ok(crate::providers::openrouter_quota::current(&key).await)
}
