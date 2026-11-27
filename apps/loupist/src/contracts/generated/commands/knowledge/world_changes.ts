// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { TimePoint } from "../../knowledge/model";

export type AboutInput = string | AboutRefInput;

export type AboutRefInput = { node?: string, asset?: string, lens?: string };

export type BetweenInput = { asset?: string, to?: string, from?: string };

export type ChangeClaim = {
  claimId: string,
  proposition: string,
  relation: string,
  status: string,
  validAt: TimePoint | null,
  invalidAt: TimePoint | null,
};

export type ChangeConflict = { claimId: string, withClaimId: string };

export type ChangePair = { from: string, to: string };

export type ChangeSource = {
  recordId: string,
  title: string,
  publicationTime: string | null,
  episodes: number,
  /** True while World is still reading the newer report. */
  pending: boolean,
};

export type ClaimHistory = {
  created: unknown,
  supported: unknown [],
  ended: unknown,
  /** Endings a later reopen replaced. The current relationship is `ended`. */
  endings: unknown [],
  endedClaims: Array<string>,
  endedByClaimId: string | null,
};

export type ClaimHistoryRequest = {
  loupistId: string,
  claimId: string,
  /**
   * The claim's history in World as of a calendar day (`YYYY-MM-DD`,
   * today or earlier), as `WorldGraphQuery.asOf` reads it.
   */
  asOf?: string,
};

/**
 * Bridge read of the change feed. `between.asset` picks the latest report and the one before it.
 */
export type WorldChangesRequest = {
  loupistId: string,
  since?: string,
  axis?: string,
  about?: Array<AboutInput>,
  kinds?: Array<string>,
  limit?: number,
  between?: BetweenInput,
  /**
   * A `since` read of World as of a calendar day (`YYYY-MM-DD`, today or
   * earlier), as `WorldGraphQuery.asOf` reads it: the rows of the World
   * that had read only what was published by then.
   */
  asOf?: string,
};

export type WorldChangesView = {
  mode: string,
  /** Null when the asset has no earlier report of the same company and form. */
  pair: ChangePair | null,
  from: ChangeSource | null,
  to: ChangeSource | null,
  endedByTo: Array<ChangeClaim>,
  newInTo: Array<ChangeClaim>,
  restated: Array<ChangeClaim>,
  notRestated: Array<ChangeClaim>,
  conflicts: Array<ChangeConflict>,
  window: unknown,
  pending: number,
  failed: number,
  changes: unknown [],
  lens: unknown [],
  firstRun: boolean,
  capped: boolean,
  next: string | null,
};
