/** App-level factual context shared by every Loupist. No suitability profile. */
export interface WatchlistEntry {
  symbol: string;
  market: string;
  displayName: string | null;
  addedAtMs: number;
}
export interface WatchlistAddRequest {
  symbol: string;
  market: string;
  displayName?: string | null;
  /** The Loupist open when the user followed it: that Loupist watches the asset with the default kinds (finance-watchers.md §3.2). */
  loupistId?: string | null;
}
export interface HoldingsPosition {
  symbol: string;
  /** UNKNOWN when the CSV omits its market. */
  market: string;
  displayName: string | null;
  quantity: string;
  /** Average acquisition price per unit; exact decimal string. */
  costBasis: string | null;
  marketValue: string | null;
  /** Empty if unspecified. No implicit currency conversion. */
  currency: string;
}
export interface HoldingsSnapshot {
  snapshotId: string;
  source: "csv" | "kis" | "crypto_file" | "wallet";
  importedAtMs: number;
  /** Empty if positions have mixed or unspecified currencies. */
  currency: string;
  note: string | null;
  positions: HoldingsPosition[];
}
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
export interface CryptoFilePreview {
  previewId: string;
  format: "kraken_ledger" | "balances" | "mapped" | null;
  rowCount: number;
  /** The latest ledger time, when the file has one. */
  asOfMs: number | null;
  rows: Array<{ symbol: string; quantity: string; from: string[] }>;
  skipped: Array<{ symbol: string; reason: "cash" | "zero" }>;
  errors: Array<{ row: number; field: string; message: string }>;
  /** Present until the user picks the asset and balance columns. */
  mappingHeaders: string[] | null;
  /** A trade history without balances; refused. */
  tradesOnly: boolean;
}
export type WalletFamily = "bitcoin" | "evm" | "solana";
/** A read-only public address. Never balances; `address` is only for Copy address. */
export interface WalletView {
  id: string;
  family: WalletFamily;
  /** First six and last four characters; the only form shown. */
  masked: string;
  address: string;
  name: string | null;
  addedAtMs: number;
  lastReadAtMs: number | null;
  lastGoodReadAtMs: number | null;
  status: "unread" | "ok" | "failed";
  unlistedCount: number;
}
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
