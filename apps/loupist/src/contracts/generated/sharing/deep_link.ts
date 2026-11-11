// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

/** What the renderer hears about an arriving link. */
export type DeepLinkEvent = { "status": "ok", source: string } | { "status": "bad" };
