//! Connector credentials. Debug sessions use process memory; release builds
//! persist credentials in the OS store. Development never opens existing keys.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    app::AppState,
    finance_data::openfigi::SOURCE_ID as ASSET_LOOKUP_SOURCE_ID,
    keychain_operation::{check_mutation_deadline, KeychainError, KeychainOperation},
};

mod presence;
#[cfg(test)]
pub(crate) use presence::reconcile_with_timeout;
pub(crate) use presence::{reconcile_presence, retry_presence, PresencePass};

const CREDENTIAL_SCHEMA_VERSION: &str = "loupist-finance-credential/1";

const NATIVE_SERVICE: &str = "com.monarchjuno.loupist.finance";

#[derive(Debug, Error)]
pub enum FinanceCredentialError {
    #[error("the native credential store is unavailable on this platform")]
    Unsupported,
    #[error("the native credential store operation failed")]
    Store,
    /// macOS did not let Loupist read this saved key (the access prompt was
    /// denied or could not be answered). Remembered for the session.
    #[error("access to this saved key was not allowed in macOS")]
    Locked,
    #[error("access to this saved key needs permission in macOS")]
    PermissionRequired,
    #[error("a keychain operation is still pending for this item")]
    Busy,
    #[error("the keychain is unavailable")]
    Unavailable,
    #[error("Loupist is installing an update")]
    UpdateInstalling,
}

impl From<KeychainError> for FinanceCredentialError {
    fn from(error: KeychainError) -> Self {
        match error {
            KeychainError::UpdateInstalling => Self::UpdateInstalling,
            KeychainError::Busy => Self::Busy,
            KeychainError::Unavailable | KeychainError::TimedOut => Self::Unavailable,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CandidateVerification {
    Never,
    Rejected,
    TemporarilyUnavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerificationOutcome {
    Verified,
    Rejected,
    TemporarilyUnavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FinishVerification {
    Applied,
    Stale,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum KrxServiceAccess {
    Verified,
    NeedsApproval,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct KrxCoverage {
    pub kospi: KrxServiceAccess,
    pub kosdaq: KrxServiceAccess,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct CredentialStatus {
    pub stored: bool,
    pub active: bool,
    pub pending: bool,
    pub active_fields: BTreeSet<String>,
    pub candidate_fields: BTreeSet<String>,
    pub verification: CredentialVerification,
    pub verified_at: Option<i64>,
    pub krx_coverage: Option<KrxCoverage>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub enum CredentialVerification {
    #[default]
    Never,
    Verified,
    Rejected,
    TemporarilyUnavailable,
}

/// A snapshot of the exact staged candidate that a caller may verify.
///
/// This type intentionally has no `Debug` or serialization implementation so
/// the secret cannot accidentally cross the command boundary or enter logs.
pub struct CredentialCandidate {
    revision: String,
    already_active: bool,
    secrets: BTreeMap<String, String>,
}

impl CredentialCandidate {
    pub fn revision(&self) -> &str {
        &self.revision
    }

    pub fn secrets(&self) -> &BTreeMap<String, String> {
        &self.secrets
    }

    pub(crate) fn already_active(&self) -> bool {
        self.already_active
    }
}

/// An active credential bundle that finance execution may use. Only the
/// optional OpenFIGI key becomes active without verification.
///
/// This type intentionally has no `Debug` or serialization implementation so
/// secrets cannot accidentally cross the command boundary or enter logs.
pub struct ActiveCredentialBundle {
    revision: String,
    secrets: BTreeMap<String, String>,
}

impl ActiveCredentialBundle {
    pub fn revision(&self) -> &str {
        &self.revision
    }

    pub fn get(&self, credential_id: &str) -> Option<&str> {
        self.secrets.get(credential_id).map(String::as_str)
    }

    /// Stored values by reference, for sharing's existing private context only.
    /// No other caller may use this accessor. Do not log or serialize these
    /// values or retain them beyond that private context.
    pub(crate) fn stored_values_for_sharing(&self) -> impl Iterator<Item = &str> {
        self.secrets.values().map(String::as_str)
    }
}

#[derive(Clone, Deserialize, Serialize)]
struct StoredCredentialBundle {
    revision: String,
    secrets: BTreeMap<String, String>,
}

#[derive(Clone, Deserialize, Serialize)]
struct ActiveCredential {
    credential: StoredCredentialBundle,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    verified_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    krx_coverage: Option<KrxCoverage>,
}

#[derive(Clone, Deserialize, Serialize)]
struct CandidateCredential {
    credential: StoredCredentialBundle,
    verification: CandidateVerification,
}

#[derive(Clone, Deserialize, Serialize)]
struct CredentialEnvelope {
    schema_version: String,
    active: Option<ActiveCredential>,
    candidate: Option<CandidateCredential>,
}

impl Default for CredentialEnvelope {
    fn default() -> Self {
        Self {
            schema_version: CREDENTIAL_SCHEMA_VERSION.to_owned(),
            active: None,
            candidate: None,
        }
    }
}

fn validate_entry_id(entry_id: &str) -> Result<(), FinanceCredentialError> {
    if entry_id.is_empty()
        || entry_id.len() > 96
        || !entry_id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b".-".contains(&byte))
    {
        return Err(FinanceCredentialError::Store);
    }
    Ok(())
}

fn validate_credential_id(credential_id: &str) -> Result<(), FinanceCredentialError> {
    if credential_id.is_empty()
        || credential_id.len() > 64
        || !credential_id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
    {
        return Err(FinanceCredentialError::Store);
    }
    Ok(())
}

fn validate_secret(secret: &str) -> Result<(), FinanceCredentialError> {
    if secret.is_empty()
        || secret.len() > 512
        || secret.contains('\0')
        || secret.chars().any(|character| character.is_control())
    {
        return Err(FinanceCredentialError::Store);
    }
    Ok(())
}

fn validate_secrets(secrets: &BTreeMap<String, String>) -> Result<(), FinanceCredentialError> {
    if secrets.is_empty() || secrets.len() > 16 {
        return Err(FinanceCredentialError::Store);
    }
    for (credential_id, secret) in secrets {
        validate_credential_id(credential_id)?;
        validate_secret(secret)?;
    }
    Ok(())
}

fn validate_revision(revision: &str) -> Result<(), FinanceCredentialError> {
    let parsed = Uuid::parse_str(revision).map_err(|_| FinanceCredentialError::Store)?;
    if parsed.hyphenated().to_string() != revision {
        return Err(FinanceCredentialError::Store);
    }
    Ok(())
}

fn validate_envelope(
    entry_id: &str,
    envelope: &CredentialEnvelope,
) -> Result<(), FinanceCredentialError> {
    if envelope.schema_version != CREDENTIAL_SCHEMA_VERSION {
        return Err(FinanceCredentialError::Store);
    }
    if let Some(active) = &envelope.active {
        validate_revision(&active.credential.revision)?;
        validate_secrets(&active.credential.secrets)?;
        if !active
            .verified_at
            .map_or(entry_id == ASSET_LOOKUP_SOURCE_ID, |at| at >= 0)
        {
            return Err(FinanceCredentialError::Store);
        }
    }
    if let Some(candidate) = &envelope.candidate {
        validate_revision(&candidate.credential.revision)?;
        validate_secrets(&candidate.credential.secrets)?;
    }
    Ok(())
}

#[cfg(any(debug_assertions, test, feature = "webdriver"))]
fn memory_store() -> &'static std::sync::Mutex<std::collections::BTreeMap<String, String>> {
    static STORE: std::sync::OnceLock<
        std::sync::Mutex<std::collections::BTreeMap<String, String>>,
    > = std::sync::OnceLock::new();
    STORE.get_or_init(Default::default)
}

fn native_account(environment_id: &str, entry_id: &str) -> String {
    format!("credential-state/v1/{environment_id}/{entry_id}")
}

/// Test seam for the in-memory store: counts reads of each account and can
/// refuse them the way macOS refuses an item whose access prompt was denied.
#[cfg(test)]
pub(crate) mod test_store {
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::{Mutex, OnceLock};

    fn reads() -> &'static Mutex<BTreeMap<String, usize>> {
        static READS: OnceLock<Mutex<BTreeMap<String, usize>>> = OnceLock::new();
        READS.get_or_init(Default::default)
    }

    fn denied() -> &'static Mutex<BTreeSet<String>> {
        static DENIED: OnceLock<Mutex<BTreeSet<String>>> = OnceLock::new();
        DENIED.get_or_init(Default::default)
    }

    type Barriers = BTreeMap<String, std::sync::mpsc::Receiver<bool>>;

    fn write_barriers() -> &'static Mutex<Barriers> {
        static BARRIERS: OnceLock<Mutex<Barriers>> = OnceLock::new();
        BARRIERS.get_or_init(Default::default)
    }

    fn read_barriers() -> &'static Mutex<Barriers> {
        static BARRIERS: OnceLock<Mutex<Barriers>> = OnceLock::new();
        BARRIERS.get_or_init(Default::default)
    }

    fn presence_barriers() -> &'static Mutex<Barriers> {
        static BARRIERS: OnceLock<Mutex<Barriers>> = OnceLock::new();
        BARRIERS.get_or_init(Default::default)
    }

    fn presence_checks() -> &'static Mutex<BTreeMap<String, usize>> {
        static CHECKS: OnceLock<Mutex<BTreeMap<String, usize>>> = OnceLock::new();
        CHECKS.get_or_init(Default::default)
    }

    pub(crate) fn block_next_presence(
        environment: &str,
        id: &str,
    ) -> std::sync::mpsc::Sender<bool> {
        let (release, pending) = std::sync::mpsc::channel();
        presence_barriers()
            .lock()
            .unwrap()
            .insert(super::native_account(environment, id), pending);
        release
    }

    pub(super) fn record_presence(account: &str) -> bool {
        *presence_checks()
            .lock()
            .unwrap()
            .entry(account.into())
            .or_default() += 1;
        let pending = presence_barriers().lock().unwrap().remove(account);
        pending.is_none_or(|pending| pending.recv().unwrap_or(false))
    }

    pub(crate) fn presence_calls(environment: &str) -> usize {
        let prefix = format!("credential-state/v1/{environment}/");
        presence_checks()
            .lock()
            .unwrap()
            .iter()
            .filter(|(account, _)| account.starts_with(&prefix))
            .map(|(_, count)| count)
            .sum()
    }

    fn mutations() -> &'static Mutex<BTreeMap<String, usize>> {
        static MUTATIONS: OnceLock<Mutex<BTreeMap<String, usize>>> = OnceLock::new();
        MUTATIONS.get_or_init(Default::default)
    }

