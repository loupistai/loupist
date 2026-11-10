// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

export type CustomProviderRequest = {
  /** Present when editing an existing custom provider. */
  provider?: string | null,
  name: string,
  base_url: string,
  api: string,
  api_key?: string | null,
  model_ids: Array<string>,
};
