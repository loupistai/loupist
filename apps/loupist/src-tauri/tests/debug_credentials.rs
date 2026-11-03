//! Link the ordinary debug library without its `cfg(test)` backend override.
#![cfg(all(
    debug_assertions,
    not(feature = "webdriver"),
    any(target_os = "macos", windows)
))]

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use loupist_desktop::finance_credentials as finance;

static KEYRING_ENTRIES: AtomicUsize = AtomicUsize::new(0);
const ENVIRONMENT: &str = "2f381b27-6028-4d98-aabf-bb5c14a2c64f";
const ENTRY: &str = "test.debug-session";
const CHILD_MARKER: &str = "LOUPIST_CREDENTIAL_SESSION_CHILD";

struct RejectAnyEntry;

impl keyring::credential::CredentialBuilderApi for RejectAnyEntry {
    fn build(
        &self,
        _: Option<&str>,
        _: &str,
        _: &str,
    ) -> keyring::Result<Box<keyring::credential::Credential>> {
        KEYRING_ENTRIES.fetch_add(1, Ordering::SeqCst);
        Err(keyring::Error::NoEntry)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[test]
fn normal_debug_credentials_are_session_only_and_never_construct_keyring_entries() {
    // Even a regressed backend is prevented from touching the real OS store.
    keyring::set_default_credential_builder(Box::new(RejectAnyEntry));
    assert!(!finance::status(ENVIRONMENT, ENTRY).unwrap().stored);
    if std::env::var_os(CHILD_MARKER).is_some() {
        assert_eq!(KEYRING_ENTRIES.load(Ordering::SeqCst), 0);
        return;
    }

    finance::stage(
        ENVIRONMENT,
        ENTRY,
        &BTreeMap::from([("api_key".into(), "synthetic-session-key".into())]),
    )
    .unwrap();
    let candidate = finance::candidate(ENVIRONMENT, ENTRY).unwrap().unwrap();
    finance::finish_verification(
        ENVIRONMENT,
        ENTRY,
        candidate.revision(),
        finance::VerificationOutcome::Verified,
        1,
        None,
    )
    .unwrap();
    assert!(finance::status(ENVIRONMENT, ENTRY).unwrap().active);
    assert_eq!(
        finance::get(ENVIRONMENT, ENTRY)
            .unwrap()
            .unwrap()
            .get("api_key"),
        Some("synthetic-session-key")
    );

    let fresh_process = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "normal_debug_credentials_are_session_only_and_never_construct_keyring_entries",
        ])
        .env(CHILD_MARKER, "1")
        .status()
        .unwrap();
    assert!(fresh_process.success());
    assert!(finance::status(ENVIRONMENT, ENTRY).unwrap().active);

    finance::delete(ENVIRONMENT, ENTRY).unwrap();
    assert!(!finance::status(ENVIRONMENT, ENTRY).unwrap().stored);
    assert_eq!(KEYRING_ENTRIES.load(Ordering::SeqCst), 0);
}
