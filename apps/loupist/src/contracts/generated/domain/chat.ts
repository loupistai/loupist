// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { CalcOperation } from "../calculations";
import type { HandoffStatus } from "../chat/handoff";
import type { DatasetRef } from "../datasets";
import type { KnowledgeReference } from "../domain/memory";
import type { JsonValue } from "../external";
import type { LearnedDigest } from "../knowledge/lens";
import type { RunStatus, Schedule, TaskKind } from "../scheduled_tasks/types";

export type ChatEvidenceDelivery = {
  coverage: ChatEvidenceDeliveryCoverage,
  excerpt: string,
  excerpt_truncated: boolean,
  content_sha256: string,
};

export type ChatEvidenceDeliveryCoverage = "bounded_sample";

export type ChatEvidenceKind = "original" | "discovery" | "derived" | "local_file" | "saved_context";

/**
 * End-exclusive UTF-8 byte offsets into the retained excerpt, supplied only
 * after the host verifies a literal passage selected from its cached source.
 */
export type ChatEvidenceMatch = { start: number, end: number };

/**
 * A bounded, host-created excerpt of the retained tool result. When the agent
 * received a compacted sample, `model_delivery` records that distinct payload.
 * Neither snapshot verifies a claim or source. Works with Memory off.
 */
export type ChatEvidenceRef = {
  id: string,
  title: string,
  kind: ChatEvidenceKind,
  url?: string,
  provider?: string,
  tool_name: string,
  /** For a `calculate` result: its operation, the stable id it is worded by. */
  calc_operation?: CalcOperation,
  read_ref?: string,
  retrieved_at: string,
  published_at?: string,
  as_of?: string,
  coverage?: string,
  excerpt: string,
  excerpt_match?: ChatEvidenceMatch,
  excerpt_truncated: boolean,
  content_sha256: string,
  warnings: Array<string>,
  dataset_ref?: DatasetRef,
  model_delivery?: ChatEvidenceDelivery,
  upstream_evidence_ids?: Array<string>,
};

/** Stable allowlist: never store provider error bodies or internal error strings. */
export type ChatFailureKind =
  | "empty_reply"
  | "provider_error"
  | "rate_limited"
  | "needs_reconnect"
  | "plan_limit"
  | "plan_not_eligible"
  | "out_of_credit"
  | "model_gone"
  | "offline"
  | "provider_unavailable"
  | "context_too_long"
  | "provider_timeout"
  | "pi_exited"
  | "protocol_error"
  | "output_limit"
  | "memory_commit_failed"
  | "store_failed"
  | "cancelled_by_conflict"
  | "other";

export type ChatFeedback = {
  rating: ChatFeedbackRating | null,
  correction: string | null,
  updatedAt: number,
};

export type ChatFeedbackRating = "up" | "down";

export type ChatMessageStatus = "complete" | "aborted" | "error";

/**
 * Content-free measurements captured before the completed message is sealed.
 * These remain attached to that response after Chat reload or app restart.
 */
export type ChatPerformance = {
  setupMs: number,
  firstTextMs?: number,
  generationMs: number,
  /** Elapsed time through preparation of the canonical completion snapshot. */
  totalMs: number,
  sessionCache: ChatSessionCacheStatus,
  inputTokens: number,
  outputTokens: number,
  cacheReadTokens: number,
  cacheWriteTokens: number,
};

export type ChatSessionCacheStatus = "cold" | "warm";

export type ChatToolRefusal = {
  connection_id: string,
  connection_name: string,
  tool_name: string,
  reason: ChatToolRefusalReason,
};

export type ChatToolRefusalReason =
  | "needs_approval"
  | "blocked_unattended"
  | "taint"
  | "sensitive_memory"
  | "declined"
  | "other";

export type FiledOutcome = { opId: string, outcome: string, key?: string };

/**
 * One crossed line on an asset page, posted once into the home Thread. Built
 * only from the page and its move: names, the user's claim and the observed
 * value, never holdings quantities.
 */
export type HomeAlert = {
  asset_id: string,
  move_id: string,
  name: string,
  symbol: string,
  market: string,
  claim: string,
  /** The observed value as written, with its unit ("27%"). */
  value: string,
  /** When the user wrote the line. */
  set_at_ms: number,
  fact_date: string,
  source_url?: string,
  source_title: string,
};

/**
 * Where a host-posted Thread message came from: one finished scheduled run.
 * The text is the run's first paragraph; the run's own Chat keeps the answer.
 */
export type HomeDelivery = {
  run_id: string,
  task_id: string,
  task_title: string,
  task_kind: TaskKind,
  /** None once the task was deleted. */
  schedule: Schedule | null,
  run_thread_id: string | null,
  status: RunStatus,
  finished_at_ms: number,
  learned?: LearnedDigest,
};

/**
 * A results message: "{name} reported {period} results." with the two most
 * material measures, or a before-capture that failed (Try again).
 */
export type HomeEarnings = {
  asset_id: string,
  event_id: string,
  name: string,
  symbol: string,
  market: string,
  period: string,
  /** `reported` or `failed`. */
  outcome: string,
  side?: string,
  /** The card's measure rows (`MeasureView`), at most two. */
  measures: Array<JsonValue>,
};

export type HomeHandoff = {
  handoffId: string,
  chatId: string,
  title: string,
  status: HandoffStatus,
  learned?: LearnedDigest,
};

export type HomeLink = {
  connectionId: string,
  seedName: string,
  farName: string,
  path: Array<HomeLinkHop>,
  nodeIds: Array<string>,
};

export type HomeLinkHop = { name: string, relation: string, reversed?: boolean };

export type HomeVocabulary = {
  changeId: string,
  producer: string,
  labels: Array<string>,
  labelsKo?: Array<string>,
};

/**
 * Watcher alerts in one host message: at most five, then "+{more} more".
 * Events are the stored `WatchEventView`s (names and dated facts, never
 * holdings); kept as JSON so the message stays a plain record.
 */
export type HomeWatch = { event_ids: Array<string>, events: Array<JsonValue>, more: number };

export type MapKind = { name: string, label: string, labelKo?: string };

export type MemoryUpdateChange = {
  worldJobId?: string | null,
  worldStatus?: string | null,
  knowledge?: KnowledgeReference | null,
  recordId: string,
  kind: string,
  operation: string,
  title: string,
  lesson: string,
  basis: string,
  futureUse: string,
  outcomes?: Array<FiledOutcome>,
};

export type MemoryUpdateResult = {
  status: MemoryUpdateStatus,
  commitId: string | null,
  changes: Array<MemoryUpdateChange>,
  /**
   * Kinds the answer asked the map to track that its commit could not add:
   * the map gained a kind of that name first, or has no room left.
   */
  skippedKinds?: Array<MapKind>,
  /** How `loupist_setup` changed what this Loupist follows and researches. */
  setup?: Array<SetupChange>,
};

export type MemoryUpdateStatus = "applied" | "no_change";

/** One applied `loupist_setup` change. A market the user did not give is `None`. */
export type SetupChange =
  | { "change": "followed", symbol: string, market?: string, name?: string }
  | { "change": "unfollowed", symbol: string, market?: string }
  | { "change": "focus", focus: string }
  | { "change": "tracks", kinds: Array<MapKind> };

/**
 * A thesis update the tool accepted but the reply's commit did not save,
 * because the page changed first. The receipt says it was not saved.
 */
export type UnsavedThesis = {
  assetId: string,
  name: string,
  /** The refusal code (`thesis_after_undo`, `thesis_none`, `thesis_twice`, …). */
  reason: string,
};
