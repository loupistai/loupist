//! Explicit recovery of legacy finance items. The batch owns one prompt at a
//! time. Cancellation discards any in-flight result and stops the sequence.

use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{ipc::Channel, State};

use crate::{
    app::{AppState, CommandError},
    finance_credentials,
    keychain_operation::KeychainError,
};

use super::{bundled_catalog, MarketplaceEntryDto};

#[derive(Clone, Default)]
pub(crate) struct PermissionBatch(Arc<Mutex<Option<Batch>>>);

struct Batch {
    id: String,
    cancelled: Arc<Mutex<bool>>,
}

pub(super) struct ActiveBatch {
    owner: PermissionBatch,
    cancelled: Arc<Mutex<bool>>,
}

impl Drop for ActiveBatch {
    fn drop(&mut self) {
        if let Ok(mut batch) = self.owner.0.lock() {
            *batch = None;
        }
    }
}

impl PermissionBatch {
    pub(super) fn start(&self, id: String) -> Result<ActiveBatch, CommandError> {
        let mut batch = self
            .0
            .lock()
            .map_err(|_| KeychainError::Unavailable.command_error())?;
        if batch.is_some() {
            return Err(KeychainError::Busy.command_error());
        }
        let cancelled = Arc::new(Mutex::new(false));
        *batch = Some(Batch {
            id,
            cancelled: cancelled.clone(),
        });
        Ok(ActiveBatch {
            owner: self.clone(),
            cancelled,
        })
    }

    pub(super) fn cancel(&self, id: &str) {
        if let Ok(batch) = self.0.lock() {
            if let Some(batch) = batch.as_ref().filter(|batch| batch.id == id) {
                if let Ok(mut cancelled) = batch.cancelled.lock() {
                    *cancelled = true;
                }
            }
        }
    }
}

pub(crate) fn entries_needing_permission(
    state: &AppState,
) -> Result<Vec<MarketplaceEntryDto>, CommandError> {
    Ok(bundled_catalog()?
        .entries
        .into_iter()
        .filter(|entry| finance_credentials::needs_permission(state, &entry.id))
        .collect())
}

#[derive(Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct PermissionProgress {
    pub(super) settled: usize,
    pub(super) total: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    failed_entry_id: Option<String>,
}

pub(super) fn run_batch(
    state: &AppState,
    batch: &ActiveBatch,
    progress: impl Fn(PermissionProgress) -> bool,
) -> Result<(), CommandError> {
    let entries = entries_needing_permission(state)?;
    for entry in &entries {
        if finance_credentials::operation_pending(state, &entry.id)
            .map_err(super::credentials::credential_store_error)?
        {
            return Err(KeychainError::Busy.command_error());
        }
    }
    let total = entries.len();
    if !progress(PermissionProgress {
        settled: 0,
        total,
        failed_entry_id: None,
    }) {
        return Ok(());
    }
    for (index, entry) in entries.into_iter().enumerate() {
        if *batch
            .cancelled
            .lock()
            .map_err(|_| KeychainError::Unavailable.command_error())?
        {
            break;
        }
        // The reservation is held through native completion even if the user
        // cancels. Another caller cannot start this item during the OS dialog.
        let result = finance_credentials::reserve(state, &entry.id).and_then(|_item| {
            finance_credentials::read_saved_key_permission(state, &entry.id, &batch.cancelled)
        });
        if !progress(PermissionProgress {
            settled: index + 1,
            total,
            failed_entry_id: result.is_err().then_some(entry.id),
        }) {
            break;
        }
    }
    Ok(())
}

#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn marketplace_credentials_allow_access(
    request: String,
    on_event: Channel<PermissionProgress>,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    if uuid::Uuid::parse_str(&request).is_err() {
        return Err(CommandError::invalid("Invalid saved key request."));
    }
    let batch = state.key_permission_batch.start(request)?;
    let state = state.inner().clone();
    let (send, receive) = tokio::sync::oneshot::channel();
    // This explicit read-only sequence waits for the OS, independently of
    // Tokio's bounded work and the normal 30-second keychain wait.
    std::thread::Builder::new()
        .name("loupist-saved-key-permission".into())
        .spawn(move || {
            let result = run_batch(&state, &batch, |update| on_event.send(update).is_ok());
            drop(batch);
            let _ = send.send(result);
        })
        .map_err(|_| KeychainError::Unavailable.command_error())?;
    receive
        .await
        .map_err(|_| KeychainError::Unavailable.command_error())?
}

#[tauri::command(rename_all = "snake_case")]
pub(crate) fn marketplace_credentials_cancel_access(request: String, state: State<'_, AppState>) {
    state.key_permission_batch.cancel(&request);
}
