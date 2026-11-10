// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { CommandErrorCode } from "./app/error_code";

/** A failed command, as the renderer receives it. */
export type CommandError = { code: CommandErrorCode, message: string };

export type LoupistAvailability =
  | { "status": "available" }
  | { "status": "workspace_unavailable" }
  | { "status": "earlier_version" }
  | { "status": "newer_version" }
  | { "status": "recovery_required", reason: LoupistRecoveryReason, action: LoupistRecoveryAction };

export type LoupistRecoveryAction = "recover_memory";

export type LoupistRecoveryReason = "interrupted_memory_update";
