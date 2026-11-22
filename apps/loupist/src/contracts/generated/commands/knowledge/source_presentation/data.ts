// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

export type SourceDataCell =
  | { "kind": "text", "value": string }
  | { "kind": "number", "value": string }
  | { "kind": "boolean", "value": boolean }
  | { "kind": "empty" };

export type SourceDataCellPosition = {
  /** Both indexes are zero-based and relative to the returned table. */
  row: number,
  column: number,
};

export type SourceDataTable = {
  columns: Array<string>,
  rows: Array<Array<SourceDataCell>>,
  rowOffset: number,
  totalRows: number,
  focusedCell?: SourceDataCellPosition,
};
