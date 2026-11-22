// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

export type DesktopSettings = {
  loginItem: LoginItemStatus,
  loginItemTurnedOff: boolean,
  keepRunning: boolean,
  notificationPermissionAsked: boolean,
  /** None follows the automatic default; Some preserves the user's choice. */
  keepRunningChoice: boolean | null,
  loginHintDismissed?: boolean,
};

export type LoginItemRequest = { enabled: boolean };

export type LoginItemStatus = "off" | "on" | "requiresApproval" | "error" | "unavailable";
