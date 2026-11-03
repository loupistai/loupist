use std::{
    future::pending,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, Weak,
    },
    time::Duration,
};

use tokio::sync::Notify;

use crate::{
    app::CommandError,
    runs::maintenance::{ActivityLease, MaintenanceBlocker, MaintenanceCoordinator},
};

const DISCOVERY_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Debug, Default)]
pub struct ProviderSupportCoordinator {
    state: Arc<Mutex<ProviderSupportState>>,
    maintenance: MaintenanceCoordinator,
}

#[derive(Debug)]
pub struct ProviderSupportLease {
    state: Weak<Mutex<ProviderSupportState>>,
    operation: Arc<SupportOperation>,
    _activity: ActivityLease,
}

#[derive(Debug, Default)]
struct ProviderSupportState {
    active: Option<Weak<SupportOperation>>,
    pending_mutation: Option<Weak<SupportOperation>>,
    shutdown_failed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SupportKind {
    Ordinary,
    Discovery,
    OAuth,
}

#[derive(Debug)]
struct SupportOperation {
    kind: SupportKind,
    cancellation: SupportCancellation,
    released: AtomicBool,
    release_notify: Notify,
}

impl SupportOperation {
    async fn released(&self) {
        let notified = self.release_notify.notified();
        tokio::pin!(notified);
        if self.released.load(Ordering::Acquire) {
            return;
        }
        notified.await;
    }
}

#[derive(Debug, Default)]
struct SupportCancellation {
    cancelled: AtomicBool,
    notify: Notify,
    authorization_url: Mutex<Option<String>>,
    manual_entry: Mutex<Option<ManualEntry>>,
}

/// What a sign-in's paste field accepts (providers spec §3.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasteRule {
    /// Sign in with ChatGPT: exactly the loopback address the browser was
    /// sent to, passed to Pi as it is.
    ChatgptLoopback,
    /// Pi's browser sign-in with its own callback (Claude; the development
    /// Codex client): the address the browser shows,
    /// `http://localhost:<port><path>?code=…&state=…` (or `127.0.0.1`), or a
    /// `code#state` value. Pi receives `code#state`.
    BrowserCallback { port: u16, path: &'static str },
}

impl PasteRule {
    /// The rule for a provider's browser sign-in; none for device flows.
    pub fn for_provider(provider: &str) -> Option<Self> {
        match provider {
            "openai" => Some(Self::ChatgptLoopback),
            "anthropic" => Some(Self::BrowserCallback {
                port: 53692,
                path: "/callback",
            }),
            "openai-codex" => Some(Self::BrowserCallback {
                port: 1455,
                path: "/auth/callback",
            }),
            _ => None,
        }
    }
}

/// Pi's waiting `manual_code` prompt for the active sign-in.
#[derive(Debug)]
pub struct ManualEntry {
    pub request_id: String,
    pub rule: PasteRule,
    pub responder: tokio::sync::mpsc::UnboundedSender<crate::pi::support::UiAnswer>,
}

const MAX_PASTED_CODE_BYTES: usize = 2 * 1024;
const MAX_PASTED_ADDRESS_BYTES: usize = 4 * 1024;

fn paste_invalid() -> CommandError {
    CommandError::new("paste_invalid", "That isn't the sign-in address or code.")
}

/// The one value of a query parameter that must appear exactly once.
fn single_param(url: &reqwest::Url, name: &str) -> Option<String> {
    let mut values = url
        .query_pairs()
        .filter(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned());
    let value = values.next()?;
    (values.next().is_none() && !value.is_empty()).then_some(value)
}

/// A loopback callback address: plain http, the exact host, port and path,
/// no user info or fragment.
fn loopback_callback(
    value: &str,
    hosts: &[&str],
    port: u16,
    path: &str,
) -> Result<reqwest::Url, CommandError> {
    if value.len() > MAX_PASTED_ADDRESS_BYTES {
        return Err(paste_invalid());
    }
    let url = reqwest::Url::parse(value).map_err(|_| paste_invalid())?;
    if url.scheme() != "http"
        || !url.host_str().is_some_and(|host| hosts.contains(&host))
        || url.port() != Some(port)
        || url.path() != path
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(paste_invalid());
    }
    Ok(url)
}

