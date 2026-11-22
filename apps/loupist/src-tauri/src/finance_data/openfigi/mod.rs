//! Submitted listing lookups through the bounded OpenFIGI transport.
mod http;
mod response;

use std::sync::OnceLock;

use crate::{
    app::{AppState, CommandError},
    commands::assets::wire::AssetLookupCandidate,
    finance_credentials::{self, ActiveCredentialBundle, FinanceCredentialError},
    keychain_operation::KeychainError,
};

use http::{Clock, Endpoint, Https, Limiter, RuntimeClock, Transport, DEADLINE};

/// The published equity list includes two distinct venues with code TT.
pub(crate) fn exchange_name(code: &str) -> Option<&'static str> {
    #[derive(serde::Deserialize)]
    struct Directory {
        exchanges: std::collections::BTreeMap<String, Vec<(String, Option<String>)>>,
    }
    static DIRECTORY: OnceLock<Directory> = OnceLock::new();
    let directory = DIRECTORY.get_or_init(|| {
        serde_json::from_str(include_str!("equity-exchanges.json"))
            .expect("bundled OpenFIGI exchanges")
    });
    match directory.exchanges.get(code)?.as_slice() {
        [(name, _)] => Some(name),
        _ => None,
    }
}

pub(crate) const SOURCE_ID: &str = "openfigi.lookup";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LookupStatus {
    Partial,
    GrantOff,
    CredentialLocked,
    CredentialUnavailable,
    Keychain(KeychainError),
    InvalidKey,
    Unavailable,
    Timeout,
    Offline,
    Busy { retry_after_seconds: u64 },
    Cancelled,
    InvalidQuery,
}

impl From<LookupStatus> for CommandError {
    fn from(value: LookupStatus) -> Self {
        match value {
            LookupStatus::InvalidQuery => Self::invalid("Enter a listing name or ticker."),
            LookupStatus::GrantOff => Self::new("connection_unavailable", "Enable Asset lookup."),
            LookupStatus::CredentialLocked => {
                Self::new("credential_locked", "Unlock the Asset lookup credential.")
            }
            LookupStatus::CredentialUnavailable => Self::new(
                "credential_unavailable",
                "Asset lookup credential unavailable.",
            ),
            LookupStatus::Keychain(error) => error.command_error(),
            LookupStatus::InvalidKey => {
                Self::new("credential_rejected", "OpenFIGI rejected the key.")
            }
            LookupStatus::Busy {
                retry_after_seconds,
            } => Self::with_detail(
                "rate_limited",
                "OpenFIGI is rate limited.",
                serde_json::json!({"retryAfterSeconds":retry_after_seconds}),
            ),
            LookupStatus::Cancelled => Self::new(
                "asset_lookup_cancelled",
                "Asset lookup authorization changed.",
            ),
            LookupStatus::Offline => Self::new("offline", "OpenFIGI is unavailable."),
            LookupStatus::Timeout => Self::new("asset_lookup_timeout", "OpenFIGI timed out."),
            LookupStatus::Partial | LookupStatus::Unavailable => {
                Self::new("asset_lookup_unavailable", "OpenFIGI is unavailable.")
            }
        }
    }
}

struct Authority {
    grant_revision: i64,
    key: Option<ActiveCredentialBundle>,
}

impl Authority {
    fn key_revision(&self) -> Option<&str> {
        self.key.as_ref().map(ActiveCredentialBundle::revision)
    }
}

fn grant_revision(state: &AppState, loupist_id: &str) -> Result<i64, LookupStatus> {
    state
        .ensure_loupist_available(loupist_id)
        .map_err(|_| LookupStatus::GrantOff)?;
    let binding = crate::marketplace::loupist_binding(state, loupist_id, SOURCE_ID)
        .map_err(|_| LookupStatus::GrantOff)?
        .ok_or(LookupStatus::GrantOff)?;
    if !binding.enabled || !crate::commands::binding_grants_execute(&binding) {
        return Err(LookupStatus::GrantOff);
    }
    Ok(binding.updated_at_ms)
}

async fn authority(state: &AppState, loupist_id: &str) -> Result<Authority, LookupStatus> {
    let state = state.clone();
    let loupist_id = loupist_id.to_owned();
    tokio::task::spawn_blocking(move || {
        crate::marketplace::reconcile_credential_presence(&state)
            .map_err(|_| LookupStatus::Unavailable)?;
        let revision = {
            let _guard = crate::marketplace::connector_lifecycle_lock()
                .map_err(|_| LookupStatus::Unavailable)?;
            grant_revision(&state, &loupist_id)?
        };
        if finance_credentials::status_recovery(&state, SOURCE_ID)
            == Some(finance_credentials::StatusRecovery::PresenceUnavailable)
        {
            return Err(LookupStatus::CredentialUnavailable);
        }
        let key =
            finance_credentials::get_for_state(&state, SOURCE_ID).map_err(|error| match error {
                FinanceCredentialError::Locked => LookupStatus::CredentialLocked,
                FinanceCredentialError::Busy => LookupStatus::Keychain(KeychainError::Busy),
                FinanceCredentialError::Unavailable => {
                    LookupStatus::Keychain(KeychainError::Unavailable)
                }
                _ => LookupStatus::CredentialUnavailable,
            })?;
        let _guard = crate::marketplace::connector_lifecycle_lock()
            .map_err(|_| LookupStatus::Unavailable)?;
        if grant_revision(&state, &loupist_id)? != revision {
            return Err(LookupStatus::Cancelled);
        }
        Ok(Authority {
            grant_revision: revision,
            key,
        })
    })
    .await
    .map_err(|_| LookupStatus::Unavailable)?
}

