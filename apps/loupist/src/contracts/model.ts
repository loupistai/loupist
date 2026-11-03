export type ExecutionModelLock = {
  profile_id: string;
  name: string;
  provider: string;
  model: string;
  thinking_level: string;
  run_options: Record<string, string>;
};

export type ModelRunSelection = {
  model_profile_id: string;
  thinking_level: string;
  run_options: Record<string, string>;
};

export type ModelRunControlChoice = {
  id: string;
  label: string;
  description: string;
};

export type ModelRunControl = {
  id: string;
  label: string;
  default_choice: string;
  choices: ModelRunControlChoice[];
};

export type AgentHarnessSnapshot = {
  schema: string;
  mode: "chat";
  skill_ids: string[];
  capability_ids: string[];
  digest: string;
};

export type ProviderOauthOption = {
  label: string;
};

/** Where a provider is listed: onboarding "Other ways" and the Settings picker share these groups. */
export type ProviderGroup = "sign_in" | "api_key" | "local" | "advanced" | "more";
export type ProviderState = "connected" | "needs_reconnect" | "disconnected";
export type ProviderCredentialKind = "plan" | "api_key" | "local" | "custom" | "environment" | "none";
export type ProviderNoticeKind = "plan_limit" | "plan_not_eligible" | "extra_usage_off" | "out_of_credit";

/** Rust-owned provider state (spec providers §3.1); arrives in the catalog and by the `provider-status` event. */
export type ProviderStatusView = {
  state: ProviderState;
  credential_kind: ProviderCredentialKind;
  /** At most one notice; the newest wins. */
  notice: { kind: ProviderNoticeKind; at_ms: number } | null;
  last_limit_at_ms: number | null;
  connected_at_ms: number | null;
  /** What still points at a disconnected account. */
  used_by: { schedules: number; chat_default: boolean; world_models: number };
};

export type ProviderStatusEvent = { provider: string; status: ProviderStatusView };

export type ModelProviderOption = {
  id: string;
  label: string;
  credential_label: string;
  description: string;
  api_key: boolean;
  oauth: ProviderOauthOption | null;
  credential_source: "saved" | "environment" | "missing";
  recommended: boolean;
  group: ProviderGroup;
  /** Present when Rust can open the provider's key page (`providerOpenConsole`). */
  console_url: string | null;
  sign_in_flow: "browser" | "device" | null;
  billing_note: "chatgpt_plan" | "claude_extra_usage" | null;
  status: ProviderStatusView;
  /** Set by the renderer for a user-added provider; Rust sends these in `custom_providers`. */
  custom?: CustomModelProvider;
};

export type ProviderModelOption = {
  id: string;
  name: string;
  reasoning: boolean;
  context_window: number;
  max_tokens: number;
  input: string[];
  thinking_levels: string[];
  thinking_level_map: Record<string, string | null>;
  run_controls: ModelRunControl[];
};

export type ProviderConnectFailure =
  | "plan_permission_declined"
  | "plan_not_eligible"
  | "port_in_use"
  | "timed_out"
  | "browser_failed"
  | "offline"
  | "denied"
  | "other";

export type ProviderConnectionEvent =
  /** `opened` is false when the browser could not be opened; `url` is then the validated sign-in link to copy. */
  | { type: "opening_browser"; message: string; opened: boolean; url: string | null }
  | { type: "waiting"; message: string }
  | { type: "connected"; message: string }
  /** Device sign-in (SuperGrok): the code to enter on the provider's page. */
  | { type: "device_code"; user_code: string; verification_uri: string; expires_in_s: number | null }
  /** Pi is ready for a pasted redirect address (ChatGPT) or code (Claude). */
  /** Pi waits for the address the browser shows after signing in. */
  | { type: "manual_entry" }
  | { type: "failed"; reason: ProviderConnectFailure };

export type ConfiguredModel = {
  id: string;
  name: string;
  provider: string;
  model: string;
  input: string[];
  reasoning: boolean;
  context_window: number;
  max_tokens: number;
  thinking_levels: string[];
  thinking_level_map: Record<string, string | null>;
  run_controls: ModelRunControl[];
  credential_source: "saved" | "environment" | "missing";
};

/** One pending "default changed" line above the composer (W17.5); acknowledged with `modelNoticeAck`. */
export type ModelNotice = { id: string; kind: "now_using" | "removed"; model: string };

export type ModelCatalog = {
  models: ConfiguredModel[];
  providers: ModelProviderOption[];
  /** User-added OpenAI- or Anthropic-compatible endpoints. */
  custom_providers?: CustomModelProvider[];
  hidden_model_profile_ids: string[];
  model_notice?: ModelNotice | null;
};

export type CustomProviderApi = "openai-responses" | "openai-completions" | "anthropic-messages";

export type CustomModelProvider = {
  id: string;
  name: string;
  base_url: string;
  api: CustomProviderApi;
  model_ids: string[];
  credential_source: "saved" | "environment" | "missing";
  status?: ProviderStatusView;
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

export type ModelVisibilityUpdateRequest = {
  model_profile_id: string;
  visible_in_chat: boolean;
};

export type ProviderConfigureRequest = {
  provider: string;
  api_key?: string;
  clear_saved_key?: boolean;
};

/** A model app found on this Mac by Rust's loopback probes. */
export type LocalEngineId = "ollama" | "lm_studio" | "llama_cpp";
export type LocalEngine = {
  engine: LocalEngineId;
  /** Untranslated product name: Ollama, LM Studio or llama.cpp. */
  label: string;
  base_url: string;
  running: boolean;
  models: { id: string; name: string; context_tokens: number | null }[];
};
export type LocalDiscovery = { engines: LocalEngine[] };

export type ProviderCustomTestRequest = { base_url: string; api: CustomProviderApi; api_key?: string };
export type ProviderCustomTestOutcome = "ok" | "unreachable" | "unauthorized" | "timeout" | "not_found" | "wrong_dialect" | "invalid";
export type ProviderCustomTestResult = { outcome: ProviderCustomTestOutcome; models: string[] };

/** A provider Rust saves as a fixed custom endpoint (Upstage Solar). */
export type ProviderPresetRequest = { preset: "upstage"; api_key: string };
