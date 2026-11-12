// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { WatchEventView, WatchKind } from "../knowledge/watch";

export type ConnectionRef = { id: string, name: string };

export type KindFailure = { connection: string, atMs: number };

export type KindStateView = {
  kind: WatchKind,
  enabled: boolean,
  needsConnection?: ConnectionRef,
  failed?: KindFailure,
};

export type WatchAssetView = {
  assetId: string,
  symbol: string,
  market: string,
  name: string,
  kinds: Array<KindStateView>,
  revision: number,
  lastAlert?: WatchEventView,
};

export type WatchCheckNowRequest = { loupistId: string };

export type WatchCheckResult = { newAlerts: number, settings: WatchSettingsView };

export type WatchDefaultsApplyRequest = { loupistId: string };

export type WatchDefaultsApplyResult = {
  watchedAssets: number,
  needsConnections: Array<WatchDefaultsConnectionNeed>,
};

export type WatchDefaultsConnectionNeed = {
  kind: WatchKind,
  connection: ConnectionRef,
  assetIds: Array<string>,
};

export type WatchEventsListRequest = { loupistId: string, sinceMs?: number | null, limit: number };

export type WatchEventsSeenRequest = {
  loupistId: string,
  /** Empty: every delivered alert (Schedule › Watching was opened). */
  eventIds: Array<string>,
};

export type WatchPauseSetRequest = { loupistId: string, paused: boolean };

export type WatchSettingSetRequest = {
  loupistId: string,
  assetId: string,
  kind: WatchKind,
  enabled: boolean,
  expectedRevision: number,
};

export type WatchSettingsListRequest = { loupistId: string };

export type WatchSettingsView = {
  loupistId: string,
  paused: boolean,
  consentOff: boolean,
  checking: boolean,
  lastCheckedMs?: number,
  watched: Array<WatchAssetView>,
  notWatched: Array<WatchAssetView>,
};
