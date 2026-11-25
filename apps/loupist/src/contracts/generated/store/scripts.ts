// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { ReadSummary } from "../python_run/ledger";

export type ScriptDraft = {
  name: string,
  title?: string,
  summary: string,
  args?: Record<string, { default: unknown; description: string }>,
  code: string,
};

export type ScriptMetadata = {
  title: string,
  summary: string,
  args: Record<string, { default: unknown; description: string }>,
  origin: ScriptOrigin,
  afterOutsideText: boolean,
  reads: Array<ReadSummary>,
  lastTest?: ScriptTest,
};

export type ScriptOrigin =
  | { "kind": "user" }
  | { "kind": "chat", chatId: string, messageId: string }
  | { "kind": "run", taskId: string, runId: string }
  | { "kind": "template", id: string };

export type ScriptRef = { name: string, version: number };

export type ScriptSummary = {
  name: string,
  version: number,
  sha256: string,
  createdAtMs: number,
  metadata: ScriptMetadata,
  usedBy: Array<string>,
};

export type ScriptTest = { atMs: number, status: string, rounds: number };

export type ScriptVersion = { summary: ScriptSummary, code: string, versions: Array<number> };
