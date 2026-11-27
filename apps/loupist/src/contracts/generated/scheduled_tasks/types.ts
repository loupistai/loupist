// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { BriefingContext } from "../briefings";
import type { ChatCostEstimate, DiscardedMemoryItem, MemorySaveSkipReason } from "../domain/chat";
import type { MessageUndo } from "../knowledge/lens";
import type { ResolvedModelSelection } from "../providers/selection";
import type { Hold } from "../scheduled_tasks/allowance";

export type AllLearningConnections = "all_enabled";

/**
 * Which built-in briefing a task is. Missing on a briefing written before
 * editions means the morning one.
 */
export type BriefingEdition = "morning" | "evening";

export type BudgetOutcome = "none" | "soft" | "refused" | "cancelled";

export type DeliveryTarget = "bot" | "none" | "briefing";

/** Whether the event's day is announced or only expected. */
export type EventPrecision = "exact" | "estimated";

export type FailureStreak = { id: string, count: number };

export type LearningBudget = { soft: number, hard: number };

export type LearningConnections = AllLearningConnections | Array<string>;

export type LearningDepth = "light" | "standard" | "deep";

export type LearningKeeps = { sources: boolean, lens: boolean, asset_reads: boolean };

/** A bundled learning preset, by the name of its file in `agent/learning-presets/`. */
export type LearningPresetId =
  | "asset-daily"
  | "theme"
  | "company-filings"
  | "macro-series"
  | "question"
  | "market";

export type LearningProgram = {
  depth: LearningDepth,
  scope: LearningScope,
  keeps: LearningKeeps,
  /**
   * The learning preset the program was created from, for display. After
   * creation the program is ordinary data. Absent on older programs.
   */
  preset?: string,
};

export type LearningScope = { connections: LearningConnections, web: boolean };

export type NotifyPolicy = "now" | "digest" | "off";

/**
 * What `scheduled_task_from_preset` opens: a watch preset or a learning
 * preset. `asset-daily` reads as the watch id it has always been.
 */
export type PresetId = WatchPreset | LearningPresetId;

export type ProposalTarget = { id: string, revision: number };

export type QuestionReceipt = { task_id: string, title: string, question: string };

export type ReportPolicy = "when_met" | "every_run" | "when_new" | "on_condition";

export type Run = {
  usage?: RunUsage,
  budget?: RunBudget,
  failure_streak?: FailureStreak,
  first_run?: boolean,
  memory_save_skip?: MemorySaveSkipReason,
  discarded_memory_items?: Array<DiscardedMemoryItem>,
  briefing_context: BriefingContext | null,
  kind: TaskKind,
  read_at_ms: number | null,
  summary: string,
  quiet: boolean,
  message_id: string | null,
  skipped_tools: Array<SkippedTool>,
  id: string,
  task_id: string,
  loupist_id: string,
  task_revision: number,
  task_title: string,
  thread_id: string | null,
  run_id: string,
  scheduled_for_ms: number,
  started_at_ms: number,
  finished_at_ms: number | null,
  status: RunStatus,
  error: string | null,
  trigger?: RunTrigger,
  /** Sections this run's message restored and kept, when that undo is already stored. */
  undo?: MessageUndo,
};

export type RunBudget = {
  requests: number,
  soft: number,
  hard: number,
  left_at_start: number,
  outcome: BudgetOutcome,
};

export type RunStatus = "running" | "succeeded" | "failed" | "interrupted";

export type RunTrigger = { version: number, reading: string, as_of: string };

export type RunUsage = {
  requests: number | null,
  inputTokens: number | null,
  cachedInputTokens: number | null,
  outputTokens: number | null,
  durationMs: number,
  worldRequests?: number,
  costEstimate?: ChatCostEstimate,
};

export type Schedule =
  | { "kind": "once", at_ms: number }
  | { "kind": "interval", every_minutes: number }
  | { "kind": "daily", hour: number, minute: number, timezone: string }
  | { "kind": "weekly", days: Array<number>, hour: number, minute: number, timezone: string };

export type ScheduleAction =
  | "created"
  | "updated"
  | "paused"
  | "resumed"
  | "deleted"
  | "started"
  | "stopped"
  | "viewed";

export type ScheduleReceipt = TaskReceipt | QuestionReceipt;

export type SkippedTool = { name: string, reason: string };

