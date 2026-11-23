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

/** Why a numeric citation did not verify against its retained evidence. */
export type ComparisonReason =
  | "digits_differ"
  | "scale_conflict"
  | "sign_conflict"
  | "incompatible_unit"
  | "no_candidate";

/**
 * Read-time diagnostic for an unresolved check. The saved comparison
 * remains compatible with readers that know the earlier reason values.
 */
export type ComparisonReplay =
  | { "reason": "no_candidate", searched: ComparisonScope }
  | { "reason": "evidence_unavailable" };

/** Material that the host searched while replaying a saved comparison. */
export type ComparisonScope = { "kind": "retained_passage", title: string };

/** One number link in an answer, checked against the evidence it links. */
export type NumberCheck = {
  /** Index among the answer's `[text](#evidence-…)` links, in order. */
  link_index: number,
  text: string,
  evidence_id: string,
  /**
   * The evidence id the answer wrote, when the host rebound a link that
   * named no retained result to the one this reply retained near it.
   */
  repairedFrom?: string,
  kind: NumberCheckKind,
  status: NumberCheckStatus,
  /** Nearest calculation result or plausible source figure for an unmatched citation. */
  nearest?: string,
  /** The unmatched candidate exactly as printed in the evidence. */
  comparedValue?: string,
  /** Unit of the unmatched candidate when the source identifies it. */
  nearestSourceUnit?: string,
  /** Power of ten of the stated source scale (6 for millions). */
  nearestSourceScale?: number,
  /** Why a numeric citation did not verify, including on historical replay. */
  comparisonReason?: ComparisonReason,
  comparisonReplay?: ComparisonReplay,
  /** For a rounded or rescaled match: the value as the evidence prints it. */
  exact?: string,
  /** Present on a matched citation that changes monetary scale exactly. */
  scale_conversion?: NumberCheckScaleConversion,
  unstated_scale?: UnstatedScale,
  /** Present on a match the source prints as an outflow. */
  sourceSign?: SourceSign,
  /** The matched or compared cell (JSON Pointer into the retained result). */
  pointer?: string,
  /** The matched or compared figure (UTF-8 byte offsets into the excerpt). */
  span?: ChatEvidenceMatch,
  /** The location in retained content when the excerpt has no span. */
  context?: NumberCheckContext,
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

/** A bounded part of retained content containing the matched or compared figure. */
export type NumberCheckContext = {
  text: string,
  /** End-exclusive UTF-8 byte offsets of that number inside `text`. */
  start: number,
  end: number,
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
  /** Whether a conversion uses the amount currency declared in the request. */
  currency_basis?: string,
};

export type NumberCheckKind = "calculated" | "passage" | "data" | "step";

/** Powers of ten in an exact conversion from the source to the answer. */
export type NumberCheckScaleConversion = {
  source_scale: number,
  answer_scale: number,
  /**
   * Currency from the evidence, or from the answer's stated unit when
   * the retained source window has only the base-unit values.
   */
  currency?: string,
};

export type NumberCheckStatus = "matched" | "rounded" | "not_found" | "differs" | "unlinked";

/** How the source signs a matched amount that the answer states unsigned. */
export type SourceSign = "outflow";

/** The side that leaves the scale of an exact digit match unstated. */
export type UnstatedScale = "source" | "answer";
