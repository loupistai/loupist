// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { Hop } from "../knowledge/connections";

export type ConnectionStatus = "new" | "shown" | "opened" | "dismissed" | "filed";

export type ConnectionView = {
  connectionId: string,
  seedNodeId: string,
  farNodeId: string,
  seedName: string,
  farName: string,
  path: Array<Hop>,
  score: number,
  status: ConnectionStatus,
  createdAtMs: number,
};