export type Task = {
  program?: LearningProgram,
  budget?: LearningBudget,
  hold?: Hold,
  kind: TaskKind,
  id: string,
  loupist_id: string,
  revision: number,
  title: string,
  prompt: string,
  schedule: Schedule,
  target: TaskTarget,
  session_thread_id: string | null,
  model_selection?: ResolvedModelSelection,
  model_profile_id: string,
  thinking_level: string,
  run_options: { [key in string]: string },
  use_memory: boolean,
  update_memory: boolean,
  status: TaskStatus,
  next_run_at_ms: number | null,
  last_run_at_ms: number | null,
  created_at_ms: number,
  updated_at_ms: number,
  last_error: string | null,
  /**
   * A task the agent created from a conversation stays paused until the user confirms it
   * in the Chat card; any later status decision clears this.
   */
  awaiting_confirmation: boolean,
  declined_in_visit?: number,
  /** The reply that proposed it; that reply cannot turn it on in the user's place. */
  proposed_in_message_id?: string,
  trigger?: TaskTrigger,
  /** Set on a briefing. Absent means morning, including tasks saved before editions. */
  edition?: BriefingEdition,
  replaces?: ProposalTarget,
  /** Lets the most recent approved edit card continue to open this task. */
  confirmed_proposal_id?: string,
  report?: ReportPolicy,
  delivery?: TaskDelivery,
  subjects?: Array<TaskSubject>,
  preset?: WatchPreset,
  terms?: string,
  /** The dated event a results follow-up is about. Absent on older tasks. */
  event?: TaskEvent,
};

export type TaskDelivery = { to: DeliveryTarget, notify?: NotifyPolicy };

/** A dated event, such as a results release, that a task follows up. */
export type TaskEvent = {
  /** The event's day, `YYYY-MM-DD`. */
  date: string,
  precision: EventPrecision,
};

/**
 * A kind another Loupist version wrote and this one does not know reads as
 * `Unknown` with its stored name, so every rewrite of its task or run (a
 * finish, a pause after a failure, a deleted target Chat) writes the name back
 * unchanged. Such a task is listed but never run, and its definition and
 * status are not changed here (`TaskKind::is_known`); it can still be deleted.
 */
export type TaskKind = "custom" | "briefing" | "learning" | string;

/** Read-only list projection; the latest occurrence is not stored in Task JSON. */
export type TaskListItem = {
  latest_run: Run | null,
  program?: LearningProgram,
  budget?: LearningBudget,
  hold?: Hold,
  kind: TaskKind,
  id: string,
  loupist_id: string,
  revision: number,
  title: string,
  prompt: string,
  schedule: Schedule,
  target: TaskTarget,
  session_thread_id: string | null,
  model_selection?: ResolvedModelSelection,
  model_profile_id: string,
  thinking_level: string,
  run_options: { [key in string]: string },
  use_memory: boolean,
  update_memory: boolean,
  status: TaskStatus,
  next_run_at_ms: number | null,
  last_run_at_ms: number | null,
  created_at_ms: number,
  updated_at_ms: number,
  last_error: string | null,
  /**
   * A task the agent created from a conversation stays paused until the user confirms it
   * in the Chat card; any later status decision clears this.
   */
  awaiting_confirmation: boolean,
  declined_in_visit?: number,
  /** The reply that proposed it; that reply cannot turn it on in the user's place. */
  proposed_in_message_id?: string,
  trigger?: TaskTrigger,
  /** Set on a briefing. Absent means morning, including tasks saved before editions. */
  edition?: BriefingEdition,
  replaces?: ProposalTarget,
  /** Lets the most recent approved edit card continue to open this task. */
  confirmed_proposal_id?: string,
  report?: ReportPolicy,
  delivery?: TaskDelivery,
  subjects?: Array<TaskSubject>,
  preset?: WatchPreset,
  terms?: string,
  /** The dated event a results follow-up is about. Absent on older tasks. */
  event?: TaskEvent,
};

/** A saved action snapshot, displayed even if the rest of the Chat stops. */
export type TaskReceipt = {
  program?: LearningProgram,
  budget?: LearningBudget,
  task_id: string,
  title: string,
  revision: number,
  action: ScheduleAction,
  status: TaskStatus,
  schedule: Schedule,
  target: TaskTarget,
  next_run_at_ms: number | null,
  use_memory: boolean,
  update_memory: boolean,
  /** Created from a conversation and waiting for the user's one-tap OK in its card. */
  awaiting_confirmation: boolean,
  trigger?: TaskTrigger,
  report?: ReportPolicy,
  delivery?: TaskDelivery,
  subjects?: Array<TaskSubject>,
  preset?: WatchPreset,
  terms?: string,
  /** The dated event a results follow-up is about. Absent on older tasks. */
  event?: TaskEvent,
};

export type TaskStatus = "active" | "paused" | "completed";

export type TaskSubject =
  | { asset: string }
  | { world: string }
  | { lens: string }
  | { name: string };

