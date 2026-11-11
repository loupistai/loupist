// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

export type LearningSettings = { learnMethods: boolean };

export type Review = {
  reviewId: string,
  status: string,
  reason: string | null,
  lensRecordId: string | null,
  rationale: string | null,
  evidenceCount: number,
  createdAtMs: number,
};

export type ReviewQuery = { loupistId: string, limit?: number | null };
