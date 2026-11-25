// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { DatasetRef } from "../datasets";

export type PythonStep = {
  code: string,
  stdout: string,
  stderr: string,
  status: string,
  reads: Array<ReadSummary>,
  rounds: number,
};

export type ReadSummary = {
  tool: string,
  status: string,
  citationUrl?: string,
  provider?: string,
  asOf?: string,
  dataset?: DatasetRef,
};