    pub(super) fn record_mutation(account: &str) {
        *mutations()
            .lock()
            .unwrap()
            .entry(account.to_owned())
            .or_default() += 1;
    }

    pub(super) fn await_write(account: &str) -> bool {
        let pending = write_barriers().lock().unwrap().remove(account);
        pending.is_none_or(|pending| pending.recv().unwrap_or(false))
    }

    pub(super) fn block_next_write(environment: &str, id: &str) -> std::sync::mpsc::Sender<bool> {
        let (release, pending) = std::sync::mpsc::channel();
        write_barriers()
            .lock()
            .unwrap()
            .insert(super::native_account(environment, id), pending);
        release
    }

    pub(crate) fn block_next_read(environment: &str, id: &str) -> std::sync::mpsc::Sender<bool> {
        let (release, pending) = std::sync::mpsc::channel();
        read_barriers()
            .lock()
            .unwrap()
            .insert(super::native_account(environment, id), pending);
        release
    }

    pub(crate) fn backend_calls(environment: &str) -> usize {
        let prefix = format!("credential-state/v1/{environment}/");
        let count = |accounts: &BTreeMap<String, usize>| {
            accounts
                .iter()
                .filter(|(name, _)| name.starts_with(&prefix))
                .map(|(_, count)| count)
                .sum::<usize>()
        };
        count(&reads().lock().unwrap())
            + count(&mutations().lock().unwrap())
            + presence_calls(environment)
    }

    pub(super) fn record_read(account: &str) -> bool {
        *reads()
            .lock()
            .unwrap()
            .entry(account.to_owned())
            .or_default() += 1;
        let pending = read_barriers().lock().unwrap().remove(account);
        pending.map_or_else(
            || !denied().lock().unwrap().contains(account),
            |pending| pending.recv().unwrap_or(false),
        )
    }

    /// How many times the secret of this credential was read.
    pub(crate) fn secret_reads(environment_id: &str, entry_id: &str) -> usize {
        reads()
            .lock()
            .unwrap()
            .get(&super::native_account(environment_id, entry_id))
            .copied()
            .unwrap_or_default()
    }

    /// Makes reads of this credential fail as a denied macOS prompt does.
    pub(crate) fn deny(environment_id: &str, entry_id: &str, deny: bool) {
        let account = super::native_account(environment_id, entry_id);
        let mut denied = denied().lock().unwrap();
        if deny {
            denied.insert(account);
        } else {
            denied.remove(&account);
        }
    }

    /// Forgets what this session learned about the credential, as a restart does.
    pub(crate) fn restart(environment_id: &str, entry_id: &str) {
        super::session()
            .lock()
            .unwrap()
            .remove(&super::native_account(environment_id, entry_id));
    }
}

#[cfg(any(debug_assertions, test, feature = "webdriver"))]
fn read_memory_blob(
    environment_id: &str,
    entry_id: &str,
) -> Result<Option<String>, FinanceCredentialError> {
    #[cfg(test)]
    if !test_store::record_read(&native_account(environment_id, entry_id)) {
        return Err(FinanceCredentialError::Store);
    }
    Ok(memory_store()
        .lock()
        .map_err(|_| FinanceCredentialError::Store)?
        .get(&native_account(environment_id, entry_id))
        .cloned())
}

#[cfg(any(debug_assertions, test, feature = "webdriver"))]
fn write_memory_blob(
    environment_id: &str,
    entry_id: &str,
    value: &str,
) -> Result<(), FinanceCredentialError> {
    #[cfg(test)]
    {
        let account = native_account(environment_id, entry_id);
        test_store::record_mutation(&account);
        if !test_store::await_write(&account) {
            return Err(FinanceCredentialError::Store);
        }
    }
    memory_store()
        .lock()
        .map_err(|_| FinanceCredentialError::Store)?
        .insert(native_account(environment_id, entry_id), value.to_owned());
    Ok(())
}

#[cfg(any(debug_assertions, test, feature = "webdriver"))]
fn delete_memory_blob(environment_id: &str, entry_id: &str) -> Result<(), FinanceCredentialError> {
    #[cfg(test)]
    test_store::record_mutation(&native_account(environment_id, entry_id));
    memory_store()
        .lock()
        .map_err(|_| FinanceCredentialError::Store)?
        .remove(&native_account(environment_id, entry_id));
    Ok(())
}

#[cfg(all(
    not(any(debug_assertions, test, feature = "webdriver")),
    any(target_os = "macos", windows),
))]
fn native_entry(
    environment_id: &str,
    service: &str,
    entry_id: &str,
) -> Result<keyring::Entry, FinanceCredentialError> {
    keyring::Entry::new(service, &native_account(environment_id, entry_id))
        .map_err(|_| FinanceCredentialError::Store)
}

