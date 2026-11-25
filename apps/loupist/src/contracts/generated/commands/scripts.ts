// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { PythonStep } from "../python_run/ledger";
import type { ScriptDraft } from "../store/scripts";

export type PythonReplay = { step: PythonStep, matches: boolean, stdout: string, stderr: string };

export type PythonStepReplayRequest = { chatId: string, messageId: string, stepId: string };

export type ScriptGetRequest = { loupistId: string, name: string, version?: number };

export type ScriptSaveRequest = { loupistId: string, draft: ScriptDraft };

export type ScriptTestRequest = {
  loupistId: string,
  name: string,
  version: number,
  args?: Record<string, unknown>,
};

export type ScriptsListRequest = { loupistId: string, query?: string };
