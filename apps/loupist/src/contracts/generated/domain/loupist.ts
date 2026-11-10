// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

/**
 * The template a Loupist was made from (onboarding and New Loupist cards).
 * Kept so a class Loupist's watchers start on for its class and its first
 * morning message can name the connections it works best with.
 */
export type LoupistTemplate =
  | "portfolio"
  | "semiconductors"
  | "dividends"
  | "technology"
  | "crypto"
  | "macro"
  | "commodities"
  | "realAssets"
  | "custom";

/** How a Loupist talks. Presentation only: it never changes evidence requirements. */
export type LoupistTone = "careful" | "balanced" | "direct";
