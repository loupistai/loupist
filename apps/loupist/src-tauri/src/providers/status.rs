//! Provider state (providers spec §3.1): Connected / Needs reconnect /
//! Disconnected, at most one notice, and what still uses an account. Rust
//! applies every transition and emits `provider-status`; the renderer never
//! sees provider text.
use std::{collections::BTreeMap, sync::OnceLock};

use serde::{Deserialize, Serialize};
use tauri::Emitter;

use crate::{
    app::{AppState, CommandError},
    domain::ChatFailureKind,
    store::{provider_status::ProviderStatusRecord, LoupistStore},
};

static APP: OnceLock<tauri::AppHandle> = OnceLock::new();
const EVENT: &str = "provider-status";

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderState {
    Connected,
    NeedsReconnect,
    Disconnected,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CredentialKind {
    Plan,
    ApiKey,
    Local,
    Custom,
    Environment,
    None,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NoticeKind {
    PlanLimit,
    PlanNotEligible,
    ExtraUsageOff,
    OutOfCredit,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub struct ProviderNotice {
    pub kind: NoticeKind,
    pub at_ms: i64,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct UsedBy {
    pub schedules: u32,
    pub chat_default: bool,
    pub world_models: u32,
}

impl UsedBy {
    pub fn is_empty(&self) -> bool {
        self.schedules == 0 && !self.chat_default && self.world_models == 0
    }
}

/// Exactly `ProviderStatusView` in `src/contracts/model.ts`.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub struct ProviderStatusView {
    pub state: ProviderState,
    pub credential_kind: CredentialKind,
    pub notice: Option<ProviderNotice>,
    pub last_limit_at_ms: Option<i64>,
    pub connected_at_ms: Option<i64>,
    pub used_by: UsedBy,
}

impl ProviderStatusView {
    pub const DISCONNECTED: Self = Self {
        state: ProviderState::Disconnected,
        credential_kind: CredentialKind::None,
        notice: None,
        last_limit_at_ms: None,
        connected_at_ms: None,
        used_by: UsedBy {
            schedules: 0,
            chat_default: false,
            world_models: 0,
        },
    };

    /// The view before any stored state: a saved credential reads as connected.
    pub const fn from_credential(kind: CredentialKind) -> Self {
        match kind {
            CredentialKind::None => Self::DISCONNECTED,
            kind => Self {
                state: ProviderState::Connected,
                credential_kind: kind,
                notice: None,
                last_limit_at_ms: None,
                connected_at_ms: None,
                used_by: UsedBy {
                    schedules: 0,
                    chat_default: false,
                    world_models: 0,
                },
            },
        }
    }
}

/// One provider's stored state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderStatusRow {
    pub provider: String,
    pub credential_kind: CredentialKind,
    pub state: ProviderState,
    pub notice: Option<ProviderNotice>,
    pub last_limit_at_ms: Option<i64>,
    pub connected_at_ms: Option<i64>,
    pub updated_at_ms: i64,
    pub used_by: UsedBy,
    /// The model Auto last resolved to for this provider (W17.5).
    pub last_auto_model: Option<String>,
}

impl ProviderStatusRow {
    /// A connected account with no stored history yet.
    pub fn fresh(provider: &str, kind: CredentialKind, now: i64) -> Self {
        Self::new(provider, kind, now)
    }

    fn new(provider: &str, kind: CredentialKind, now: i64) -> Self {
        Self {
            provider: provider.to_owned(),
            credential_kind: kind,
            state: ProviderState::Connected,
            notice: None,
            last_limit_at_ms: None,
            connected_at_ms: None,
            updated_at_ms: now,
            used_by: UsedBy::default(),
            last_auto_model: None,
        }
    }

    /// Background work (World processing, method learning) waits on this account.
    pub fn holds_background_work(&self) -> bool {
        self.state == ProviderState::NeedsReconnect || self.notice.is_some()
    }

    fn same_state(&self, other: &Self) -> bool {
        self.credential_kind == other.credential_kind
            && self.state == other.state
            && self.notice == other.notice
            && self.last_limit_at_ms == other.last_limit_at_ms
            && self.connected_at_ms == other.connected_at_ms
            && self.used_by == other.used_by
    }
}

/// What happened on a provider's credential.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    /// An OAuth or key connect succeeded.
    Connected(CredentialKind),
    /// A request on this credential failed with this kind.
    Failure(ChatFailureKind),
    /// A request on this credential went through.
    Success,
    /// The credential was removed; `used_by` is what still points at it.
    Disconnected(UsedBy),
}

/// The §3.1 table. Pure: `None` means no row (deleted or never stored).
pub fn transition(
    row: Option<ProviderStatusRow>,
    provider: &str,
    trigger: Trigger,
    now: i64,
) -> Option<ProviderStatusRow> {
    let with_notice = |row: Option<ProviderStatusRow>, kind: NoticeKind| {
        let mut next =
            row.unwrap_or_else(|| ProviderStatusRow::new(provider, CredentialKind::None, now));
        next.state = ProviderState::Connected;
        next.notice = Some(ProviderNotice { kind, at_ms: now });
        if kind == NoticeKind::PlanLimit {
            next.last_limit_at_ms = Some(now);
        }
        next.updated_at_ms = now;
        Some(next)
    };
    match trigger {
        Trigger::Connected(kind) => {
            let mut next = row.unwrap_or_else(|| ProviderStatusRow::new(provider, kind, now));
            next.credential_kind = kind;
            next.state = ProviderState::Connected;
            next.notice = None;
            next.connected_at_ms = Some(now);
            next.used_by = UsedBy::default();
            next.updated_at_ms = now;
            Some(next)
        }
        Trigger::Failure(ChatFailureKind::PlanLimit) => with_notice(row, NoticeKind::PlanLimit),
        Trigger::Failure(ChatFailureKind::PlanNotEligible) => {
            with_notice(row, NoticeKind::PlanNotEligible)
        }
        Trigger::Failure(ChatFailureKind::ExtraUsageOff) => {
            with_notice(row, NoticeKind::ExtraUsageOff)
        }
        Trigger::Failure(ChatFailureKind::OutOfCredit) => with_notice(row, NoticeKind::OutOfCredit),
        Trigger::Failure(ChatFailureKind::NeedsReconnect) => {
            let mut next =
                row.unwrap_or_else(|| ProviderStatusRow::new(provider, CredentialKind::None, now));
            next.state = ProviderState::NeedsReconnect;
            next.notice = None;
            next.updated_at_ms = now;
            Some(next)
        }
        // Rate limits, outages, a gone model and everything else leave the account as it is.
        Trigger::Failure(_) => row,
        Trigger::Success => row.map(|mut next| {
            if next.state != ProviderState::Connected || next.notice.is_some() {
                next.state = ProviderState::Connected;
                next.notice = None;
                next.updated_at_ms = now;
            }
            next
        }),
        Trigger::Disconnected(used_by) => {
            if used_by.is_empty() {
                return None;
            }
            let mut next =
                row.unwrap_or_else(|| ProviderStatusRow::new(provider, CredentialKind::None, now));
            next.credential_kind = CredentialKind::None;
            next.state = ProviderState::Disconnected;
            next.notice = None;
            next.connected_at_ms = None;
            next.used_by = used_by;
            next.updated_at_ms = now;
            Some(next)
        }
    }
}

/// The view from a stored row and the credential Pi holds right now.
pub fn view(row: Option<&ProviderStatusRow>, live: CredentialKind) -> ProviderStatusView {
    match (row, live) {
        (Some(row), CredentialKind::None) if row.state == ProviderState::Disconnected => {
            ProviderStatusView {
                state: ProviderState::Disconnected,
                credential_kind: CredentialKind::None,
                notice: None,
                last_limit_at_ms: row.last_limit_at_ms,
                connected_at_ms: None,
                used_by: row.used_by,
            }
        }
        (_, CredentialKind::None) => ProviderStatusView::DISCONNECTED,
        (None, kind) => ProviderStatusView::from_credential(kind),
        (Some(row), kind) => ProviderStatusView {
            state: if row.state == ProviderState::Disconnected {
                ProviderState::Connected
            } else {
                row.state
            },
            credential_kind: kind,
            notice: row.notice,
            last_limit_at_ms: row.last_limit_at_ms,
            connected_at_ms: row.connected_at_ms,
            used_by: UsedBy::default(),
        },
    }
}

fn enum_text<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn enum_from<T: for<'de> Deserialize<'de>>(value: &str) -> Option<T> {
    serde_json::from_value(serde_json::Value::String(value.to_owned())).ok()
}

impl From<&ProviderStatusRow> for ProviderStatusRecord {
    fn from(row: &ProviderStatusRow) -> Self {
        Self {
            provider: row.provider.clone(),
            credential_kind: enum_text(&row.credential_kind),
            state: enum_text(&row.state),
            notice_kind: row.notice.map(|notice| enum_text(&notice.kind)),
            notice_at_ms: row.notice.map(|notice| notice.at_ms),
            last_limit_at_ms: row.last_limit_at_ms,
            connected_at_ms: row.connected_at_ms,
            updated_at_ms: row.updated_at_ms,
            used_by_json: serde_json::to_string(&row.used_by).unwrap_or_else(|_| "{}".into()),
            last_auto_model: row.last_auto_model.clone(),
        }
    }
}

impl ProviderStatusRow {
    /// A row that no longer parses reads as absent; the next transition replaces it.
    fn from_record(record: ProviderStatusRecord) -> Option<Self> {
        let notice = match (record.notice_kind.as_deref(), record.notice_at_ms) {
            (Some(kind), Some(at_ms)) => Some(ProviderNotice {
                kind: enum_from(kind)?,
                at_ms,
            }),
            _ => None,
        };
        Some(Self {
            credential_kind: enum_from(&record.credential_kind)?,
            state: enum_from(&record.state)?,
            notice,
            last_limit_at_ms: record.last_limit_at_ms,
            connected_at_ms: record.connected_at_ms,
            updated_at_ms: record.updated_at_ms,
            used_by: serde_json::from_str(&record.used_by_json).unwrap_or_default(),
            last_auto_model: record.last_auto_model,
            provider: record.provider,
        })
    }
}

fn map_store(error: crate::store::StoreError) -> CommandError {
    CommandError::internal(error.to_string())
}

pub fn load_all(state: &AppState) -> Result<BTreeMap<String, ProviderStatusRow>, CommandError> {
    Ok(state
        .store
        .provider_status_all()
        .map_err(map_store)?
        .into_iter()
        .filter_map(ProviderStatusRow::from_record)
        .map(|row| (row.provider.clone(), row))
        .collect())
}

pub fn load(state: &AppState, provider: &str) -> Result<Option<ProviderStatusRow>, CommandError> {
    Ok(state
        .store
        .provider_status_get(provider)
        .map_err(map_store)?
        .and_then(ProviderStatusRow::from_record))
}

/// One transition of a provider's stored status, read and written in one
/// transaction that never touches Auto's remembered model.
fn update_row(
    store: &crate::store::SqliteStore,
    provider: &str,
    change: impl FnOnce(Option<ProviderStatusRow>) -> Option<ProviderStatusRow>,
) -> Result<(Option<ProviderStatusRow>, Option<ProviderStatusRow>), CommandError> {
    let mut before = None;
    let mut after = None;
    store
        .provider_status_transition(provider, |record| {
            before = record.and_then(ProviderStatusRow::from_record);
            after = change(before.clone());
            after.as_ref().map(ProviderStatusRecord::from)
        })
        .map_err(map_store)?;
    Ok((before, after))
}

/// The credential Pi holds for a built-in or custom provider, read without secrets.
pub fn live_credential_kind(state: &AppState, provider: &str) -> CredentialKind {
    match state.pi_agent_data_dir() {
        Ok(agent_dir) => credential_kind_in(&agent_dir, provider),
        Err(_) => CredentialKind::None,
    }
}

fn credential_kind_in(agent_dir: &std::path::Path, provider: &str) -> CredentialKind {
    if crate::providers::custom::is_custom_id(provider) {
        let saved = crate::providers::catalog::provider_credential_kind(agent_dir, provider)
            .ok()
            .flatten()
            .is_some();
        return match crate::providers::custom::find(agent_dir, provider) {
            Ok(Some(custom)) if saved => custom_kind(&custom.base_url),
            _ => CredentialKind::None,
        };
    }
    match crate::providers::catalog::provider_credential_kind(agent_dir, provider)
        .ok()
        .flatten()
        .as_deref()
    {
        Some("oauth") => CredentialKind::Plan,
        Some("api_key") => CredentialKind::ApiKey,
        Some("environment") => CredentialKind::Environment,
        _ => CredentialKind::None,
    }
}

/// A custom endpoint on this Mac reads as local; anything else as custom.
pub fn custom_kind(base_url: &str) -> CredentialKind {
    let local = reqwest::Url::parse(base_url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .is_some_and(|host| matches!(host.as_str(), "localhost" | "127.0.0.1" | "[::1]" | "::1"));
    if local {
        CredentialKind::Local
    } else {
        CredentialKind::Custom
    }
}

/// Applies one trigger, persists it and emits `provider-status` when the view changed.
pub fn record(state: &AppState, provider: &str, trigger: Trigger) -> Result<(), CommandError> {
    let released = record_in(&state.store, &state.pi_agent_data_dir()?, provider, trigger)?;
    if released {
        // Held World processing and method learning resume.
        state.graph.wake();
    }
    Ok(())
}

/// `record` for host work that holds only the store and Pi's directory
/// (background inference). Returns whether held background work may resume.
pub fn record_in(
    store: &crate::store::SqliteStore,
    agent_dir: &std::path::Path,
    provider: &str,
    trigger: Trigger,
) -> Result<bool, CommandError> {
    let now = crate::commands::now_ms();
    if matches!(trigger, Trigger::Connected(_)) {
        // A new credential may offer models the old one had lost.
        store.gone_models_clear(provider).map_err(map_store)?;
    }
    let (current, next) = update_row(store, provider, |current| {
        transition(current, provider, trigger, now)
    })?;
    let changed = match (&current, &next) {
        (None, None) => false,
        (Some(old), Some(new)) => !old.same_state(new),
        _ => true,
    };
    if !changed {
        return Ok(false);
    }
    emit(
        provider,
        view(next.as_ref(), credential_kind_in(agent_dir, provider)),
    );
    Ok(current
        .as_ref()
        .is_some_and(ProviderStatusRow::holds_background_work)
        && !next
            .as_ref()
            .is_some_and(ProviderStatusRow::holds_background_work))
}

/// Chat and scheduled outcomes on one credential (`ChatEventSink` hook). The
/// turn already retried a gone model once; a second one is pruned too.
pub fn record_chat_outcome(
    state: &AppState,
    model: &crate::providers::catalog::ExecutionModelLock,
    failure: Option<ChatFailureKind>,
) {
    let provider = model.provider.as_str();
    let result = match failure {
        None => record(state, provider, Trigger::Success),
        Some(ChatFailureKind::ModelGone) => {
            crate::providers::auto::prune_gone(state, &model.profile_id);
            Ok(())
        }
        Some(kind) => record(state, provider, Trigger::Failure(kind)),
    };
    if result.is_err() {
        eprintln!("Loupist could not record a provider status change");
    }
}

/// Puts stored state on a catalog view: each provider's status and the
/// pending model notice. A credential saved before its row existed gets one
/// now, so every connected card has a "since" date.
pub fn decorate(
    state: &AppState,
    catalog: &mut crate::providers::catalog::ModelCatalogView,
) -> Result<(), CommandError> {
    let mut rows = load_all(state)?;
    let now = crate::commands::now_ms();
    let unsettled = |row: Option<&ProviderStatusRow>| {
        row.is_none_or(|row| {
            row.connected_at_ms.is_none() || row.state == ProviderState::Disconnected
        })
    };
    let mut settle = |id: &str, live: CredentialKind| {
        if !matches!(live, CredentialKind::None | CredentialKind::Environment)
            && unsettled(rows.get(id))
        {
            // Settle against the stored row, not this view's snapshot.
            let settled = update_row(&state.store, id, |current| {
                if !unsettled(current.as_ref()) {
                    return current;
                }
                let mut row = current.unwrap_or_else(|| ProviderStatusRow::new(id, live, now));
                row.credential_kind = live;
                if row.state == ProviderState::Disconnected {
                    row.state = ProviderState::Connected;
                    row.used_by = UsedBy::default();
                }
                row.connected_at_ms.get_or_insert(now);
                Some(row)
            });
            if let Ok((_, Some(row))) = settled {
                rows.insert(id.to_owned(), row);
            }
        }
        view(rows.get(id), live)
    };
    for option in &mut catalog.providers {
        option.status = settle(option.id, option.status.credential_kind);
    }
    for custom in &mut catalog.custom_providers {
        custom.status = settle(&custom.id, custom.status.credential_kind);
    }
    catalog.model_notice = state
        .store
        .model_notice_get()
        .map_err(map_store)?
        .map(|notice| crate::providers::catalog::ModelNoticeView {
            id: notice.id,
            kind: notice.kind,
            model: notice.model,
        });
    Ok(())
}

/// One background attempt per this window while an account is held, so a plan
/// that recovered on its own clears its hold (providers spec §3.1).
pub const PROBE_WINDOW_MS: i64 = 30 * 60 * 1000;

/// Whether background work (World processing, method learning) may use a provider.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackgroundHold {
    Clear,
    /// Waits for the user (reconnect, or a plan that cannot be shared) or for
    /// the next back-off window.
    Held,
    /// Held, but this window's one probe attempt is still unclaimed.
    ProbeDue,
}

impl ProviderStatusRow {
    /// Notices that can clear without the user: a plan limit resets, extra
    /// usage is turned on, credit is added.
    fn probe_may_clear(&self) -> bool {
        self.state == ProviderState::Connected
            && self.notice.is_some_and(|notice| {
                matches!(
                    notice.kind,
                    NoticeKind::PlanLimit | NoticeKind::ExtraUsageOff | NoticeKind::OutOfCredit
                )
            })
    }
}

pub fn background_hold(
    store: &crate::store::SqliteStore,
    provider: &str,
    now: i64,
) -> BackgroundHold {
    let Some(row) = store
        .provider_status_get(provider)
        .ok()
        .flatten()
        .and_then(ProviderStatusRow::from_record)
    else {
        return BackgroundHold::Clear;
    };
    if !row.holds_background_work() {
        return BackgroundHold::Clear;
    }
    if row.probe_may_clear() && probe_due_at(store, &row) <= now {
        BackgroundHold::ProbeDue
    } else {
        BackgroundHold::Held
    }
}

/// Claims this window's single probe; false when another caller already did.
pub fn claim_probe(store: &crate::store::SqliteStore, provider: &str, now: i64) -> bool {
    store
        .provider_probe_claim(provider, now, PROBE_WINDOW_MS)
        .unwrap_or(false)
}

/// A window starts when the notice was recorded or when the last probe ran.
fn probe_due_at(store: &crate::store::SqliteStore, row: &ProviderStatusRow) -> i64 {
    let notice_at = row.notice.map_or(0, |notice| notice.at_ms);
    let probed_at = store
        .provider_probe_at(&row.provider)
        .ok()
        .flatten()
        .unwrap_or(notice_at);
    notice_at.max(probed_at).saturating_add(PROBE_WINDOW_MS)
}

/// When the next probe may start, for the background loop's wait.
pub fn next_probe_at(store: &crate::store::SqliteStore) -> Option<i64> {
    store
        .provider_status_all()
        .ok()?
        .into_iter()
        .filter_map(ProviderStatusRow::from_record)
        .filter(ProviderStatusRow::probe_may_clear)
        .map(|row| probe_due_at(store, &row))
        .min()
}

/// What still points at a provider: schedules, the Chat default and Loupist World models.
pub fn used_by(state: &AppState, provider: &str) -> UsedBy {
    let prefix = format!("{provider}/");
    let mut used = UsedBy {
        chat_default: crate::scoped_config::resolve(&state.artifacts.app_data_dir, None)
            .ok()
            .and_then(|config| config.chat.model_profile_id)
            .is_some_and(|id| id.starts_with(&prefix)),
        ..UsedBy::default()
    };
    let Ok(loupists) = state.store.list_loupists() else {
        return used;
    };
    for loupist in loupists {
        if let Ok(tasks) = state.store.scheduled_tasks_list_with_latest(&loupist.id) {
            used.schedules += tasks
                .iter()
                .filter(|item| item.task.model_profile_id.starts_with(&prefix))
                .count() as u32;
        }
        if let Ok(connection) = state.store.lock_loupist(&loupist.id) {
            if crate::knowledge::load_settings(&connection, &loupist.id)
                .is_ok_and(|settings| settings.helper_provider.as_deref() == Some(provider))
            {
                used.world_models += 1;
            }
        }
    }
    used
}

pub fn install(app: &tauri::AppHandle) {
    let _ = APP.set(app.clone());
}

#[derive(Clone, Serialize)]
struct StatusEvent<'a> {
    provider: &'a str,
    status: ProviderStatusView,
}

