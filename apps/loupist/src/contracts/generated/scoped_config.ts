// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

export type AppearanceConfig = { theme?: string, language?: string };

export type ChatConfig = {
  model_profile_id?: string,
  thinking_level?: string,
  run_options?: { [key in string]: string },
  use_memory?: boolean,
  update_memory?: boolean,
};

export type ConfigDiagnostic = {
  path: string,
  message: string,
  line: number | null,
  column: number | null,
};

export type ConfigDocument = {
  appearance?: AppearanceConfig,
  models?: ModelsConfig,
  connectors?: { [key in string]: { [key in string]: string } },
  chat?: ChatConfig,
  skills?: { [key in string]: SkillConfig },
  pi?: PartialPiConfig,
};

export type ConfigReadRequest = { scope: ConfigScope };

export type ConfigScope = { "kind": "global" } | { "kind": "project", loupistId: string };

export type ConfigSnapshot = {
  scope: ConfigScope,
  path: string,
  revision: string,
  explicit: ConfigDocument | null,
  effective: ResolvedConfig,
  sources: { [key in string]: string },
  /**
   * Errors: the file is malformed or a known key has an invalid value. Any
   * error blocks dependent runs and settings saves until the file is fixed.
   */
  diagnostics: Array<ConfigDiagnostic>,
  /**
   * Settings this version ignores because its bundled catalog no longer
   * has that connector or connector field, or keeps that Skill always on.
   * The text stays in the file; nothing is blocked.
   */
  warnings?: Array<ConfigDiagnostic>,
};

export type ConfigUpdateRequest = {
  scope: ConfigScope,
  expectedRevision: string,
  set?: ConfigDocument,
  unset?: Array<Array<string>>,
};

export type ModelsConfig = { hidden_model_profile_ids?: Array<string> };

export type PartialPiCompactionConfig = { reserve_tokens?: number, keep_recent_tokens?: number };

export type PartialPiConfig = {
  compaction?: PartialPiCompactionConfig,
  retry?: PartialPiRetryConfig,
};

export type PartialPiRetryConfig = {
  enabled?: boolean,
  max_retries?: number,
  base_delay_ms?: number,
  max_agent_delay_ms?: number,
};

export type PiCompactionConfig = { reserve_tokens: number, keep_recent_tokens: number };

export type PiConfig = { compaction: PiCompactionConfig, retry: PiRetryConfig };

export type PiRetryConfig = {
  enabled: boolean,
  max_retries: number,
  base_delay_ms: number,
  max_agent_delay_ms: number,
};

export type ResolvedAppearanceConfig = { theme: string, language: string };

export type ResolvedChatConfig = {
  model_profile_id: string | null,
  thinking_level: string | null,
  run_options: { [key in string]: string },
  use_memory: boolean,
  update_memory: boolean,
};

export type ResolvedConfig = {
  appearance: ResolvedAppearanceConfig,
  models: ResolvedModelsConfig,
  connectors: { [key in string]: { [key in string]: string } },
  chat: ResolvedChatConfig,
  skills: { [key in string]: ResolvedSkillConfig },
  pi: PiConfig,
};

export type ResolvedModelsConfig = { hidden_model_profile_ids: Array<string> };

export type ResolvedSkillConfig = {
  /** Missing on a custom Skill only. Included Skills are filled from the catalog first. */
  enabled: boolean,
};

export type SkillConfig = { enabled?: boolean };
