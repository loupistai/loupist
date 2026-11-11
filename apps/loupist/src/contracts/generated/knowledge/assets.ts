// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { DatasetRef } from "../datasets";
import type { JsonValue } from "../external";
import type { AssetClass, ClassSource } from "../knowledge/assets/class";
import type { Verdict } from "../knowledge/assets/ladder";
import type { Section } from "../knowledge/assets/sections";

export type AssetChangeRow = {
  id: string,
  /**
   * filing, results, earnings, dividend, price, holdings, fees, positioning, rates, map, source, thesis.
   */
  kind: string,
  factDate: string,
  recordedAtMs: number,
  facts: JsonValue,
  evidence: Array<AssetEvidence>,
  /**
   * For a 10-K/10-Q/사업보고서/분기보고서 filing: compared sections, or empty when not compared yet.
   */
  sections?: Array<SectionCountView>,
  /** Recorded after the chosen date (the page's accent dot). */
  new: boolean,
  /** Worth a Thread or briefing line (a material price move, any other kind). */
  material: boolean,
};

export type AssetDelta = {
  assetId: string,
  symbol: string,
  market: string,
  name: string,
  class: AssetClass,
  /** The World entity the page matched, for the Map inspector. */
  entityId?: string,
  verdict?: Verdict,
  verdictSince?: string,
  /** The move behind the current read; absent while it stands where it started. */
  latestMove?: MoveView,
  /** Newest fact first, most material first within a day. */
  rows: Array<AssetChangeRow>,
  /** A read move recorded after the date. */
  moved: boolean,
  crossed: boolean,
};

/**
 * Where a row came from: a retained dataset row, an original URL, a Source,
 * a World claim, or a cited result of the turn that recorded it.
 */
export type AssetEvidence = {
  tool: string,
  provider?: string,
  title: string,
  url?: string,
  dataset?: DatasetRef,
  pointer?: string,
  recordId?: string,
  worldId?: string,
  citationUrl?: string,
  /** The evidence's own date (filing, publication or observation). */
  date?: string,
  retrievedAt?: string,
};

export type AssetPageView = {
  assetId: string,
  symbol: string,
  market: string,
  name: string,
  class: AssetClass,
  classSource: ClassSource,
  entityId?: string,
  /** `watching`, `holding` or `none`; never a quantity. */
  watch: string,
  reasons: Array<ReasonView>,
  lines: Array<LineView>,
  read?: ReadView,
  /** Newest first. */
  history: Array<MoveView>,
  glance: Array<GlanceRow>,
  checks: ChecksView,
  map?: MapCounts,
  /** Korean equities with Korea Investment on: the latest investor flows. */
  flows?: JsonValue,
  refreshedAtMs?: number,
  createdAtMs: number,
  revision: number,
};

export type CheckFailure = {
  kind: string,
  connection: string,
  /** `failed` or `needs_connection`. */
  reason: string,
};

export type ChecksView = {
  /** Kinds the last check read (the whole plan before the first check). */
  kinds: Array<string>,
  /** Connections at least one of those reads came from. */
  connections: Array<string>,
  notYet: Array<string>,
  failures: Array<CheckFailure>,
  sourcesOnly: boolean,
};

export type GlanceRow = {
  /** lastFiling, lastResults, nextEarnings, dividend, price, drawdown, expenseRatio, level. */
  key: string,
  facts: JsonValue,
  date?: string,
  evidence: Array<AssetEvidence>,
};

/** A Journal entry's place in an asset's thesis. */
export type JournalThesis = { assetId: string, role: ThesisRole };

export type LineCrossing = {
  journalId: string,
  claim: string,
  metric: string,
  direction: string,
  threshold: number,
  unit?: string,
  observed: number,
  crossed: boolean,
  /** When the user wrote the line. */
  setAtMs: number,
};

export type LineReadingView = { observed: number, factDate: string, evidence: Array<AssetEvidence> };

export type LineView = {
  journalId: string,
  claim: string,
  metric: string,
  direction: string,
  threshold: number,
  unit?: string,
  byDate?: string,
  createdAtMs: number,
  updatedAtMs: number,
  originChatId?: string,
  /** `not_checked`, `not_crossed` or `crossed`. */
  status: string,
  last?: LineReadingView,
};

export type MapCounts = { suppliers: number, customers: number };

export type MoveView = {
  moveId: string,
  assetId: string,
  from?: Verdict,
  to: Verdict,
  factDate: string,
  reason: string,
  evidence: Array<AssetEvidence>,
  reasonId?: string,
  line?: LineCrossing,
  /** `loupist` or `user`. */
  actor: string,
  atMs: number,
  undone: boolean,
  undoesMoveId?: string,
  originChatId?: string,
  originMessageId?: string,
  seenAtMs?: number,
};

export type ReadView = {
  verdict: Verdict,
  /** The fact date of the move that set this step, or the first reason's day. */
  since: string,
  /** True while the read still stands where it started. */
  started: boolean,
  latestMove?: MoveView,
  /** Set when the latest move is the Loupist's and can be undone. */
  undoMoveId?: string,
};

export type ReasonView = {
  journalId: string,
  text: string,
  createdAtMs: number,
  updatedAtMs: number,
  originChatId?: string,
  /** The last move that named this reason. */
  lastMove?: MoveView,
};

export type SectionCountView = { section: Section, added: number, removed: number, changed: number };

export type ThesisRole = "reason" | "line";
