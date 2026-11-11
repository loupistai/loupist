// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { AssetClass } from "../knowledge/assets/class";

export type AssetClassSetRequest = {
  loupistId: string,
  assetId: string,
  class: AssetClass,
  sinceMs?: number | null,
};

export type AssetDeltaRequest = {
  loupistId: string,
  /** Every page when omitted. */
  assetId?: string | null,
  sinceMs: number,
};

export type AssetPageGetRequest = {
  loupistId: string,
  /** An existing page; otherwise `symbol` and `market` name (and create) it. */
  assetId?: string | null,
  symbol?: string | null,
  market?: string | null,
  name?: string | null,
  /** The last visit, for "price since your last look"; 30 days when omitted. */
  sinceMs?: number | null,
};

export type AssetPageSeenRequest = { loupistId: string, assetId: string };

export type AssetRefreshRequest = { loupistId: string, assetId: string, sinceMs?: number | null };

export type AssetVerdictUndoRequest = {
  loupistId: string,
  assetId: string,
  /** The move the user saw as the latest; a newer move makes this a conflict. */
  moveId: string,
  sinceMs?: number | null,
};

export type UpcomingEventsRequest = { loupistId: string };
