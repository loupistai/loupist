// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { TransportConfig } from "../custom_connections/config";
import type { ApprovalChoice, ToolEligibility } from "../custom_connections/policy";
import type { JsonValue } from "../external";

export type AuthState = "not_required" | "signed_in" | "sign_in_needed" | "in_progress";

export type CustomConnection = {
  id: string,
  kind: string,
  name: string,
  transport: TransportConfig,
  command_line: string | null,
  dont_save_to_memory: boolean,
  pending: boolean,
  enabled: boolean,
  tools: Array<ToolCard>,
  env_keys: Array<string>,
  header_names: Array<string>,
  last_error: string | null,
  needs_review: boolean,
  auth_state: AuthState,
};

export type ToolCard = {
  definition_changed: boolean,
  name: string,
  /** The UI must label this as the server's own text, never host guidance. */
  server_description: string | null,
  input_schema: JsonValue,
  annotations: JsonValue | null,
  definition_hash: string,
  eligibility: ToolEligibility,
  approval: ApprovalChoice | null,
};
