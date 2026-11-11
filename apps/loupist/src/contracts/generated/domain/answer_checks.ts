// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { CalcOperation } from "../calculations";
import type { ChatEvidenceMatch } from "../domain/chat";

/** A read that had nothing dated on or before the cutoff. */
export type AsOfEmpty = { tool: string, target?: string };

/** Saved Memory the cutoff left out, by kind. */
export type AsOfLeftOut = { facts: number, sources: number, notes: number, methods: number };

/** A read a tool refused because it cannot show what was known on the date. */
export type AsOfRefusal = { tool: string, connection?: string };

/** What an as-of answer could not use: refusals, empty reads and later Memory. */
export type AsOfReport = {
  cutoff: string,
  refused?: Array<AsOfRefusal>,
  empty?: Array<AsOfEmpty>,
  left_out: AsOfLeftOut,
};

/**
 * How a `calculate` input is printed: Rust reads its kind from the input's
 * name and the request's `unit` (finance-show-the-math.md §2.3).
 */
export type CalcInputUnit = {
  kind: CalcInputUnitKind,
  /** ISO 4217 code the request's `unit` names ("KRW"). */
  currency?: string,
  /** Power of ten the amount is stated in: 9 for "USD billions", 8 for 억원. */
  scale?: number,
  /** A unit the host cannot place ("stores"), printed after the number. */
  text?: string,
};

/** What a `calculate` input measures, as the host printed it. */
export type CalcInputUnitKind =
  | "number"
  | "percent"
  | "points"
  | "basis_points"
  | "multiple"
  | "amount"
  | "price";

/** One number link in an answer, checked against the evidence it links. */
export type NumberCheck = {
  /** Index among the answer's `[text](#evidence-…)` links, in order. */
  link_index: number,
  text: string,
  evidence_id: string,
  kind: NumberCheckKind,
  status: NumberCheckStatus,
  /** For `differs`: the calculated value nearest to the printed number. */
  nearest?: string,
  /** For `rounded`: the value as the evidence prints it. */
  exact?: string,
  /** The matched data cell (JSON Pointer into the retained result). */
  pointer?: string,
  /** The matched passage text (UTF-8 byte offsets into the evidence excerpt). */
  span?: ChatEvidenceMatch,
  /** Up to four fields of the matched data row. */
  fields?: Array<NumberCheckField>,
  calc?: NumberCheckCalc,
};

/** What the calculated popover shows, copied from the host's own result. */
export type NumberCheckCalc = {
  /** The `calculate` operation; missing on answers saved before it was kept. */
  operation?: CalcOperation,
  title: string,
  formulas: Array<string>,
  inputs: Array<NumberCheckInput>,
  /** Rows × columns of a DCF or reverse-DCF grid, which has no single value. */
  grid?: [number, number],
};

/** One field of a matched data row, as the provider printed it. */
export type NumberCheckField = { key: string, value: string, matched?: boolean };

/** One `calculate` input and where it came from. */
export type NumberCheckInput = {
  name: string,
  /** The name its formula prints ("Operating income", "from"). */
  label: string,
  unit: CalcInputUnit,
  display: string,
  /**
   * The evidence the input was cited from or read out of; absent when the
   * model provided the value without a source.
   */
  evidence_id?: string,
  /** Rust's check of a cited input against its evidence. */
  status?: NumberCheckStatus,
};

export type NumberCheckKind = "calculated" | "passage" | "data" | "step";

export type NumberCheckStatus = "matched" | "rounded" | "not_found" | "differs";

/** A paragraph that is only a link to a grid result, shown as a card. */
export type SensitivityCard = {
  link_index: number,
  evidence_id: string,
  subject: string,
  operation: string,
  row_values: Array<number>,
  column_values: Array<number>,
  values: Array<Array<number | null>>,
  unit: string,
  forecast_years: number,
  reference?: SensitivityReference,
  above_reference?: Array<Array<boolean>>,
  price?: SensitivityPrice,
  amount_unit?: string,
  inputs: Array<SensitivityInput>,
  formulas: Array<string>,
};

/**
 * A `dcf_grid` or `reverse_dcf` result as the host computed it; never one
 * point value. Rows are discount rates, columns long-run growth rates.
 */
export type SensitivityGrid = {
  operation: string,
  row_values: Array<number>,
  column_values: Array<number>,
  values: Array<Array<number | null>>,
  unit: string,
  forecast_years: number,
  reference?: SensitivityReference,
  above_reference?: Array<Array<boolean>>,
  price?: SensitivityPrice,
  amount_unit?: string,
  inputs: Array<SensitivityInput>,
  formulas: Array<string>,
};

/** One grid input (cash flow, shares, net debt, growth) and its source. */
export type SensitivityInput = {
  name: string,
  label?: string,
  display: string,
  evidence_id?: string,
  status?: NumberCheckStatus,
};

/** The price a reverse-DCF grid solves for, with its date and source. */
export type SensitivityPrice = {
  value: number,
  display: string,
  currency?: string,
  date?: string,
  evidence_id?: string,
  status?: NumberCheckStatus,
};

/** The past growth a reverse-DCF grid is read against (usually a CAGR). */
export type SensitivityReference = {
  value_pct: number,
  /** The period it covers, as the calculation was labelled ("FY2020–FY2025"). */
  label?: string,
  /** The years it spans, from a cited `cagr` result. */
  years?: number,
  evidence_id?: string,
  status?: NumberCheckStatus,
};
