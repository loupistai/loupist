// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

/** The run that filed a section's newest op, while no later op changed it. */
export type LearnedMark = {
  atMs: number,
  taskId: string,
  /** The run's `run_id`, which Schedule opens. */
  runId: string,
  /** The program's current title. */
  programTitle: string,
};
