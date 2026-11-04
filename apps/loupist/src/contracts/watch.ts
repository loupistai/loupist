/**
 * Local watchers (finance-watchers.md; Rust `knowledge/watch`, `commands/watch.rs`).
 * Checks run on this computer without a model call; Rust owns cadence,
 * detection, defaults and delivery. React renders and submits typed intent.
 */
import type { AssetEvidence } from "./assets";

/** Equity kinds (finance-watchers.md §2.1). */
export const EQUITY_WATCH_KINDS = ["filings", "results", "insiders", "holders", "flows", "dividends"] as const;
/** Crypto kinds (finance-crypto-and-templates.md §3.3): price moves, the fall from the 90-day high, a stablecoin's peg, value locked. */
export const CRYPTO_WATCH_KINDS = ["cryptoMove", "drawdown", "depeg", "tvl"] as const;
/** Commodity kinds: the front contract's weekly move, CFTC positioning, EIA's weekly petroleum report. */
export const COMMODITY_WATCH_KINDS = ["futuresMove", "positioning", "oilInventory"] as const;
export type WatchKind = typeof EQUITY_WATCH_KINDS[number] | typeof CRYPTO_WATCH_KINDS[number] | typeof COMMODITY_WATCH_KINDS[number];
/** Kinds by asset class, in column order; an asset's kinds all come from one group. */
export const WATCH_KIND_GROUPS = [
  { id: "equity", kinds: EQUITY_WATCH_KINDS },
  { id: "crypto", kinds: CRYPTO_WATCH_KINDS },
  { id: "commodity", kinds: COMMODITY_WATCH_KINDS },
] as const satisfies readonly { id: string; kinds: readonly WatchKind[] }[];

export type WatchKindState = {
  kind: WatchKind;
  enabled: boolean;
  /** The connection this kind needs is off: the switch is disabled and says which one (Settings › Connections id). */
  needsConnection?: { id: string; name: string };
  /** The last check of this kind failed: the connection that did not answer. */
  failed?: { connection: string; atMs: number };
};

export type WatchAsset = {
  assetId: string;
  symbol: string;
  market: string;
  name: string;
  /** Only the kinds that apply to this asset (flows: Korean stocks; dividends: paid in the last 2 years). */
  kinds: WatchKindState[];
  /** Guards `watchSettingSet` against a concurrent change. */
  revision: number;
  /** The newest alert for this asset. */
  lastAlert?: WatchEvent;
};

export type WatchSettings = {
  loupistId: string;
  paused: boolean;
  /** Proactive updates are off (no morning update set up): nothing is watched until they are. */
  consentOff: boolean;
  /** A check of this Loupist is running now. */
  checking: boolean;
  lastCheckedMs?: number;
  /** Followed assets with at least one kind on, in follow order. */
  watched: WatchAsset[];
  /** Followed assets with every kind off ("Also followed, not watched here"). */
  notWatched: WatchAsset[];
};

/**
 * One event a check found. `facts` by kind —
 * filings: { form, title }; results: { date, period? };
 * insiders: { count, from, to, buyNotChecked? };
 * holders: { form?, title?, holder?, ratio?, before? };
 * flows: { direction: "in" | "out", to };
 * dividends: { amount?, currency?, recordDate?, exDate?, declared? };
 * cryptoMove / futuresMove: { window: "day" | "week", change (fraction), from, to, close };
 * drawdown: { level, drawdown (fraction), high, highDate, close, to };
 * depeg: { close, to }; tvl: { change (fraction), from, to, value, target };
 * positioning: { direction: "long" | "short", net, before, to, market };
 * oilInventory: { to, stocks?, change? (thousand barrels) }.
 */
export type WatchEvent = {
  eventId: string;
  assetId: string;
  name: string;
  symbol: string;
  market: string;
  kind: WatchKind;
  factDate: string;
  detectedAtMs: number;
  seenAtMs?: number;
  /** Where Rust delivered it: a Thread message, or the morning message's "What you follow". */
  deliveredIn?: "message" | "morning";
  facts: Record<string, unknown>;
  evidence: AssetEvidence[];
};

export type WatchSettingSetRequest = { loupistId: string; assetId: string; kind: WatchKind; enabled: boolean; expectedRevision: number };
export type WatchEventsListRequest = { loupistId: string; sinceMs?: number; limit: number };

export interface WatchBridge {
  watchSettingsList(request: { loupistId: string }): Promise<WatchSettings>;
  /** One switch or matrix checkbox. "Watch" on a not-watched row sets each default kind in turn. */
  watchSettingSet(request: WatchSettingSetRequest): Promise<WatchSettings>;
  watchPauseSet(request: { loupistId: string; paused: boolean }): Promise<WatchSettings>;
  /** Runs every enabled kind once for this Loupist; resolves when the check finished. */
  watchCheckNow(request: { loupistId: string }): Promise<{ newAlerts: number; settings: WatchSettings }>;
  /** Newest first. */
  watchEventsList(request: WatchEventsListRequest): Promise<WatchEvent[]>;
  /** The alerts were on screen (Thread message for 1 s, or Schedule › Watching opened). */
  watchEventsSeen(request: { loupistId: string; eventIds: string[] }): Promise<void>;
  /** Rust emits `watch-changed` after a check, a setting change or a delivery. */
  onWatchChanged(handler: (event: { loupistId: string }) => void): () => void;
}
