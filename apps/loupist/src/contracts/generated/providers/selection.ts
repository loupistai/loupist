// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { ModelRunControl } from "../providers/catalog";

export type DefaultModelChange = {
  id: string,
  previous: ModelIdentity | null,
  current: ModelIdentity | null,
};

export type ModelIdentity = { id: string, provider: string, model: string, name: string };

export type ModelUnavailableReason =
  | "no_connected_account"
  | "no_eligible_model"
  | "account_disconnected"
  | "model_hidden"
  | "invalid_configuration"
  | "catalog_unavailable";

export type ResolvedModelSelection = {
  model: ModelIdentity | null,
  thinking_levels: Array<string>,
  run_controls: Array<ModelRunControl>,
  is_default: boolean,
  unavailable_reason: ModelUnavailableReason | null,
  thinking_level: string | null,
  run_options: { [key in string]: string },
  /**
   * Why the stored model, still listed, cannot run now. The selection is
   * then Auto in its place; the stored choice applies again once it can run.
   */
  stored_unavailable: ModelUnavailableReason | null,
};
