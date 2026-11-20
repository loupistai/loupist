// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

export type ResearchHandoffReceipt = { chatId: string, handoffId: string };

export type ResearchHandoffRequest = {
  loupistId: string,
  connectionId: string,
  modelProfileId?: string | null,
  thinkingLevel?: string | null,
  runOptions?: { [key in string]: string } | null,
  useMemory?: boolean | null,
  updateMemory?: boolean | null,
};
