// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { License } from "../../sharing/pack";
import type { ImportMode } from "../../sharing/store";

export type PackImportRequest = { loupistId: string, importId: string };

export type PackImportView = {
  importId: string,
  packId: string,
  name: string,
  author: string | null,
  version: number,
  license: License,
  sourceUrl: string | null,
  addedAtMs: number,
  mode: ImportMode,
  blockedReason: string | null,
  removable: Removable,
  /** When "Check for updates" last ran; null before the first check. */
  checkedAtMs: number | null,
};

export type PackOriginRequest = { loupistId: string, subjectId: string };

export type PackRemoveRequest = { loupistId: string, importId: string, archiveAccepted: boolean };

export type Removable = { names: number, links: number, accepted: number };
