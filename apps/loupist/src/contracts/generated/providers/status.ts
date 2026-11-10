// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

export type CredentialKind = "plan" | "api_key" | "local" | "custom" | "environment" | "none";

export type NoticeKind = "plan_limit" | "plan_not_eligible" | "out_of_credit";

export type ProviderNotice = { kind: NoticeKind, at_ms: number };

export type ProviderState = "connected" | "needs_reconnect" | "disconnected";

/** A provider's new state (`crate::events::PROVIDER_STATUS`). */
export type ProviderStatusEvent = { provider: string, status: ProviderStatusView };

/** Rust-owned provider state: in the model catalog and the `provider-status` event. */
export type ProviderStatusView = {
  state: ProviderState,
  credential_kind: CredentialKind,
  notice: ProviderNotice | null,
  last_limit_at_ms: number | null,
  connected_at_ms: number | null,
  used_by: UsedBy,
};

export type UsedBy = { schedules: number, chat_default: boolean, world_models: number };
