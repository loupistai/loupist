// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

export type LocalDiscovery = { engines: Array<LocalEngine> };

/** A model app found on this Mac by the loopback probes. */
export type LocalEngine = {
  engine: LocalEngineId,
  label: string,
  base_url: string,
  running: boolean,
  models: Array<LocalModel>,
};

export type LocalEngineId = "ollama" | "lm_studio" | "llama_cpp";

export type LocalModel = { id: string, name: string, context_tokens: number | null };

export type ProviderCustomTestRequest = { base_url: string, api: string, api_key?: string | null };

export type ProviderCustomTestResult = { outcome: TestOutcome, models: Array<string> };

export type ProviderPresetRequest = { preset: string, api_key: string };

export type TestOutcome =
  | "ok"
  | "unreachable"
  | "unauthorized"
  | "timeout"
  | "not_found"
  | "wrong_dialect"
  | "invalid";
