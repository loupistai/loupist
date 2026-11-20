// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { Review } from "./knowledge/learning";

/** A rate refresh, grant or preference changed what a window may display. */
export type FxChangedEvent = { revision?: string, state: FxDisplayState };

export type FxDisplayState = "unset" | "invalidated" | "ready" | "waiting" | "unavailable";

/** A learning review finished, failed or was deferred. */
export type LearningReviewFinishedEvent = { loupistId: string, review: Review };

/** A Loupist's World kinds changed (added by processing, undone or removed). */
export type WorldSchemaChangedEvent = { loupistId: string };
