// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

/**
 * Whether World processing keeps this computer awake. One choice for every
 * Loupist; it holds only while plugged in and work remains.
 */
export type KeepAwakeStatus = {
  /** The host can prevent idle sleep (macOS). */
  supported: boolean,
  enabled: boolean,
  /** The host is keeping this computer awake now. */
  active: boolean,
  /** `null` when the power source is unknown or keep-awake is unsupported. */
  onBattery: boolean | null,
  /** Sources the World still has to process. */
  remaining: number,
};
