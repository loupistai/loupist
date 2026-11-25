// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { ChatProgressStatus } from "../chat/progress";
import type { ChatEvidenceRef } from "../domain/chat";
import type { PythonStep } from "../python_run/ledger";

export type ChatActivityDetail = {
  id: string,
  tool_name: string,
  step_id?: string,
  python?: PythonStep,
  python_message_id?: string,
  python_sources?: Array<ChatEvidenceRef>,
  status: ChatProgressStatus,
  input: string,
  output: string,
  truncated: boolean,
  file_path: string | null,
  started_at_ms: number,
  finished_at_ms: number | null,
};
