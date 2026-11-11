// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { LoupistTone } from "../../domain/loupist";

export type AddMode =
  | { "new": { name: string, tone: LoupistTone } }
  | { "merge": { loupistId: string } };

export type AddProgress = { done: number, total: number };

export type AddSections = {
  perspective: boolean,
  methods: boolean,
  principles: boolean,
  map: boolean,
};

export type PackAddRequest = { stagingId: string, mode: AddMode, sections: AddSections };
