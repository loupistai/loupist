// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { CalcOperation } from "../calculations";
import type { HandoffStatus, HandoffStopReason } from "../chat/handoff";
import type { DatasetRef } from "../datasets";
import type { KnowledgeReference } from "../domain/memory";
import type { JsonValue } from "../external";
import type { LearnedDigest } from "../knowledge/lens";
import type { FailureStreak, RunStatus, Schedule, TaskKind, WatchPreset } from "../scheduled_tasks/types";

export type ChatCostEstimate = { microusd: number, fromProvider: boolean };

export type ChatEvidenceDelivery = {
  coverage: ChatEvidenceDeliveryCoverage,
  excerpt: string,
  excerpt_truncated: boolean,
  content_sha256: string,
};

export type ChatEvidenceDeliveryCoverage = "bounded_sample";

export type ChatEvidenceKind =
  | "original"
  | "discovery"
  | "derived"
  | "local_file"
  | "user_provided"
  | "saved_context";

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
  /**
   * The read returned a user-provided Source (`SourceClass::UserProvided`):
   * material the user pasted earlier, kept as given and never verified.
   */
  user_provided?: boolean,
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
  /**
   * First text delta of the final stopping assistant message, from turn start.
   * Absent when that message did not stream text.
   */
  answerTextMs?: number,
  generationMs: number,
  /** Elapsed time through preparation of the canonical completion snapshot. */
  totalMs: number,
  sessionCache: ChatSessionCacheStatus,
  inputTokens: number,
  outputTokens: number,
  cacheReadTokens: number,
  cacheWriteTokens: number,
  modelRequests?: number,
  costEstimate?: ChatCostEstimate,
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
  | "saved_key_permission"
  | "taint"
  | "sensitive_memory"
  | "declined"
  | "other";

/** Portable per-turn diagnostics. `truncated` also covers omitted calls. */
export type ChatToolTrace = {
  entries: Array<ChatToolTraceEntry>,
  truncated: boolean,
  /**
   * Host trust receipt, persisted in the existing message JSON. It remains
   * meaningful when diagnostic entries are bounded or omitted.
   */
  untrusted_text: boolean,
};

export type ChatToolTraceEntry = {
  call_id: string | null,
  name: string,
  component: string | null,
  args: JsonValue | null,
  start_offset_ms: number,
  duration_ms: number,
  outcome: ToolTraceOutcome,
  /** The broker's model-facing envelope, with bounded/redacted text. */
  error: JsonValue | null,
  result_bytes: number | null,
  origin?: ToolTraceOrigin,
  parentStep?: string,
  python?: PythonRunTrace,
};

export type DiscardedMemoryItem = { kind: DiscardedMemoryKind, title: string };

export type DiscardedMemoryKind =
  | "note"
  | "journal_entry"
  | "source"
  | "lens_section"
  | "lens_page"
  | "principle"
  | "method"
  | "world_vocabulary"
  | "thesis"
  | "price_read"
  | "bot_memory"
  | "watchlist"
  | "research_focus"
  | "research_handoff";

export type FiledOutcome = { opId: string, outcome: string, key?: string };

export type HandoffAction = { "kind": "link", connectionId: string };

/**
 * One crossed line on an asset page, posted once into the home Thread. Built
 * only from the page and its move: names, the user's claim and the observed
 * value.
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

/** One question a scheduled run left. The run does not wait. */
export type HomeAsk = {
  id: string,
  question: string,
  options: Array<HomeAskOption>,
  recommended: number,
  createdAtMs: number,
  expiresAtMs: number,
  choice?: number,
};

export type HomeAskOption = { label: string };

/**
 * Where a host-posted Thread message came from: one finished scheduled run.
 * The text is the run's first paragraph; the run's own Chat keeps the answer.
 */
export type HomeDelivery = {
  firstRun?: boolean,
  evidence?: HomeResultEvidence,
  failure_streak?: FailureStreak,
  run_id: string,
  task_id: string,
  task_title: string,
  task_kind: TaskKind,
  /** None once the task was deleted. */
  schedule: Schedule | null,
  run_thread_id: string | null,
  status: RunStatus,
  memory_save_skip?: MemorySaveSkipReason,
  discarded_memory_items?: Array<DiscardedMemoryItem>,
  finished_at_ms: number,
  skipped_saved_keys?: Array<string>,
  learned?: LearnedDigest,
  /** Set when a check was met. The same card is filled when its run ends. */
  trigger?: HomeTrigger,
};

/**
 * Bot's post for a research session it handed off, once that session finished,
 * failed or stopped. A session at work posts nothing.
 */
export type HomeHandoff = {
  handoffId: string,
  chatId: string,
  title: string,
  connectionId?: string,
  status: HandoffStatus,
  memorySaveSkip?: MemorySaveSkipReason,
  discardedMemoryItems?: Array<DiscardedMemoryItem>,
  stopReason?: HandoffStopReason,
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

/** A preset the user can turn on with a tap. Nothing is scheduled until they do. */
export type HomeOffer = {
  id: string,
  preset: WatchPreset,
  assetId: string,
  assetName: string,
  resultsDate?: string,
  runAtMs?: number,
  reason: string,
  createdAtMs: number,
  expiresAtMs: number,
  taskId?: string,
  dismissed?: boolean,
};

export type HomeResultEvidence = {
  figuresChecked: number,
  figuresUnmatched: number,
  sources: number,
};

/** The reading a check posted, labelled as from the user's check. */
export type HomeTrigger = {
  reading: string,
  as_of: string,
  sources: Array<string>,
  checked_at_ms: number,
  explained: boolean,
  reason?: string,
  coalesced: boolean,
};

export type HomeVocabulary = {
  changeId: string,
  producer: string,
  labels: Array<string>,
  labelsKo?: Array<string>,
};

export type MapKind = { name: string, label: string, labelKo?: string };

export type MemorySaveSkipReason = "user" | "deadline" | "interrupted" | "failed" | "policy_changed";

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
  /** A Source the user pasted into the Chat (`SourceClass::UserProvided`). */
  userProvided?: boolean,
};

export type MemoryUpdateResult = {
  status: MemoryUpdateStatus,
  skipReason?: MemorySaveSkipReason,
  discardedMemoryItems?: Array<DiscardedMemoryItem>,
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

export type PythonRunTrace = { reads: number, rounds: number };

/** Which run of which scheduled task sent a request into its Chat. */
export type ScheduledTurn = {
  task_id: string,
  run_id: string,
  task_title: string,
  task_kind: TaskKind,
};

/** One applied `loupist_setup` change. A market the user did not give is `None`. */
export type SetupChange =
  | { "change": "followed", symbol: string, market?: string, name?: string }
  | { "change": "unfollowed", symbol: string, market?: string }
  | { "change": "focus", focus: string }
  | { "change": "tracks", kinds: Array<MapKind> };

export type ToolTraceOrigin = "python";

export type ToolTraceOutcome = "ok" | "error";

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
