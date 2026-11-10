// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

export type CalcInput = { name: string, display: string };

/** One calculation behind a number on the card. */
export type CalcView = {
  title: string,
  formulas: Array<string>,
  inputs: Array<CalcInput>,
  display: string,
  value: number,
  unit: string,
};

export type DifferenceView =
  | { "kind": "range", position: Position, calc?: CalcView }
  | { "kind": "change", calc: CalcView }
  | { "kind": "guidance", change: GuidanceChange, calc?: CalcView }
  | { "kind": "not_comparable", reason: NotComparable };

export type GuidanceChange = "new" | "raised" | "lowered" | "kept";

export type NotComparable = "basis" | "period" | "unit";

export type Position = "above" | "within" | "below";
