// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { ChatThreadDto } from "./commands/types";
import type { Run } from "./scheduled_tasks/types";

export type WatchSnapshot = { run: Run, thread: ChatThreadDto };