#[cfg(all(
    not(any(debug_assertions, test, feature = "webdriver")),
    any(target_os = "macos", windows),
))]
fn read_native_blob(
    environment_id: &str,
    service: &str,
    entry_id: &str,
) -> Result<Option<String>, FinanceCredentialError> {
    match native_entry(environment_id, service, entry_id)?.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err(FinanceCredentialError::Store),
    }
}

#[cfg(all(
    not(any(debug_assertions, test, feature = "webdriver")),
    any(target_os = "macos", windows),
))]
fn write_native_blob(
    environment_id: &str,
    service: &str,
    entry_id: &str,
    value: &str,
) -> Result<(), FinanceCredentialError> {
    native_entry(environment_id, service, entry_id)?
        .set_password(value)
        .map_err(|_| FinanceCredentialError::Store)
}

#[cfg(all(
    not(any(debug_assertions, test, feature = "webdriver")),
    any(target_os = "macos", windows),
))]
fn delete_native_blob(
    environment_id: &str,
    service: &str,
    entry_id: &str,
) -> Result<(), FinanceCredentialError> {
    crate::keychain_operation::delete_entry(native_entry(environment_id, service, entry_id)?)
        .map_err(Into::into)
}

#[cfg(any(debug_assertions, test, feature = "webdriver"))]
fn read_blob(
    environment_id: &str,
    entry_id: &str,
) -> Result<Option<String>, FinanceCredentialError> {
    read_memory_blob(environment_id, entry_id)
}

#[cfg(any(debug_assertions, test, feature = "webdriver"))]
fn write_blob(
    environment_id: &str,
    entry_id: &str,
    value: &str,
) -> Result<(), FinanceCredentialError> {
    write_memory_blob(environment_id, entry_id, value)
}

#[cfg(any(debug_assertions, test, feature = "webdriver"))]
fn delete_blob(environment_id: &str, entry_id: &str) -> Result<(), FinanceCredentialError> {
    delete_memory_blob(environment_id, entry_id)
}

#[cfg(all(
    not(any(debug_assertions, test, feature = "webdriver")),
    any(target_os = "macos", windows)
))]
fn read_blob(
    environment_id: &str,
    entry_id: &str,
) -> Result<Option<String>, FinanceCredentialError> {
    read_native_blob(environment_id, NATIVE_SERVICE, entry_id)
}

#[cfg(all(
    not(any(debug_assertions, test, feature = "webdriver")),
    any(target_os = "macos", windows)
))]
fn write_blob(
    environment_id: &str,
    entry_id: &str,
    value: &str,
) -> Result<(), FinanceCredentialError> {
    write_native_blob(environment_id, NATIVE_SERVICE, entry_id, value)
}

#[cfg(all(
    not(any(debug_assertions, test, feature = "webdriver")),
    any(target_os = "macos", windows)
))]
fn delete_blob(environment_id: &str, entry_id: &str) -> Result<(), FinanceCredentialError> {
    delete_native_blob(environment_id, NATIVE_SERVICE, entry_id)
}

#[cfg(all(
    not(any(debug_assertions, test, feature = "webdriver")),
    not(any(target_os = "macos", windows))
))]
fn read_blob(
    environment_id: &str,
    _entry_id: &str,
) -> Result<Option<String>, FinanceCredentialError> {
    validate_revision(environment_id)?;
    Err(FinanceCredentialError::Unsupported)
}

#[cfg(all(
    not(any(debug_assertions, test, feature = "webdriver")),
    not(any(target_os = "macos", windows))
))]
fn write_blob(
    environment_id: &str,
    _entry_id: &str,
    _value: &str,
) -> Result<(), FinanceCredentialError> {
    validate_revision(environment_id)?;
    Err(FinanceCredentialError::Unsupported)
}

#[cfg(all(
    not(any(debug_assertions, test, feature = "webdriver")),
    not(any(target_os = "macos", windows))
))]
fn delete_blob(environment_id: &str, _entry_id: &str) -> Result<(), FinanceCredentialError> {
    validate_revision(environment_id)?;
    Err(FinanceCredentialError::Unsupported)
}

/// What this session learned about one saved credential, without its secret.
///
/// The session records whether macOS refused an item. Status and the random
/// credential revision contain no secret material.
#[derive(Clone, Deserialize, Serialize)]
enum SessionAccess {
    Known {
        status: CredentialStatus,
        active_revision: Option<String>,
        #[serde(default)]
        last_store_failure: bool,
    },
    Locked,
    Unconfirmed,
    StoreUnavailable,
    /// The item exists. Its fields and active revision are still unknown.
    PresentUnread,
    PresentDenied,
}

fn session() -> &'static std::sync::Mutex<BTreeMap<String, SessionAccess>> {
    static SESSION: std::sync::OnceLock<std::sync::Mutex<BTreeMap<String, SessionAccess>>> =
        std::sync::OnceLock::new();
    SESSION.get_or_init(Default::default)
}

fn remembered(environment_id: &str, entry_id: &str) -> Option<SessionAccess> {
    session()
        .lock()
        .ok()?
        .get(&native_account(environment_id, entry_id))
        .cloned()
}

fn remember(environment_id: &str, entry_id: &str, access: Option<SessionAccess>) {
    if let Ok(mut session) = session().lock() {
        let account = native_account(environment_id, entry_id);
        match access {
            Some(access) => session.insert(account, access),
            None => session.remove(&account),
        };
    }
}

fn remember_envelope(environment_id: &str, entry_id: &str, envelope: &CredentialEnvelope) {
    remember(
        environment_id,
        entry_id,
        Some(SessionAccess::Known {
            status: status_from(envelope),
            last_store_failure: false,
            active_revision: envelope
                .active
                .as_ref()
                .map(|active| active.credential.revision.clone()),
        }),
    );
}

/// Reads the envelope for a passive caller. A credential this session was
/// refused stays refused without asking macOS again.
fn load(
    environment_id: &str,
    entry_id: &str,
) -> Result<CredentialEnvelope, FinanceCredentialError> {
    validate_revision(environment_id)?;
    validate_entry_id(entry_id)?;
    if matches!(
        remembered(environment_id, entry_id),
        Some(SessionAccess::Locked)
    ) {
        return Err(FinanceCredentialError::Locked);
    }
    let envelope = match read_envelope(environment_id, entry_id) {
        Ok(envelope) => envelope,
        Err(FinanceCredentialError::Locked) => {
            remember(environment_id, entry_id, Some(SessionAccess::Locked));
            return Err(FinanceCredentialError::Locked);
        }
        Err(error) => return Err(error),
    };
    remember_envelope(environment_id, entry_id, &envelope);
    Ok(envelope)
}

fn read_envelope(
    environment_id: &str,
    entry_id: &str,
) -> Result<CredentialEnvelope, FinanceCredentialError> {
    let Some(value) = read_blob(environment_id, entry_id).map_err(|error| match error {
        FinanceCredentialError::Store => FinanceCredentialError::Locked,
        error => error,
    })?
    else {
        return Ok(CredentialEnvelope::default());
    };
    let envelope = serde_json::from_str(&value).map_err(|_| FinanceCredentialError::Store)?;
    validate_envelope(entry_id, &envelope)?;
    Ok(envelope)
}

/// Reads the envelope for something the user just asked for (save, check,
/// Try again): a remembered denial is set aside and macOS may ask once more.
fn load_for_user(
    environment_id: &str,
    entry_id: &str,
) -> Result<CredentialEnvelope, FinanceCredentialError> {
    validate_revision(environment_id)?;
    validate_entry_id(entry_id)?;
    if matches!(
        remembered(environment_id, entry_id),
        Some(SessionAccess::Locked)
    ) {
        remember(environment_id, entry_id, None);
    }
    load(environment_id, entry_id)
}

