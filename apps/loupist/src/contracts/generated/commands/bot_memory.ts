// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { BotMemoryKind } from "../chat/bot_memory";

export type BotMemoryForgetRequest = { loupistId: string, id: string };

export type BotMemoryListEntry = {
  id: string,
  kind: BotMemoryKind,
  text: string,
  sinceMs: number,
  updatedMs: number,
};

export type BotMemoryListRequest = { loupistId: string };

export type BotMemoryUpdateRequest = { loupistId: string, id: string, text: string };
