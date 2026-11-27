// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { PassageRange } from "../knowledge/model";
import type { UnreadyReason } from "../providers/inference";

/** A fact left out because a figure it states differs from the passage. */
export type DroppedFigure = {
  /** The proposition, at most 240 characters. */
  proposition: string,
  /** The figure as the proposition states it. */
  stated: string,
  /** The chunk it was extracted from. Absent: unknown. */
  passage?: PassageRange,
};

export type GraphJobView = {
  jobId: string,
  loupistId: string,
  worldRecordId: string,
  worldRevisionId: string,
  status: string,
  payloadDigest: string,
  createdAtMs: number,
  hostMayRedrive: boolean,
  /** Why the job stopped, when one was recorded. */
  failure?: JobFailure,
  /** Try again can run this stopped span again. Absent when it cannot. */
  retryable?: boolean,
  /** Waiting on the daily allowance. Absent when the job can start. */
  hold?: JobHold,
  /** Facts this job left out. Absent when it left none out. */
  dropped?: JobDropped,
  /** The first eight facts counted in `dropped.figureMismatch`. */
  droppedFigures?: Array<DroppedFigure>,
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

/** Facts a job left out of World. */
export type JobDropped = {
  /** Facts not published because a figure differs from the passage. */
  figureMismatch: number,
};

/**
 * A recorded stop. `kind` uses the recovery vocabulary and the model-output
 * and capacity stops.
 */
export type JobFailure = { kind: JobFailureKind, atMs: number };

export type JobFailureKind =
  | "interrupted"
  | "timed_out"
  | "usage_uncertain"
  | "invalid_json"
  | "schema_mismatch"
  | "incomplete_response"
  | "output_size"
  | "content_type"
  | "world_full";

/**
 * Why a queued job is waiting on the daily allowance. Computed for the view;
 * the queue row stays `queued`.
 */
export type JobHold = { cause: JobHoldCause, provider?: string };

export type JobHoldCause = "allowance" | "plan_limit" | "out_of_credit";

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
