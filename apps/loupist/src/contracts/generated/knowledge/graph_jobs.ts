// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { TimePoint } from "../knowledge/model";
import type { UnreadyReason } from "../providers/inference";

export type ExtractionBudget = {
  modelProfile: string,
  thinkingLevel: string,
  runOptions: { [key in string]: string },
  embeddingProfile: string,
  maxLlmCalls: number,
  maxEmbeddingCalls: number,
  timeoutMs: number,
};

export type GraphJobView = {
  jobId: string,
  loupistId: string,
  worldRecordId: string,
  worldRevisionId: string,
  status: string,
  payloadDigest: string,
  createdAtMs: number,
  document: JobDocument,
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

export type JobDocument = {
  version: number,
  episodeKind: string,
  generationId: string,
  episodeKey: string,
  /** Prior chunk of the same source span. Empty for a single-episode span. */
  previousEpisodeKey: string | null,
  name: string,
  body: string,
  sourceBinding: StoredSourceBinding | null,
  /** Chat and message that retained this Source. Absent for Map adds, imports, and recovery. */
  originChatId?: string,
  originMessageId?: string,
  artifactUtf8: string | null,
  logicalDigest: string | null,
  publicationTime: TimePoint | null,
  effectiveTime: TimePoint | null,
  referenceTime: string,
  /** Provenance shown to extraction. */
  sourceLabel: string,
  extraction: ExtractionBudget,
  dedupKey: string,
  mutationSent: boolean,
  inferenceAttempted: boolean,
  usageUncertain: boolean,
  leaseExpiresAtMs: number | null,
  sealedRevisionId: string | null,
  attemptId: string | null,
  /**
   * Paid attempts that ended without publishing anything. The first runs
   * again; the second quarantines the generation.
   */
  interruptedAttempts: number,
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

export type StoredSourceBinding = {
  sourceId: string,
  captureId: string,
  representationId: string,
  spanId: string,
  payloadDigest: string,
  bindingRole: string,
  attribution: string,
  sourceRecordId: string,
  sourceRevisionId: string,
};

export type WorldAnchor = { name: string, aliases: Array<string>, identifiers: Array<string> };
