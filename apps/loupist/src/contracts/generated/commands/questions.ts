// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

export type QuestionAnswerRequest = {
  loupist_id: string,
  request_id: string,
  choice?: number | null,
  text?: string | null,
};
