// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { DraftIdentity } from "../../commands/workspace_ui";

export type ComposerModelRead = {
  /** Null reads the last model sent from any Chat on this profile. */
  chat: DraftIdentity | null,
};

export type ComposerModelSelection = {
  modelProfileId: string,
  thinkingLevel: string,
  runOptions: Record<string, string>,
};

export type ComposerModelWrite =
  | {
    "kind": "set",
    chat: DraftIdentity,
    selection: ComposerModelSelection | null,
    lastSent: ComposerModelSelection | null,
  }
  | { "kind": "reset", loupistId: string | null };
