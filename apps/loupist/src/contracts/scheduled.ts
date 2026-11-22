import type { ChatThread } from "./chat";
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
/** The scheduler holds only agent work: the briefing and custom tasks (an alert is a custom task whose prompt states the condition). */
export type ScheduledTaskKind = "custom" | "briefing";
export type ScheduledTask = Omit<ScheduledTaskDefinition, "status"> & {
  model_selection?: Wire.ResolvedModelSelection;
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
export type BriefingContext = Wire.BriefingContext;
export type ScheduledRun = Wire.Run;
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
/** `keepRunningChoice` is the user's explicit menu-bar choice, which the host never overrides; null keeps the automatic default. */
export type BriefingResidentSettings = Wire.DesktopSettings;
export type BriefingSettingsRequest = Wire.BriefingSettingsRequest;
export type InboxItem = Wire.InboxItem;
export type InboxRequest = Wire.InboxRequest;
export type InboxPage = { items: InboxItem[]; nextCursor: string | null };
export type InboxUnreadCount = { loupistId: string; count: number };
export type InboxChangedEvent = Wire.InboxChanged;
export type InboxOpenEvent = Wire.InboxOpen;
