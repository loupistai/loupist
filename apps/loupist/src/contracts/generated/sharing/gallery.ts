// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { License } from "../sharing/pack";

/**
 * A validated, listed pack. The renderer gets everything but the location,
 * which Rust resolves from the id when the person presses Preview or Use.
 */
export type Entry = {
  id: string,
  /** "github.com/owner/repo" */
  source: string,
  name: string,
  focus: string,
  author: string | null,
  language: string,
  license: License,
  assetClasses: Array<string>,
  counts: EntryCounts,
  stars: number,
  forks: number,
  updatedAt: string,
  hasPreview: boolean,
};

export type EntryCounts = { methods: number, principles: number, names: number, links: number };
