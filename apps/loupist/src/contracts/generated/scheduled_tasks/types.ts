// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { BriefingContext } from "../briefings";

export type QuestionReceipt = { task_id: string, title: string, question: string };

export type Run = {
  briefing_context: BriefingContext | null,
  kind: TaskKind,
  read_at_ms: number | null,
  summary: string,
  summary_items?: Array<RunSummaryItem>,
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
};

export type RunStatus = "running" | "succeeded" | "failed" | "interrupted";

/**
 * What a host run did, one item per capture, as fields the renderer words in
 * the reader's language; the results task (`commands/earnings.rs`) writes them,
 * so each kind is named for it.
 */
export type RunSummaryItem =
  | { "kind": "earnings_expected", name: string, period: string, count: number }
  | { "kind": "earnings_reported", name: string, period: string, count: number }
  | { "kind": "earnings_failed", name: string, period: string }
  | { "kind": "earnings_nothing" };

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
  kind: TaskKind,
  id: string,
  loupist_id: string,
  revision: number,
  title: string,
  prompt: string,
  schedule: Schedule,
  target: TaskTarget,
  session_thread_id: string | null,
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
  /** The reply that proposed it; that reply cannot turn it on in the user's place. */
  proposed_in_message_id?: string,
};

/**
 * A kind another Loupist version wrote and this one does not know reads as
 * `Unknown` with its stored name, so every rewrite of its task or run (a
 * finish, a pause after a failure, a deleted target Chat) writes the name back
 * unchanged. Such a task is listed but never run, and its definition and
 * status are not changed here (`TaskKind::is_known`); it can still be deleted.
 */
export type TaskKind = "custom" | "briefing" | "earnings" | string;

/** Read-only list projection; the latest occurrence is not stored in Task JSON. */
export type TaskListItem = {
  latest_run: Run | null,
  kind: TaskKind,
  id: string,
  loupist_id: string,
  revision: number,
  title: string,
  prompt: string,
  schedule: Schedule,
  target: TaskTarget,
  session_thread_id: string | null,
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
  /** The reply that proposed it; that reply cannot turn it on in the user's place. */
  proposed_in_message_id?: string,
};

/** A saved action snapshot, displayed even if the rest of the Chat stops. */
export type TaskReceipt = {
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
};

export type TaskStatus = "active" | "paused" | "completed";

export type TaskTarget =
  | { "kind": "new_chat" }
  | { "kind": "dedicated_chat" }
  | { "kind": "existing_chat", thread_id: string };
