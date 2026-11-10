// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

export type KnowledgeReference = {
  kind: string,
  recordId: string,
  revisionId: string,
  objectId: string,
  spanId?: string,
  receiptId?: string,
};

/** Host-observed Lens delivery. Discovery and model-written links never create one. */
export type LensReceipt = {
  recordId: string,
  revisionId: string,
  revisionDigest: string,
  kind: string,
  title: string,
  delivery: string,
};
