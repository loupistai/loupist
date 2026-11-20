/**
 * Event names Rust emits to the renderer (`src-tauri/src/events.rs`). The bridge
 * subscribes only through these; `events.test.ts` keeps both lists equal.
 */
export const BRIDGE_EVENTS = {
  menuAction: "menu-action",
  inboxChanged: "inbox-changed",
  inboxOpen: "inbox-open",
  providerStatus: "provider-status",
  modelCatalogChanged: "model-catalog-changed",
  learningReviewFinished: "learning-review-finished",
  worldSchemaChanged: "world-schema-changed",
  deepLinkImport: "deep-link-import",
  legacyHoldingsChanged: "legacy-holdings-changed",
  assetsChanged: "assets-changed",
  fxChanged: "fx-changed",
} as const;

export type BridgeEventName = (typeof BRIDGE_EVENTS)[keyof typeof BRIDGE_EVENTS];
