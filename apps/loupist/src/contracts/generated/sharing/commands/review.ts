// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { JsonValue } from "../../external";

export type Decision = "accept" | "skip";

export type PackDecideRequest = {
  loupistId: string,
  importId: string,
  itemKeys: Array<string>,
  decision: Decision,
};

export type PackItemRequest = { loupistId: string, importId: string, key: string };

export type Suggestion = {
  key: string,
  kind: string,
  title: string,
  flags: Array<JsonValue>,
  state: string,
  /** The author's newer text for an item already accepted (W19.5). */
  update: boolean,
};

export type SuggestionGroup = { importId: string, packName: string, items: Array<Suggestion> };
