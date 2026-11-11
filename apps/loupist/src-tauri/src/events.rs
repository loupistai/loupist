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
/// Payload: [`WatchChanged`].
pub(crate) const WATCH_CHANGED: &str = "watch-changed";
/// Payload: [`crate::providers::status::StatusEvent`].
pub(crate) const PROVIDER_STATUS: &str = "provider-status";

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

/// A Loupist's watch settings or events changed.
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "WatchChangedEvent"))]
#[serde(rename_all = "camelCase")]
pub(crate) struct WatchChanged {
    pub loupist_id: String,
}
