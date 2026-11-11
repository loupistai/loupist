// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { ProviderStatusView } from "../providers/status";

export type BillingNote = "chatgpt_plan";

export type ConfiguredModelView = {
  id: string,
  name: string,
  provider: string,
  model: string,
  input: Array<string>,
  reasoning: boolean,
  context_window: number,
  max_tokens: number,
  thinking_levels: Array<string>,
  thinking_level_map: { [key in string]: string | null },
  run_controls: Array<ModelRunControl>,
  credential_source: CredentialSource,
};

export type CredentialSource = "saved" | "environment" | "missing";

/** A user-added OpenAI- or Anthropic-compatible endpoint. Its key is never shown. */
export type CustomProviderView = {
  id: string,
  name: string,
  base_url: string,
  api: string,
  model_ids: Array<string>,
  credential_source: CredentialSource,
  status: ProviderStatusView,
};

export type ExecutionModelLock = {
  profile_id: string,
  name: string,
  provider: string,
  model: string,
  thinking_level: string,
  run_options: { [key in string]: string },
};

export type ModelCatalogView = {
  models: Array<ConfiguredModelView>,
  providers: Array<ModelProviderOption>,
  custom_providers: Array<CustomProviderView>,
  hidden_model_profile_ids: Array<string>,
  /** The pending "Now using …" line above the composer (W17.5). */
  model_notice: ModelNoticeView | null,
};

export type ModelNoticeView = {
  id: string,
  kind: string,
  /** The model's display name. */
  model: string,
};

export type ModelProviderOption = {
  id: string,
  label: string,
  credential_label: string,
  description: string,
  api_key: boolean,
  oauth: ProviderOauth | null,
  credential_source: CredentialSource,
  recommended: boolean,
  /** Where onboarding and the Settings picker list the provider. */
  group: ProviderGroup,
  /** The provider's key page, opened by Rust (`provider_open_console`). */
  console_url: string | null,
  sign_in_flow: SignInFlow | null,
  billing_note: BillingNote | null,
  status: ProviderStatusView,
};

export type ModelRunControl = {
  id: string,
  label: string,
  default_choice: string,
  choices: Array<ModelRunControlChoice>,
};

export type ModelRunControlChoice = { id: string, label: string, description: string };

export type ProviderGroup = "sign_in" | "api_key" | "more";

export type ProviderOauth = { label: string };

export type SignInFlow = "browser" | "device";