fn code_and_state(code: &str, state: &str) -> Result<String, CommandError> {
    let value = format!("{code}#{state}");
    if code.is_empty()
        || state.is_empty()
        || code.contains('#')
        || state.contains('#')
        || value.len() > MAX_PASTED_CODE_BYTES
    {
        return Err(paste_invalid());
    }
    Ok(value)
}

/// Accepts only what the active sign-in's rule allows and returns what Pi
/// expects. The value is never logged; anything else is refused before Pi sees it.
pub fn validate_pasted_value(rule: PasteRule, value: &str) -> Result<String, CommandError> {
    let value = value.trim();
    if value.is_empty() || value.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(paste_invalid());
    }
    match rule {
        PasteRule::ChatgptLoopback => {
            let url = loopback_callback(value, &["127.0.0.1"], 1455, "/auth/callback")?;
            if single_param(&url, "code").is_none() || single_param(&url, "state").is_none() {
                return Err(paste_invalid());
            }
            Ok(value.to_owned())
        }
        PasteRule::BrowserCallback { port, path } if value.starts_with("http") => {
            let url = loopback_callback(value, &["localhost", "127.0.0.1"], port, path)?;
            let (Some(code), Some(state)) =
                (single_param(&url, "code"), single_param(&url, "state"))
            else {
                return Err(paste_invalid());
            };
            code_and_state(&code, &state)
        }
        PasteRule::BrowserCallback { .. } => {
            let mut parts = value.split('#');
            let (Some(code), Some(state), None) = (parts.next(), parts.next(), parts.next()) else {
                return Err(paste_invalid());
            };
            code_and_state(code, state)
        }
    }
}
impl SupportCancellation {
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.notify.notify_waiters();
    }

    async fn cancelled(&self) {
        let notified = self.notify.notified();
        tokio::pin!(notified);
        if self.cancelled.load(Ordering::Acquire) {
            return;
        }
        notified.await;
    }
}

impl Drop for ProviderSupportLease {
    fn drop(&mut self) {
        if let Some(state) = self.state.upgrade() {
            let mut state = state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if state
                .active
                .as_ref()
                .and_then(Weak::upgrade)
                .is_some_and(|current| Arc::ptr_eq(&current, &self.operation))
            {
                state.active = None;
            }
            if state
                .pending_mutation
                .as_ref()
                .and_then(Weak::upgrade)
                .is_some_and(|current| Arc::ptr_eq(&current, &self.operation))
            {
                state.pending_mutation = None;
            }
        }
        self.operation.released.store(true, Ordering::Release);
        self.operation.release_notify.notify_waiters();
    }
}

impl ProviderSupportCoordinator {
    pub fn new(maintenance: MaintenanceCoordinator) -> Self {
        Self {
            state: Arc::default(),
            maintenance,
        }
    }

    pub fn try_acquire(&self) -> Result<ProviderSupportLease, CommandError> {
        self.try_acquire_inner(SupportKind::Ordinary)
    }

    pub fn try_acquire_discovery(&self) -> Result<ProviderSupportLease, CommandError> {
        self.try_acquire_inner(SupportKind::Discovery)
    }

    pub async fn acquire_mutation(&self) -> Result<ProviderSupportLease, CommandError> {
        self.acquire_after_discovery(false, DISCOVERY_SHUTDOWN_TIMEOUT)
            .await
    }

    pub async fn acquire_oauth(&self) -> Result<ProviderSupportLease, CommandError> {
        self.acquire_after_discovery(true, DISCOVERY_SHUTDOWN_TIMEOUT)
            .await
    }

