// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

export type CsvCommitRequest = { previewId: string };

export type CsvImportRequest = { path: string };

export type MapColumnsRequest = { previewId: string, asset: string, balance: string };

export type WalletAddRequest = { address: string, name?: string | null };

export type WalletIdRequest = { walletId: string };

export type WalletRefreshRequest = { walletId?: string | null };

export type WalletRenameRequest = { walletId: string, name?: string | null };

export type WatchlistAddRequest = {
  symbol: string,
  market: string,
  displayName?: string | null,
  /**
   * The Loupist open when the user followed it: it watches the asset with
   * the default kinds (finance-watchers.md §3.2).
   */
  loupistId?: string | null,
};

export type WatchlistRemoveRequest = { symbol: string, market: string };
