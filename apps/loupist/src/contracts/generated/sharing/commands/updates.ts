// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { PackSource } from "../../sharing/commands/open";
import type { Rejection } from "../../sharing/pack";
import type { PackUpdate } from "../../sharing/update";

export type PackApplyUpdateRequest = {
  loupistId: string,
  importId: string,
  stagingId: string,
  itemKeys: Array<string>,
  applyMap: boolean,
};

export type PackCheckRequest = {
  loupistId: string,
  importId: string,
  /** The section opened (at most once a day); false for the button. */
  automatic?: boolean,
};

export type PackChooseNewerRequest = { loupistId: string, importId: string, source: PackSource };

export type UpdateCheck =
  | { "kind": "upToDate" }
  | { "kind": "notChecked" }
  | { "kind": "local" }
  | { "kind": "originGone" }
  | { "kind": "offline" }
  | { "kind": "differentPack" }
  | { "kind": "rejected", reason: Rejection }
  | { "kind": "available", update: PackUpdate };
