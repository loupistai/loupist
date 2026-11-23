//! Submitted listing lookups through the bounded OpenFIGI transport.
mod http;
mod response;

use std::sync::OnceLock;

use crate::{
    app::{AppState, CommandError},
    commands::assets::wire::{AssetLookupCandidate, AssetLookupResult},
    finance_credentials::{self, ActiveCredentialBundle, KeychainReadStatus},
};

use http::{Clock, Endpoint, Https, Limiter, RuntimeClock, Transport, DEADLINE};

/// Each code's venues as (name, ISO country). The published equity list
/// includes two distinct venues with code TT.
fn venues(code: &str) -> Option<&'static [(String, Option<String>)]> {
    #[derive(serde::Deserialize)]
    struct Directory {
        exchanges: std::collections::BTreeMap<String, Vec<(String, Option<String>)>>,
    }
    static DIRECTORY: OnceLock<Directory> = OnceLock::new();
    let directory = DIRECTORY.get_or_init(|| {
        serde_json::from_str(include_str!("equity-exchanges.json"))
            .expect("bundled OpenFIGI exchanges")
    });
    directory.exchanges.get(code).map(Vec::as_slice)
}

pub(crate) fn exchange_name(code: &str) -> Option<&'static str> {
    match venues(code)? {
        [(name, _)] => Some(name),
        _ => None,
    }
}

/// The currency a listing on this exchange is quoted in, only where every
/// venue of the market quotes one currency in major units: the US venues and
/// the US composite, the Korean venues and the Japanese venues. Venues that
/// quote several currencies or minor units (London, Johannesburg, Tel Aviv,
/// Hong Kong, SIX, Euronext and the rest) have none. `exchange` is an OpenFIGI
/// exchange code or one of the US exchange names a listing is written with.
pub(crate) fn listing_currency(exchange: &str) -> Option<&'static str> {
    const COUNTRIES: [(&str, &str); 3] = [("US", "USD"), ("KR", "KRW"), ("JP", "JPY")];
    if matches!(
        exchange,
        "NYSE" | "NASDAQ" | "Nasdaq" | "New York Stock Exchange"
    ) {
        return Some("USD");
    }
    let venues = venues(exchange)?;
    let country = venues.first()?.1.as_deref()?;
    if venues
        .iter()
        .any(|(_, other)| other.as_deref() != Some(country))
    {
        return None;
    }
    COUNTRIES
        .iter()
        .find(|(code, _)| *code == country)
        .map(|(_, currency)| *currency)
}

pub(crate) const SOURCE_ID: &str = "openfigi.lookup";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LookupStatus {
    Partial,
    GrantOff,
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
    /// A key the status record says was saved, which this lookup could not read.
    saved_key_unread: bool,
    keychain_status: Option<KeychainReadStatus>,
}

impl Authority {
    fn key_revision(&self) -> Option<&str> {
        self.key.as_ref().map(ActiveCredentialBundle::revision)
    }

    fn api_key(&self) -> Option<&str> {
        self.key.as_ref().and_then(|key| key.get("api_key"))
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

/// The key is optional: a key that cannot be read leaves the public route.
/// Only a status record that says a key was saved makes that worth reporting.
fn optional_key(state: &AppState) -> (Option<ActiveCredentialBundle>, bool) {
    let unread = || (None, finance_credentials::has_saved_key(state, SOURCE_ID));
    if finance_credentials::status_recovery(state, SOURCE_ID)
        == Some(finance_credentials::StatusRecovery::PresenceUnavailable)
    {
        return unread();
    }
    match finance_credentials::get_for_state(state, SOURCE_ID) {
        Ok(Some(key)) if key.get("api_key").is_some() => (Some(key), false),
        Ok(None) => (None, false),
        Ok(Some(_)) | Err(_) => unread(),
    }
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
        let (key, saved_key_unread) = optional_key(&state);
        let keychain_status = finance_credentials::refused_read_status(&state, SOURCE_ID);
        let _guard = crate::marketplace::connector_lifecycle_lock()
            .map_err(|_| LookupStatus::Unavailable)?;
        if grant_revision(&state, &loupist_id)? != revision {
            return Err(LookupStatus::Cancelled);
        }
        Ok(Authority {
            grant_revision: revision,
            key,
            saved_key_unread,
            keychain_status,
        })
    })
    .await
    .map_err(|_| LookupStatus::Unavailable)?
}

/// One search: its candidates, and whether a saved key could not be used.
#[derive(Debug)]
pub(crate) struct Lookup {
    pub(crate) candidates: Vec<AssetLookupCandidate>,
    pub(crate) saved_key_unread: bool,
    /// The Security status category of a refused key read, for run diagnostics.
    pub(crate) keychain_status: Option<KeychainReadStatus>,
}

impl From<Lookup> for AssetLookupResult {
    fn from(lookup: Lookup) -> Self {
        Self {
            candidates: lookup.candidates,
            saved_key_unread: lookup.saved_key_unread,
        }
    }
}

struct Lookups<T, C> {
    transport: T,
    clock: C,
    limiter: Limiter,
}

