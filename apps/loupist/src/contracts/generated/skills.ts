// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { ConfigSnapshot } from "./scoped_config";

export type ManagedSkill = {
  id: string,
  name: string,
  description: string,
  source: string,
  path: string | null,
  enabled: boolean,
  fixed: boolean,
  editable: boolean,
  shadowed: boolean,
  revision: string,
  error: string | null,
};

export type SkillCatalog = { skills: Array<ManagedSkill>, configuration: ConfigSnapshot };

export type SkillDocument = { skill: ManagedSkill, instructions: string, files: Array<string> };
