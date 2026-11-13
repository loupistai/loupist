// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

/**
 * Every file the app saves for the user: Map images and replays, chat transcripts and data tables.
 */
export type ExportKind = "png" | "mp4" | "webm" | "md" | "csv";

export type ExportRequest = {
  suggestedName: string,
  bytes: Array<number>,
  kind: ExportKind,
  /**
   * The Save dialog's title in the interface language, as the renderer
   * words it (like `pick_file`'s).
   */
  title: string,
};
