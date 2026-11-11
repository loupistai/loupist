// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { ConfigScope } from "../scoped_config";

export type ImportKind = "folder" | "zip" | "github";

export type SkillCreateRequest = {
  scope: ConfigScope,
  name: string,
  description: string,
  instructions: string,
};

export type SkillDeleteRequest = { scope: ConfigScope, key: string, expectedRevision: string };

export type SkillImportRequest = { scope: ConfigScope, kind: ImportKind, url?: string | null };

export type SkillReadRequest = { scope: ConfigScope, key: string };

export type SkillSaveRequest = {
  scope: ConfigScope,
  key: string,
  expectedRevision: string,
  description: string,
  instructions: string,
};
