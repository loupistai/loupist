// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { License, PackKind, Rejection } from "../sharing/pack";
import type { Flag, ItemFinding } from "../sharing/scan";

export type CreditView = {
  name: string,
  author: string | null,
  license: License | null,
  url: string | null,
  version: number,
};

export type ExtensionCounts = { types: number, relations: number };

export type HostCount = { host: string, count: number };

export type ItemView = {
  key: string,
  kind: string,
  title: string,
  markdown: string,
  findings: Array<ItemFinding>,
};

export type ManifestView = {
  id: string,
  kind: PackKind,
  name: string,
  version: number,
  createdAt: string,
  language: string,
  license: License,
  licenseUrl: string,
  redistributable: boolean,
  author: string | null,
  origin: Array<OriginView>,
  credits: Array<CreditView>,
  assetClasses: Array<string>,
};

/**
 * One map record whose text looks like an instruction, names a tool, asks
 * for secrets, could send data out or had hidden characters (removed).
 */
export type MapFinding = {
  kind: string,
  /** The record's own name, relationship or title, hidden characters removed. */
  label: string,
  flags: Array<Flag>,
};

export type OpenResult =
  | { "kind": "preview", preview: PackPreview }
  | { "kind": "rejected", reason: Rejection, sourceLabel: string | null };

export type OriginView = { id: string, name: string, version: number, url: string | null };

export type PackPreview = {
  stagingId: string,
  manifest: ManifestView,
  sourceLabel: string | null,
  sourceUrl: string | null,
  profile: ProfileView,
  perspective: ItemView | null,
  methods: Array<ItemView>,
  principles: Array<ItemView>,
  world: WorldView,
  /** Flags on the pack's own words: its name, author, origin and profile. */
  profileFindings: Array<Flag>,
  flaggedCount: number,
  blocked: string | null,
};

export type ProfileView = { name: string, focus: string, tone: string, description: string };

export type WorldView = {
  names: number,
  links: number,
  sources: number,
  sourceHosts: Array<HostCount>,
  moreHosts: number,
  kinds: Array<string>,
  relations: Array<string>,
  extension: ExtensionCounts,
  findings: Array<MapFinding>,
  /** Every flagged map record, listed or not. */
  flagged: number,
};
