// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { DatasetRef } from "../../datasets";
import type { LineView, MapCounts, MoveView, ReadView, ReasonView } from "../../knowledge/assets";

export type AssetFollowNameRequest = {
  loupistId: string,
  name: string,
  reference?: AssetReferenceFields | null,
};

export type AssetIdRequest = { loupistId: string, assetId: string };

export type AssetLookupCandidate = {
  figi: string,
  name: string,
  ticker: string | null,
  exchange: string | null,
  exchangeName?: string | null,
  securityType: string | null,
  marketSector: string | null,
};

export type AssetLookupRequest = { loupistId: string, query: string };

export type AssetOpenRequest = {
  loupistId: string,
  assetId?: string | null,
  ticker?: string | null,
  entityId?: string | null,
  storedOnly?: boolean | null,
};

export type AssetPricePoint = { value: number, date: string };

export type AssetPriceSeriesRequest = { loupistId: string, assetId: string, storedOnly?: boolean };

export type AssetQuoteKind =
  | "open"
  | "high"
  | "low"
  | "close"
  | "adjusted_close"
  | "last"
  | "bid"
  | "ask";

export type AssetReference = {
  origin: AssetReferenceOrigin,
  figi?: string | null,
  ticker?: string | null,
  exchange?: string | null,
  exchangeName?: string | null,
  securityType?: string | null,
};

export type AssetReferenceFields = {
  figi?: string | null,
  ticker?: string | null,
  exchange?: string | null,
  exchangeName?: string | null,
  securityType?: string | null,
};

export type AssetReferenceOrigin = "user" | "agent" | "openfigi" | "imported";

export type AssetRow = {
  assetId: string,
  name: string,
  reference: AssetReference | null,
  /** When the requesting Loupist followed the asset; null when it does not follow it. */
  followedAtMs: number | null,
  createdAtMs: number,
};

export type AssetStoredPrice = {
  kind?: AssetQuoteKind | null,
  value: number,
  date: string,
  currency: string | null,
  provider: string,
  sourceUrl: string | null,
  dataset: DatasetRef,
};

export type AssetsListRequest = { loupistId: string };

export type PriceFields = {
  value: string,
  date: string,
  currency?: string | null,
  rows?: string | null,
};

export type PriceReadState = "none" | "ready" | "failed" | "needs_connection";

export type RollingDates = { fromArg: string, toArg: string, days: number };

export type SavedPriceRead = {
  state: PriceReadState,
  current: SavedReadCall,
  history: SavedReadCall | null,
};

export type SavedReadCall = {
  tool: string,
  server: string | null,
  arguments: Record<string, unknown>,
  fields: PriceFields,
  rolling: RollingDates | null,
};

export type StoredAssetQuote = { assetId: string, price: AssetStoredPrice };

export type ThinAssetPageView = {
  asset: AssetRow,
  priceSetupRunning: boolean,
  currentPrice: AssetStoredPrice | null,
  savedRead: SavedPriceRead | null,
  entityId: string | null,
  reasons: Array<ReasonView>,
  lines: Array<LineView>,
  read: ReadView | null,
  history: Array<MoveView>,
  map: MapCounts | null,
  revision: number,
};
