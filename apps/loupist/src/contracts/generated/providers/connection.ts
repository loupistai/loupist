// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

/** Why a sign-in ended without a credential; the renderer maps each to copy. */
export type ConnectFailure =
  | "plan_permission_declined"
  | "plan_not_eligible"
  | "port_in_use"
  | "timed_out"
  | "browser_failed"
  | "offline"
  | "denied"
  | "other";

export type ModelNoticeAckRequest = { id: string };

export type ProviderConfigureRequest = {
  provider: string,
  api_key?: string | null,
  clear_saved_key?: boolean,
};

/** One step of a browser or device sign-in, sent on `provider_connect`'s channel. */
export type ProviderConnectionEvent =
  | { "type": "opening_browser", message: string, opened: boolean, url: string | null }
  | { "type": "waiting", message: string }
  | { "type": "connected", message: string }
  | {
    "type": "device_code",
    user_code: string,
    verification_uri: string,
    expires_in_s: number | null,
  }
  | { "type": "manual_entry" }
  | { "type": "failed", reason: ConnectFailure };

export type ProviderConnectionRequest = { provider: string };

export type ProviderModelsRequest = { provider: string };

export type ProviderPageRequest = { provider: string };

export type ProviderSubmitCodeRequest = { value: string };
