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
#[serde(rename_all = "snake_case")]
pub enum KrxServiceAccess {
    Verified,
    NeedsApproval,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
struct StoredCredentialBundle {
    revision: String,
    secrets: BTreeMap<String, String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ActiveCredential {
    credential: StoredCredentialBundle,
    verified_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    krx_coverage: Option<KrxCoverage>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CandidateCredential {
    credential: StoredCredentialBundle,
    verification: CandidateVerification,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
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

#[cfg(any(debug_assertions, test, feature = "webdriver"))]
fn read_memory_blob(
    environment_id: &str,
    entry_id: &str,
) -> Result<Option<String>, FinanceCredentialError> {
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

fn load_locked(
    environment_id: &str,
    entry_id: &str,
) -> Result<CredentialEnvelope, FinanceCredentialError> {
    validate_revision(environment_id)?;
    validate_entry_id(entry_id)?;
    let Some(value) = read_blob(environment_id, entry_id)? else {
        return Ok(CredentialEnvelope::default());
    };
    let envelope: CredentialEnvelope =
        serde_json::from_str(&value).map_err(|_| FinanceCredentialError::Store)?;
    validate_envelope(&envelope)?;
    Ok(envelope)
}

fn save_locked(
    environment_id: &str,
    entry_id: &str,
    envelope: &CredentialEnvelope,
) -> Result<(), FinanceCredentialError> {
    validate_envelope(envelope)?;
    let value = serde_json::to_string(envelope).map_err(|_| FinanceCredentialError::Store)?;
    write_blob(environment_id, entry_id, &value)
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
    let mut envelope = load_locked(environment_id, entry_id)?;
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
    Ok(load_locked(environment_id, entry_id)?
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
    let mut envelope = load_locked(environment_id, entry_id)?;
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
    let mut envelope = load_locked(environment_id, entry_id)?;
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
    Ok(status_from(&load_locked(environment_id, entry_id)?))
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
    Ok(load_locked(environment_id, entry_id)?
        .active
        .map(|active| active.credential.revision))
}

pub fn delete(environment_id: &str, entry_id: &str) -> Result<(), FinanceCredentialError> {
    validate_revision(environment_id)?;
    validate_entry_id(entry_id)?;
    let _guard = operation_lock()
        .lock()
        .map_err(|_| FinanceCredentialError::Store)?;
    delete_blob(environment_id, entry_id)
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
