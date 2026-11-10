// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

/** Why a provider cannot run World processing now. */
export type UnreadyReason =
  | "isolation_unsupported"
  | "runtime_missing"
  | "choose_connected_model"
  | "not_connected"
  | "account_waiting";
