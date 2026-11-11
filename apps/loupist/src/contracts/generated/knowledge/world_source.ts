// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

export type SourceWorldClaim = {
  worldRecordId: string,
  worldRevisionId: string,
  claimId: string,
  proposition: string,
  relation: string,
};

export type WorldSourceTarget = {
  recordId: string,
  revisionId: string,
  sourceId: string,
  spanId: string,
  worldRecordId: string,
  worldRevisionId: string,
};
