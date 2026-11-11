// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

export type ChatRetryReceipt = {
  prompt: string,
  /** The as-of date the request was sent with; Retry repeats it. */
  as_of?: string,
};

export type ChatRetryRequest = { loupist_id: string, thread_id: string };
