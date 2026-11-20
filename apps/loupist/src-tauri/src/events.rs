//! Events Rust emits to the renderer.
//!
//! Every `emit` names one of these consts, and the renderer subscribes through
//! the matching `BRIDGE_EVENTS` entry in `apps/loupist/src/bridge/events.ts`.
//! `apps/loupist/src/bridge/events.test.ts` keeps the two lists equal.
//! Payloads are typed so their TypeScript is generated with the bridge types.

use serde::Serialize;

/// Payload: the menu item id (`new-chat`, `settings`, ...).
pub(crate) const MENU_ACTION: &str = "menu-action";
/// Payload: [`crate::briefings::desktop::InboxChanged`].
pub(crate) const INBOX_CHANGED: &str = "inbox-changed";
/// Payload: [`crate::briefings::desktop::InboxOpen`].
pub(crate) const INBOX_OPEN: &str = "inbox-open";
/// Payload: [`LearningReviewFinished`].
pub(crate) const LEARNING_REVIEW_FINISHED: &str = "learning-review-finished";
/// Payload: [`WorldSchemaChanged`].
pub(crate) const WORLD_SCHEMA_CHANGED: &str = "world-schema-changed";
/// Payload: [`crate::sharing::deep_link::DeepLinkEvent`]. Only builds that
/// receive OS deep links emit it.
#[cfg(all(any(target_os = "macos", windows), not(feature = "e2e")))]
pub(crate) const DEEP_LINK_IMPORT: &str = "deep-link-import";
/// Payload: [`crate::commands::legacy_holdings::LegacyHoldingsStatus`].
pub(crate) const LEGACY_HOLDINGS_CHANGED: &str = "legacy-holdings-changed";
/// A followed asset or a Loupist's stored price changed. No payload.
pub(crate) const ASSETS_CHANGED: &str = "assets-changed";
/// Payload: [`FxChanged`].
pub(crate) const FX_CHANGED: &str = "fx-changed";
/// Payload: [`crate::providers::status::StatusEvent`].
pub(crate) const PROVIDER_STATUS: &str = "provider-status";
/// No payload. Background discovery published a provider's models.
pub(crate) const MODEL_CATALOG_CHANGED: &str = "model-catalog-changed";

/// A learning review finished, failed or was deferred.
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "LearningReviewFinishedEvent"))]
#[serde(rename_all = "camelCase")]
pub(crate) struct LearningReviewFinished {
    pub loupist_id: String,
    pub review: crate::knowledge::learning::Review,
}

/// A Loupist's World kinds changed (added by processing, undone or removed).
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "WorldSchemaChangedEvent"))]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorldSchemaChanged {
    pub loupist_id: String,
}

/// A rate refresh, grant or preference changed what a window may display.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "FxChangedEvent"))]
#[serde(rename_all = "camelCase")]
pub(crate) struct FxChanged {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub revision: Option<String>,
    pub state: FxDisplayState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub(crate) enum FxDisplayState {
    // Part of the invalidation contract. The shared refresh itself has no
    // preference and therefore only emits ready or unavailable on settlement.
    #[allow(dead_code)]
    Unset,
    /// A grant or preference changed independently of rates.
    Invalidated,
    Ready,
    Waiting,
    Unavailable,
}
