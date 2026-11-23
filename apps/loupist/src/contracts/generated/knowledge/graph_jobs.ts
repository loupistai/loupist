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

export type GraphProcessingView = {
  settings: GraphPublicSettings,
  readiness: GraphReadiness,
  /** Empty while no connected account offers a listed model. */
  recommended: RecommendedWorldModel | null,
};

export type GraphPublicSettings = {
  enrichmentEnabled: boolean,
  paused: boolean,
  helperModelProfileId: string | null,
  /**
   * Recommended ignores the model, thinking level and run options sent with
   * it: the host saves what it recommends.
   */
  worldModelChoice: WorldModelChoice,
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
  | "no_account"
  | "choose_model"
  | "model_unavailable"
  | "connect_provider"
  | UnreadyReason;

/**
 * What the bundled list recommends for this Loupist with the accounts
 * connected now, whichever choice the Loupist has.
 */
export type RecommendedWorldModel = {
  modelProfileId: string,
  thinkingLevel: string,
  runOptions: { [key in string]: string },
};

/** What one "Continue all" did. */
export type SourcesRetried = {
  /** Sources given a fresh allowance and queued again. */
  continued: number,
  /** Sources [`retry_source_processing`] refused, left as they were. */
  skipped: number,
};

export type WorldAnchor = { name: string, aliases: Array<string>, identifiers: Array<string> };

/**
 * Whether a Loupist's World model follows the bundled list
 * (`world-model-defaults.json`) or stays the one the user picked.
 */
export type WorldModelChoice = "recommended" | "chosen";
