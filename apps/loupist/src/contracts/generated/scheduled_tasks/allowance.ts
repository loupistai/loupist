// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

export type AllowanceState = {
  background: Bucket,
  worldChat: Bucket,
  worldShare: WorldShare,
  /** Whether the single app-wide learning slot is occupied. */
  learningRunning: boolean,
  providerHolds: { [key in string]: Hold },
};

export type Bucket = { limit: number, used: number, reserved: number, state: Level };

export type Hold = { reason: HoldReason, local_day: string, provider: string | null };

export type HoldReason = "allowance" | "plan_limit" | "out_of_credit";

export type Level = "ok" | "warning" | "reached" | "off";

export type WorldShare = { used: number, limit: number };
