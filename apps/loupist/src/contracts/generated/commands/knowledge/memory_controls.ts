// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { ResetCounts } from "../../knowledge/memory_reset";

export type PauseRequest = { loupistId: string, recordId: string, paused: boolean };

export type ResetRequest = { loupistId: string, confirm: boolean, expected: ResetCounts };
