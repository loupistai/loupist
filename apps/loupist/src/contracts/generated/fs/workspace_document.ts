// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

export type DocumentRefusal =
  | "outside_workspace"
  | "unsupported_type"
  | "inaccessible_file"
  | "unsafe_document";

export type WorkspaceDocumentHandler =
  | { "status": "allowed", application_name: string | null }
  | { "status": "refused", reason: DocumentRefusal };

export type WorkspaceDocumentOpen =
  | { "status": "requested", application_name: string | null }
  | { "status": "refused", reason: DocumentRefusal }
  | { "status": "handler_unavailable", application_name?: string, reason?: string };

export type WorkspaceDocumentRequest = { loupist_id: string, path: string };
