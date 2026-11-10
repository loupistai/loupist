// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

export type EarningsEventsListRequest = { loupistId: string, assetId: string };

export type EarningsPrepareRequest = { loupistId: string, assetId: string, eventId: string };

export type EarningsSnapshotGetRequest = {
  loupistId: string,
  assetId: string,
  eventId?: string | null,
};
