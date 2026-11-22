//! Bounded waits for native credential calls. A timed-out native operation
//! still owns its item until the operating system and its completion finish.

use std::{
    cell::Cell,
    collections::{btree_map::Entry, BTreeMap},
    sync::{mpsc, Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

use crate::runs::maintenance::{ActivityLease, MaintenanceActivityKind, MaintenanceCoordinator};

pub(crate) const TIMEOUT: Duration = Duration::from_secs(30);

thread_local! {
    static DEADLINE: Cell<Option<Instant>> = const { Cell::new(None) };
}

/// Validate immediately before starting a native write or delete. A read in a
/// compound operation may have consumed the caller's entire wait budget.
pub(crate) fn check_mutation_deadline() -> Result<(), KeychainError> {
    if DEADLINE
        .get()
        .is_some_and(|deadline| Instant::now() >= deadline)
    {
        return Err(KeychainError::Unavailable);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub(crate) enum KeychainError {
    #[error("a keychain operation is still pending for this item")]
    Busy,
    #[error("the keychain is unavailable")]
    Unavailable,
    #[error("the keychain operation timed out")]
    TimedOut,
    #[error("Loupist is installing an update")]
    UpdateInstalling,
}

impl KeychainError {
    pub(crate) fn command_error(self) -> crate::app::CommandError {
        let (kind, message) = match self {
            Self::UpdateInstalling => return crate::app::CommandError::new(
                crate::app::CommandErrorCode::MaintenanceActive,
                "Loupist is installing an update; wait for the restart before starting new work",
            ),
            Self::Busy => (
                "keychain_busy",
                "A saved-key operation is still waiting for macOS.",
            ),
            Self::Unavailable | Self::TimedOut => (
                "keychain_unavailable",
                "The Keychain did not respond in time.",
            ),
        };
        crate::app::CommandError::with_detail(
            crate::app::CommandErrorCode::CredentialUnavailable,
            message,
            serde_json::json!({ "kind": kind }),
        )
    }
}

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd)]
struct Item {
    service: &'static str,
    account: String,
}

fn pending() -> &'static Mutex<BTreeMap<Item, bool>> {
    // The value records whether the caller's wait for this read expired.
    static PENDING: OnceLock<Mutex<BTreeMap<Item, bool>>> = OnceLock::new();
    PENDING.get_or_init(Default::default)
}

struct Reservation {
    item: Item,
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if let Ok(mut pending) = pending().lock() {
            pending.remove(&self.item);
        }
    }
}

pub(crate) struct KeychainOperation(Arc<Reservation>);

impl KeychainOperation {
    pub(crate) fn pending(service: &'static str, account: String) -> Result<bool, KeychainError> {
        let item = Item { service, account };
        Ok(pending()
            .lock()
            .map_err(|_| KeychainError::Unavailable)?
            .contains_key(&item))
    }

    pub(crate) fn read_timed_out(service: &'static str, account: String) -> bool {
        pending()
            .lock()
            .is_ok_and(|pending| pending.get(&Item { service, account }) == Some(&true))
    }

    pub(crate) fn begin(service: &'static str, account: String) -> Result<Self, KeychainError> {
        let item = Item { service, account };
        let mut pending = pending().lock().map_err(|_| KeychainError::Unavailable)?;
        let Entry::Vacant(slot) = pending.entry(item.clone()) else {
            return Err(KeychainError::Busy);
        };
        slot.insert(false);
        Ok(Self(Arc::new(Reservation { item })))
    }

    pub(crate) fn read<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Result<T, KeychainError> {
        self.read_with_timeout(TIMEOUT, work)
    }

    pub(crate) fn read_with_timeout<T: Send + 'static>(
        &self,
        timeout: Duration,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Result<T, KeychainError> {
        let result = self.wait(timeout, None, work);
        // The worker releases the reservation only after it has published the
        // actual result. Keep this transient marker off the durable record.
        if matches!(result, Err(KeychainError::TimedOut)) {
            let mut pending = pending().lock().map_err(|_| KeychainError::Unavailable)?;
            pending.insert(self.0.item.clone(), true);
        }
        result
    }

