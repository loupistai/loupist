// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

export type ApprovalDecision = "once" | "always" | "deny";

export type ApprovalState = "waiting" | "allowed_once" | "always_allowed" | "declined" | "expired";

/** One approval the reply asked for, shown as a card at its step. */
export type ChatApproval = {
  requestId: string,
  /** The progress step that waits; absent when the call carried no tool call id. */
  itemId?: string,
  connectionId: string,
  connectionName: string,
  toolName: string,
  /** Plain values from the call's arguments (company, period), never raw JSON. */
  target?: string,
  state: ApprovalState,
  requestedAtMs: number,
};
