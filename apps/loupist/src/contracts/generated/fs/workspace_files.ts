// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

export type WorkspaceDirectory = {
  path: string,
  entries: Array<WorkspaceEntry>,
  truncated: boolean,
};

export type WorkspaceEntry = { path: string, name: string, kind: string, size_bytes: number | null };

export type WorkspaceFile = {
  path: string,
  size_bytes: number,
  content: string | null,
  image_data_url?: string,
  content_sha256: string | null,
  media_type: string,
  truncated: boolean,
  binary: boolean,
};
