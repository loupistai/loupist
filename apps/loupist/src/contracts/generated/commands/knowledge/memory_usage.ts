// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

export type MemoryUsage = {
  recordId: string,
  usedCount: number,
  lastUsedAtMs: number,
  usedIn: Array<MemoryUsedIn>,
};

export type MemoryUsageRequest = { loupistId: string };

export type MemoryUsedIn = { chatId: string, atMs: number };
