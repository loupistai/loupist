import type * as Wire from "./generated";
/** App-level factual context shared by every Loupist. No suitability profile. */
export type WatchlistEntry = Wire.WatchlistEntry;
export type WatchlistAddRequest = Wire.WatchlistAddRequest;
export type HoldingsPosition = Wire.HoldingsPosition;
export type HoldingsSnapshot = Wire.HoldingsSnapshot;
export interface HoldingsCsvPreview {
  previewId: string;
  mapping: Partial<Record<keyof HoldingsPosition, string>>;
  positions: HoldingsPosition[];
  errors: Array<{ row: number; field: string; message: string }>;
  rowCount: number;
}
/** A count and a date; positions, quantities and values never reach this read. */
export interface HoldingsCount {
  importedAtMs: number;
  count: number;
}
/**
 * Holdings are the union of the latest import per source. Per source rows cover
 * the files and Korea Investment (wallets come from `walletsList`); per kind
 * rows feed About you.
 */
export interface HoldingsSummary {
  sources: Array<HoldingsCount & { source: "csv" | "kis" | "crypto_file" }>;
  stocks: HoldingsCount | null;
  crypto: HoldingsCount | null;
}
/** An exchange balance file. Quantities show only here so the user can check the file. */
export type CryptoFilePreview = Wire.CryptoFilePreview;
export type WalletFamily = Wire.WalletFamily;
/** A read-only public address. Never balances; `address` is only for Copy address. */
export type WalletView = Wire.WalletView;
export interface UserContextBridge {
  holdingsSummary(): Promise<HoldingsSummary>;
  /** Rust detects the format (Kraken ledger, balance file) or asks for the columns. */
  holdingsImportCryptoFile(request: { path: string }): Promise<CryptoFilePreview>;
  holdingsMapColumns(request: { previewId: string; asset: string; balance: string }): Promise<CryptoFilePreview>;
  walletsList(): Promise<WalletView[]>;
  /** Rust refuses recovery phrases and private keys (`secret_refused`) before storing anything, then reads once. */
  walletAdd(request: { address: string; name?: string | null }): Promise<WalletView[]>;
  walletRename(request: { walletId: string; name?: string | null }): Promise<WalletView>;
  /** Deletes the address and its balances from this computer. */
  walletRemove(request: { walletId: string }): Promise<void>;
  /** Reads one wallet, or all of them; a failed read keeps the last good one. */
  walletRefresh(request?: { walletId?: string }): Promise<WalletView[]>;
  watchlistList(): Promise<WatchlistEntry[]>;
  watchlistAdd(request: WatchlistAddRequest): Promise<WatchlistEntry>;
  /** Idempotent: removing an entry that is not followed succeeds. */
  watchlistRemove(request: { symbol: string; market: string }): Promise<void>;
  /** Path selected by a native file chooser. Rust owns parsing and the frozen preview. */
  holdingsImportCsv(request: { path: string }): Promise<HoldingsCsvPreview>;
  holdingsCommitCsv(request: { previewId: string }): Promise<HoldingsSnapshot>;
  /** Rejects with needs_account_profile if the connection has no account profile. */
  holdingsImportKis(): Promise<HoldingsSnapshot>;
}