    /// Presence timeouts do not imply a pending read of secret data.
    pub(crate) fn probe<T: Send + 'static>(
        &self,
        timeout: Duration,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Result<T, KeychainError> {
        self.wait(timeout, None, work)
    }

    pub(crate) fn mutate<T: Send + 'static>(
        &self,
        maintenance: &MaintenanceCoordinator,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Result<T, KeychainError> {
        self.mutate_with_timeout(maintenance, TIMEOUT, work)
    }

    pub(crate) fn mutate_with_timeout<T: Send + 'static>(
        &self,
        maintenance: &MaintenanceCoordinator,
        timeout: Duration,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Result<T, KeychainError> {
        let activity = maintenance
            .admit_kind(MaintenanceActivityKind::KeychainMutation)
            .map_err(|error| {
                if error.code == crate::app::CommandErrorCode::MaintenanceActive {
                    KeychainError::UpdateInstalling
                } else {
                    KeychainError::Unavailable
                }
            })?;
        self.wait(timeout, Some(activity), work)
    }

    fn wait<T: Send + 'static>(
        &self,
        timeout: Duration,
        activity: Option<ActivityLease>,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Result<T, KeychainError> {
        // Dropping the receiver cannot cancel a system call. The worker still
        // reconciles its durable outcome before releasing the item or update.
        let reservation = Arc::clone(&self.0);
        let deadline = Instant::now() + timeout;
        let (send, receive) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("loupist-keychain".into())
            .spawn(move || {
                DEADLINE.set(Some(deadline));
                let value = work();
                drop(activity);
                drop(reservation);
                let _ = send.send(value);
            })
            .map_err(|_| KeychainError::Unavailable)?;
        receive
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => KeychainError::TimedOut,
                mpsc::RecvTimeoutError::Disconnected => KeychainError::Unavailable,
            })
    }
}

#[cfg(all(
    not(any(debug_assertions, test, feature = "webdriver")),
    any(target_os = "macos", windows)
))]
pub(crate) fn delete_entry(entry: keyring::Entry) -> Result<(), KeychainError> {
    match entry.delete_credential() {
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err(KeychainError::Unavailable),
        Ok(()) => {
            // The pinned macOS backend does not return the status of the
            // actual delete after lookup. Do not attest success until absent.
            #[cfg(target_os = "macos")]
            if !matches!(entry.get_password(), Err(keyring::Error::NoEntry)) {
                return Err(KeychainError::Unavailable);
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn blocked_mutation_times_out_without_releasing_item_or_update_exclusion() {
        let maintenance = MaintenanceCoordinator::default();
        let operation = KeychainOperation::begin("test", "blocked".into()).unwrap();
        let blocker = maintenance
            .admit_kind(MaintenanceActivityKind::KeychainMutation)
            .unwrap();
        let (release, pending) = mpsc::channel();
        let completed = Arc::new(AtomicBool::new(false));
        let publish = completed.clone();
        assert_eq!(
            operation.wait(Duration::from_millis(20), Some(blocker), move || {
                pending.recv().unwrap();
                publish.store(true, Ordering::SeqCst);
            }),
            Err(KeychainError::TimedOut)
        );
        drop(operation);
        assert!(matches!(
            KeychainOperation::begin("test", "blocked".into()),
            Err(KeychainError::Busy)
        ));
        assert_eq!(
            maintenance.begin_update().unwrap_err()[0].kind,
            "keychain_mutation"
        );
        let other = KeychainOperation::begin("test", "other".into()).unwrap();
        assert_eq!(other.read(|| 42), Ok(42));
        assert!(!completed.load(Ordering::SeqCst));

        release.send(()).unwrap();
        let until = std::time::Instant::now() + Duration::from_secs(5);
        let current = loop {
            match KeychainOperation::begin("test", "blocked".into()) {
                Ok(operation) => break operation,
                Err(KeychainError::Busy) => {
                    assert!(std::time::Instant::now() < until);
                    std::thread::yield_now();
                }
                Err(error) => panic!("{error}"),
            }
        };
        assert!(completed.load(Ordering::SeqCst));
        assert!(maintenance.begin_update().is_ok());
        assert_eq!(current.read(|| "settled"), Ok("settled"));
    }
}