    async fn acquire_after_discovery(
        &self,
        oauth: bool,
        shutdown_timeout: Duration,
    ) -> Result<ProviderSupportLease, CommandError> {
        let kind = if oauth {
            SupportKind::OAuth
        } else {
            SupportKind::Ordinary
        };
        let (lease, discovery) = self.admit_operation(kind, true)?;
        let Some(discovery) = discovery else {
            return Ok(lease);
        };
        // Pi can refresh saved credentials even while loading its catalog.
        // Reserve the write slot, then stop that helper before using it.
        discovery.cancellation.cancel();
        tokio::time::timeout(shutdown_timeout, async {
            tokio::select! {
                _ = lease.cancelled() => Err(CommandError::new("cancelled", "Provider sign-in was cancelled.")),
                _ = discovery.released() => Ok(()),
            }
        }).await.map_err(|_| CommandError::conflict(
                "Model discovery is still stopping. Try connecting again shortly.",
        ))??;
        if lease.is_cancelled() {
            return Err(CommandError::new(
                "cancelled",
                "Provider sign-in was cancelled.",
            ));
        }
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        ensure_shutdown_confirmed(&state)?;
        state.pending_mutation = None;
        state.active = Some(Arc::downgrade(&lease.operation));
        drop(state);
        Ok(lease)
    }

    fn oauth_operation(&self) -> Option<Arc<SupportOperation>> {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state
            .pending_mutation
            .as_ref()
            .and_then(Weak::upgrade)
            .filter(|operation| operation.kind == SupportKind::OAuth)
            .or_else(|| {
                state
                    .active
                    .as_ref()
                    .and_then(Weak::upgrade)
                    .filter(|operation| operation.kind == SupportKind::OAuth)
            })
    }

    fn try_acquire_inner(&self, kind: SupportKind) -> Result<ProviderSupportLease, CommandError> {
        self.admit_operation(kind, false).map(|(lease, _)| lease)
    }

    fn admit_operation(
        &self,
        kind: SupportKind,
        preempt_discovery: bool,
    ) -> Result<(ProviderSupportLease, Option<Arc<SupportOperation>>), CommandError> {
        let activity = self.maintenance.admit_activity(MaintenanceBlocker {
            id: "provider-support".into(),
            kind: "provider_support".into(),
            label: "Provider connection or model discovery".into(),
        })?;
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        ensure_shutdown_confirmed(&state)?;
        let active = state.active.as_ref().and_then(Weak::upgrade);
        if state
            .pending_mutation
            .as_ref()
            .and_then(Weak::upgrade)
            .is_some()
            || active
                .as_ref()
                .is_some_and(|active| !preempt_discovery || active.kind != SupportKind::Discovery)
        {
            return Err(CommandError::conflict(
                "another provider support operation is already active",
            ));
        }
        let operation = Arc::new(SupportOperation {
            kind,
            cancellation: SupportCancellation::default(),
            released: AtomicBool::new(false),
            release_notify: Notify::new(),
        });
        if active.is_some() {
            state.pending_mutation = Some(Arc::downgrade(&operation));
        } else {
            state.active = Some(Arc::downgrade(&operation));
        }
        Ok((
            ProviderSupportLease {
                state: Arc::downgrade(&self.state),
                operation,
                _activity: activity,
            },
            active,
        ))
    }

    pub fn cancel_oauth(&self) {
        if let Some(active) = self.oauth_operation() {
            active.cancellation.cancel();
        }
    }

    /// Forwards a pasted sign-in address or code to Pi's waiting prompt once.
    pub fn submit_oauth_code(&self, value: &str) -> Result<(), CommandError> {
        let operation = self
            .oauth_operation()
            .ok_or_else(|| CommandError::conflict("no sign-in is waiting for a pasted address"))?;
        let mut entry = operation
            .cancellation
            .manual_entry
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let rule = entry
            .as_ref()
            .ok_or_else(|| {
                CommandError::conflict("the sign-in is not ready for a pasted address yet")
            })?
            .rule;
        let value = validate_pasted_value(rule, value)?;
        let Some(waiting) = entry.take() else {
            return Err(CommandError::conflict("the sign-in already ended"));
        };
        waiting
            .responder
            .send((waiting.request_id, value))
            .map_err(|_| CommandError::conflict("the sign-in already ended"))
    }

    pub fn oauth_authorization_url(&self) -> Option<String> {
        self.oauth_operation().and_then(|active| {
            active
                .cancellation
                .authorization_url
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
        })
    }
}

