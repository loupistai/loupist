// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

export type RunActivity = {
  run_id: string,
  loupist_id: string,
  kind: string,
  target: string,
  started_at_ms: number,
};