fn save_envelope(
    environment_id: &str,
    entry_id: &str,
    envelope: &CredentialEnvelope,
) -> Result<(), FinanceCredentialError> {
    validate_envelope(entry_id, envelope)?;
    let value = serde_json::to_string(envelope).map_err(|_| FinanceCredentialError::Store)?;
    check_mutation_deadline()?;
    write_blob(environment_id, entry_id, &value)?;
    remember_envelope(environment_id, entry_id, envelope);
    Ok(())
}

fn status_from(envelope: &CredentialEnvelope) -> CredentialStatus {
    let active_fields = envelope
        .active
        .as_ref()
        .map(|active| active.credential.secrets.keys().cloned().collect())
        .unwrap_or_default();
    let candidate_fields = envelope
        .candidate
        .as_ref()
        .map(|candidate| candidate.credential.secrets.keys().cloned().collect())
        .unwrap_or_default();
    let verified_at = envelope
        .active
        .as_ref()
        .and_then(|credential| credential.verified_at);
    let verification = envelope
        .candidate
        .as_ref()
        .map(|candidate| match candidate.verification {
            CandidateVerification::Never => CredentialVerification::Never,
            CandidateVerification::Rejected => CredentialVerification::Rejected,
            CandidateVerification::TemporarilyUnavailable => {
                CredentialVerification::TemporarilyUnavailable
            }
        })
        .unwrap_or_else(|| {
            if verified_at.is_some() {
                CredentialVerification::Verified
            } else {
                CredentialVerification::Never
            }
        });
    CredentialStatus {
        stored: envelope.active.is_some() || envelope.candidate.is_some(),
        active: envelope.active.is_some(),
        pending: envelope.candidate.is_some(),
        active_fields,
        candidate_fields,
        verification,
        verified_at,
        krx_coverage: envelope
            .active
            .as_ref()
            .and_then(|credential| credential.krx_coverage),
    }
}

/// Atomically stages a partial credential patch without changing the active
/// bundle. Existing active values seed the candidate, an existing candidate
/// takes precedence over them, and the submitted patch wins last. This lets a
/// native caller update one profile field without asking React to resubmit or
/// observe any previously stored secret.
pub fn stage(
    environment_id: &str,
    entry_id: &str,
    patch: &BTreeMap<String, String>,
) -> Result<CredentialStatus, FinanceCredentialError> {
    stage_with_validation(environment_id, entry_id, patch, |_| {
        Ok::<_, std::convert::Infallible>(())
    })?
    .map_err(|never| match never {})
}

pub(crate) fn stage_with_validation<E>(
    environment_id: &str,
    entry_id: &str,
    patch: &BTreeMap<String, String>,
    validate: impl FnOnce(&CredentialStatus) -> Result<(), E>,
) -> Result<Result<CredentialStatus, E>, FinanceCredentialError> {
    validate_revision(environment_id)?;
    validate_entry_id(entry_id)?;
    validate_secrets(patch)?;
    let mut envelope = load_for_user(environment_id, entry_id)?;
    if let Err(error) = validate(&status_from(&envelope)) {
        return Ok(Err(error));
    }
    let mut secrets = envelope
        .active
        .as_ref()
        .map(|active| active.credential.secrets.clone())
        .unwrap_or_default();
    if let Some(candidate) = &envelope.candidate {
        secrets.extend(candidate.credential.secrets.clone());
    }
    secrets.extend(patch.clone());
    validate_secrets(&secrets)?;
    envelope.candidate = Some(CandidateCredential {
        credential: StoredCredentialBundle {
            revision: Uuid::new_v4().hyphenated().to_string(),
            secrets,
        },
        verification: CandidateVerification::Never,
    });
    save_envelope(environment_id, entry_id, &envelope)?;
    Ok(Ok(status_from(&envelope)))
}

/// Stores the optional OpenFIGI key as the active credential in one write.
pub(crate) fn save_lookup_key(
    environment_id: &str,
    key: &str,
) -> Result<(), FinanceCredentialError> {
    // An explicit save may unlock a key whose access was previously refused.
    load_for_user(environment_id, ASSET_LOOKUP_SOURCE_ID)?;
    let envelope = CredentialEnvelope {
        active: Some(ActiveCredential {
            credential: StoredCredentialBundle {
                revision: Uuid::new_v4().hyphenated().to_string(),
                secrets: BTreeMap::from([("api_key".to_owned(), key.to_owned())]),
            },
            verified_at: None,
            krx_coverage: None,
        }),
        ..CredentialEnvelope::default()
    };
    save_envelope(environment_id, ASSET_LOOKUP_SOURCE_ID, &envelope)
}

/// Loads only the current candidate. An active credential is never returned by
/// this function and therefore cannot be confused with an unverified replacement.
pub fn candidate(
    environment_id: &str,
    entry_id: &str,
) -> Result<Option<CredentialCandidate>, FinanceCredentialError> {
    Ok(load_for_user(environment_id, entry_id)?
        .candidate
        .map(|candidate| CredentialCandidate {
            revision: candidate.credential.revision,
            already_active: false,
            secrets: candidate.credential.secrets,
        }))
}

/// Captures a candidate for an explicit verification request. Rechecking an
/// active connection stages a fresh revision while preserving its executable
/// bundle until the provider accepts the check. A pending replacement always
/// takes precedence, including a retry after a temporary provider failure.
pub fn candidate_for_verification(
    environment_id: &str,
    entry_id: &str,
) -> Result<Option<CredentialCandidate>, FinanceCredentialError> {
    Ok(
        verification_envelope(environment_id, entry_id)?.and_then(|envelope| {
            let already_active = envelope.active.is_some();
            envelope.candidate.map(|candidate| CredentialCandidate {
                revision: candidate.credential.revision,
                already_active,
                secrets: candidate.credential.secrets,
            })
        }),
    )
}

fn verification_envelope(
    environment_id: &str,
    entry_id: &str,
) -> Result<Option<CredentialEnvelope>, FinanceCredentialError> {
    let mut envelope = load_for_user(environment_id, entry_id)?;
    if envelope.candidate.is_none() {
        let Some(active) = &envelope.active else {
            return Ok(None);
        };
        envelope.candidate = Some(CandidateCredential {
            credential: StoredCredentialBundle {
                revision: Uuid::new_v4().hyphenated().to_string(),
                secrets: active.credential.secrets.clone(),
            },
            verification: CandidateVerification::Never,
        });
        save_envelope(environment_id, entry_id, &envelope)?;
    }
    Ok(Some(envelope))
}

/// Applies a verification result only when `expected_revision` is still the
/// staged candidate. This compare-and-swap prevents a slow verification from
/// promoting or rejecting a newer replacement.
pub fn finish_verification(
    environment_id: &str,
    entry_id: &str,
    expected_revision: &str,
    outcome: VerificationOutcome,
    verified_at: i64,
    krx_coverage: Option<KrxCoverage>,
) -> Result<FinishVerification, FinanceCredentialError> {
    validate_revision(expected_revision)?;
    if verified_at < 0 {
        return Err(FinanceCredentialError::Store);
    }
    let mut envelope = load_for_user(environment_id, entry_id)?;
    let Some(candidate) = envelope.candidate.as_ref() else {
        return Ok(FinishVerification::Stale);
    };
    if candidate.credential.revision != expected_revision {
        return Ok(FinishVerification::Stale);
    }
    apply_outcome(&mut envelope, outcome, verified_at, krx_coverage);
    save_envelope(environment_id, entry_id, &envelope)?;
    Ok(FinishVerification::Applied)
}

