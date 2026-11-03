use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{
    auth::{AuthorizationMetadata, Registration, Tokens},
    ConnectionError,
};

/// Never implement Debug, or expose this type through a command result.
#[derive(Default, Clone, Deserialize, Serialize)]
pub(crate) struct Secrets {
    pub headers: BTreeMap<String, String>,
    pub env: BTreeMap<String, String>,
    pub registration: Option<Registration>,
    pub metadata: Option<AuthorizationMetadata>,
    pub tokens: Option<Tokens>,
    #[serde(default)]
    pub retired_values: Vec<String>,
}

impl Secrets {
    pub fn protected_values(&self) -> Vec<String> {
        self.headers
            .values()
            .chain(self.env.values())
            .cloned()
            .chain(
                self.registration
                    .iter()
                    .filter_map(|r| r.client_secret.clone()),
            )
            .chain(self.tokens.iter().flat_map(|t| {
                std::iter::once(t.access_token.clone()).chain(t.refresh_token.clone())
            }))
            .chain(self.retired_values.clone())
            .collect()
    }

    pub fn replace_tokens(&mut self, tokens: Tokens) -> Result<(), ConnectionError> {
        if self.retired_values.len() >= 256 {
            return Err(ConnectionError::CredentialStore);
        }
        if let Some(previous) = self.tokens.replace(tokens) {
            self.retired_values.push(previous.access_token);
            self.retired_values.extend(previous.refresh_token);
        }
        Ok(())
    }
}

#[cfg(all(
    not(any(debug_assertions, test, feature = "webdriver")),
    any(target_os = "macos", windows)
))]
fn entry(environment: &str, id: &str) -> Result<keyring::Entry, ConnectionError> {
    keyring::Entry::new(
        "com.monarchjuno.loupist.custom-connections",
        &format!("{environment}:{id}"),
    )
    .map_err(|_| ConnectionError::CredentialStore)
}

#[cfg(all(
    not(any(debug_assertions, test, feature = "webdriver")),
    any(target_os = "macos", windows)
))]
pub(super) fn load(environment: &str, id: &str) -> Result<Secrets, ConnectionError> {
    match entry(environment, id)?.get_password() {
        Ok(encoded) => serde_json::from_str(&encoded).map_err(|_| ConnectionError::CredentialStore),
        Err(keyring::Error::NoEntry) => Ok(Secrets::default()),
        Err(_) => Err(ConnectionError::CredentialStore),
    }
}

#[cfg(all(
    not(any(debug_assertions, test, feature = "webdriver")),
    any(target_os = "macos", windows)
))]
pub(super) fn save(environment: &str, id: &str, secrets: &Secrets) -> Result<(), ConnectionError> {
    let encoded = serde_json::to_string(secrets).map_err(|_| ConnectionError::CredentialStore)?;
    entry(environment, id)?
        .set_password(&encoded)
        .map_err(|_| ConnectionError::CredentialStore)
}

#[cfg(all(
    not(any(debug_assertions, test, feature = "webdriver")),
    any(target_os = "macos", windows)
))]
pub(super) fn remove(environment: &str, id: &str) -> Result<(), ConnectionError> {
    match entry(environment, id)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err(ConnectionError::CredentialStore),
    }
}

#[cfg(all(
    not(any(debug_assertions, test, feature = "webdriver")),
    not(any(target_os = "macos", windows))
))]
pub(super) fn load(_: &str, _: &str) -> Result<Secrets, ConnectionError> {
    Err(ConnectionError::CredentialStore)
}
#[cfg(all(
    not(any(debug_assertions, test, feature = "webdriver")),
    not(any(target_os = "macos", windows))
))]
pub(super) fn save(_: &str, _: &str, _: &Secrets) -> Result<(), ConnectionError> {
    Err(ConnectionError::CredentialStore)
}
#[cfg(all(
    not(any(debug_assertions, test, feature = "webdriver")),
    not(any(target_os = "macos", windows))
))]
pub(super) fn remove(_: &str, _: &str) -> Result<(), ConnectionError> {
    Err(ConnectionError::CredentialStore)
}

#[cfg(any(debug_assertions, test, feature = "webdriver"))]
fn memory_store() -> &'static std::sync::Mutex<BTreeMap<(String, String), Secrets>> {
    static STORE: std::sync::OnceLock<std::sync::Mutex<BTreeMap<(String, String), Secrets>>> =
        std::sync::OnceLock::new();
    STORE.get_or_init(Default::default)
}
#[cfg(any(debug_assertions, test, feature = "webdriver"))]
pub(super) fn load(environment: &str, id: &str) -> Result<Secrets, ConnectionError> {
    Ok(memory_store()
        .lock()
        .unwrap()
        .get(&(environment.into(), id.into()))
        .cloned()
        .unwrap_or_default())
}
#[cfg(any(debug_assertions, test, feature = "webdriver"))]
pub(super) fn save(environment: &str, id: &str, secrets: &Secrets) -> Result<(), ConnectionError> {
    memory_store()
        .lock()
        .unwrap()
        .insert((environment.into(), id.into()), secrets.clone());
    Ok(())
}
#[cfg(any(debug_assertions, test, feature = "webdriver"))]
pub(super) fn remove(environment: &str, id: &str) -> Result<(), ConnectionError> {
    memory_store()
        .lock()
        .unwrap()
        .remove(&(environment.into(), id.into()));
    Ok(())
}
