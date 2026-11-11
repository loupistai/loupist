// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

/** "Holding up" is stored as `holding`; the label never reads as ownership. */
export type Verdict = "holding" | "weakening" | "challenged" | "broken";
