// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

export type WorkbookCell = {
  kind: WorkbookCellKind,
  display: string,
  formula: boolean,
  number_format?: string,
};

export type WorkbookCellKind = "number" | "text" | "date" | "boolean" | "error" | "empty";

export type WorkbookOmission = "charts" | "images" | "visual_styles" | "comments" | "macros";

export type WorkbookProgress = "checking_file" | "reading_sheet";

export type WorkbookProjection = {
  sheets: Array<WorkbookSheet>,
  selected_sheet: number | null,
  first_row: number | null,
  first_column: number | null,
  rows_total: number,
  columns_total: number,
  rows: Array<Array<WorkbookCell>>,
  truncated: boolean,
  omitted: Array<WorkbookOmission>,
};

export type WorkbookRead =
  | { "status": "ready" } & WorkbookProjection
  | { "status": "unsupported_format" }
  | { "status": "encrypted" }
  | { "status": "invalid_workbook" }
  | { "status": "unknown_sheet" }
  | { "status": "unsupported_sheet" }
  | { "status": "limit_exceeded" }
  | { "status": "worker_unavailable" }
  | { "status": "timed_out" };

export type WorkbookRequest = { loupist_id: string, path: string, sheet_index?: number | null };

export type WorkbookSheet = { name: string, hidden: boolean, worksheet: boolean };
