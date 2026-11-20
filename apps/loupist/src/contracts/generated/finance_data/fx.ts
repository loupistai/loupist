// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

export type FxDisplay =
  | { "state": "unset" }
  | {
    "state": "ready",
    currency: string,
    revision: string,
    date: string,
    source: string,
    fetchedAt: string,
    usable_until?: string,
    behind?: FxDisplayBehind,
    rates: Record<string, number>,
  }
  | { "state": "waiting", currency: string }
  | { "state": "unavailable", currency: string, reason: FxUnavailableReason, lastDate?: string };

export type FxDisplayBehind = "offline" | "provider_failed" | "connection_off";

export type FxUnavailableReason =
  | "offline"
  | "provider_failed"
  | "connection_off"
  | "no_rate_for_currency"
  | "too_old";
