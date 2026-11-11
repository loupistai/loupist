// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

export type AgentHarnessSnapshot = {
  schema: string,
  mode: string,
  skill_ids: Array<string>,
  capability_ids: Array<string>,
  digest: string,
};

export type AgentSkillSummary = {
  id: string,
  name: string,
  description: string,
  enabled: boolean,
  fixed: boolean,
};
