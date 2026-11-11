// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { TaskStatus } from "../scheduled_tasks/types";

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