fn emit(provider: &str, status: ProviderStatusView) {
    if let Some(app) = APP.get() {
        let _ = app.emit(EVENT, StatusEvent { provider, status });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connected(now: i64) -> Option<ProviderStatusRow> {
        transition(
            None,
            "openai",
            Trigger::Connected(CredentialKind::Plan),
            now,
        )
    }

    #[test]
    fn connect_sets_the_date_and_clears_any_notice() {
        let row = connected(10).unwrap();
        assert_eq!(row.state, ProviderState::Connected);
        assert_eq!(row.connected_at_ms, Some(10));
        let limited = transition(
            Some(row),
            "openai",
            Trigger::Failure(ChatFailureKind::PlanLimit),
            20,
        );
        let reconnected = transition(
            limited,
            "openai",
            Trigger::Connected(CredentialKind::ApiKey),
            30,
        )
        .unwrap();
        assert_eq!(reconnected.notice, None);
        assert_eq!(reconnected.credential_kind, CredentialKind::ApiKey);
        assert_eq!(reconnected.connected_at_ms, Some(30));
        // The dated "last reached its limit" note survives the reconnect.
        assert_eq!(reconnected.last_limit_at_ms, Some(20));
    }

    #[test]
    fn plan_limit_holds_background_work_until_a_request_goes_through() {
        let limited = transition(
            connected(1),
            "openai",
            Trigger::Failure(ChatFailureKind::PlanLimit),
            5,
        )
        .unwrap();
        assert_eq!(limited.state, ProviderState::Connected);
        assert_eq!(
            limited.notice,
            Some(ProviderNotice {
                kind: NoticeKind::PlanLimit,
                at_ms: 5
            })
        );
        assert_eq!(limited.last_limit_at_ms, Some(5));
        assert!(limited.holds_background_work());
        let cleared = transition(Some(limited), "openai", Trigger::Success, 9).unwrap();
        assert_eq!(cleared.notice, None);
        assert_eq!(cleared.last_limit_at_ms, Some(5));
        assert!(!cleared.holds_background_work());
    }

    #[test]
    fn each_failure_kind_follows_the_state_table() {
        for (kind, state, notice) in [
            (
                ChatFailureKind::PlanNotEligible,
                ProviderState::Connected,
                Some(NoticeKind::PlanNotEligible),
            ),
            (
                ChatFailureKind::ExtraUsageOff,
                ProviderState::Connected,
                Some(NoticeKind::ExtraUsageOff),
            ),
            (
                ChatFailureKind::OutOfCredit,
                ProviderState::Connected,
                Some(NoticeKind::OutOfCredit),
            ),
            (
                ChatFailureKind::NeedsReconnect,
                ProviderState::NeedsReconnect,
                None,
            ),
        ] {
            let next = transition(connected(1), "anthropic", Trigger::Failure(kind), 4).unwrap();
            assert_eq!(next.state, state, "{kind:?}");
            assert_eq!(next.notice.map(|notice| notice.kind), notice, "{kind:?}");
            assert!(next.holds_background_work(), "{kind:?}");
        }
        // Outages, short rate limits and a gone model leave the account unchanged.
        for kind in [
            ChatFailureKind::ProviderUnavailable,
            ChatFailureKind::RateLimited,
            ChatFailureKind::ModelGone,
            ChatFailureKind::Offline,
        ] {
            assert_eq!(
                transition(connected(1), "openai", Trigger::Failure(kind), 4),
                connected(1),
                "{kind:?}"
            );
            assert_eq!(transition(None, "openai", Trigger::Failure(kind), 4), None);
        }
        // A success on a credential that needed reconnecting means it works again.
        let reconnect = transition(
            connected(1),
            "openai",
            Trigger::Failure(ChatFailureKind::NeedsReconnect),
            2,
        );
        assert_eq!(
            transition(reconnect, "openai", Trigger::Success, 3)
                .unwrap()
                .state,
            ProviderState::Connected
        );
        assert_eq!(transition(None, "openai", Trigger::Success, 3), None);
    }

    #[test]
    fn disconnect_deletes_the_row_unless_something_still_uses_the_account() {
        assert_eq!(
            transition(
                connected(1),
                "openai",
                Trigger::Disconnected(UsedBy::default()),
                2
            ),
            None
        );
        let used = UsedBy {
            schedules: 2,
            chat_default: false,
            world_models: 1,
        };
        let kept = transition(connected(1), "openai", Trigger::Disconnected(used), 2).unwrap();
        assert_eq!(kept.state, ProviderState::Disconnected);
        assert_eq!(kept.credential_kind, CredentialKind::None);
        assert_eq!(kept.used_by, used);
        let shown = view(Some(&kept), CredentialKind::None);
        assert_eq!(shown.state, ProviderState::Disconnected);
        assert_eq!(shown.used_by.schedules, 2);
        // Connecting again turns it back into an ordinary connected account.
        let again = transition(
            Some(kept),
            "openai",
            Trigger::Connected(CredentialKind::Plan),
            3,
        );
        assert_eq!(again.unwrap().used_by, UsedBy::default());
    }

    #[test]
    fn the_view_follows_the_live_credential() {
        assert_eq!(
            view(None, CredentialKind::None),
            ProviderStatusView::DISCONNECTED
        );
        let fresh = view(None, CredentialKind::ApiKey);
        assert_eq!(fresh.state, ProviderState::Connected);
        assert_eq!(fresh.credential_kind, CredentialKind::ApiKey);
        let limited = transition(
            connected(1),
            "openai",
            Trigger::Failure(ChatFailureKind::PlanLimit),
            5,
        )
        .unwrap();
        let shown = view(Some(&limited), CredentialKind::Plan);
        assert_eq!(shown.notice.unwrap().kind, NoticeKind::PlanLimit);
        assert_eq!(shown.connected_at_ms, Some(1));
        // A stored row for a credential that is gone reads as disconnected.
        assert_eq!(
            view(Some(&limited), CredentialKind::None),
            ProviderStatusView::DISCONNECTED
        );
        let json = serde_json::to_value(shown).unwrap();
        assert_eq!(json["state"], "connected");
        assert_eq!(json["credential_kind"], "plan");
        assert_eq!(json["notice"]["kind"], "plan_limit");
        assert_eq!(json["notice"]["at_ms"], 5);
        assert_eq!(json["used_by"]["chat_default"], false);
    }

    #[test]
    fn chat_outcomes_hold_background_work_on_a_plan_limit_until_a_success() {
        let directory = tempfile::tempdir().unwrap();
        let state = AppState::for_test(directory.path().join("app"));
        let lock = crate::providers::catalog::ExecutionModelLock {
            profile_id: "openai/gpt-6.1-sol".into(),
            name: "GPT-6.1 Sol".into(),
            provider: "openai".into(),
            model: "gpt-6.1-sol".into(),
            thinking_level: "medium".into(),
            run_options: BTreeMap::new(),
        };
        let held = |state: &AppState| {
            background_hold(&state.store, "openai", crate::commands::now_ms())
                != BackgroundHold::Clear
        };
        record(&state, "openai", Trigger::Connected(CredentialKind::Plan)).unwrap();
        record_chat_outcome(&state, &lock, Some(ChatFailureKind::PlanLimit));
        assert!(held(&state));
        let row = load(&state, "openai").unwrap().unwrap();
        assert_eq!(row.notice.unwrap().kind, NoticeKind::PlanLimit);
        // A rate limit or an outage neither holds nor releases anything.
        record_chat_outcome(&state, &lock, Some(ChatFailureKind::ProviderUnavailable));
        assert!(held(&state));
        record_chat_outcome(&state, &lock, None);
        assert!(!held(&state));
        assert_eq!(
            load(&state, "openai").unwrap().unwrap().last_limit_at_ms,
            row.last_limit_at_ms
        );
        // Nothing uses the account after a disconnect: the row goes away.
        record(
            &state,
            "openai",
            Trigger::Disconnected(used_by(&state, "openai")),
        )
        .unwrap();
        assert!(load(&state, "openai").unwrap().is_none());
    }

    #[test]
    fn a_held_plan_gets_one_background_probe_per_window_and_a_sign_in_to_fix_none() {
        let directory = tempfile::tempdir().unwrap();
        let state = AppState::for_test(directory.path().join("app"));
        let store = &state.store;
        record(&state, "openai", Trigger::Connected(CredentialKind::Plan)).unwrap();
        record(
            &state,
            "openai",
            Trigger::Failure(ChatFailureKind::PlanLimit),
        )
        .unwrap();
        let limited_at = load(&state, "openai")
            .unwrap()
            .unwrap()
            .notice
            .unwrap()
            .at_ms;
        // The first window starts at the limit itself.
        assert_eq!(
            background_hold(store, "openai", limited_at),
            BackgroundHold::Held
        );
        assert_eq!(next_probe_at(store), Some(limited_at + PROBE_WINDOW_MS));
        let now = limited_at + PROBE_WINDOW_MS;
        assert_eq!(
            background_hold(store, "openai", now),
            BackgroundHold::ProbeDue
        );
        assert!(claim_probe(store, "openai", now));
        // The window's one attempt is spent, also for a concurrent caller.
        assert!(!claim_probe(store, "openai", now + 1));
        assert_eq!(
            background_hold(store, "openai", now + 1),
            BackgroundHold::Held
        );
        assert_eq!(next_probe_at(store), Some(now + PROBE_WINDOW_MS));
        assert_eq!(
            background_hold(store, "openai", now + PROBE_WINDOW_MS),
            BackgroundHold::ProbeDue
        );
        // A sign-in to reconnect waits for the user; no probe can fix it.
        record(
            &state,
            "openai",
            Trigger::Failure(ChatFailureKind::NeedsReconnect),
        )
        .unwrap();
        assert_eq!(
            background_hold(store, "openai", now + 10 * PROBE_WINDOW_MS),
            BackgroundHold::Held
        );
        assert_eq!(next_probe_at(store), None);
    }

    #[test]
    fn rows_round_trip_through_the_store_record() {
        let mut row = transition(
            connected(1),
            "openai",
            Trigger::Failure(ChatFailureKind::ExtraUsageOff),
            4,
        )
        .unwrap();
        row.last_auto_model = Some("openai/gpt-6.1-sol".into());
        let record = ProviderStatusRecord::from(&row);
        assert_eq!(record.notice_kind.as_deref(), Some("extra_usage_off"));
        assert_eq!(ProviderStatusRow::from_record(record), Some(row));
        assert_eq!(
            custom_kind("http://localhost:11434/v1"),
            CredentialKind::Local
        );
        assert_eq!(
            custom_kind("https://api.upstage.ai/v1"),
            CredentialKind::Custom
        );
    }
}
