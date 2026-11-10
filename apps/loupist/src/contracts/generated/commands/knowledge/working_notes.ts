// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { WorkingNote } from "../../knowledge/model";

export type WorkingNoteReadRequest = { loupistId: string, noteId: string };

export type WorkingNotesPage = { current: WorkingNote | null };

export type WorkingNotesRequest = { loupistId: string, chatId: string };
