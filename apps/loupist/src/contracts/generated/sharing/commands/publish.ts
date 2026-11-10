// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { ScopeRequest } from "../../sharing/commands/share";
import type { Sections } from "../../sharing/export";
import type { License } from "../../sharing/pack";

export type PublishEvent = { "kind": "creating" } | { "kind": "uploading", files: number };

export type PublishTarget =
  | { "repo": { name: string } }
  | { "update": boolean }
  | { "gist": string };

export type SharePublishRequest = {
  loupistId: string,
  scope: ScopeRequest,
  sections: Sections,
  excludedItemKeys: Array<string>,
  acknowledgedFindingIds: Array<string>,
  license?: License | null,
  byline?: string | null,
  target: PublishTarget,
};
