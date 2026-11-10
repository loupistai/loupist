// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { ApprovalDecision } from "../chat/approvals";

export type ApprovalDecideRequest = {
  loupist_id: string,
  request_id: string,
  decision: ApprovalDecision,
};
