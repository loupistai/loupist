/**
 * Results before and after (finance-earnings-snapshot.md; Rust `knowledge/earnings`,
 * `commands/earnings.rs`). Rust records events, captures, computes every
 * difference, revision and chain state; React renders what it returns and
 * submits typed intent. Holdings quantities never appear here.
 */
import type { AssetEvidence } from "./assets";
import type { SensitivityGrid } from "./chat";

/** One calculation Rust ran for a value on the card (percent change, points, reverse DCF); its chip shows the formula. */
export type EarningsCalc = {
  title: string;
  formulas: string[];
  inputs: { name: string; display: string }[];
  /** The result as the calculation prints it ("+3.3%", "−1.2 pp", "6.1–9.8%"). */
  display: string;
  value: number;
  /** "%" for a percent change, "pp" for a difference in percentage points. */
  unit: string;
};

/**
 * A value in base units: 1270000000 for $1.27bn, 14.8 for 14.8 %. A range has
 * `low` and `high`, a point has `point`. The renderer formats it in its own
 * currency through `@/i18n/format`; values are never converted.
 */
export type EarningsAmount = {
  low?: number;
  high?: number;
  point?: number;
  unit: "currency" | "percent" | "per_share" | "number";
  /** ISO code for `currency` and `per_share` values. */
  currency?: string;
};

export type EarningsValueKind = "guidance" | "consensus" | "broker_estimate" | "implied" | "reported";
export type EarningsStage = "preliminary" | "final";

export type EarningsValue = EarningsAmount & {
  rowId: string;
  kind: EarningsValueKind;
  stage?: EarningsStage;
  /** The provider that served consensus or a broker estimate ("FMP", "Intrinio", "Korea Investment"). */
  provider?: string;
  /** The source's own date: guidance publication, consensus retrieval or the report's filing date. */
  factDate: string;
  /** 1–3 sources; the first is the source line's citation. */
  evidence: AssetEvidence[];
};

/** Computed by Rust; words first, then the chip-marked number. Never "beat" or "miss". */
export type EarningsDifference =
  | { kind: "range"; position: "above" | "within" | "below"; calc?: EarningsCalc }
  | { kind: "change"; calc: EarningsCalc }
  | { kind: "guidance"; change: "new" | "raised" | "lowered" | "kept"; calc?: EarningsCalc }
  | { kind: "not_comparable"; reason: "basis" | "period" | "unit" };

export type EarningsMeasureKey = "revenue" | "operatingIncome" | "operatingMargin" | "netIncome" | "eps" | "fcf";

export type EarningsMeasure = {
  /** A fixed key (`assets:earnings.measure.*`), or absent with the run's own `label`. */
  key?: EarningsMeasureKey;
  label?: string;
  /** "Q3 2026"; a new-guidance row names the later period it guides. */
  period: string;
  /** "GAAP", "adjusted", "consolidated" or "separate" when the source says so. */
  basis?: string;
  /** Guidance, then consensus, then a broker estimate. */
  expected: EarningsValue[];
  /** The reported value (final stage first); for a guidance row, the new guidance. */
  reported: EarningsValue[];
  /** One per expected value, in the same order; a guidance row has exactly one `guidance` difference. */
  differences: EarningsDifference[];
  /** The row carries new guidance for a later period (Expected shows "—"). */
  guidance?: boolean;
};

export type EarningsChainStage = "release" | "preliminary" | "quarterly" | "half" | "annual" | "audit";
export type EarningsChainStep = {
  stage: EarningsChainStage;
  filed: boolean;
  /** Filing date when filed. */
  date?: string;
  /** "8-K", "10-Q", "분기보고서"… */
  form?: string;
  /** Legal deadline while not filed yet. */
  dueBy?: string;
  evidence: AssetEvidence[];
};

/** A preliminary value the final report changed by more than half a unit of its last shown digit. */
export type EarningsRevision = {
  key?: EarningsMeasureKey;
  label?: string;
  from: EarningsAmount;
  to: EarningsAmount;
  calc: EarningsCalc;
  evidence: AssetEvidence[];
};

export type EarningsStatus = "upcoming" | "before_captured" | "reported" | "final";
export type EarningsDateSource = "calendar" | "ir_notice" | "legal_deadline";

export type EarningsSnapshot = {
  eventId: string;
  assetId: string;
  name: string;
  symbol: string;
  market: string;
  /** "Q3 2026". */
  period: string;
  /** YYYY-MM-DD: expected, announced or (legal deadline) the date it is due by. */
  reportDate: string;
  dateSource: EarningsDateSource;
  status: EarningsStatus;
  /** While upcoming: when the before-capture is scheduled (ms). */
  captureAtMs?: number;
  /** The before-capture; `lateAsOf` is set for a late capture (only what was published by that date). */
  before?: { capturedAtMs: number; lateAsOf?: string };
  /** The after-capture and the date the results were reported. */
  after?: { capturedAtMs: number; date: string };
  measures: EarningsMeasure[];
  /** PRICE IMPLIED: the reverse-DCF range at a dated close on or before the day before the report. */
  /** `grid` is the whole reverse-DCF grid behind the range, opened by "Open the grid". */
  implied?: { lowPct: number; highPct: number; years: number; price: number; currency?: string; closeDate: string; calc: EarningsCalc; grid?: SensitivityGrid; evidence: AssetEvidence[] };
  /** PRICE REACTION: close the day before → the first close after the report. */
  reaction?: { from: number; to: number; currency?: string; fromDate: string; toDate: string; calc: EarningsCalc; evidence: AssetEvidence[] };
  /** Reported, but the first close after the report does not exist yet. */
  reactionWaiting: boolean;
  /** CALL TONE: one sentence from a transcript, with 1–2 exact spans. */
  tone?: { text: string; spans: string[]; evidence: AssetEvidence[] };
  /** FILED SO FAR, in order. */
  chain: EarningsChainStep[];
  revisions: EarningsRevision[];
  /** The final report arrived and changed no number. */
  unchanged: boolean;
  /** A late capture could not use consensus (undated before the report). */
  consensusLate: boolean;
  /** Neither FMP nor Intrinio is connected, so consensus is not read. */
  consensusNeedsKey: boolean;
  /** No company guidance was found for this period. */
  noGuidance: boolean;
  /** The last capture for this event failed; the card keeps the last good state. */
  failed?: { side: "before" | "after"; atMs: number };
  /** A capture for this event is running now. */
  preparing: boolean;
};

export type EarningsEventSummary = {
  eventId: string;
  period: string;
  reportDate: string;
  dateSource: EarningsDateSource;
  status: EarningsStatus;
};

export type EarningsSnapshotGetRequest = { loupistId: string; assetId: string; eventId?: string };
export type EarningsEventsListRequest = { loupistId: string; assetId: string };
export type EarningsPrepareRequest = { loupistId: string; assetId: string; eventId: string };

export interface EarningsBridge {
  /**
   * One event's snapshot. Without `eventId`: the event the asset page shows now
   * (report date within 14 days ahead, or reported in the last 30 days), else null.
   */
  earningsSnapshotGet(request: EarningsSnapshotGetRequest): Promise<EarningsSnapshot | null>;
  /** Every recorded event of the asset, newest report date first ("Earlier results"). */
  earningsEventsList(request: EarningsEventsListRequest): Promise<EarningsEventSummary[]>;
  /** Run now on the earnings task for this event's pending side; resolves with the snapshot once the capture committed. */
  earningsPrepare(request: EarningsPrepareRequest): Promise<EarningsSnapshot>;
}
