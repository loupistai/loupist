// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

export type Flag = "instruction" | "tool" | "secret" | "link" | "hidden";

export type ItemFinding = {
  flag: Flag,
  /** 1-based lines of the item's Markdown. */
  lines: Array<number>,
};

export type PrivateClass = "credential" | "account" | "holding" | "personal";
