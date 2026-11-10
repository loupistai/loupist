/**
 * Event names Rust emits to the renderer (`src-tauri/src/events.rs`). The bridge
 * subscribes only through these; `events.test.ts` keeps both lists equal.
 */
export const BRIDGE_EVENTS = {
  menuAction: "menu-action",
  inboxChanged: "inbox-changed",
  inboxOpen: "inbox-open",
  providerStatus: "provider-status",
  learningReviewFinished: "learning-review-finished",
  worldSchemaChanged: "world-schema-changed",
  deepLinkImport: "deep-link-import",
  watchChanged: "watch-changed",
} as const;

export type BridgeEventName = (typeof BRIDGE_EVENTS)[keyof typeof BRIDGE_EVENTS];
