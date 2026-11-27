// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { PresetParams } from "../scheduled_tasks/presets";
import type { ProgramPatch } from "../scheduled_tasks/program_edit";
import type { DeliveryTarget, NotifyPolicy, PresetId, Task, TaskStatus, TaskSubject, TriggerDryRun } from "../scheduled_tasks/types";

export type DeliveryRequest = {
  loupistId: string,
  taskId: string,
  expectedRevision: number,
  to?: DeliveryTarget | null,
  notify?: NotifyPolicy | null,
};

export type LearningProgramUndoAllRequest = {
  loupistId: string,
  taskId: string,
  /** Counts what Undo all would do and changes nothing. */
  preview?: boolean,
};

/** The current state of a task a Chat card shows; `None` once it was deleted. */
export type LearningProgramUpdateRequest = {
  loupistId: string,
  taskId: string,
  revision: number,
  patch: ProgramPatch,
};

/**
 * Watch presets and learning presets share this command. `preset` is the watch
 * id or a learning preset id such as `asset-daily` or `market`. `subject` is
 * the one learning subject; watch presets keep using `subjects`. `slots` fills
 * a learning preset's text slots, such as the market's name.
 */
export type PresetCommand = {
  loupistId: string,
  preset: PresetId,
  subjects?: Array<TaskSubject> | null,
  subject?: TaskSubject | null,
  params?: PresetParams | null,
  /**
   * A learning preset's text slots by name, such as the market's name.
   * Watch presets take none.
   */
  slots?: { [key in string]: string } | null,
};

export type ProposalDeclineRequest = {
  loupist_id: string,
  task_id: string,
  visit: number,
  declined: boolean,
};

export type ProposalVisitRequest = { loupist_id: string, visit: number };

export type RevisionRequest = { loupist_id: string, task_id: string, expected_revision: number };

export type RunNowRequest = { loupist_id: string, task_id: string, confirm?: boolean };

export type RunRequest = { loupist_id: string, task_id: string, run_id: string };

export type ScheduledTasksLoupistRequest = { loupist_id: string };

export type StatusRequest = {
  loupist_id: string,
  task_id: string,
  expected_revision: number,
  status: TaskStatus,
};

export type TaskRequest = { loupist_id: string, task_id: string };

export type TriggerGetRequest = { loupist_id: string, task_id: string, version: number | null };

export type TriggerRevertRequest = { loupistId: string, taskId: string, toVersion: number };

export type TriggerReviewRequest = {
  loupist_id: string,
  task_id: string,
  expected_revision: number,
  version: number,
  sha256: string,
};

export type TriggerView = {
  code: string,
  summary: string,
  version: number,
  sha256: string,
  dry_run?: TriggerDryRun,
  task: Task,
};
