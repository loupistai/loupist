// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { JsonValue } from "../external";
import type { AssetEvidence } from "../knowledge/assets";

/** A stored event with its asset's names (never holdings). */
export type WatchEventView = {
  eventId: string,
  assetId: string,
  name: string,
  symbol: string,
  market: string,
  kind: WatchKind,
  factDate: string,
  detectedAtMs: number,
  seenAtMs?: number,
  deliveredIn?: string,
  facts: JsonValue,
  evidence: Array<AssetEvidence>,
};

/**
 * What a Loupist can watch per asset: six kinds for equities (v1) and the
 * class kinds for crypto and commodities (finance-crypto-and-templates.md §3.3).
 */
export type WatchKind =
  | "filings"
  | "results"
  | "insiders"
  | "holders"
  | "flows"
  | "dividends"
  | "cryptoMove"
  | "drawdown"
  | "depeg"
  | "tvl"
  | "futuresMove"
  | "positioning"
  | "oilInventory";
