// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { LegacyHoldingsCounts } from "../store/legacy_holdings";

export type LegacyHoldingsState = "none" | "pending" | "exported" | "failed";

export type LegacyHoldingsStatus = {
  state: LegacyHoldingsState,
  exportedAtMs?: number,
  counts?: LegacyHoldingsCounts,
  dismissed: boolean,
};
