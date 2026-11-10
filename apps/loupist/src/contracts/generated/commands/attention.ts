// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

export type AttentionItem = {
  kind: AttentionKind,
  loupistId: string,
  loupistName: string,
  /** The chat to open for an approval or a question. */
  chatId?: string,
  /** The Journal entry for a check. */
  recordId?: string,
  /**
   * What the row names: a tool, the chat's title for a question, a check claim, or empty for briefings.
   */
  title: string,
  count?: number,
  dueDate?: string,
  atMs: number,
};

export type AttentionKind =
  | "approval"
  | "question"
  | "lineCrossed"
  | "briefingConflict"
  | "unreadResults"
  | "watchAlerts"
  | "checkDue";

export type NotifyRequest = {
  loupistId: string,
  chatId: string,
  /** The localized title the renderer chose ("Quality Compounder finished"). */
  title: string,
};