fn apply_outcome(
    envelope: &mut CredentialEnvelope,
    outcome: VerificationOutcome,
    verified_at: i64,
    krx_coverage: Option<KrxCoverage>,
) {
    let candidate = envelope.candidate.as_mut().expect("candidate is current");
    match outcome {
        VerificationOutcome::Verified => {
            let candidate = envelope
                .candidate
                .take()
                .expect("candidate was checked above");
            envelope.active = Some(ActiveCredential {
                credential: candidate.credential,
                verified_at: Some(verified_at),
                krx_coverage,
            });
        }
        VerificationOutcome::Rejected => {
            candidate.verification = CandidateVerification::Rejected;
        }
        VerificationOutcome::TemporarilyUnavailable => {
            candidate.verification = CandidateVerification::TemporarilyUnavailable;
        }
    }
}

pub fn status(
    environment_id: &str,
    entry_id: &str,
) -> Result<CredentialStatus, FinanceCredentialError> {
    #[cfg(feature = "eval-harness")]
    if crate::eval_harness::replay::credential_environment(environment_id, entry_id) {
        return Ok(CredentialStatus {
            stored: true,
            active: true,
            pending: false,
            active_fields: eval_fields().into_keys().collect(),
            candidate_fields: Default::default(),
            verification: CredentialVerification::Verified,
            verified_at: Some(0),
            krx_coverage: None,
        });
    }
    match remembered(environment_id, entry_id) {
        Some(SessionAccess::Known { status, .. }) => Ok(status),
        Some(SessionAccess::Locked) => Err(FinanceCredentialError::Locked),
        Some(
            SessionAccess::Unconfirmed
            | SessionAccess::StoreUnavailable
            | SessionAccess::PresentUnread
            | SessionAccess::PresentDenied,
        )
        | None => Ok(CredentialStatus::default()),
    }
}

/// Whether this session was refused the credential. Reads nothing.
pub fn is_locked(environment_id: &str, entry_id: &str) -> bool {
    matches!(
        remembered(environment_id, entry_id),
        Some(SessionAccess::Locked)
    )
}

/// Try again: forgets a remembered denial and reads the credential once more,
/// so macOS may ask again. Only an explicit user action calls this.
pub fn unlock(
    environment_id: &str,
    entry_id: &str,
) -> Result<CredentialStatus, FinanceCredentialError> {
    Ok(status_from(&load_for_user(environment_id, entry_id)?))
}

/// Returns the active bundle. Credentials that require verification become
/// active only after successful verification.
pub fn get(
    environment_id: &str,
    entry_id: &str,
) -> Result<Option<ActiveCredentialBundle>, FinanceCredentialError> {
    #[cfg(feature = "eval-harness")]
    if crate::eval_harness::replay::credential_environment(environment_id, entry_id) {
        return Ok(Some(ActiveCredentialBundle {
            revision: "eval-replay".into(),
            secrets: eval_fields(),
        }));
    }
    Ok(load(environment_id, entry_id)?
        .active
        .map(|active| ActiveCredentialBundle {
            revision: active.credential.revision,
            secrets: active.credential.secrets,
        }))
}

/// Returns the revision of the active credential, if one exists.
///
/// This deliberately returns only the revision needed to bind a Pi session
/// cache to credential authority. It never constructs or exposes a credential
/// bundle, and staged candidates do not affect the result.
#[cfg(any(test, feature = "eval-harness"))]
pub(crate) fn active_revision(
    environment_id: &str,
    entry_id: &str,
) -> Result<Option<String>, FinanceCredentialError> {
    #[cfg(feature = "eval-harness")]
    if crate::eval_harness::replay::credential_environment(environment_id, entry_id) {
        return Ok(Some("eval-replay".into()));
    }
    match remembered(environment_id, entry_id) {
        Some(SessionAccess::Known {
            active_revision, ..
        }) => Ok(active_revision),
        Some(SessionAccess::Locked) => Err(FinanceCredentialError::Locked),
        Some(
            SessionAccess::Unconfirmed
            | SessionAccess::StoreUnavailable
            | SessionAccess::PresentUnread
            | SessionAccess::PresentDenied,
        )
        | None => Ok(None),
    }
}

pub fn delete(environment_id: &str, entry_id: &str) -> Result<(), FinanceCredentialError> {
    validate_revision(environment_id)?;
    validate_entry_id(entry_id)?;
    check_mutation_deadline()?;
    delete_blob(environment_id, entry_id)?;
    remember_envelope(environment_id, entry_id, &CredentialEnvelope::default());
    Ok(())
}

fn publish_session(state: &AppState, entry_id: &str) -> Result<(), FinanceCredentialError> {
    let access = remembered(&state.credential_environment, entry_id);
    if let Some(access) = access {
        let serialized =
            serde_json::to_string(&access).map_err(|_| FinanceCredentialError::Store)?;
        state
            .store
            .set_connector_credential_status(entry_id, &serialized)
            .map_err(|_| FinanceCredentialError::Store)?;
    }
    Ok(())
}

fn status_projection(
    state: &AppState,
    entry_id: &str,
) -> Result<Option<SessionAccess>, FinanceCredentialError> {
    let environment = &state.credential_environment;
    if let Some(access) = remembered(environment, entry_id) {
        return Ok(Some(access));
    }
    let saved = state
        .store
        .connector_credential_status(entry_id)
        .map_err(|_| FinanceCredentialError::Store)?
        .map(|encoded| serde_json::from_str(&encoded).map_err(|_| FinanceCredentialError::Store))
        .transpose()?;
    Ok(remembered(environment, entry_id).or(saved))
}

pub(crate) fn status_for_state(
    state: &AppState,
    entry_id: &str,
) -> Result<CredentialStatus, FinanceCredentialError> {
    #[cfg(feature = "eval-harness")]
    if crate::eval_harness::replay::credential_environment(&state.credential_environment, entry_id)
    {
        return status(&state.credential_environment, entry_id);
    }
    match status_projection(state, entry_id)? {
        Some(SessionAccess::Known { status, .. }) => Ok(status),
        Some(SessionAccess::Locked) => Err(FinanceCredentialError::Locked),
        Some(
            SessionAccess::Unconfirmed
            | SessionAccess::StoreUnavailable
            | SessionAccess::PresentUnread
            | SessionAccess::PresentDenied,
        )
        | None => Ok(CredentialStatus::default()),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum StatusRecovery {
    SavedUnread,
    SavedDenied,
    PresenceUnavailable,
    Unknown,
}

impl StatusRecovery {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::SavedUnread => "credential_saved_unread",
            Self::SavedDenied => "credential_saved_denied",
            Self::PresenceUnavailable => "credential_presence_unavailable",
            Self::Unknown => "credential_status_unknown",
        }
    }
}

pub(crate) fn status_recovery(state: &AppState, entry_id: &str) -> Option<StatusRecovery> {
    let unknown = if presence::SUPPORTED {
        Some(StatusRecovery::PresenceUnavailable)
    } else if cfg!(windows) {
        Some(StatusRecovery::Unknown)
    } else {
        None
    };
    match status_projection(state, entry_id) {
        Ok(Some(SessionAccess::PresentUnread)) => Some(StatusRecovery::SavedUnread),
        Ok(Some(SessionAccess::PresentDenied)) => Some(StatusRecovery::SavedDenied),
        Ok(Some(SessionAccess::Unconfirmed | SessionAccess::StoreUnavailable)) => unknown,
        Ok(None) if state.store.legacy_credential_namespace().unwrap_or(true) => unknown,
        _ => None,
    }
}

pub(crate) fn needs_permission(state: &AppState, entry_id: &str) -> bool {
    matches!(
        status_projection(state, entry_id),
        Ok(Some(
            SessionAccess::PresentUnread | SessionAccess::PresentDenied
        ))
    )
}

