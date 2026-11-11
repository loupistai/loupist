import type * as Wire from "./generated";
export type ExecutionModelLock = Wire.ExecutionModelLock;

export type ModelRunSelection = {
  model_profile_id: string;
  thinking_level: string;
  run_options: Record<string, string>;
};

export type ModelRunControlChoice = Wire.ModelRunControlChoice;

export type ModelRunControl = Wire.ModelRunControl;

export type AgentHarnessSnapshot = {
  schema: string;
  mode: "chat";
  skill_ids: string[];
  capability_ids: string[];
  digest: string;
};

export type ProviderOauthOption = Wire.ProviderOauth;

/**
 * Where a provider is listed: onboarding "Other ways" and the Settings picker share
 * these groups. `advanced` is the renderer's group for a user-added provider.
 */
export type ProviderGroup = Wire.ProviderGroup | "advanced";
export type ProviderState = Wire.ProviderState;
export type ProviderCredentialKind = Wire.CredentialKind;
export type ProviderNoticeKind = Wire.NoticeKind;

/**
 * Rust-owned provider state (spec providers §3.1); arrives in the catalog and by the
 * `provider-status` event. At most one notice (the newest); `used_by` is what still
 * points at a disconnected account.
 */
export type ProviderStatusView = Wire.ProviderStatusView;

export type ProviderStatusEvent = Wire.ProviderStatusEvent;

/**
 * A catalog provider. The renderer also lists each user-added provider (Rust sends
 * those in `custom_providers`) as one, in the `advanced` group with `custom` set.
 */
export type ModelProviderOption = Omit<Wire.ModelProviderOption, "group"> & {
  group: ProviderGroup;
  custom?: CustomModelProvider;
};

export type ProviderConnectFailure = Wire.ConnectFailure;

/**
 * One sign-in step. `opening_browser`: `opened` is false when the browser could not be
 * opened, and `url` is then the validated sign-in link to copy. `device_code`: device
 * sign-in (SuperGrok), the code to enter on the provider's page. `manual_entry`: Pi
 * waits for the address the browser shows after signing in.
 */
export type ProviderConnectionEvent = Wire.ProviderConnectionEvent;

export type ConfiguredModel = Wire.ConfiguredModelView;

/** One pending "default changed" line above the composer (W17.5); acknowledged with `modelNoticeAck`. */
export type ModelNotice = { id: string; kind: "now_using" | "removed"; model: string };

export type ModelCatalog = {
  models: ConfiguredModel[];
  providers: ModelProviderOption[];
  /** User-added OpenAI- or Anthropic-compatible endpoints. */
  custom_providers: CustomModelProvider[];
  hidden_model_profile_ids: string[];
  model_notice: ModelNotice | null;
};

export type CustomProviderApi = "openai-responses" | "openai-completions" | "anthropic-messages";

export type CustomModelProvider = {
  id: string;
  name: string;
  base_url: string;
  api: CustomProviderApi;
  model_ids: string[];
  credential_source: Wire.CredentialSource;
  status: ProviderStatusView;
};

export type CustomProviderRequest = {
  /** Present when editing an existing custom provider. */
  provider?: string;
  name: string;
  base_url: string;
  api: CustomProviderApi;
  /** Blank keeps a saved key; a new provider without one is treated as a local server. */
  api_key?: string;
  model_ids: string[];
};

export type ModelVisibilityUpdateRequest = Wire.ModelVisibilityUpdateRequest;

export type ProviderConfigureRequest = {
  provider: string;
  api_key?: string;
  clear_saved_key?: boolean;
};

/** A model app found on this Mac by Rust's loopback probes; `label` is its untranslated product name. */
export type LocalEngineId = Wire.LocalEngineId;
export type LocalEngine = Wire.LocalEngine;
export type LocalDiscovery = Wire.LocalDiscovery;

export type ProviderCustomTestRequest = { base_url: string; api: CustomProviderApi; api_key?: string };
export type ProviderCustomTestOutcome = Wire.TestOutcome;
export type ProviderCustomTestResult = Wire.ProviderCustomTestResult;

/** A provider Rust saves as a fixed custom endpoint (Upstage Solar). */
export type ProviderPresetRequest = { preset: "upstage"; api_key: string };
