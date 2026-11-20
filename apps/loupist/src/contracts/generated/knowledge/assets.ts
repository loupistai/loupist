// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { DatasetRef } from "../datasets";
import type { Verdict } from "../knowledge/assets/ladder";

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

export type ThesisRole = "reason" | "line";