export type TaskTarget =
  | { "kind": "new_chat" }
  | { "kind": "dedicated_chat" }
  | { "kind": "existing_chat", thread_id: string };

/**
 * The check on a custom task. Code lives in `scripts`; this is the pin,
 * the approval, and the last evaluations.
 */
export type TaskTrigger = {
  script: TriggerScriptRef,
  summary: string,
  /**
   * One line the card shows for "When it is met". Missing on a trigger saved
   * before this field; the view then uses the preset's locale line, or one
   * generic line when the task has no preset.
   */
  when_met_summary?: string,
  tools: Array<TriggerToolGrant>,
  args: Record<string, unknown>,
  active_hours?: TriggerActiveHours,
  approved?: TriggerApproval,
  pending?: TriggerPending,
  state: TriggerPhase,
  /** The script's `loupist.state`, kept between evaluations. Not the phase. */
  script_state: Record<string, unknown>,
  last?: TriggerEvaluation,
  evaluations: Array<TriggerEvaluation>,
  counters: TriggerCounters,
  /** Versions the user has turned on, so Go back can re-pin only those. */
  approved_versions: Array<TriggerApproval>,
  stale_after_days: number,
  reads_per_check: number,
  proposal_test?: TriggerDryRun,
};

export type TaskWatch = {
  report?: ReportPolicy,
  delivery?: TaskDelivery,
  subjects?: Array<TaskSubject>,
  preset?: WatchPreset,
  terms?: string,
  /** The dated event a results follow-up is about. Absent on older tasks. */
  event?: TaskEvent,
};

export type TriggerActiveHours = {
  days: Array<number>,
  start: string,
  end: string,
  timezone: string,
};

export type TriggerApproval = {
  version: number,
  sha256: string,
  at_ms: number,
  configuration?: TriggerConfiguration,
  test?: TriggerDryRun,
};

export type TriggerConfiguration = {
  summary?: string,
  /** What the user is told when the check is met. A preset stores a locale key. */
  when_met_summary?: string,
  args: Record<string, unknown>,
  tools: Array<TriggerToolGrant>,
  reads_per_check: number,
  active_hours?: TriggerActiveHours,
};

export type TriggerCounters = {
  today: TriggerDayCount,
  recent: Array<TriggerDayCount>,
  agent_runs_today: number,
  error_streak: number,
  no_data_streak: number,
  backoff_ms: number,
  as_of_since_ms?: number,
  as_of_value?: string,
  reads_blocked: boolean,
};

export type TriggerDayCount = { day: string, evaluations: number, reads: number, met: number };

export type TriggerDryDay = { date: string, outcome: TriggerOutcome, reading: string };

/** What `trigger_save` shows on the proposal or update card. */
export type TriggerDryRun = {
  today_met: boolean,
  today_reading: string,
  today_outcome: TriggerOutcome,
  fired_on: Array<string>,
  replayed: number,
  unreplayed: number,
  days: Array<TriggerDryDay>,
  reads: number,
  error?: string,
  scores?: TriggerScores,
};

export type TriggerEvaluation = {
  at_ms: number,
  scheduled_for_ms: number,
  outcome: TriggerOutcome,
  reading: string,
  as_of?: string,
  evidence: unknown,
  version: number,
  sha256: string,
  reads: number,
  error?: string,
  coalesced: boolean,
  ledger: unknown [],
  card_message_id?: string,
  retry_after_seconds?: number,
};

export type TriggerOutcome =
  | "met"
  | "not_met"
  | "error"
  | "no_data"
  | "rate_limited"
  | "stale"
  | "skipped_busy";

export type TriggerPending = {
  version: number,
  sha256: string,
  test?: TriggerDryRun,
  configuration?: TriggerConfiguration,
};

/**
 * `proposed` waits for Turn on. `on` is the approved check. `update_proposed`
 * keeps running the approved hash while a newer one waits. `failing` has
 * stopped after three code errors.
 */
export type TriggerPhase = "proposed" | "on" | "update_proposed" | "failing";

export type TriggerScoreDay = { date: string, previous: TriggerOutcome, next: TriggerOutcome };

export type TriggerScores = {
  previous_met: number,
  next_met: number,
  compared: number,
  days: Array<TriggerScoreDay>,
};

export type TriggerScriptRef = { name: string, version: number, sha256: string };

export type TriggerToolGrant = { name: string, provider?: string };

export type WatchPreset =
  | "filings-and-results"
  | "thesis-lines"
  | "results-follow-up"
  | "price-move"
  | "week-in-review"
  | "fact-changes"
  | "asset-daily";
