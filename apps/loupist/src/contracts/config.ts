export type LanguagePreference = "system" | "en" | "ko";
export type ConfigScope = { kind: "global" } | { kind: "project"; loupistId: string };
export type ConfigSource = "default" | "global" | "project";

/** TOML keys intentionally retain their spelling at the file/bridge boundary. */
export type ConfigDocument = {
  appearance?: { theme?: "light" | "dark" | "system"; language?: LanguagePreference };
  models?: { hidden_model_profile_ids?: string[] };
  connectors?: Record<string, Record<string, string>>;
  chat?: { model_profile_id?: string; thinking_level?: string; run_options?: Record<string, string>; use_memory?: boolean; update_memory?: boolean };
  skills?: Record<string, { enabled?: boolean }>;
  pi?: {
    compaction?: { reserve_tokens?: number; keep_recent_tokens?: number };
    retry?: { enabled?: boolean; max_retries?: number; base_delay_ms?: number; max_agent_delay_ms?: number };
  };
};

export type EffectiveConfig = {
  appearance: { theme: "light" | "dark" | "system"; language: LanguagePreference };
  models: { hidden_model_profile_ids: string[] };
  connectors: Record<string, Record<string, string>>;
  chat: { model_profile_id: string | null; thinking_level: string | null; run_options: Record<string, string>; use_memory: boolean; update_memory: boolean };
  skills: Record<string, { enabled: boolean }>;
  pi: {
    compaction: { reserve_tokens: number; keep_recent_tokens: number };
    retry: { enabled: boolean; max_retries: number; base_delay_ms: number; max_agent_delay_ms: number };
  };
};

export type ConfigDiagnostic = { path: string; message: string; line: number | null; column: number | null };
export type ConfigSnapshot = {
  scope: ConfigScope;
  path: string;
  revision: string;
  explicit: ConfigDocument | null;
  effective: EffectiveConfig;
  sources: Record<string, ConfigSource>;
  diagnostics: ConfigDiagnostic[];
};
export type ConfigUpdateRequest = { scope: ConfigScope; expectedRevision: string; set: ConfigDocument; unset: string[][] };
