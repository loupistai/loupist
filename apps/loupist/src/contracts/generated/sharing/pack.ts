// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

export type License =
  | "CC-BY-4.0"
  | "CC-BY-SA-4.0"
  | "CC0-1.0"
  | "CC-BY-NC-4.0"
  | "CC-BY-NC-SA-4.0"
  | "LicenseRef-Loupist-Personal";

export type LicenseConflict = { code: string, items: Array<LicensedItem> };

export type LicensedItem = { key: string, name: string, license: License };

export type PackKind = "loupist" | "method" | "principle";

/** Why a pack cannot be added. The renderer maps each to its own sentence. */
export type Rejection = "notPack" | "newer" | "tooLarge" | "unsafe" | "damaged" | "spoof";