fn ensure_shutdown_confirmed(state: &ProviderSupportState) -> Result<(), CommandError> {
    if state.shutdown_failed {
        return Err(CommandError::internal(
            "A provider helper could not stop. Restart Loupist before changing provider connections.",
        ));
    }
    Ok(())
}

impl ProviderSupportLease {
    pub fn set_oauth_authorization_url(&self, url: String) -> Result<(), CommandError> {
        if self.operation.kind != SupportKind::OAuth {
            return Err(CommandError::internal(
                "OAuth authorization URL has no active sign-in lease",
            ));
        }
        *self
            .operation
            .cancellation
            .authorization_url
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(url);
        Ok(())
    }

    pub fn set_manual_entry(&self, entry: ManualEntry) -> Result<(), CommandError> {
        if self.operation.kind != SupportKind::OAuth {
            return Err(CommandError::internal(
                "a pasted sign-in value has no active sign-in lease",
            ));
        }
        *self
            .operation
            .cancellation
            .manual_entry
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(entry);
        Ok(())
    }

    pub async fn cancelled(&self) {
        match self.operation.kind {
            SupportKind::Discovery | SupportKind::OAuth => {
                self.operation.cancellation.cancelled().await
            }
            SupportKind::Ordinary => pending().await,
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.operation
            .cancellation
            .cancelled
            .load(Ordering::Acquire)
    }

    pub fn mark_shutdown_failed(&self) {
        if let Some(state) = self.state.upgrade() {
            state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .shutdown_failed = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chatgpt_accepts_only_its_exact_loopback_callback() {
        let rule = PasteRule::ChatgptLoopback;
        let address = "http://127.0.0.1:1455/auth/callback?code=abc&state=xyz&client_id=issued";
        assert_eq!(
            validate_pasted_value(
                rule,
                &format!(
                    "  {address}
"
                )
            )
            .unwrap(),
            address
        );
        for refused in [
            "https://127.0.0.1:1455/auth/callback?code=abc&state=xyz",
            "http://localhost:1455/auth/callback?code=abc&state=xyz",
            "http://127.0.0.1:1456/auth/callback?code=abc&state=xyz",
            "http://127.0.0.1:1455/other?code=abc&state=xyz",
            "http://127.0.0.1:1455/auth/callback?code=abc",
            "http://127.0.0.1:1455/auth/callback?state=xyz",
            "http://127.0.0.1:1455/auth/callback?code=abc&code=def&state=xyz",
            "http://user@127.0.0.1:1455/auth/callback?code=abc&state=xyz",
            "http://127.0.0.1:1455/auth/callback?code=abc&state=xyz#fragment",
            "code#state",
            "",
        ] {
            assert_eq!(
                validate_pasted_value(rule, refused).unwrap_err().code,
                "paste_invalid",
                "{refused}"
            );
        }
    }

    #[test]
    fn claude_accepts_the_address_its_browser_shows_and_hands_pi_code_and_state() {
        let rule = PasteRule::for_provider("anthropic").unwrap();
        for address in [
            "http://localhost:53692/callback?code=abc123&state=verifier456",
            "http://127.0.0.1:53692/callback?state=verifier456&code=abc123",
        ] {
            assert_eq!(
                validate_pasted_value(rule, address).unwrap(),
                "abc123#verifier456",
                "{address}"
            );
        }
        assert_eq!(
            validate_pasted_value(rule, "abc123#verifier456").unwrap(),
            "abc123#verifier456"
        );
        for refused in [
            "https://localhost:53692/callback?code=abc&state=xyz",
            "http://localhost.evil.example:53692/callback?code=abc&state=xyz",
            "http://example.com:53692/callback?code=abc&state=xyz",
            "http://localhost:53693/callback?code=abc&state=xyz",
            "http://localhost:53692/other?code=abc&state=xyz",
            "http://localhost:53692/callback?code=abc",
            "http://localhost:53692/callback?state=xyz",
            "http://localhost:53692/callback?code=&state=xyz",
            "http://localhost:53692/callback?code=a%23b&state=xyz",
            "http://user@localhost:53692/callback?code=abc&state=xyz",
            "http://localhost:53692/callback?code=abc&state=xyz#fragment",
            "http://127.0.0.1:1455/auth/callback?code=abc&state=xyz",
            "abc123",
            "#state",
            "code#",
            "a#b#c",
            "code #state",
        ] {
            assert_eq!(
                validate_pasted_value(rule, refused).unwrap_err().code,
                "paste_invalid",
                "{refused}"
            );
        }
        let long = format!("{}#{}", "a".repeat(MAX_PASTED_CODE_BYTES), "b");
        assert!(validate_pasted_value(rule, &long).is_err());
        // The development Codex client uses its own port and path.
        let codex = PasteRule::for_provider("openai-codex").unwrap();
        assert_eq!(
            validate_pasted_value(codex, "http://localhost:1455/auth/callback?code=c&state=s")
                .unwrap(),
            "c#s"
        );
        assert!(PasteRule::for_provider("xai").is_none());
    }

    #[tokio::test]
    async fn a_pasted_value_reaches_only_the_active_sign_in_once() {
        let coordinator = ProviderSupportCoordinator::default();
        assert_eq!(
            coordinator.submit_oauth_code("abc#def").unwrap_err().code,
            "conflict"
        );
        let lease = coordinator.acquire_oauth().await.unwrap();
        assert_eq!(
            coordinator.submit_oauth_code("abc#def").unwrap_err().code,
            "conflict"
        );
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        lease
            .set_manual_entry(ManualEntry {
                request_id: "request-1".into(),
                rule: PasteRule::for_provider("anthropic").unwrap(),
                responder: sender,
            })
            .unwrap();
        assert_eq!(
            coordinator
                .submit_oauth_code("not a code")
                .unwrap_err()
                .code,
            "paste_invalid"
        );
        coordinator.submit_oauth_code("abc#def").unwrap();
        assert_eq!(
            receiver.recv().await,
            Some(("request-1".to_owned(), "abc#def".to_owned()))
        );
        // The prompt is answered; a second paste has nothing to answer.
        assert_eq!(
            coordinator.submit_oauth_code("abc#def").unwrap_err().code,
            "conflict"
        );
    }

    #[test]
    fn provider_support_is_single_flight_and_raii_releases_every_exit() {
        let coordinator = ProviderSupportCoordinator::default();
        let lease = coordinator.try_acquire().unwrap();
        assert_eq!(coordinator.try_acquire().unwrap_err().code, "conflict");
        drop(lease);

        fn fail_after_admission(
            coordinator: &ProviderSupportCoordinator,
        ) -> Result<(), CommandError> {
            let _lease = coordinator.try_acquire()?;
            Err(CommandError::internal("synthetic support failure"))
        }
        assert!(fail_after_admission(&coordinator).is_err());
        coordinator.try_acquire().unwrap();
    }

    #[tokio::test]
    async fn oauth_cancellation_targets_only_the_active_oauth_lease() {
        let coordinator = ProviderSupportCoordinator::default();
        let oauth = coordinator.acquire_oauth().await.unwrap();
        oauth
            .set_oauth_authorization_url(
                "https://auth.openai.com/oauth/authorize?state=test".into(),
            )
            .unwrap();
        assert_eq!(
            coordinator.oauth_authorization_url().as_deref(),
            Some("https://auth.openai.com/oauth/authorize?state=test")
        );
        coordinator.cancel_oauth();
        tokio::time::timeout(std::time::Duration::from_millis(100), oauth.cancelled())
            .await
            .unwrap();
        drop(oauth);
        assert_eq!(coordinator.oauth_authorization_url(), None);

        coordinator.cancel_oauth();
        let ordinary = coordinator.try_acquire().unwrap();
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), ordinary.cancelled())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn credential_mutation_cancels_discovery_and_waits_for_its_shutdown() {
        for oauth in [false, true] {
            let coordinator = ProviderSupportCoordinator::default();
            let discovery = coordinator.try_acquire_discovery().unwrap();
            let mutation = coordinator.acquire_after_discovery(oauth, Duration::from_secs(1));
            tokio::pin!(mutation);
            tokio::select! {
                _ = discovery.cancelled() => {},
                result = &mut mutation => panic!("mutation started before discovery stopped: {result:?}"),
            }
            assert!(discovery.is_cancelled());
            assert!(
                tokio::time::timeout(Duration::from_millis(10), &mut mutation)
                    .await
                    .is_err()
            );
            assert_eq!(
                coordinator.try_acquire_discovery().unwrap_err().code,
                "conflict"
            );
            assert_eq!(
                coordinator.acquire_mutation().await.unwrap_err().code,
                "conflict"
            );
            assert_eq!(
                coordinator.acquire_oauth().await.unwrap_err().code,
                "conflict"
            );

            // The caller releases this lease only after its owned process has stopped.
            drop(discovery);
            let mutation = tokio::time::timeout(Duration::from_secs(1), &mut mutation)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                coordinator.acquire_mutation().await.unwrap_err().code,
                "conflict"
            );
            assert_eq!(
                coordinator.acquire_oauth().await.unwrap_err().code,
                "conflict"
            );
            assert!(!mutation.is_cancelled());
            drop(mutation);
            coordinator.try_acquire_discovery().unwrap();
        }
    }

    #[tokio::test]
    async fn discovery_shutdown_wait_is_bounded_without_releasing_its_lease() {
        let coordinator = ProviderSupportCoordinator::default();
        let discovery = coordinator.try_acquire_discovery().unwrap();
        let error = coordinator
            .acquire_after_discovery(true, Duration::from_millis(1))
            .await
            .unwrap_err();
        assert_eq!(error.code, "conflict");
        assert!(discovery.is_cancelled());
        assert_eq!(coordinator.try_acquire().unwrap_err().code, "conflict");
        drop(discovery);
        coordinator.acquire_oauth().await.unwrap();
    }

    #[tokio::test]
    async fn oauth_can_be_cancelled_while_waiting_for_discovery_to_stop() {
        let coordinator = ProviderSupportCoordinator::default();
        let discovery = coordinator.try_acquire_discovery().unwrap();
        let reconnect = coordinator.acquire_oauth();
        tokio::pin!(reconnect);
        tokio::select! {
            _ = discovery.cancelled() => {},
            result = &mut reconnect => panic!("reconnect started before discovery stopped: {result:?}"),
        }
        coordinator.cancel_oauth();
        assert_eq!(reconnect.await.unwrap_err().code, "cancelled");
        assert_eq!(coordinator.try_acquire().unwrap_err().code, "conflict");
        drop(discovery);
        coordinator.acquire_oauth().await.unwrap();
    }

    #[tokio::test]
    async fn unconfirmed_support_shutdown_blocks_new_credential_writes() {
        let coordinator = ProviderSupportCoordinator::default();
        let discovery = coordinator.try_acquire_discovery().unwrap();
        discovery.mark_shutdown_failed();
        drop(discovery);
        assert_eq!(
            coordinator.acquire_mutation().await.unwrap_err().code,
            "internal"
        );
        assert_eq!(
            coordinator.acquire_oauth().await.unwrap_err().code,
            "internal"
        );
        assert_eq!(
            coordinator.try_acquire_discovery().unwrap_err().code,
            "internal"
        );
    }

    #[test]
    fn update_maintenance_blocks_provider_admission_and_reports_active_support() {
        let maintenance = MaintenanceCoordinator::default();
        let coordinator = ProviderSupportCoordinator::new(maintenance.clone());
        let support = coordinator.try_acquire().unwrap();

        let blockers = maintenance.begin_update().unwrap_err();
        assert_eq!(blockers.len(), 1);
        assert_eq!(blockers[0].kind, "provider_support");

        drop(support);
        let update = maintenance.begin_update().unwrap();
        let error = coordinator.try_acquire().unwrap_err();
        assert_eq!(error.code, "maintenance_active");
        drop(update);
        coordinator.try_acquire().unwrap();
    }
}
