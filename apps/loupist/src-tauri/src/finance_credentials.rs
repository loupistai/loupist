//! Connector credentials. Debug sessions use process memory; release builds
//! persist credentials in the OS store. Development never opens existing keys.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

const CREDENTIAL_SCHEMA_VERSION: &str = "loupist-finance-credential/1";

#[cfg(all(
    not(any(debug_assertions, test, feature = "webdriver")),
    any(target_os = "macos", windows)
))]
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

#[derive(Clone, Debug, PartialEq, Eq)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CredentialVerification {
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
    secrets: BTreeMap<String, String>,
}

impl CredentialCandidate {
    pub fn revision(&self) -> &str {
        &self.revision
    }

    pub fn secrets(&self) -> &BTreeMap<String, String> {
        &self.secrets
    }
}

/// A verified credential bundle that finance execution may use.
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
}

#[derive(Clone, Deserialize, Serialize)]
struct StoredCredentialBundle {
    revision: String,
    secrets: BTreeMap<String, String>,
}

#[derive(Clone, Deserialize, Serialize)]
struct ActiveCredential {
    credential: StoredCredentialBundle,
    verified_at: i64,
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

fn validate_envelope(envelope: &CredentialEnvelope) -> Result<(), FinanceCredentialError> {
    if envelope.schema_version != CREDENTIAL_SCHEMA_VERSION {
        return Err(FinanceCredentialError::Store);
    }
    if let Some(active) = &envelope.active {
        validate_revision(&active.credential.revision)?;
        validate_secrets(&active.credential.secrets)?;
        if active.verified_at < 0 {
            return Err(FinanceCredentialError::Store);
        }
    }
    if let Some(candidate) = &envelope.candidate {
        validate_revision(&candidate.credential.revision)?;
        validate_secrets(&candidate.credential.secrets)?;
    }
    Ok(())
}

fn operation_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(Default::default)
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

    pub(super) fn record_read(account: &str) -> bool {
        *reads()
            .lock()
            .unwrap()
            .entry(account.to_owned())
            .or_default() += 1;
        !denied().lock().unwrap().contains(account)
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
    memory_store()
        .lock()
        .map_err(|_| FinanceCredentialError::Store)?
        .insert(native_account(environment_id, entry_id), value.to_owned());
    Ok(())
}

#[cfg(any(debug_assertions, test, feature = "webdriver"))]
fn delete_memory_blob(environment_id: &str, entry_id: &str) -> Result<(), FinanceCredentialError> {
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
    match native_entry(environment_id, service, entry_id)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err(FinanceCredentialError::Store),
    }
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
/// macOS ties a keychain item to the build that saved it, so the first read
/// after an update asks for the login password. Every read takes
/// `operation_lock` (the gate), so callers that arrive while the prompt is up
/// wait for its answer instead of asking again. The answer is kept for the
/// session: the status (so listing connections reads nothing more) or a
/// denial (so no screen asks again until the user chooses Try again). The
/// secret itself is never kept here.
#[derive(Clone)]
enum SessionAccess {
    Known {
        status: CredentialStatus,
        active_revision: Option<String>,
    },
    Locked,
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
            active_revision: envelope
                .active
                .as_ref()
                .map(|active| active.credential.revision.clone()),
        }),
    );
}

/// Reads the envelope for a passive caller. A credential this session was
/// refused stays refused without asking macOS again.
fn load_locked(
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
    let value = match read_blob(environment_id, entry_id) {
        Ok(Some(value)) => value,
        Ok(None) => {
            let envelope = CredentialEnvelope::default();
            remember_envelope(environment_id, entry_id, &envelope);
            return Ok(envelope);
        }
        Err(FinanceCredentialError::Store | FinanceCredentialError::Locked) => {
            remember(environment_id, entry_id, Some(SessionAccess::Locked));
            return Err(FinanceCredentialError::Locked);
        }
        Err(error) => return Err(error),
    };
    let envelope: CredentialEnvelope =
        serde_json::from_str(&value).map_err(|_| FinanceCredentialError::Store)?;
    validate_envelope(&envelope)?;
    remember_envelope(environment_id, entry_id, &envelope);
    Ok(envelope)
}

/// Reads the envelope for something the user just asked for (save, check,
/// Try again): a remembered denial is set aside and macOS may ask once more.
fn load_for_user_locked(
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
    load_locked(environment_id, entry_id)
}

