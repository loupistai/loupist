// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { SensitivityGrid } from "../domain/answer_checks";
import type { AssetEvidence } from "../knowledge/assets";
import type { CalcView, DifferenceView } from "../knowledge/earnings/diff";

export type AfterView = { capturedAtMs: number, date: string };

export type AmountView = {
  low?: number,
  high?: number,
  point?: number,
  unit: Unit,
  currency?: string,
};

export type BeforeView = { capturedAtMs: number, lateAsOf?: string };

export type ChainStepView = {
  stage: string,
  filed: boolean,
  date?: string,
  form?: string,
  dueBy?: string,
  evidence: Array<AssetEvidence>,
};

export type DateSource = "calendar" | "ir_notice" | "legal_deadline";

export type EventSummary = {
  eventId: string,
  period: string,
  reportDate: string,
  dateSource: DateSource,
  status: Status,
};

export type Failure = { side: string, atMs: number };

export type ImpliedView = {
  lowPct: number,
  highPct: number,
  years: number,
  price: number,
  currency?: string,
  closeDate: string,
  calc: CalcView,
  /** The whole reverse-DCF grid behind the range ("Open the grid"). */
  grid?: SensitivityGrid,
  evidence: Array<AssetEvidence>,
};

export type MeasureView = {
  key?: string,
  label?: string,
  period: string,
  basis?: string,
  expected: Array<ValueView>,
  reported: Array<ValueView>,
  differences: Array<DifferenceView>,
  guidance?: boolean,
};

export type ReactionView = {
  from: number,
  to: number,
  currency?: string,
  fromDate: string,
  toDate: string,
  calc: CalcView,
  evidence: Array<AssetEvidence>,
};

export type RevisionView = {
  key?: string,
  label?: string,
  from: AmountView,
  to: AmountView,
  calc: CalcView,
  evidence: Array<AssetEvidence>,
};

export type SnapshotView = {
  eventId: string,
  assetId: string,
  name: string,
  symbol: string,
  market: string,
  period: string,
  reportDate: string,
  dateSource: DateSource,
  status: Status,
  captureAtMs?: number,
  before?: BeforeView,
  after?: AfterView,
  measures: Array<MeasureView>,
  implied?: ImpliedView,
  reaction?: ReactionView,
  reactionWaiting: boolean,
  tone?: ToneView,
  chain: Array<ChainStepView>,
  revisions: Array<RevisionView>,
  unchanged: boolean,
  consensusLate: boolean,
  consensusNeedsKey: boolean,
  noGuidance: boolean,
  failed?: Failure,
  preparing: boolean,
};

export type Stage = "preliminary" | "final";

export type Status = "upcoming" | "before_captured" | "reported" | "final";

export type ToneView = { text: string, spans: Array<string>, evidence: Array<AssetEvidence> };

export type Unit = "currency" | "percent" | "per_share" | "number";

export type ValueKind = "guidance" | "consensus" | "broker_estimate" | "implied" | "reported";

export type ValueView = {
  rowId: string,
  kind: ValueKind,
  stage?: Stage,
  provider?: string,
  factDate: string,
  evidence: Array<AssetEvidence>,
  low?: number,
  high?: number,
  point?: number,
  unit: Unit,
  currency?: string,
};
