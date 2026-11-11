// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

export type PackOpenRequest = { source: PackSource };

export type PackSource =
  | { url: string }
  | { zip: boolean }
  | { folder: boolean }
  | { pending: boolean }
  | { gallery: string };