impl<T: Transport, C: Clock> Lookups<T, C> {
    async fn search(
        &self,
        state: &AppState,
        loupist_id: &str,
        search: response::Search<'_>,
    ) -> Result<Lookup, LookupStatus> {
        let before = authority(state, loupist_id).await?;
        let grant = before.grant_revision;
        let revision = before.key_revision().map(str::to_owned);
        let key = before.api_key();
        let endpoint = Endpoint::Search;
        let request = http::request(endpoint, &search, key)?;
        let lease = self
            .limiter
            .dispatch(endpoint, key.is_some(), self.clock.now())?;
        let (saved_key_unread, keychain_status) = (before.saved_key_unread, before.keychain_status);
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
            .map_err(|_| LookupStatus::Cancelled)?;
        if grant != after.grant_revision || revision.as_deref() != after.key_revision() {
            return Err(LookupStatus::Cancelled);
        }
        drop(after);
        status?;
        Ok(Lookup {
            candidates: response::decode(search.query, &reply.body)?,
            saved_key_unread,
            keychain_status,
        })
    }
}

pub(crate) async fn search(
    state: &AppState,
    loupist_id: &str,
    query: &str,
) -> Result<Lookup, CommandError> {
    static LOOKUP: OnceLock<Lookups<Https, RuntimeClock>> = OnceLock::new();
    let search = response::Search::new(query)?;
    LOOKUP
        .get_or_init(|| Lookups {
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
            let lookup = Lookups {
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

    /// Answers as OpenFIGI's public route does and records whether a key was sent.
    struct PublicRoute(std::sync::Mutex<Vec<bool>>);

    #[async_trait::async_trait]
    impl Transport for PublicRoute {
        async fn send(&self, request: reqwest::Request) -> Result<http::Reply, LookupStatus> {
            self.0
                .lock()
                .unwrap()
                .push(request.headers().contains_key("x-openfigi-apikey"));
            Ok(http::Reply {
                status: 200,
                headers: reqwest::header::HeaderMap::new(),
                body: serde_json::to_vec(&serde_json::json!({"data":[{
                    "figi":"BBG000F6H8W8","name":"COSTCO WHOLESALE CORP","ticker":"COST",
                    "exchCode":"US","securityType":"Common Stock","marketSector":"Equity",
                    "compositeFIGI":"BBG000F6H8W8"
                }]}))
                .unwrap(),
            })
        }
    }

    fn public_route() -> Lookups<PublicRoute, RuntimeClock> {
        Lookups {
            transport: PublicRoute(Default::default()),
            clock: RuntimeClock,
            limiter: Limiter::default(),
        }
    }

    #[tokio::test]
    async fn an_unreadable_keychain_without_a_saved_key_searches_the_public_route() {
        use finance_credentials::test_store;
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_test(root.path().join("app"));
        let id = "lookup-desk";
        state
            .store
            .create_loupist(&crate::test_fixtures::loupist_profile(id))
            .unwrap();
        let environment = state.credential_environment.clone();
        // errSecNoDefaultKeychain: a profile with no keychain at all.
        test_store::fail_with_security_status(&environment, SOURCE_ID, -25307);
        let lookup = public_route();
        let found = lookup
            .search(
                &state,
                id,
                response::Search::new("Costco Wholesale").unwrap(),
            )
            .await
            .map_err(CommandError::from)
            .unwrap();
        assert_eq!(found.candidates[0].ticker.as_deref(), Some("COST"));
        assert!(!found.saved_key_unread);
        assert_eq!(
            found.keychain_status,
            Some(KeychainReadStatus::NoDefaultKeychain)
        );
        assert_eq!(*lookup.transport.0.lock().unwrap(), [false]);
        test_store::restart(&environment, SOURCE_ID);
    }

    #[tokio::test]
    async fn a_saved_key_that_cannot_be_read_still_answers_and_is_reported() {
        use finance_credentials::test_store;
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_test(root.path().join("app"));
        let id = "lookup-desk";
        state
            .store
            .create_loupist(&crate::test_fixtures::loupist_profile(id))
            .unwrap();
        // Saving through the host records the non-secret status that a key exists.
        finance_credentials::mutate(&state, SOURCE_ID, |state, _| {
            finance_credentials::save_lookup_key(&state.credential_environment, "synthetic-key")
        })
        .unwrap();
        let environment = state.credential_environment.clone();
        for status in [-25307, -67869, -25308] {
            test_store::restart(&environment, SOURCE_ID);
            test_store::fail_with_security_status(&environment, SOURCE_ID, status);
            let lookup = public_route();
            let found = lookup
                .search(&state, id, response::Search::new("COST").unwrap())
                .await
                .map_err(CommandError::from)
                .unwrap();
            assert_eq!(found.candidates.len(), 1);
            assert!(found.saved_key_unread, "{status}");
            assert_eq!(*lookup.transport.0.lock().unwrap(), [false]);
        }
        // Once the key reads again, it is sent and nothing is reported.
        test_store::restart(&environment, SOURCE_ID);
        let lookup = Lookups {
            transport: RejectKey(200),
            clock: RuntimeClock,
            limiter: Limiter::default(),
        };
        let error = lookup
            .search(&state, id, response::Search::new("COST").unwrap())
            .await
            .map_err(CommandError::from)
            .unwrap_err();
        // RejectKey's empty body is not a search result; the key was sent.
        assert_eq!(error.code, "asset_lookup_unavailable");
        finance_credentials::delete(&environment, SOURCE_ID).unwrap();
    }
}
