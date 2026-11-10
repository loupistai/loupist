//! App-level renderer intent. No Loupist identifier or prompt injection.
use crate::{
    app::{AppState, CommandError},
    store::user_context::{crypto_file, wallets, *},
};
use serde::Deserialize;
use std::collections::BTreeMap;
use tauri::State;

fn map_context_error(error: crate::store::StoreError) -> CommandError {
    match error {
        crate::store::StoreError::Invalid(message) => CommandError::invalid(message),
        other => super::map_store(other),
    }
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WatchlistAddRequest {
    pub symbol: String,
    pub market: String,
    pub display_name: Option<String>,
    /// The Loupist open when the user followed it: it watches the asset with
    /// the default kinds (finance-watchers.md §3.2).
    #[serde(default)]
    pub loupist_id: Option<String>,
}
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WatchlistRemoveRequest {
    pub symbol: String,
    pub market: String,
}
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CsvImportRequest {
    pub path: String,
}
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CsvCommitRequest {
    pub preview_id: String,
}

#[tauri::command]
pub fn watchlist_list(state: State<'_, AppState>) -> Result<Vec<WatchlistEntry>, CommandError> {
    state.store.watchlist_list().map_err(map_context_error)
}
#[tauri::command]
pub fn watchlist_add(
    request: WatchlistAddRequest,
    state: State<'_, AppState>,
) -> Result<WatchlistEntry, CommandError> {
    follow(
        &state,
        &request.symbol,
        &request.market,
        request.display_name.as_deref(),
        request.loupist_id.as_deref(),
    )
}

/// Following adds the asset to the watchlist; once a burst of follows settles,
/// every Loupist reads it from a new "What you follow" Source.
pub(crate) fn follow(
    state: &AppState,
    symbol: &str,
    market: &str,
    display_name: Option<&str>,
    loupist_id: Option<&str>,
) -> Result<WatchlistEntry, CommandError> {
    let entry = state
        .store
        .watchlist_add(symbol, market, display_name)
        .map_err(map_context_error)?;
    if let Some(loupist_id) = loupist_id {
        // Following never fails because the open Loupist could not take defaults.
        if let Err(error) = crate::commands::watch::on_followed(state, loupist_id, symbol, market) {
            eprintln!("Watch defaults were not applied: {}", error.code);
        }
    }
    state.followed.schedule(state);
    Ok(entry)
}
#[tauri::command]
pub fn watchlist_remove(
    request: WatchlistRemoveRequest,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    state
        .store
        .watchlist_remove(&request.symbol, &request.market)
        .map_err(map_context_error)
}
#[tauri::command]
pub async fn holdings_import_csv(
    request: CsvImportRequest,
    state: State<'_, AppState>,
) -> Result<HoldingsCsvPreview, CommandError> {
    let store = state.store.clone();
    tokio::task::spawn_blocking(move || {
        store.holdings_import_csv(std::path::Path::new(&request.path))
    })
    .await
    .map_err(|_| CommandError::internal("CSV import task failed"))?
    .map_err(map_context_error)
}
#[tauri::command]
pub fn holdings_commit_csv(
    request: CsvCommitRequest,
    state: State<'_, AppState>,
) -> Result<HoldingsSnapshot, CommandError> {
    let snapshot = state
        .store
        .holdings_commit_csv(&request.preview_id)
        .map_err(map_context_error)?;
    state.followed.schedule(&state);
    Ok(snapshot)
}

fn account_profile(
    values: BTreeMap<String, String>,
) -> Result<crate::finance_data::KisAccountProfile, CommandError> {
    if ["account_number", "account_product_code"]
        .iter()
        .any(|key| values.get(*key).is_none_or(|value| value.is_empty()))
    {
        return Err(CommandError::new("needs_account_profile", "Add the optional account profile to the Korea Investment connection to import holdings."));
    }
    crate::finance_data::KisAccountProfile::from_values(values).map_err(|_| {
        CommandError::new(
            "needs_account_profile",
            "The Korea Investment account profile is incomplete or invalid.",
        )
    })
}
#[tauri::command]
pub async fn holdings_import_kis(
    state: State<'_, AppState>,
) -> Result<HoldingsSnapshot, CommandError> {
    crate::marketplace::connector_config::reconcile_connector_configs(&state)?;
    let environment_id = state.credential_environment.clone();
    let credential = tokio::task::spawn_blocking(move || {
        crate::finance_credentials::get(&environment_id, crate::finance_data::KIS_SOURCE_ID)
    })
    .await
    .map_err(|_| CommandError::internal("credential lookup failed"))?
    .map_err(|_| CommandError::internal("credential lookup failed"))?
    .ok_or_else(|| {
        CommandError::new(
            "connection_unavailable",
            "Configure the Korea Investment connection first.",
        )
    })?;
    let profile = account_profile(
        ["account_number", "account_product_code"]
            .into_iter()
            .filter_map(|key| {
                credential
                    .get(key)
                    .map(|value| (key.to_owned(), value.to_owned()))
            })
            .collect(),
    )?;
    let environment = crate::marketplace::connector_config_value(
        &state,
        crate::finance_data::KIS_SOURCE_ID,
        "environment",
    )?
    .ok_or_else(|| {
        CommandError::new(
            "connection_unavailable",
            "Choose the Korea Investment connection environment.",
        )
    })?;
    let app_key = credential.get("app_key").ok_or_else(|| {
        CommandError::new(
            "connection_unavailable",
            "Connection credentials are unavailable.",
        )
    })?;
    let app_secret = credential.get("app_secret").ok_or_else(|| {
        CommandError::new(
            "connection_unavailable",
            "Connection credentials are unavailable.",
        )
    })?;
    let positions = state.finance_data.kis_holdings_read(&environment, app_key, app_secret, &profile).await
        .map_err(|_| CommandError::new("holdings_import_failed", "Could not import a complete Korea Investment balance. Check the connection or use a CSV export."))?;
    let snapshot = state
        .store
        .save_holdings_snapshot(HoldingsSource::Kis, positions)
        .map_err(map_context_error)?;
    state.followed.schedule(&state);
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn user_context_kis_needs_account_profile() {
        for values in [
            BTreeMap::new(),
            BTreeMap::from([("account_number".into(), "12345678".into())]),
        ] {
            assert_eq!(
                account_profile(values).err().unwrap().code,
                "needs_account_profile"
            );
        }
        assert!(account_profile(BTreeMap::from([
            ("account_number".into(), "12345678".into()),
            ("account_product_code".into(), "01".into())
        ]))
        .is_ok());
    }
}

/// Counts and dates per source and per kind only: no positions, quantities,
/// values or cost basis ever reach this read.
#[tauri::command]
pub fn holdings_summary(state: State<'_, AppState>) -> Result<HoldingsOverview, CommandError> {
    state.store.holdings_summary().map_err(map_context_error)
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MapColumnsRequest {
    pub preview_id: String,
    pub asset: String,
    pub balance: String,
}
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WalletAddRequest {
    pub address: String,
    pub name: Option<String>,
}
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WalletRenameRequest {
    pub wallet_id: String,
    pub name: Option<String>,
}
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WalletIdRequest {
    pub wallet_id: String,
}
#[derive(Deserialize, Default)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WalletRefreshRequest {
    pub wallet_id: Option<String>,
}

/// An exchange balance file: Rust detects the format and freezes the preview.
#[tauri::command]
pub async fn holdings_import_crypto_file(
    request: CsvImportRequest,
    state: State<'_, AppState>,
) -> Result<crypto_file::CryptoFilePreview, CommandError> {
    let store = state.store.clone();
    tokio::task::spawn_blocking(move || {
        store.holdings_import_crypto_file(std::path::Path::new(&request.path))
    })
    .await
    .map_err(|_| CommandError::internal("CSV import task failed"))?
    .map_err(map_context_error)
}

#[tauri::command]
pub fn holdings_map_columns(
    request: MapColumnsRequest,
    state: State<'_, AppState>,
) -> Result<crypto_file::CryptoFilePreview, CommandError> {
    state
        .store
        .holdings_map_columns(&request.preview_id, &request.asset, &request.balance)
        .map_err(map_context_error)
}

#[tauri::command]
pub fn wallets_list(state: State<'_, AppState>) -> Result<Vec<wallets::WalletView>, CommandError> {
    state.store.wallets_list().map_err(map_context_error)
}

/// Validates (secret guard first), stores the address and reads it once.
/// A refused secret is never stored, logged or echoed back.
#[tauri::command]
pub async fn wallet_add(
    request: WalletAddRequest,
    state: State<'_, AppState>,
) -> Result<Vec<wallets::WalletView>, CommandError> {
    let added = state
        .store
        .wallet_add(&request.address, request.name.as_deref())
        .map_err(|error| match error {
            wallets::WalletAddError::Secret => CommandError::new(
                "secret_refused",
                "This looks like a recovery phrase or private key. Loupist never needs it.",
            ),
            wallets::WalletAddError::Unsupported => {
                CommandError::new("unsupported_address", "Not a supported address.")
            }
            wallets::WalletAddError::Limit => {
                CommandError::new("wallet_limit", "You can add up to 10 wallets.")
            }
            wallets::WalletAddError::Duplicate => {
                CommandError::conflict("This wallet is already added.")
            }
            wallets::WalletAddError::Store(message) => CommandError::invalid(message),
        })?;
    let wallets = state.store.wallets_list().map_err(map_context_error)?;
    if wallets.len() == 1 {
        crate::marketplace::enable_on_first_use(
            &state,
            crate::finance_data::onchain::ONCHAIN_SOURCE_ID,
        )?;
    }
    refresh_wallets(&state, Some(&added.id)).await;
    state.store.wallets_list().map_err(map_context_error)
}

#[tauri::command]
pub fn wallet_rename(
    request: WalletRenameRequest,
    state: State<'_, AppState>,
) -> Result<wallets::WalletView, CommandError> {
    state
        .store
        .wallet_rename(&request.wallet_id, request.name.as_deref())
        .map_err(map_context_error)
}

/// Deletes the address and its balances from this computer.
#[tauri::command]
pub fn wallet_remove(
    request: WalletIdRequest,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    state
        .store
        .wallet_remove(&request.wallet_id)
        .map_err(map_context_error)
}

#[tauri::command]
pub async fn wallet_refresh(
    request: Option<WalletRefreshRequest>,
    state: State<'_, AppState>,
) -> Result<Vec<wallets::WalletView>, CommandError> {
    let request = request.unwrap_or_default();
    refresh_wallets(&state, request.wallet_id.as_deref()).await;
    state.store.wallets_list().map_err(map_context_error)
}

/// The longest one refresh may take (Add, Refresh, before a briefing).
pub(crate) const WALLET_REFRESH_BUDGET: std::time::Duration = std::time::Duration::from_secs(30);

/// Reads the given wallet, or all of them, within the 30 s budget. A failed
/// read keeps the last good snapshot; wallets not reached in time are left as
/// they were. Nothing here is returned to a model or written to a log.
pub(crate) async fn refresh_wallets(state: &AppState, wallet_id: Option<&str>) {
    let Ok(targets) = state.store.wallet_targets(wallet_id) else {
        return;
    };
    let reads = async {
        for target in targets {
            match state.finance_data.wallet_read(&target).await {
                Ok(read) => {
                    let _ = state.store.wallet_save_read(&target.id, read);
                }
                Err(_) => {
                    let _ = state.store.wallet_mark_failed(&target.id);
                }
            }
        }
    };
    let _ = tokio::time::timeout(WALLET_REFRESH_BUDGET, reads).await;
    // A token that appeared in a wallet is followed from now on.
    state.followed.schedule(state);
}
