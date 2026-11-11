// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { CsvValidationError } from "../../store/user_context";

export type CryptoFileFormat = "kraken_ledger" | "balances" | "mapped";

export type CryptoFilePreview = {
  previewId: string,
  format: CryptoFileFormat | null,
  rowCount: number,
  /** The latest ledger time, when the file has one. */
  asOfMs: number | null,
  rows: Array<CryptoPreviewRow>,
  skipped: Array<CryptoSkipped>,
  errors: Array<CsvValidationError>,
  /** Present until the user picks the asset and balance columns. */
  mappingHeaders: Array<string> | null,
  /** A trade history without balances; refused. */
  tradesOnly: boolean,
};

export type CryptoPreviewRow = {
  symbol: string,
  quantity: string,
  /** The file's own codes that merged into this asset (XETH, ETH2.S). */
  from: Array<string>,
};

export type CryptoSkipReason = "cash" | "zero";

export type CryptoSkipped = { symbol: string, reason: CryptoSkipReason };
