// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { MaintenanceBlocker } from "./runs/maintenance";

/**
 * What the install confirmation says, worded by the renderer (like the Save
 * panel's title). Each part is bounded; anything unusable falls back to
 * the English wording.
 */
export type UpdateConfirmationText = {
  title: string,
  message: string,
  install: string,
  cancel: string,
};

export type UpdateInstallOutcome = "blocked" | "cancelled";

export type UpdateInstallRequest = {
  offer_id: string,
  /**
   * The confirmation alert's words in the interface language. Only text:
   * the host still decides what is installed and adds the versions itself.
   */
  confirmation: UpdateConfirmationText,
};

export type UpdateInstallResult = {
  outcome: UpdateInstallOutcome,
  blockers: Array<MaintenanceBlocker>,
};

export type UpdateOfferDto = {
  offer_id: string,
  version: string,
  notes: string | null,
  published_at: string | null,
};

export type UpdatePhase =
  | "idle"
  | "checking"
  | "confirming"
  | "downloading"
  | "installing"
  | "restarting";

export type UpdateStateDto = {
  supported: boolean,
  current_version: string,
  phase: UpdatePhase,
  offer: UpdateOfferDto | null,
  downloaded_bytes: number,
  total_bytes: number | null,
  last_checked_at_ms: number | null,
  next_auto_check_at_ms: number | null,
  error: string | null,
  blockers: Array<MaintenanceBlocker>,
};
