// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

export type HandoffStatus = "created" | "running" | "done" | "failed" | "stopped";

export type HandoffStopReason =
  | "before_update"
  | "origin_unavailable"
  | "invalid_authority"
  | "interrupted";

export type LinkClaim = { from: string, relation: string, to: string };

export type LinkPath = { seed: string, far: string, path: Array<LinkClaim> };
