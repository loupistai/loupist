// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { DateSource } from "../../knowledge/earnings";

/** The page an event belongs to; the renderer names it in the reader's language. */
export type UpcomingAsset = {
  assetId: string,
  symbol: string,
  market: string,
  name: string,
  entityId?: string,
};

export type UpcomingEvent = {
  kind: UpcomingKind,
  /** The calendar day, YYYY-MM-DD. */
  date: string,
  asset?: UpcomingAsset,
  /** Results: where the date comes from; a legal deadline is a bound, never an expected date. */
  dateSource?: DateSource,
  /** Results: the event the asset page's results card opens on. */
  eventId?: string,
  period?: string,
  /** Check: the Journal entry it opens. */
  journalId?: string,
  claim?: string,
};

/** Ordered as rows of one day are listed: results, then dividends, then checks. */
export type UpcomingKind = "results" | "exDividend" | "recordDate" | "check";
