// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

export type LearningPhase = "waiting" | "idle" | "queued" | "running";

export type LearningSettings = { learnMethods: boolean };

/** Where method learning stands for a Loupist, and why it waits. */
export type LearningState = {
  state: LearningPhase,
  /** Set exactly when `state` is `waiting`. */
  reason: WaitReason | null,
  /**
   * How many distinct cases the research no review has consumed holds.
   * Pages about one subject, by node, key or component, are one case.
   */
  cases: number,
};

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

/** Why method learning waits. */
export type WaitReason =
  | { "kind": "off" }
  | { "kind": "no_second_case" }
  | { "kind": "subjects_pending", n: number }
  | { "kind": "sources_unread", n: number }
  | { "kind": "rate_limited", untilMs: number }
  | { "kind": "allowance" };
