// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { MaintenanceBlocker } from "./runs/maintenance";

export type UpdateInstallOutcome = "blocked" | "cancelled";

export type UpdateInstallRequest = { offer_id: string };

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