fn save_locked(
    environment_id: &str,
    entry_id: &str,
    envelope: &CredentialEnvelope,
) -> Result<(), FinanceCredentialError> {
    validate_envelope(envelope)?;
    let value = serde_json::to_string(envelope).map_err(|_| FinanceCredentialError::Store)?;
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
            if envelope.active.is_some() {
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
        verified_at: envelope
            .active
            .as_ref()
            .map(|credential| credential.verified_at),
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
    validate_revision(environment_id)?;
    validate_entry_id(entry_id)?;
    validate_secrets(patch)?;
    let _guard = operation_lock()
        .lock()
        .map_err(|_| FinanceCredentialError::Store)?;
    let mut envelope = load_for_user_locked(environment_id, entry_id)?;
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
    save_locked(environment_id, entry_id, &envelope)?;
    Ok(status_from(&envelope))
}

/// Loads only the current candidate. An active credential is never returned by
/// this function and therefore cannot be confused with an unverified replacement.
pub fn candidate(
    environment_id: &str,
    entry_id: &str,
) -> Result<Option<CredentialCandidate>, FinanceCredentialError> {
    let _guard = operation_lock()
        .lock()
        .map_err(|_| FinanceCredentialError::Store)?;
    Ok(load_for_user_locked(environment_id, entry_id)?
        .candidate
        .map(|candidate| CredentialCandidate {
            revision: candidate.credential.revision,
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
    let _guard = operation_lock()
        .lock()
        .map_err(|_| FinanceCredentialError::Store)?;
    let mut envelope = load_for_user_locked(environment_id, entry_id)?;
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
        save_locked(environment_id, entry_id, &envelope)?;
    }
    Ok(envelope.candidate.map(|candidate| CredentialCandidate {
        revision: candidate.credential.revision,
        secrets: candidate.credential.secrets,
    }))
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
    let _guard = operation_lock()
        .lock()
        .map_err(|_| FinanceCredentialError::Store)?;
    let mut envelope = load_for_user_locked(environment_id, entry_id)?;
    let Some(candidate) = envelope.candidate.as_mut() else {
        return Ok(FinishVerification::Stale);
    };
    if candidate.credential.revision != expected_revision {
        return Ok(FinishVerification::Stale);
    }
    match outcome {
        VerificationOutcome::Verified => {
            let candidate = envelope
                .candidate
                .take()
                .expect("candidate was checked above");
            envelope.active = Some(ActiveCredential {
                credential: candidate.credential,
                verified_at,
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
    save_locked(environment_id, entry_id, &envelope)?;
    Ok(FinishVerification::Applied)
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
    let _guard = operation_lock()
        .lock()
        .map_err(|_| FinanceCredentialError::Store)?;
    match remembered(environment_id, entry_id) {
        Some(SessionAccess::Known { status, .. }) => Ok(status),
        Some(SessionAccess::Locked) => Err(FinanceCredentialError::Locked),
        None => Ok(status_from(&load_locked(environment_id, entry_id)?)),
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
    let _guard = operation_lock()
        .lock()
        .map_err(|_| FinanceCredentialError::Store)?;
    Ok(status_from(&load_for_user_locked(
        environment_id,
        entry_id,
    )?))
}

/// Returns only a verified active bundle. Staged or rejected values are never
/// visible to finance execution.
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
    let _guard = operation_lock()
        .lock()
        .map_err(|_| FinanceCredentialError::Store)?;
    Ok(load_locked(environment_id, entry_id)?
        .active
        .map(|active| ActiveCredentialBundle {
            revision: active.credential.revision,
            secrets: active.credential.secrets,
        }))
}

/// Returns the revision of the verified active credential, if one exists.
///
/// This deliberately returns only the revision needed to bind a Pi session
/// cache to credential authority. It never constructs or exposes a credential
/// bundle, and staged candidates do not affect the result.
pub(crate) fn active_revision(
    environment_id: &str,
    entry_id: &str,
) -> Result<Option<String>, FinanceCredentialError> {
    #[cfg(feature = "eval-harness")]
    if crate::eval_harness::replay::credential_environment(environment_id, entry_id) {
        return Ok(Some("eval-replay".into()));
    }
    let _guard = operation_lock()
        .lock()
        .map_err(|_| FinanceCredentialError::Store)?;
    match remembered(environment_id, entry_id) {
        Some(SessionAccess::Known {
            active_revision, ..
        }) => Ok(active_revision),
        Some(SessionAccess::Locked) => Err(FinanceCredentialError::Locked),
        None => Ok(load_locked(environment_id, entry_id)?
            .active
            .map(|active| active.credential.revision)),
    }
}

pub fn delete(environment_id: &str, entry_id: &str) -> Result<(), FinanceCredentialError> {
    validate_revision(environment_id)?;
    validate_entry_id(entry_id)?;
    let _guard = operation_lock()
        .lock()
        .map_err(|_| FinanceCredentialError::Store)?;
    delete_blob(environment_id, entry_id)?;
    remember_envelope(environment_id, entry_id, &CredentialEnvelope::default());
    Ok(())
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

    fn saved(environment: &str, entry: &str) {
        let key = BTreeMap::from([("api_key".into(), "fixture-gate-key".into())]);
        stage(environment, entry, &key).unwrap();
        let pending = candidate(environment, entry).unwrap().unwrap();
        finish_verification(
            environment,
            entry,
            pending.revision(),
            VerificationOutcome::Verified,
            5,
            None,
        )
        .unwrap();
    }

    #[test]
    fn listing_status_reads_a_saved_secret_at_most_once_per_session() {
        let environment = Uuid::new_v4().to_string();
        let entry = "fred.macro";
        saved(&environment, entry);
        let after_save = test_store::secret_reads(&environment, entry);
        for _ in 0..5 {
            assert!(status(&environment, entry).unwrap().active);
            assert!(active_revision(&environment, entry).unwrap().is_some());
        }
        assert_eq!(test_store::secret_reads(&environment, entry), after_save);

        // After a restart (an update) the first status read is the only one.
        test_store::restart(&environment, entry);
        for _ in 0..5 {
            assert!(status(&environment, entry).unwrap().active);
        }
        assert_eq!(
            test_store::secret_reads(&environment, entry),
            after_save + 1
        );
        delete(&environment, entry).unwrap();
    }

    #[test]
    fn concurrent_first_reads_wait_for_one_answer() {
        let environment = Uuid::new_v4().to_string();
        let entry = "fred.macro";
        saved(&environment, entry);
        test_store::restart(&environment, entry);
        let before = test_store::secret_reads(&environment, entry);
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let threads = (0..8)
            .map(|_| {
                let barrier = barrier.clone();
                let environment = environment.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    status(&environment, entry).unwrap().active
                })
            })
            .collect::<Vec<_>>();
        for thread in threads {
            assert!(thread.join().unwrap());
        }
        assert_eq!(test_store::secret_reads(&environment, entry), before + 1);

        // A denial reached by concurrent callers is also asked once.
        test_store::restart(&environment, entry);
        test_store::deny(&environment, entry, true);
        let before = test_store::secret_reads(&environment, entry);
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let threads = (0..8)
            .map(|_| {
                let barrier = barrier.clone();
                let environment = environment.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    status(&environment, entry).is_err()
                })
            })
            .collect::<Vec<_>>();
        for thread in threads {
            assert!(thread.join().unwrap());
        }
        assert_eq!(test_store::secret_reads(&environment, entry), before + 1);
        test_store::deny(&environment, entry, false);
        delete(&environment, entry).unwrap();
    }

    #[test]
    fn a_denial_is_remembered_for_its_own_credential_until_try_again() {
        let environment = Uuid::new_v4().to_string();
        let (locked, other) = ("fred.macro", "krx.market-data");
        saved(&environment, locked);
        saved(&environment, other);
        test_store::restart(&environment, locked);
        test_store::restart(&environment, other);
        test_store::deny(&environment, locked, true);

        let before = test_store::secret_reads(&environment, locked);
        assert!(matches!(
            status(&environment, locked),
            Err(FinanceCredentialError::Locked)
        ));
        assert!(is_locked(&environment, locked));
        // Every later passive caller is refused without asking again.
        assert!(matches!(
            status(&environment, locked),
            Err(FinanceCredentialError::Locked)
        ));
        assert!(matches!(
            get(&environment, locked),
            Err(FinanceCredentialError::Locked)
        ));
        assert!(matches!(
            active_revision(&environment, locked),
            Err(FinanceCredentialError::Locked)
        ));
        assert_eq!(test_store::secret_reads(&environment, locked), before + 1);

        // The other connection is unaffected.
        assert!(!is_locked(&environment, other));
        assert!(status(&environment, other).unwrap().active);
        assert!(get(&environment, other).unwrap().is_some());

        // Try again asks again; still denied, still locked.
        assert!(matches!(
            unlock(&environment, locked),
            Err(FinanceCredentialError::Locked)
        ));
        assert_eq!(test_store::secret_reads(&environment, locked), before + 2);
        // Allowed this time: the connection is back.
        test_store::deny(&environment, locked, false);
        assert!(unlock(&environment, locked).unwrap().active);
        assert!(!is_locked(&environment, locked));
        assert!(get(&environment, locked).unwrap().is_some());
        delete(&environment, locked).unwrap();
        delete(&environment, other).unwrap();
    }
}
