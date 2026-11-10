// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { ApprovalChoice } from "../custom_connections/policy";

export type AddCommandRequest = {
  command: string,
  args?: Array<string>,
  env?: { [key in string]: string },
  name?: string | null,
};

export type AddJsonRequest = { json: string };

export type AddUrlRequest = {
  url: string,
  name?: string | null,
  headers?: { [key in string]: string },
};

export type ApproveRequest = {
  loupist_id: string,
  connection_id: string,
  tool_name: string,
  definition_hash: string,
  choice: ApprovalChoice,
};

export type AuthorizeRequest = {
  connection_id: string,
  issuer: string,
  client_id: string,
  client_secret?: string | null,
};

export type ConfirmRequest = {
  connection_id: string,
  /** Loupist that opened the Add dialog. Confirm enables the connection for this Loupist. */
  loupist_id?: string | null,
};

export type EnabledRequest = { loupist_id: string, connection_id: string, enabled: boolean };

export type IdentityRequest = { connection_id: string };

export type ListRequest = { loupist_id?: string | null };

export type MemoryRequest = { connection_id: string, dont_save_to_memory: boolean };
