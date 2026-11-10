// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { JsonValue } from "./external";

/** One followed asset in a briefing, computed by Rust at run start. */
export type BriefingAsset = {
  assetId: string,
  symbol: string,
  market: string,
  name: string,
  verdict: string | null,
  verdictSince: string | null,
  /** At most three rows, as the asset page renders them. */
  rows: Array<JsonValue>,
};

export type BriefingChange = {
  /** `claim_added`, `claim_disputed` or `source_retained`. */
  kind: string,
  subjectId: string | null,
  summary: string,
};

/** What changed in World since the previous briefing, counted by the host. */
export type BriefingChanged = {
  names: number,
  relations: number,
  conflicts: number,
  /** Newest first, at most `MAX_CHANGE_ROWS`. */
  rows: Array<BriefingChange>,
};

/** A Journal check that is due, pointed to rather than quoted in full. */
export type BriefingCheck = { journalId: string, title: string, byDate: string | null };

export type BriefingContext = {
  symbols: Array<string>,
  principlesChecked: number,
  newFacts: number,
  disputedEntityIds: Array<string>,
  /** Host-computed sections for the Schedule reader; the narrative is never parsed. */
  changed: BriefingChanged,
  ahead: Array<BriefingCheck>,
  /** "What you follow": followed assets with a delta since the last briefing. */
  assets: Array<BriefingAsset>,
  /**
   * Results events around this briefing (at most 8): reported ones with two
   * measures, upcoming ones for "Looking ahead".
   */
  earnings?: Array<BriefingEarnings>,
  /** Watcher alerts since the previous briefing (at most 24), as stored. */
  watchEvents?: Array<JsonValue>,
};

/** One results event in a briefing, computed by Rust at run start. */
export type BriefingEarnings = {
  assetId: string,
  eventId: string,
  name: string,
  symbol: string,
  market: string,
  period: string,
  reportDate: string,
  status: string,
  /** Expectations were captured before the report. */
  captured: boolean,
  /** At most two measures, as the card's table renders them. */
  measures: Array<JsonValue>,
};