struct Lookup<T, C> {
    transport: T,
    clock: C,
    limiter: Limiter,
}

impl<T: Transport, C: Clock> Lookup<T, C> {
    async fn search(
        &self,
        state: &AppState,
        loupist_id: &str,
        search: response::Search<'_>,
    ) -> Result<Vec<AssetLookupCandidate>, LookupStatus> {
        let before = authority(state, loupist_id).await?;
        let grant = before.grant_revision;
        let revision = before.key_revision().map(str::to_owned);
        let key = before
            .key
            .as_ref()
            .map(|key| {
                key.get("api_key")
                    .ok_or(LookupStatus::CredentialUnavailable)
            })
            .transpose()?;
        let endpoint = Endpoint::Search;
        let request = http::request(endpoint, &search, key)?;
        let lease = self
            .limiter
            .dispatch(endpoint, key.is_some(), self.clock.now())?;
        drop(before);
        let deadline = self.clock.now() + DEADLINE;
        let reply = tokio::time::timeout_at(deadline, self.transport.send(request))
            .await
            .map_err(|_| LookupStatus::Timeout)??;
        let status = self.limiter.observe(lease, &reply, self.clock.now());
        // A Keychain change outside Loupist does not update the host projection.
        // Recheck the actual item before releasing a response from the old key.
        let after = authority(state, loupist_id)
            .await
            .map_err(|error| match error {
                LookupStatus::Keychain(_) => error,
                _ => LookupStatus::Cancelled,
            })?;
        if grant != after.grant_revision || revision.as_deref() != after.key_revision() {
            return Err(LookupStatus::Cancelled);
        }
        drop(after);
        status?;
        response::decode(search.query, &reply.body)
    }
}

pub(crate) async fn search(
    state: &AppState,
    loupist_id: &str,
    query: &str,
) -> Result<Vec<AssetLookupCandidate>, CommandError> {
    static LOOKUP: OnceLock<Lookup<Https, RuntimeClock>> = OnceLock::new();
    let search = response::Search::new(query)?;
    LOOKUP
        .get_or_init(|| Lookup {
            transport: Https,
            clock: RuntimeClock,
            limiter: Limiter::default(),
        })
        .search(state, loupist_id, search)
        .await
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn lookup_authority_admits_new_and_existing_loupists_without_a_key() {
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_test(root.path().join("app"));
        let id = "lookup-desk";
        state
            .store
            .create_loupist(&crate::test_fixtures::loupist_profile(id))
            .unwrap();
        assert!(authority(&state, id).await.unwrap().key.is_none());

        state
            .store
            .lock_loupist(id)
            .unwrap()
            .execute(
                "DELETE FROM loupist_capability_bindings WHERE entry_id = ?1",
                [SOURCE_ID],
            )
            .unwrap();
        assert!(authority(&state, id).await.unwrap().key.is_none());

        let mut binding = crate::marketplace::default_asset_lookup_binding(id);
        binding.enabled = false;
        binding.granted_permissions.clear();
        state.store.save_loupist_capability(&binding).unwrap();
        assert_eq!(
            authority(&state, id).await.err(),
            Some(LookupStatus::GrantOff)
        );
    }

    struct RejectKey(u16);

    #[async_trait::async_trait]
    impl Transport for RejectKey {
        async fn send(&self, request: reqwest::Request) -> Result<http::Reply, LookupStatus> {
            assert_eq!(request.headers()["x-openfigi-apikey"], "synthetic-key");
            Ok(http::Reply {
                status: self.0,
                headers: reqwest::header::HeaderMap::new(),
                body: Vec::new(),
            })
        }
    }

    #[tokio::test]
    async fn lookup_reports_a_key_rejected_by_openfigi() {
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_test(root.path().join("app"));
        let id = "lookup-desk";
        state
            .store
            .create_loupist(&crate::test_fixtures::loupist_profile(id))
            .unwrap();
        finance_credentials::save_lookup_key(&state.credential_environment, "synthetic-key")
            .unwrap();

        for status in [401, 403] {
            let lookup = Lookup {
                transport: RejectKey(status),
                clock: RuntimeClock,
                limiter: Limiter::default(),
            };
            let error = lookup
                .search(&state, id, response::Search::new("NVIDIA").unwrap())
                .await
                .map_err(CommandError::from)
                .unwrap_err();
            assert_eq!(error.code, "credential_rejected");
            assert_eq!(error.message, "OpenFIGI rejected the key.");
        }
    }
}
