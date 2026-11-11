// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

export type AssetClass =
  | "equity"
  | "crypto"
  | "fund"
  | "commodity"
  | "rates_fx"
  | "real_asset"
  | "prediction"
  | "private";

/**
 * First match wins: the user's choice, the market code, a first-refresh
 * lookup, the World entity's class, then equity.
 */
export type ClassSource = "user" | "market" | "lookup" | "world" | "default";
