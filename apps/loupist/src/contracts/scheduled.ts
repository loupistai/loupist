import type { ChatThread } from "./chat";
import type { AssetChangeRow, Verdict } from "./assets";
import type { EarningsMeasure, EarningsStatus } from "./earnings";
import type { WatchEvent } from "./watch";
import type * as Wire from "./generated";

export type ScheduledCadence =
  | { kind: "once"; at_ms: number }
  | { kind: "interval"; every_minutes: number }
  | { kind: "daily"; hour: number; minute: number; timezone: string }
  | { kind: "weekly"; days: number[]; hour: number; minute: number; timezone: string };

export type ScheduledTarget = { kind: "new_chat" } | { kind: "dedicated_chat" } | { kind: "existing_chat"; thread_id: string };
export type ScheduledTaskStatus = "active" | "paused" | "completed";
export type ScheduledTaskDefinition = {
  title: string;
  prompt: string;
  schedule: ScheduledCadence;
  target: ScheduledTarget;
  model_profile_id: string;
  thinking_level: string;
  run_options: Record<string, string>;
  use_memory: boolean;
  update_memory: boolean;
  status: "active" | "paused";
};
/**
 * `earnings` is the automatic "Results before and after" task: one per Loupist,
 * created by Rust, no model call; Pause/Resume and Run now only (no Edit, no Delete).
 */
export type ScheduledTaskKind = "custom" | "briefing" | "earnings";
export type ScheduledTask = Omit<ScheduledTaskDefinition, "status"> & {
  kind: ScheduledTaskKind;
  id: string;
  loupist_id: string;
  revision: number;
  status: ScheduledTaskStatus;
  next_run_at_ms: number | null;
  last_run_at_ms: number | null;
  created_at_ms: number;
  updated_at_ms: number;
  last_error: string | null;
  session_thread_id: string | null;
  /** Created from a conversation; it stays paused until the user confirms it in the Chat card. */
  awaiting_confirmation: boolean;
  /** The reply that proposed it; that reply cannot turn it on in the user's place. */
  proposed_in_message_id?: string;
};
/** A task as `scheduledTasksList` returns it, with its latest occurrence. */
export type ScheduledTaskListItem = ScheduledTask & { latest_run: ScheduledRun | null };
export type BriefingChange = Wire.BriefingChange;
export type BriefingChanged = Wire.BriefingChanged;
export type BriefingCheck = Wire.BriefingCheck;
/** Host-computed at run start; the reader renders sections from it and never parses the narrative. */
export type BriefingContext = {
  symbols: string[]; principlesChecked: number; newFacts: number; disputedEntityIds: string[];
  changed: BriefingChanged; ahead: BriefingCheck[];
  /** "What you follow": followed assets with a delta, rows as the asset page renders them. */
  assets: BriefingAsset[];
  /** Results events around this briefing (at most 8), computed by Rust at run start. */
  earnings?: BriefingEarnings[];
  /** Watcher alerts since the previous briefing (at most 24); filings, insiders, 5% holders and flows join "What you follow". */
  watchEvents?: WatchEvent[];
};
/**
 * One results event in a briefing: reported ones show the 2-measure mini table
 * and "See before and after"; upcoming ones go to "Looking ahead".
 */
export type BriefingEarnings = {
  assetId: string; eventId: string; name: string; symbol: string; market: string;
  period: string; reportDate: string; status: EarningsStatus;
  /** Expectations were captured before the report. */
  captured: boolean;
  /** At most two measures (reported events). */
  measures: EarningsMeasure[];
};
export type BriefingAsset = {
  assetId: string; symbol: string; market: string; name: string;
  verdict: Verdict | null; verdictSince: string | null;
  rows: AssetChangeRow[];
};
export type RunSummaryItem =
  | { kind: "earnings_expected" | "earnings_reported"; name: string; period: string; count: number }
  | { kind: "earnings_failed"; name: string; period: string }
  | { kind: "earnings_nothing" };
export type ScheduledRun = {
  briefing_context: BriefingContext | null;
  kind: ScheduledTaskKind;
  summary: string;
  summary_items?: RunSummaryItem[];
  read_at_ms: number | null;
  message_id: string | null;
  skipped_tools: SkippedTool[];
  id: string;
  task_id: string;
  loupist_id: string;
  task_revision: number;
  task_title: string;
  thread_id: string | null;
  run_id: string;
  scheduled_for_ms: number;
  started_at_ms: number;
  finished_at_ms: number | null;
  status: "running" | "succeeded" | "failed" | "interrupted";
  error: string | null;
};
export type ScheduledTaskIdentity = { loupist_id: string; task_id: string };
export type ScheduledRunIdentity = ScheduledTaskIdentity & { run_id: string };
export type ScheduledTaskCreateRequest = ScheduledTaskDefinition & { loupist_id: string };
export type ScheduledTaskWatch = { run: ScheduledRun; thread: ChatThread };

export type ScheduleReceipt = { task_id: string; title: string; question: string } | TaskScheduleReceipt;
export type TaskScheduleReceipt = {
  task_id: string;
  title: string;
  revision: number;
  action: "created" | "updated" | "paused" | "resumed" | "deleted" | "started" | "stopped" | "viewed";
  status: ScheduledTaskStatus;
  schedule: ScheduledCadence;
  target: ScheduledTarget;
  next_run_at_ms: number | null;
  use_memory: boolean;
  update_memory: boolean;
  /** Created from a conversation; it stays paused until the user taps Turn on in the card. */
  awaiting_confirmation: boolean;
};

export type SkippedTool = Wire.SkippedTool;
export type BriefingSettings = { enabled: boolean; schedule: ScheduledCadence; task: ScheduledTask | null };
/** App-wide: keep the process in the menu bar after the window closes so schedules keep running. */
export type BriefingResidentSettings = { keepRunning: boolean; notificationPermissionAsked: boolean };
export type BriefingSettingsRequest = Wire.BriefingSettingsRequest;
export type InboxItem = {
  briefingContext: BriefingContext | null; error: string | null;
  runId: string; taskId: string; kind: ScheduledTaskKind; title: string;
  status: ScheduledRun["status"]; summary: string; summaryItems: RunSummaryItem[]; startedAtMs: number;
  finishedAtMs: number | null; readAtMs: number | null; threadId: string | null;
  messageId: string | null; skippedTools: SkippedTool[];
};
export type InboxRequest = Wire.InboxRequest;
export type InboxPage = { items: InboxItem[]; nextCursor: string | null };
export type InboxUnreadCount = { loupistId: string; count: number };
export type InboxChangedEvent = Wire.InboxChanged;
export type InboxOpenEvent = Wire.InboxOpen;
