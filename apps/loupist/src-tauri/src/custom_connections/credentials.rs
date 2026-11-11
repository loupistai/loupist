use std::collections::{BTreeMap, BTreeSet};

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

    pub fn retire_values(&mut self, values: impl IntoIterator<Item = String>) {
        self.retired_values.extend(values);
        let mut seen = BTreeSet::new();
        self.retired_values
            .retain(|value| seen.insert(value.clone()));
        let excess = self.retired_values.len().saturating_sub(256);
        self.retired_values.drain(..excess);
    }

    pub fn replace_tokens(&mut self, tokens: Tokens) {
        let previous = self.tokens.replace(tokens);
        self.retire_values(
            previous.into_iter().flat_map(|tokens| {
                std::iter::once(tokens.access_token).chain(tokens.refresh_token)
            }),
        );
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
        .map_err(|_| ConnectionError::CredentialStore)?
        .get(&(environment.into(), id.into()))
        .cloned()
        .unwrap_or_default())
}
#[cfg(any(debug_assertions, test, feature = "webdriver"))]
pub(super) fn save(environment: &str, id: &str, secrets: &Secrets) -> Result<(), ConnectionError> {
    memory_store()
        .lock()
        .map_err(|_| ConnectionError::CredentialStore)?
        .insert((environment.into(), id.into()), secrets.clone());
    Ok(())
}
#[cfg(any(debug_assertions, test, feature = "webdriver"))]
pub(super) fn remove(environment: &str, id: &str) -> Result<(), ConnectionError> {
    memory_store()
        .lock()
        .map_err(|_| ConnectionError::CredentialStore)?
        .remove(&(environment.into(), id.into()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(index: usize, refresh: &str) -> Tokens {
        Tokens {
            resource: "https://fixture.example/mcp".into(),
            issuer: "https://fixture.example".into(),
            access_token: format!("access-{index}"),
            refresh_token: Some(refresh.into()),
            scopes: Default::default(),
        }
    }

    #[test]
    fn repeated_token_rotation_keeps_the_newest_256_distinct_retired_values() {
        let mut secrets = Secrets::default();
        for index in 0..300 {
            secrets.replace_tokens(tokens(index, &format!("refresh-{index}")));
        }
        assert_eq!(secrets.retired_values.len(), 256);
        assert_eq!(secrets.retired_values.first().unwrap(), "access-171");
        assert_eq!(secrets.retired_values.last().unwrap(), "refresh-298");
        let protected = secrets.protected_values();
        assert!(protected.contains(&"access-299".into()));
        assert!(protected.contains(&"refresh-299".into()));
        assert!(!protected.contains(&"access-0".into()));
        let encoded = serde_json::to_vec(&secrets).unwrap();
        let mut reloaded: Secrets = serde_json::from_slice(&encoded).unwrap();
        reloaded.replace_tokens(tokens(300, "refresh-300"));
        assert_eq!(reloaded.retired_values.len(), 256);
    }

    #[test]
    fn nonrotating_refresh_tokens_and_legacy_values_are_deduplicated() {
        let mut secrets = Secrets {
            retired_values: vec!["legacy".into(); 300],
            ..Default::default()
        };
        for index in 0..300 {
            secrets.replace_tokens(tokens(index, "stable-refresh"));
        }
        assert_eq!(secrets.retired_values.len(), 256);
        assert_eq!(
            secrets.retired_values.iter().collect::<BTreeSet<_>>().len(),
            256
        );
        assert!(secrets.retired_values.contains(&"access-298".into()));
        assert!(!secrets.retired_values.contains(&"access-0".into()));
        assert!(secrets
            .protected_values()
            .contains(&"stable-refresh".into()));
    }
}
