// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { ChatMessageDto, ChatSaveState } from "./commands/types";
import type { Review } from "./knowledge/learning";

/**
 * A reply's own Memory save moved on: a new wait, or its receipt once the
 * save is applied or skipped. Its stream ended when the answer was stored.
 */
export type ChatSaveChangedEvent = {
  loupist_id: string,
  thread_id: string,
  message_id: string,
  run_id: string,
  /** The wait, while the save has not resolved. */
  pending?: ChatSaveState,
  /** The stored reply with its receipt, once the save resolved. */
  message?: ChatMessageDto,
};

/** A rate refresh, grant or preference changed what a window may display. */
export type FxChangedEvent = { revision?: string, state: FxDisplayState };

export type FxDisplayState = "unset" | "invalidated" | "ready" | "waiting" | "unavailable";

/** A learning review finished, failed or was deferred. */
export type LearningReviewFinishedEvent = { loupistId: string, review: Review };

/** A Loupist's World kinds changed (added by processing, undone or removed). */
export type WorldSchemaChangedEvent = { loupistId: string };