/// The caller has reserved this item on a background thread. The native read
/// cannot be interrupted, so Cancel discards its result before publishing.
pub(crate) fn read_saved_key_permission(
    state: &AppState,
    entry_id: &str,
    cancelled: &std::sync::Mutex<bool>,
) -> Result<(), FinanceCredentialError> {
    if *cancelled
        .lock()
        .map_err(|_| FinanceCredentialError::Store)?
        || !needs_permission(state, entry_id)
    {
        return Ok(());
    }
    let environment = &state.credential_environment;
    let result = read_envelope(environment, entry_id);
    let cancelled = cancelled
        .lock()
        .map_err(|_| FinanceCredentialError::Store)?;
    if *cancelled {
        return Ok(());
    }
    match result {
        Ok(envelope) => remember_envelope(environment, entry_id, &envelope),
        Err(FinanceCredentialError::Locked) => {
            remember(environment, entry_id, Some(SessionAccess::PresentDenied));
        }
        Err(error) => return Err(error),
    }
    publish_session(state, entry_id)
}

pub(crate) fn last_store_failure(state: &AppState, entry_id: &str) -> bool {
    matches!(
        status_projection(state, entry_id),
        Ok(Some(
            SessionAccess::Known {
                last_store_failure: true,
                ..
            } | SessionAccess::StoreUnavailable
        ))
    )
}

pub(crate) fn revision_for_state(
    state: &AppState,
    entry_id: &str,
) -> Result<Option<String>, FinanceCredentialError> {
    #[cfg(feature = "eval-harness")]
    if crate::eval_harness::replay::credential_environment(&state.credential_environment, entry_id)
    {
        return active_revision(&state.credential_environment, entry_id);
    }
    match status_projection(state, entry_id)? {
        Some(SessionAccess::Known {
            active_revision, ..
        }) => Ok(active_revision),
        Some(SessionAccess::Locked) => Err(FinanceCredentialError::Locked),
        Some(
            SessionAccess::Unconfirmed
            | SessionAccess::StoreUnavailable
            | SessionAccess::PresentUnread
            | SessionAccess::PresentDenied,
        )
        | None => Ok(None),
    }
}

pub(crate) fn operation_pending(
    state: &AppState,
    entry_id: &str,
) -> Result<bool, FinanceCredentialError> {
    Ok(KeychainOperation::pending(
        NATIVE_SERVICE,
        native_account(&state.credential_environment, entry_id),
    )?)
}

pub(crate) fn reserve(
    state: &AppState,
    entry_id: &str,
) -> Result<KeychainOperation, FinanceCredentialError> {
    validate_entry_id(entry_id)?;
    Ok(KeychainOperation::begin(
        NATIVE_SERVICE,
        native_account(&state.credential_environment, entry_id),
    )?)
}

pub(crate) fn read<T: Send + 'static>(
    state: &AppState,
    entry_id: &str,
    action: impl FnOnce(&AppState, &str) -> Result<T, FinanceCredentialError> + Send + 'static,
) -> Result<T, FinanceCredentialError> {
    read_with_timeout(state, entry_id, crate::keychain_operation::TIMEOUT, action)
}

pub(crate) fn read_timed_out(state: &AppState, entry_id: &str) -> bool {
    KeychainOperation::read_timed_out(
        NATIVE_SERVICE,
        native_account(&state.credential_environment, entry_id),
    )
}

pub(crate) fn read_with_timeout<T: Send + 'static>(
    state: &AppState,
    entry_id: &str,
    timeout: std::time::Duration,
    action: impl FnOnce(&AppState, &str) -> Result<T, FinanceCredentialError> + Send + 'static,
) -> Result<T, FinanceCredentialError> {
    let operation = reserve(state, entry_id)?;
    let state = state.clone();
    let entry_id = entry_id.to_owned();
    operation.read_with_timeout(timeout, move || {
        let result = action(&state, &entry_id);
        publish_session(&state, &entry_id)?;
        result
    })?
}

pub(crate) fn mutate<T: Send + 'static>(
    state: &AppState,
    entry_id: &str,
    action: impl FnOnce(&AppState, &str) -> Result<T, FinanceCredentialError> + Send + 'static,
) -> Result<T, FinanceCredentialError> {
    mutate_with_timeout(state, entry_id, crate::keychain_operation::TIMEOUT, action)
}

fn mutate_with_timeout<T: Send + 'static>(
    state: &AppState,
    entry_id: &str,
    timeout: std::time::Duration,
    action: impl FnOnce(&AppState, &str) -> Result<T, FinanceCredentialError> + Send + 'static,
) -> Result<T, FinanceCredentialError> {
    let operation = reserve(state, entry_id)?;
    let state = state.clone();
    let entry_id = entry_id.to_owned();
    let maintenance = state.maintenance.clone();
    operation.mutate_with_timeout(&maintenance, timeout, move || {
        // A quit after the OS changes the item but before its result publishes
        // leaves the projection unknown. The next explicit check reads the item.
        let prior = status_projection(&state, &entry_id).ok().flatten();
        let pending = serde_json::to_string(&SessionAccess::Unconfirmed)
            .map_err(|_| FinanceCredentialError::Store)?;
        state
            .store
            .set_connector_credential_status(&entry_id, &pending)
            .map_err(|_| FinanceCredentialError::Store)?;
        let result = action(&state, &entry_id);
        if matches!(
            result,
            Err(FinanceCredentialError::Store
                | FinanceCredentialError::Unavailable
                | FinanceCredentialError::Unsupported)
        ) {
            let environment = &state.credential_environment;
            let access = match remembered(environment, &entry_id).or(prior) {
                Some(SessionAccess::Known {
                    status,
                    active_revision,
                    ..
                }) => SessionAccess::Known {
                    status,
                    active_revision,
                    last_store_failure: true,
                },
                Some(SessionAccess::Locked) => SessionAccess::Locked,
                _ => SessionAccess::StoreUnavailable,
            };
            remember(environment, &entry_id, Some(access));
        }
        publish_session(&state, &entry_id)?;
        result
    })?
}

pub(crate) fn get_for_state(
    state: &AppState,
    entry_id: &str,
) -> Result<Option<ActiveCredentialBundle>, FinanceCredentialError> {
    // Replay credentials belong to the caller's runtime. The native credential
    // worker has no Tokio context and only serves real credential operations.
    #[cfg(feature = "eval-harness")]
    if crate::eval_harness::replay::credential_environment(&state.credential_environment, entry_id)
    {
        return get(&state.credential_environment, entry_id);
    }
    read(state, entry_id, |state, id| {
        if needs_permission(state, id) {
            return Err(FinanceCredentialError::PermissionRequired);
        }
        get(&state.credential_environment, id)
    })
}
#[cfg(feature = "eval-harness")]
fn eval_fields() -> BTreeMap<String, String> {
    let mut fields: BTreeMap<String, String> = ["api_key", "app_key", "app_secret"]
        .into_iter()
        .map(|key| (key.into(), "eval-placeholder".into()))
        .collect();
    // A replay that answers a Korea Investment account read carries a
    // fictional account profile; quote-only replays keep account reads off.
    if crate::eval_harness::replay::wants_kis_account() {
        fields.insert("account_number".into(), "50000000".into());
        fields.insert("account_product_code".into(), "01".into());
    }
    fields
}

#[cfg(test)]
mod tests {
    use super::*;
    const ENVIRONMENT: &str = "5e1698ab-bc17-43c9-99f9-b3d8d04df05f";

    fn bundle(values: &[(&str, &str)]) -> BTreeMap<String, String> {
        values
            .iter()
            .map(|(credential_id, secret)| ((*credential_id).to_owned(), (*secret).to_owned()))
            .collect()
    }

