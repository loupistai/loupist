// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

export type ResetCounts = {
  principles: number,
  /** Whole Loupist-origin pages that disappear. */
  pages: number,
  /**
   * Loupist-origin sections retired, and Loupist-written summaries
   * cleared, on pages the user keeps.
   */
  sections: number,
  names: number,
  relationships: number,
  sources: number,
};
