// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { KnowledgeSourceReadRequest } from "../../commands/knowledge";
import type { SourceTextSelection } from "../../commands/types";

export type SourceSelectionPin = {
  loupistId: string,
  selection?: SourceTextSelection,
  kind: string,
  recordId: string | null,
  revisionId: string | null,
  objectId: string,
  spanId: string | null,
  label: string,
};

export type SourceSelectionRequest = {
  source: KnowledgeSourceReadRequest,
  selection: SourceTextSelection,
};
