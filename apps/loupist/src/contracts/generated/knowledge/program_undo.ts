// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

/** What Undo all did, or would do with `preview`. */
export type ProgramUndo = {
  /**
   * Runs this call undid. A run already undone, or one that filed
   * nothing, is not counted.
   */
  runs: number,
  /** Lens ops restored. */
  undone: number,
  /** Sections a later change kept, each counted once across runs. */
  kept: number,
};
