// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

/**
 * A `calculate` operation; its snake_case name is the stable id the
 * renderer words results by (`NumberCheckCalc.operation`).
 */
export type CalcOperation =
  | "percent_change"
  | "cagr"
  | "growth_series"
  | "returns_summary"
  | "moving_average"
  | "ratio"
  | "margin"
  | "payout_ratio"
  | "net_debt_to_ebitda"
  | "free_cash_flow"
  | "dcf_grid"
  | "reverse_dcf"
  | "sum"
  | "holdings_overlap"
  | "goal_grid"
  | "dca_history";