    fn clean(entry_id: &str) {
        delete(ENVIRONMENT, entry_id).unwrap();
    }

    fn verify(
        entry_id: &str,
        revision: &str,
        outcome: VerificationOutcome,
        at: i64,
    ) -> FinishVerification {
        finish_verification(ENVIRONMENT, entry_id, revision, outcome, at, None).unwrap()
    }

    fn promote(entry_id: &str, secrets: &BTreeMap<String, String>, verified_at: i64) {
        stage(ENVIRONMENT, entry_id, secrets).unwrap();
        let candidate = candidate(ENVIRONMENT, entry_id).unwrap().unwrap();
        assert_eq!(
            verify(
                entry_id,
                candidate.revision(),
                VerificationOutcome::Verified,
                verified_at
            ),
            FinishVerification::Applied
        );
    }

    fn active_secret(entry_id: &str, credential_id: &str) -> Option<String> {
        get(ENVIRONMENT, entry_id)
            .unwrap()
            .and_then(|active| active.get(credential_id).map(str::to_owned))
    }

    #[test]
    fn staged_credentials_are_not_executable_until_exact_revision_is_verified() {
        let entry = "test.stage-promote";
        clean(entry);
        let secrets = bundle(&[("app_key", "candidate-app-key")]);
        let staged_status = stage(ENVIRONMENT, entry, &secrets).unwrap();
        assert!(staged_status.pending && !staged_status.active);
        assert!(get(ENVIRONMENT, entry).unwrap().is_none());
        assert_eq!(active_revision(ENVIRONMENT, entry).unwrap(), None);

        let candidate = candidate(ENVIRONMENT, entry).unwrap().unwrap();
        assert_eq!(candidate.secrets(), &secrets);
        assert_eq!(
            verify(
                entry,
                candidate.revision(),
                VerificationOutcome::Verified,
                17
            ),
            FinishVerification::Applied
        );
        assert_eq!(
            active_secret(entry, "app_key").as_deref(),
            Some("candidate-app-key")
        );
        assert_eq!(
            active_revision(ENVIRONMENT, entry).unwrap().as_deref(),
            Some(candidate.revision())
        );
        let status = status(ENVIRONMENT, entry).unwrap();
        assert!(status.active && !status.pending);
        assert_eq!(status.verified_at, Some(17));
        clean(entry);
    }

    #[test]
    fn stale_or_failed_verification_keeps_the_known_good_active_secret() {
        let entry = "test.stale-cas";
        clean(entry);
        promote(entry, &bundle(&[("api_key", "known-good-secret")]), 10);
        let known_good = active_revision(ENVIRONMENT, entry).unwrap();

        stage(
            ENVIRONMENT,
            entry,
            &bundle(&[("api_key", "slow-candidate-secret")]),
        )
        .unwrap();
        let slow = candidate(ENVIRONMENT, entry).unwrap().unwrap();
        stage(
            ENVIRONMENT,
            entry,
            &bundle(&[("api_key", "newer-candidate-secret")]),
        )
        .unwrap();
        let newer = candidate(ENVIRONMENT, entry).unwrap().unwrap();
        assert_eq!(
            verify(entry, slow.revision(), VerificationOutcome::Verified, 20),
            FinishVerification::Stale
        );
        assert_eq!(
            active_secret(entry, "api_key").as_deref(),
            Some("known-good-secret")
        );
        assert_eq!(active_revision(ENVIRONMENT, entry).unwrap(), known_good);

        assert_eq!(
            verify(entry, newer.revision(), VerificationOutcome::Rejected, 30),
            FinishVerification::Applied
        );
        assert_eq!(
            active_secret(entry, "api_key").as_deref(),
            Some("known-good-secret")
        );
        let status = status(ENVIRONMENT, entry).unwrap();
        assert!(status.active && status.pending);
        assert_eq!(status.verification, CredentialVerification::Rejected);
        assert_eq!(status.verified_at, Some(10));
        clean(entry);
    }

    #[test]
    fn partial_patch_merges_active_and_candidate_fields() {
        let entry = "test.partial-profile";
        clean(entry);
        promote(
            entry,
            &bundle(&[
                ("app_key", "known-good-key"),
                ("app_secret", "known-good-secret"),
            ]),
            10,
        );
        stage(
            ENVIRONMENT,
            entry,
            &bundle(&[("account_product_code", "01")]),
        )
        .unwrap();
        stage(
            ENVIRONMENT,
            entry,
            &bundle(&[("account_number", "12345678")]),
        )
        .unwrap();
        let candidate = candidate(ENVIRONMENT, entry).unwrap().unwrap();
        assert_eq!(
            candidate.secrets(),
            &bundle(&[
                ("account_number", "12345678"),
                ("account_product_code", "01"),
                ("app_key", "known-good-key"),
                ("app_secret", "known-good-secret"),
            ])
        );
        clean(entry);
    }

    #[test]
    fn delete_removes_active_and_candidate_material() {
        let entry = "test.delete-all";
        clean(entry);
        promote(entry, &bundle(&[("api_key", "active-secret-value")]), 10);
        stage(
            ENVIRONMENT,
            entry,
            &bundle(&[("api_key", "pending-secret-value")]),
        )
        .unwrap();
        delete(ENVIRONMENT, entry).unwrap();
        assert!(get(ENVIRONMENT, entry).unwrap().is_none());
        assert!(candidate(ENVIRONMENT, entry).unwrap().is_none());
        assert!(!status(ENVIRONMENT, entry).unwrap().stored);
    }

    #[test]
    fn rechecking_active_keys_stages_a_fresh_revision_and_preserves_the_current_connection() {
        let entry = "test.active-recheck";
        clean(entry);
        promote(
            entry,
            &bundle(&[("app_key", "current-key"), ("app_secret", "current-secret")]),
            10,
        );
        let current = active_revision(ENVIRONMENT, entry).unwrap().unwrap();
        let recheck = candidate_for_verification(ENVIRONMENT, entry)
            .unwrap()
            .unwrap();
        assert_ne!(recheck.revision(), current);
        assert_eq!(recheck.secrets().len(), 2);

        verify(
            entry,
            recheck.revision(),
            VerificationOutcome::TemporarilyUnavailable,
            20,
        );
        let retry = candidate_for_verification(ENVIRONMENT, entry)
            .unwrap()
            .unwrap();
        assert_eq!(retry.revision(), recheck.revision());
        assert_eq!(
            active_revision(ENVIRONMENT, entry).unwrap().unwrap(),
            current
        );
        verify(entry, retry.revision(), VerificationOutcome::Verified, 30);
        assert_eq!(
            active_revision(ENVIRONMENT, entry).unwrap().as_deref(),
            Some(retry.revision())
        );
        clean(entry);
    }

    #[test]
    fn deleting_or_replacing_keys_invalidates_an_active_recheck() {
        let entry = "test.recheck-revocation";
        clean(entry);
        assert!(candidate_for_verification(ENVIRONMENT, entry)
            .unwrap()
            .is_none());
        promote(entry, &bundle(&[("api_key", "current-key")]), 10);
        let recheck = candidate_for_verification(ENVIRONMENT, entry)
            .unwrap()
            .unwrap();
        stage(ENVIRONMENT, entry, &bundle(&[("api_key", "new-key")])).unwrap();
        assert_eq!(
            verify(entry, recheck.revision(), VerificationOutcome::Verified, 20),
            FinishVerification::Stale
        );
        let replacement = candidate_for_verification(ENVIRONMENT, entry)
            .unwrap()
            .unwrap();
        delete(ENVIRONMENT, entry).unwrap();
        assert_eq!(
            verify(
                entry,
                replacement.revision(),
                VerificationOutcome::Verified,
                30
            ),
            FinishVerification::Stale
        );
        assert!(get(ENVIRONMENT, entry).unwrap().is_none());
    }

