// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

export type CsvValidationError = { row: number, field: string, message: string };

export type HoldingsCsvPreview = {
  previewId: string,
  /** Canonical field -> original header. Unrecognized columns are discarded. */
  mapping: { [key in string]: string },
  positions: Array<HoldingsPosition>,
  errors: Array<CsvValidationError>,
  rowCount: number,
};

/** Per-source and per-kind counts and dates; never positions or amounts. */
export type HoldingsOverview = {
  sources: Array<SourceSummary>,
  stocks: KindSummary | null,
  crypto: KindSummary | null,
};

export type HoldingsPosition = {
  symbol: string,
  market: string,
  displayName: string | null,
  quantity: string,
  /** Average acquisition price per unit, in the position currency. */
  costBasis: string | null,
  marketValue: string | null,
  /** Empty means the source did not declare a currency; never infer FX. */
  currency: string,
};

export type HoldingsSnapshot = {
  snapshotId: string,
  source: HoldingsSource,
  importedAtMs: number,
  /** Empty for mixed or unspecified currencies. No implicit conversion. */
  currency: string,
  note: string | null,
  positions: Array<HoldingsPosition>,
};

export type HoldingsSource = "csv" | "kis" | "crypto_file" | "wallet";

export type KindSummary = { importedAtMs: number, count: number };

/** Identity-only projection for the Map; quantities and values never cross this boundary. */
export type MapContextItem = { symbol: string, market: string, displayName: string | null };

export type SourceSummary = { source: HoldingsSource, importedAtMs: number, count: number };

export type WatchlistEntry = {
  symbol: string,
  market: string,
  displayName: string | null,
  addedAtMs: number,
};
