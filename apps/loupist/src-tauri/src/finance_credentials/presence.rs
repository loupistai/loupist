//! One lazy metadata pass for namespaces that predate credential status records.

use std::{
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};

use super::{
    native_account, reserve, status_projection, status_recovery, CredentialStatus,
    FinanceCredentialError, SessionAccess, StatusRecovery,
};
use crate::{app::AppState, keychain_operation::TIMEOUT, marketplace::MarketplaceCatalogDto};

pub(super) const SUPPORTED: bool = cfg!(any(test, target_os = "macos"));

#[derive(Clone, Default)]
pub(crate) struct PresencePass(Arc<(Mutex<PassState>, Condvar)>);

#[derive(Default)]
enum PassState {
    #[default]
    Idle,
    Running(Instant),
    Finished,
}

/// Call from a background task before taking any lifecycle or store guard.
/// Concurrent status surfaces wait for the same bounded pass.
pub(crate) fn reconcile_presence(state: &AppState, catalog: &MarketplaceCatalogDto) {
    reconcile_with_timeout(state, catalog, TIMEOUT);
}

pub(crate) fn reconcile_with_timeout(
    state: &AppState,
    catalog: &MarketplaceCatalogDto,
    timeout: Duration,
) {
    if !SUPPORTED {
        return;
    }
    let (phase, changed) = &*state.credential_presence.0;
    let Ok(mut current) = phase.lock() else {
        return;
    };
    match *current {
        PassState::Finished => return,
        PassState::Running(deadline) => {
            let _waited = changed.wait_timeout_while(
                current,
                deadline.saturating_duration_since(Instant::now()),
                |phase| matches!(phase, PassState::Running(_)),
            );
            return;
        }
        PassState::Idle => {}
    }
    let deadline = Instant::now() + timeout;
    *current = PassState::Running(deadline);
    drop(current);

    if state.store.legacy_credential_namespace().unwrap_or(false) {
        let backend = Arc::new(Backend::default());
        for entry in &catalog.entries {
            if deadline <= Instant::now() {
                break;
            }
            if entry
                .setup
                .as_ref()
                .is_some_and(|setup| !setup.credential_fields.is_empty())
                && matches!(status_projection(state, &entry.id), Ok(None))
            {
                // An unsuccessful check leaves no presence attestation. The
                // row can retry explicitly, even when this pass skipped it.
                let _ = probe(state, &entry.id, &backend, deadline, false);
            }
        }
    }
    if let Ok(mut current) = phase.lock() {
        *current = PassState::Finished;
        changed.notify_all();
    }
}

/// A stale retry request also stays metadata-only if another caller settled it.
pub(crate) fn retry_presence(state: &AppState, id: &str) -> Result<(), FinanceCredentialError> {
    if status_recovery(state, id) != Some(StatusRecovery::PresenceUnavailable) {
        return Ok(());
    }
    probe(
        state,
        id,
        &Arc::new(Backend::default()),
        Instant::now() + TIMEOUT,
        true,
    )
}

fn probe(
    state: &AppState,
    id: &str,
    backend: &Arc<Backend>,
    deadline: Instant,
    retry: bool,
) -> Result<(), FinanceCredentialError> {
    let reservation = reserve(state, id)?;
    // A secret operation might have published while this probe was being
    // scheduled. Only replace the kind of record this probe was asked to fix.
    let current = status_projection(state, id)?;
    if current.is_some()
        && !(retry
            && matches!(
                current,
                Some(SessionAccess::Unconfirmed | SessionAccess::StoreUnavailable)
            ))
    {
        return Ok(());
    }
    let timeout = deadline.saturating_duration_since(Instant::now());
    if timeout.is_zero() {
        return Err(FinanceCredentialError::Unavailable);
    }
    let account = native_account(&state.credential_environment, id);
    let backend = backend.clone();
    let present = reservation.probe(timeout, move || {
        if Instant::now() >= deadline {
            return Err(FinanceCredentialError::Unavailable);
        }
        backend.contains(&account)
    })??;
    let access = if present {
        SessionAccess::PresentUnread
    } else {
        SessionAccess::Known {
            status: CredentialStatus::default(),
            active_revision: None,
            last_store_failure: false,
        }
    };
    let encoded = serde_json::to_string(&access).map_err(|_| FinanceCredentialError::Store)?;
    state
        .store
        .set_connector_credential_status(id, &encoded)
        .map_err(|_| FinanceCredentialError::Store)?;
    // A failed mutation may also have left a process-local projection.
    super::remember(&state.credential_environment, id, None);
    Ok(())
}

#[derive(Default)]
struct Backend {
    #[cfg(all(
        target_os = "macos",
        not(any(debug_assertions, test, feature = "webdriver"))
    ))]
    keychain: std::sync::OnceLock<Result<security_framework::os::macos::keychain::SecKeychain, ()>>,
}

impl Backend {
    fn contains(&self, _account: &str) -> Result<bool, FinanceCredentialError> {
        #[cfg(any(debug_assertions, test, feature = "webdriver"))]
        {
            #[cfg(test)]
            if !super::test_store::record_presence(_account) {
                return Err(FinanceCredentialError::Unavailable);
            }
            Ok(super::memory_store()
                .lock()
                .map_err(|_| FinanceCredentialError::Store)?
                .contains_key(_account))
        }
        #[cfg(all(
            target_os = "macos",
            not(any(debug_assertions, test, feature = "webdriver"))
        ))]
        {
            use security_framework::{
                item::{ItemClass, ItemSearchOptions},
                os::macos::keychain::{SecKeychain, SecPreferencesDomain},
            };

            // Match keyring's user-domain file keychain. A reference-only
            // search neither returns password data nor needs its decrypt ACL.
            let keychain = self
                .keychain
                .get_or_init(|| {
                    SecKeychain::default_for_domain(SecPreferencesDomain::User).map_err(|_| ())
                })
                .as_ref()
                .map_err(|_| FinanceCredentialError::Unavailable)?;
            match ItemSearchOptions::new()
                .keychains(std::slice::from_ref(keychain))
                .class(ItemClass::generic_password())
                .service(super::NATIVE_SERVICE)
                .account(_account)
                .load_refs(true)
                .search()
            {
                Ok(items) => Ok(!items.is_empty()),
                Err(error) => match keyring::macos::decode_error(error) {
                    keyring::Error::NoEntry => Ok(false),
                    _ => Err(FinanceCredentialError::Unavailable),
                },
            }
        }
        #[cfg(not(any(target_os = "macos", debug_assertions, test, feature = "webdriver")))]
        Err(FinanceCredentialError::Unsupported)
    }
}
