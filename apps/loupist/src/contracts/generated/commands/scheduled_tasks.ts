// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { PresetParams } from "../scheduled_tasks/presets";
import type { DeliveryTarget, NotifyPolicy, Task, TaskStatus, TaskSubject, TriggerDryRun, WatchPreset } from "../scheduled_tasks/types";

export type DeliveryRequest = {
  loupistId: string,
  taskId: string,
  expectedRevision: number,
  to?: DeliveryTarget | null,
  notify?: NotifyPolicy | null,
};

export type FromPresetRequest = {
  loupistId: string,
  preset: WatchPreset,
  subjects?: Array<TaskSubject> | null,
  params?: PresetParams | null,
};

export type ProposalDeclineRequest = {
  loupist_id: string,
  task_id: string,
  visit: number,
  declined: boolean,
};

export type ProposalVisitRequest = { loupist_id: string, visit: number };

export type RevisionRequest = { loupist_id: string, task_id: string, expected_revision: number };

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
