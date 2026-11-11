// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { JsonValue } from "../../external";
import type { Finding, Sections, SummaryCounts } from "../../sharing/export";
import type { License, LicenseConflict } from "../../sharing/pack";

export type ItemTitle = { kind: string, title: string };

export type RepoView = { fullName: string, url: string, version: number };

export type ScopeRequest = { "kind": "loupist" } | { "kind": "item", recordId: string };

export type ShareLicense = { id: License, url: string };

export type ShareRequest = {
  loupistId: string,
  scope: ScopeRequest,
  sections: Sections,
  excludedItemKeys: Array<string>,
  acknowledgedFindingIds: Array<string>,
  license?: License | null,
  byline?: string | null,
};

export type ShareSummary = {
  loupistName: string,
  scope: JsonValue,
  item: ItemTitle | null,
  counts: SummaryCounts,
  findings: Array<Finding>,
  allowedLicenses: Array<ShareLicense>,
  defaultLicense: License,
  licenseConflict: LicenseConflict | null,
  byline: string | null,
  repo: RepoView | null,
  nextVersion: number,
};

export type ShareSummaryRequest = {
  loupistId: string,
  scope: ScopeRequest,
  sections?: Sections | null,
  excludedItemKeys?: Array<string>,
};