    #[test]
    fn environment_namespaces_isolate_reads_verification_and_deletion() {
        let first = Uuid::new_v4().to_string();
        let second = Uuid::new_v4().to_string();
        let entry = "test.environment-isolation";
        stage(
            &first,
            entry,
            &bundle(&[("api_key", "isolated-first-value")]),
        )
        .unwrap();
        let first_candidate = candidate(&first, entry).unwrap().unwrap();
        assert!(!status(&second, entry).unwrap().stored);
        assert_eq!(
            finish_verification(
                &second,
                entry,
                first_candidate.revision(),
                VerificationOutcome::Verified,
                1,
                None
            )
            .unwrap(),
            FinishVerification::Stale
        );
        finish_verification(
            &first,
            entry,
            first_candidate.revision(),
            VerificationOutcome::Verified,
            1,
            None,
        )
        .unwrap();
        stage(
            &second,
            entry,
            &bundle(&[("api_key", "isolated-second-value")]),
        )
        .unwrap();
        let second_candidate = candidate(&second, entry).unwrap().unwrap();
        finish_verification(
            &second,
            entry,
            second_candidate.revision(),
            VerificationOutcome::Verified,
            2,
            None,
        )
        .unwrap();
        delete(&first, entry).unwrap();
        assert!(!status(&first, entry).unwrap().active);
        assert_eq!(
            get(&second, entry).unwrap().unwrap().get("api_key"),
            Some("isolated-second-value")
        );
        delete(&second, entry).unwrap();
    }

    #[test]
    fn unsafe_identifiers_and_secrets_are_rejected() {
        for (environment, entry, credential_id, secret) in [
            (
                "../invalid-environment",
                "fred.macro",
                "api_key",
                "test-value",
            ),
            (
                ENVIRONMENT,
                "../../credential",
                "api_key",
                "test-secret-value",
            ),
            (ENVIRONMENT, "fred.macro", "api_key", "line\nbreak-secret"),
            (ENVIRONMENT, "fred.macro", "../../key", "test-secret-value"),
        ] {
            assert!(stage(environment, entry, &bundle(&[(credential_id, secret)])).is_err());
        }
        assert!(stage(ENVIRONMENT, "fred.macro", &BTreeMap::new()).is_err());
    }
}
#[test]
fn krx_coverage_belongs_to_the_verified_key_and_survives_a_failed_replacement() {
    let environment = uuid::Uuid::new_v4().to_string();
    let entry = "krx.market-data";
    let key = BTreeMap::from([("api_key".into(), "fixture-krx-key".into())]);
    stage(&environment, entry, &key).unwrap();
    let pending = candidate(&environment, entry).unwrap().unwrap();
    let coverage = KrxCoverage {
        kospi: KrxServiceAccess::Verified,
        kosdaq: KrxServiceAccess::NeedsApproval,
    };
    finish_verification(
        &environment,
        entry,
        pending.revision(),
        VerificationOutcome::Verified,
        10,
        Some(coverage),
    )
    .unwrap();
    assert_eq!(
        status(&environment, entry).unwrap().krx_coverage,
        Some(coverage)
    );
    stage(
        &environment,
        entry,
        &BTreeMap::from([("api_key".into(), "replacement-krx-key".into())]),
    )
    .unwrap();
    let pending = candidate(&environment, entry).unwrap().unwrap();
    finish_verification(
        &environment,
        entry,
        pending.revision(),
        VerificationOutcome::Rejected,
        20,
        None,
    )
    .unwrap();
    let retained = status(&environment, entry).unwrap();
    assert!(retained.active && retained.pending);
    assert_eq!(retained.krx_coverage, Some(coverage));
    delete(&environment, entry).unwrap();
    assert_eq!(status(&environment, entry).unwrap().krx_coverage, None);
}

#[cfg(test)]
mod session_gate_tests {
    use super::*;

    #[test]
    fn passive_status_never_loads_a_native_item() {
        let environment = Uuid::new_v4().to_string();
        let id = "fred.macro";
        stage(
            &environment,
            id,
            &BTreeMap::from([("api_key".into(), "fixture".into())]),
        )
        .unwrap();
        assert!(status(&environment, id).unwrap().stored);
        let reads = test_store::secret_reads(&environment, id);
        test_store::restart(&environment, id);
        assert!(!status(&environment, id).unwrap().stored);
        assert!(active_revision(&environment, id).unwrap().is_none());
        assert_eq!(test_store::secret_reads(&environment, id), reads);
        delete(&environment, id).unwrap();
    }

    #[test]
    fn a_denial_is_remembered_for_its_own_credential_until_try_again() {
        let environment = Uuid::new_v4().to_string();
        let id = "fred.macro";
        let patch = BTreeMap::from([("api_key".into(), "fixture".into())]);
        stage(&environment, id, &patch).unwrap();
        test_store::deny(&environment, id, true);
        let before = test_store::secret_reads(&environment, id);
        assert!(matches!(
            get(&environment, id),
            Err(FinanceCredentialError::Locked)
        ));
        assert!(is_locked(&environment, id));
        assert!(matches!(
            status(&environment, id),
            Err(FinanceCredentialError::Locked)
        ));
        assert!(matches!(
            get(&environment, id),
            Err(FinanceCredentialError::Locked)
        ));
        assert_eq!(test_store::secret_reads(&environment, id), before + 1);
        assert!(matches!(
            unlock(&environment, id),
            Err(FinanceCredentialError::Locked)
        ));
        test_store::deny(&environment, id, false);
        assert!(unlock(&environment, id).unwrap().stored);
        assert_eq!(test_store::secret_reads(&environment, id), before + 3);
        delete(&environment, id).unwrap();
    }

    #[test]
    fn late_native_save_success_and_failure_update_the_durable_projection() {
        for stored in [true, false] {
            late_native_save(stored);
        }
    }

    fn late_native_save(stored: bool) {
        let temporary = tempfile::tempdir().unwrap();
        let state = AppState::for_persistent_test(temporary.path().join("app"));
        let id = "openfigi.lookup";
        let release = test_store::block_next_write(&state.credential_environment, id);
        let before = test_store::backend_calls(&state.credential_environment);
        assert!(matches!(
            mutate_with_timeout(
                &state,
                id,
                std::time::Duration::from_millis(100),
                |state, _| {
                    save_lookup_key(&state.credential_environment, "late-fixture-secret")
                }
            ),
            Err(FinanceCredentialError::Unavailable)
        ));
        assert_eq!(
            test_store::backend_calls(&state.credential_environment),
            before + 2
        );
        assert!(matches!(
            reserve(&state, id),
            Err(FinanceCredentialError::Busy)
        ));
        assert!(!status_for_state(&state, id).unwrap().active);
        assert!(state.maintenance.begin_update().is_err());
        release.send(stored).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let settled = loop {
            if let Ok(settled) = reserve(&state, id) {
                break settled;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        };
        assert_eq!(status_for_state(&state, id).unwrap().active, stored);
        let record = state
            .store
            .connector_credential_status(id)
            .unwrap()
            .unwrap();
        assert_eq!(record.contains("api_key"), stored);
        assert!(!record.contains("late-fixture-secret"));
        test_store::restart(&state.credential_environment, id);
        assert_eq!(status_for_state(&state, id).unwrap().active, stored);
        assert_eq!(last_store_failure(&state, id), !stored);
        assert!(state.maintenance.begin_update().is_ok());
        drop(settled);
        delete(&state.credential_environment, id).unwrap();
    }
}
