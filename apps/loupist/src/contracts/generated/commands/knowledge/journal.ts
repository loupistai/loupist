// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { JournalThesis } from "../../knowledge/assets";
import type { JournalCheck } from "../../knowledge/model";

export type JournalArchiveRequest = { loupistId: string, journalId: string, archived: boolean };

export type JournalCreateRequest = {
  loupistId: string,
  judgment: string,
  reasoning?: string | null,
  entities?: Array<string> | null,
  check?: JournalCheck | null,
  /** A reason or line on an existing asset page. */
  thesis?: JournalThesis | null,
};

export type JournalListRequest = { loupistId: string, includeArchived?: boolean | null };

export type JournalUpdateRequest = {
  loupistId: string,
  journalId: string,
  expectedUpdatedAtMs: number,
  judgment: string,
  reasoning?: string | null,
  entities?: Array<string> | null,
  check?: JournalCheck | null,
  thesis?: JournalThesis | null,
};
