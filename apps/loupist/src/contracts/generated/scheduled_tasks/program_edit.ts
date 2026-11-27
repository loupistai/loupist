// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { LearningDepth, LearningKeeps, LearningScope, ReportPolicy, Schedule, TaskDelivery, TaskSubject } from "../scheduled_tasks/types";

/** The fields the editor can change. An absent field stays as saved. */
export type ProgramPatch = {
  title?: string | null,
  /** What to learn: the brief, sent as each run's request. */
  prompt?: string | null,
  subjects?: Array<TaskSubject> | null,
  schedule?: Schedule | null,
  depth?: LearningDepth | null,
  /** The connections a run may read and whether it may use the web. */
  sources?: LearningScope | null,
  keeps?: LearningKeeps | null,
  report?: ReportPolicy | null,
  delivery?: TaskDelivery | null,
};
