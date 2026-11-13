// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { UnreadyReason } from "../providers/inference";

export type GraphJobView = {
  jobId: string,
  loupistId: string,
  worldRecordId: string,
  worldRevisionId: string,
  status: string,
  payloadDigest: string,
  createdAtMs: number,
  hostMayRedrive: boolean,
};

export type GraphProcessingView = { settings: GraphPublicSettings, readiness: GraphReadiness };

export type GraphPublicSettings = {
  enrichmentEnabled: boolean,
  paused: boolean,
  helperModelProfileId: string | null,
  maxLlmCalls: number,
  timeoutMs: number,
  thinkingLevel: string,
  runOptions: { [key in string]: string },
  researchFocus: string,
  relationHints: Array<string>,
  anchors: Array<WorldAnchor>,
  fastModel: string | null,
};

export type GraphReadiness = {
  status: string,
  note: ReadinessNote | null,
  providerName: string | null,
  modelName: string | null,
  authKind: string | null,
};

/**
 * The sentence under the World processing status, by code: the interface
 * renders it in its own language.
 */
export type ReadinessNote =
  | "recovery_required"
  | "recovering"
  | "runtime_unavailable"
  | "awaiting_provider"
  | "choose_model"
  | "model_unavailable"
  | "connect_provider"
  | UnreadyReason;

export type WorldAnchor = { name: string, aliases: Array<string>, identifiers: Array<string> };
