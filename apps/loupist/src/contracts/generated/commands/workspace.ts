// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

export type ChatActivityRequest = { loupist_id: string, thread_id: string, detail_id: string };

export type WorkspacePathRequest = { loupist_id: string, path?: string | null };
