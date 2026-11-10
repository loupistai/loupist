// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { NumberCheck } from "../domain/answer_checks";

export type AssetsGrid = {
  subtitle?: string,
  notes?: Array<string>,
  rows: Array<GridRow>,
  columns: Array<GridColumn>,
  cells: Array<Array<GridCell>>,
  /** The turn's as-of date: every cell is what was known by then. */
  as_of?: string,
};

export type ConflictGrid = {
  measure: ConflictMeasure,
  reports: Array<ConflictReport>,
  reason: ConflictReason,
  explanation: string,
  /** The report the answer relies on, or none when it cannot tell. */
  used?: number,
  used_reason?: string,
  /** A World entity this turn read. */
  world_entity_id?: string,
  /** Rust found a disputed claim group touching the entity: the card links the Map. */
  map_link?: boolean,
  as_of?: string,
};

export type ConflictMeasure = { label: string, entity_label: string, period?: string };

/** Why sources disagree. */
export type ConflictReason =
  | "basis"
  | "period"
  | "preliminary_vs_final"
  | "estimate_vs_reported"
  | "currency_or_unit"
  | "adjusted_vs_reported"
  | "timing"
  | "unclear";

/** One source's report. `dated` comes from the evidence, never the model. */
export type ConflictReport = {
  evidence_id: string,
  says: string,
  basis?: string,
  dated?: string,
  /** Present when `says` is a number. */
  check?: NumberCheck,
};

export type GridCell = GridValue | GridEmpty;

export type GridColumn = {
  id: string,
  label: string,
  period?: string,
  basis?: string,
  unit: GridUnit,
};

export type GridDocument =
  | { "layout": "assets" } & AssetsGrid
  | { "layout": "conflict" } & ConflictGrid;

export type GridEmpty = { empty: GridEmptyReason, note?: string };

/** Why a cell is empty; an empty cell is never a zero. */
export type GridEmptyReason = "not_reported" | "not_applicable" | "unavailable" | "not_checked";

export type GridRow = {
  symbol: string,
  market: string,
  label: string,
  /** Set when Rust resolved the row to an asset page. */
  asset_id?: string,
};

export type GridUnit = "amount" | "percent" | "multiple" | "number" | "date";

/** A value with the evidence it links and Rust's check of it. */
export type GridValue = {
  value: string,
  evidence_id: string,
  check: NumberCheck,
  /**
   * The value in base units (an amount times its scale, a percent as
   * printed), so the renderer sorts without reading numbers itself.
   */
  sort?: number,
};
