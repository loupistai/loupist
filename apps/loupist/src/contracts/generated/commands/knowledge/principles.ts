// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

export type PrincipleArchiveRequest = { loupistId: string, recordId: string, archived: boolean };

export type PrincipleCreateRequest = {
  loupistId: string,
  statement: string,
  why?: string | null,
  example?: string | null,
  origin: string,
  originChatId?: string | null,
  originMessageId?: string | null,
};

export type PrincipleListRequest = { loupistId: string, includeArchived?: boolean | null };

export type PrincipleUpdateRequest = {
  loupistId: string,
  recordId: string,
  expectedRevisionId: string,
  statement?: string | null,
  why?: string | null,
  example?: string | null,
};
