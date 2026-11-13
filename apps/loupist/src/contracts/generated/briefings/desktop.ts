// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

export type InboxChanged = { loupistId: string };

export type InboxOpen = { loupistId: string, runId: string | null };

export type ResidentRequest = { keepRunning: boolean };

export type ResidentSettings = {
  keepRunning: boolean,
  notificationPermissionAsked: boolean,
  /** None follows the automatic default; Some preserves the user's choice. */
  keepRunningChoice: boolean | null,
};
