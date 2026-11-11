// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

/** Ordered so that the most pressing state of several Sources wins. */
export type FollowedState = "none" | "off" | "done" | "failed" | "waiting" | "queued" | "running";

export type FollowedStatus = {
  state: FollowedState,
  /** How many assets the Sources list. */
  assets: number,
  /** When the oldest unfinished read was queued. */
  queuedAtMs?: number,
};
