// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { JobFailure, JobHold } from "./knowledge/graph_jobs";

export type SourceProcessing = {
  jobId: string | null,
  status: string,
  error: string | null,
  failure?: JobFailure,
  retryable?: boolean,
  /** Waiting on the daily allowance. The status stays queued. */
  hold?: JobHold,
};
