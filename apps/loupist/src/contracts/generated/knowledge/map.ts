// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { CustomOrigin } from "../custom_connections/taint";
import type { ClaimStatus } from "../knowledge/model";
import type { ExtensionCategory, ExtensionLabels } from "../knowledge/world_schema";
import type { MapContextItem } from "../store/user_context";

export type KnowledgeMap = {
  nodes: Array<KnowledgeMapNode>,
  edges: Array<KnowledgeMapEdge>,
  unmatchedHoldings: Array<MapContextItem>,
  unmatchedWatchlist: Array<MapContextItem>,
  /** Counts refer to projected nodes/edges (multi-endpoint claims form a star). */
  counts: { [key in string]: KnowledgeMapCount },
  worldRevisionId: string | null,
  /** Latest contributing stored timestamp, not wall-clock request time. */
  generatedAtMs: number,
};

export type KnowledgeMapCount = { total: number, shown: number, truncated: boolean };

export type KnowledgeMapEdge = {
  id: string,
  kind: string,
  source: string,
  target: string,
  relation?: string,
  status?: ClaimStatus,
  weight?: number,
  firstSeenAtMs?: number,
};

export type KnowledgeMapNode = {
  origin?: CustomOrigin,
  id: string,
  kind: string,
  label: string,
  /**
   * Original-language aliases: up to four trimmed names that contain a letter
   * outside Latin (a code point above U+024F). Empty for non-entity nodes.
   */
  localNames?: Array<string>,
  entityKind?: string,
  /** Map colour for an extension type. Core kinds keep the shared category map. */
  categoryOverride?: ExtensionCategory,
  /** Display names for an extension type. The inspector uses the UI language, then English. */
  kindLabels?: ExtensionLabels,
  degree: number,
  firstSeenAtMs?: number,
  recordId?: string,
  archived?: boolean,
  watched: boolean,
  held: boolean,
  learnedAtMs?: number,
  learnedRationale?: string,
};

export type KnowledgeMapQuery = { loupistId: string, maxNodes?: number | null };
