// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { ChartDocument } from "../chat/chart";
import type { GridDocument } from "../chat/grid";

export type ChatArtifact = {
  id: string,
  chat_session_id: string,
  kind: ChatArtifactKind,
  title: string,
  current_revision: number,
  created_at_ms: number,
  updated_at_ms: number,
};

export type ChatArtifactKind = "markdown" | "chart" | "grid";

export type ChatArtifactPayload =
  | { "kind": "markdown", schema: string, markdown: string }
  | { "kind": "chart", schema: string, chart: ChartDocument }
  | { "kind": "grid", schema: string, grid: GridDocument };

export type ChatArtifactRef = {
  artifact_id: string,
  revision: number,
  kind: ChatArtifactKind,
  title: string,
  digest: string,
};

export type ChatArtifactRevision = {
  artifact_id: string,
  revision: number,
  payload: ChatArtifactPayload,
  digest: string,
  source_message_id: string,
  created_at_ms: number,
};

export type ChatArtifactVersion = {
  /** Revision numbers are stable identifiers scoped to one artifact. */
  revision_id: number,
  title: string,
  digest: string,
  source_message_id: string,
  created_at_ms: number,
  restored_from_revision_id?: number,
};
