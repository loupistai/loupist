// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

export type RemoveRequest = { loupistId: string, kind: string, name: string };

export type UndoRequest = { loupistId: string, changeId: string };

export type WorldSchemaLoupistRequest = { loupistId: string };
