// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { JsonValue } from "../external";

export type ResearchHandoffReceipt = { chatId: string, handoffId: string };

export type ResearchHandoffRequest = {
  loupistId: string,
  title: string,
  question: string,
  brief?: string,
  pins?: Array<JsonValue>,
  requestKey: string,
};
