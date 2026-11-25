// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { BriefingEdition } from "./scheduled_tasks/types";

/** One unannounced run the briefing may mention. */
export type BackgroundRun = {
  taskId: string,
  task: string,
  ranAtMs: number,
  runId: string,
  threadId?: string,
  messageId?: string,
};

export type BriefingChange = {
  /** `claim_added`, `claim_disputed` or `source_retained`. */
  kind: string,
  subjectId: string | null,
  summary: string,
};

/** What changed in World since the previous briefing, counted by the host. */
export type BriefingChanged = {
  names: number,
  relations: number,
  conflicts: number,
  /** Newest first, at most `MAX_CHANGE_ROWS`. */
  rows: Array<BriefingChange>,
};

/** A Journal check that is due, pointed to rather than quoted in full. */
export type BriefingCheck = { journalId: string, title: string, byDate: string | null };

export type BriefingContext = {
  symbols: Array<string>,
  principlesChecked: number,
  newFacts: number,
  disputedEntityIds: Array<string>,
  /** Host-computed sections for the Schedule reader; the narrative is never parsed. */
  changed: BriefingChanged,
  ahead: Array<BriefingCheck>,
  /** Missing on a briefing saved before editions means morning. */
  edition?: BriefingEdition,
  /** Watches and other runs since the previous briefing that were not posted in Bot. */
  backgroundRuns?: Array<BackgroundRun>,
};
