// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { PrivateClass } from "../sharing/scan";

export type Finding = {
  id: string,
  class: PrivateClass,
  itemKey: string,
  itemKind: ItemKind,
  itemLabel: string,
  symbol?: string,
  blocking: boolean,
};

export type ItemKind =
  | "profile"
  | "perspective"
  | "method"
  | "principle"
  | "entity"
  | "claim"
  | "source";

export type Sections = {
  profile: boolean,
  perspective: boolean,
  methods: boolean,
  principles: boolean,
  map: boolean,
};

export type SummaryCounts = {
  perspective: number,
  methods: number,
  principles: number,
  names: number,
  links: number,
  sources: number,
  leftOut: number,
  leftOutLicense: number,
};
