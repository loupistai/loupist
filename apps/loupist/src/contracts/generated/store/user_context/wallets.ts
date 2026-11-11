// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { WalletFamily } from "../../store/user_context/wallet_address";

export type WalletStatus = "unread" | "ok" | "failed";

/**
 * What the renderer may know about a wallet. `address` is only for the
 * "Copy address" action; every visible surface uses `masked`.
 */
export type WalletView = {
  id: string,
  family: WalletFamily,
  masked: string,
  address: string,
  name: string | null,
  addedAtMs: number,
  lastReadAtMs: number | null,
  lastGoodReadAtMs: number | null,
  status: WalletStatus,
  unlistedCount: number,
};
