// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { JsonValue } from "../external";

export type GrowthEvent = {
  eventId: string,
  loupistId: string,
  atMs: number,
  kind: string,
  recordId: string | null,
  subjectId: string | null,
  originChatId: string | null,
  originMessageId: string | null,
  actor: string,
  origin?: JsonValue,
  rationale?: string,
  summary: string,
  beforeSummary: string | null,
  afterSummary: string | null,
  /**
   * Set on reads when the event names a Source the host wrote: its summary
   * is the stored English title, and the interface shows this key instead.
   */
  titleKey?: string,
};

export type GrowthEventsPage = { events: Array<GrowthEvent>, nextCursor: string | null };

export type GrowthEventsQuery = {
  loupistId: string,
  sinceMs?: number | null,
  untilMs?: number | null,
  limit?: number | null,
  cursor?: string | null,
};
