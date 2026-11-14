/**
 * Mock local watchers for the browser preview and renderer tests. Rust owns
 * cadence, detection, defaults and delivery; this keeps installed settings and
 * events and mirrors the revision check on each switch.
 */
import type {
  WatchAsset, WatchBridge, WatchDefaultsApplyRequest, WatchDefaultsApplyResult, WatchEvent, WatchEventsListRequest, WatchKind, WatchSettingSetRequest, WatchSettings,
} from "../../contracts/watch";
import type { WatchlistEntry } from "../../contracts/userContext";
import { mockAssetId } from "./assets";
import { commandError } from "./commandError";

const clone = <T>(value: T): T => structuredClone(value);
const KOREAN = new Set(["KRX", "KOSPI", "KOSDAQ", "KONEX", "KS", "KQ"]);
const CRYPTO = new Set(["CRYPTO", "UPBIT", "BITHUMB", "BINANCE", "COINBASE", "KRAKEN", "BYBIT", "OKX"]);
const COMMODITY = new Set(["CME", "COMEX", "NYMEX", "ICE"]);
const NOT_WATCHED = new Set(["FX", "FOREX", "RATES", "UST", "REAL", "POLYMARKET", "KALSHI", "PRIVATE"]);
/** As Rust's `watch/class.rs` maps them (a sample: stablecoins, coins DefiLlama tracks, CFTC markets, crude). */
const STABLECOINS = new Set(["USDT", "USDC", "DAI", "USDE", "USDS", "FDUSD", "PYUSD"]);
const VALUE_LOCKED = new Set(["ETH", "SOL", "BNB", "TRX", "AVAX", "ARB", "OP", "POL", "SUI", "AAVE", "UNI", "LDO", "SKY", "CRV", "ENA", "HYPE"]);
const CFTC = new Set(["GC", "SI", "HG", "PL", "PA", "CL", "NG", "HO", "RB", "ZC", "ZS", "ZW", "KC", "SB", "CT", "CC", "LE"]);
const OIL = new Set(["CL", "BZ", "MCL"]);

/** The kinds that apply to a followed asset, as Rust lists them: flows only for Korean stocks; class kinds where their source has the symbol. */
export function mockWatchKinds(market: string, enabled: readonly WatchKind[] = [], symbol = ""): WatchAsset["kinds"] {
  const code = market.trim().toUpperCase();
  const base = symbol.trim().toUpperCase();
  const kinds: WatchKind[] = CRYPTO.has(code)
    ? STABLECOINS.has(base) ? ["depeg"] : ["cryptoMove", "drawdown", ...(VALUE_LOCKED.has(base) ? ["tvl" as const] : [])]
    : COMMODITY.has(code)
      ? ["futuresMove", ...(CFTC.has(base) ? ["positioning" as const] : []), ...(OIL.has(base) ? ["oilInventory" as const] : [])]
      : ["filings", "results", "insiders", "holders", ...(KOREAN.has(code) ? ["flows" as const] : []), "dividends"];
  return kinds.map(kind => ({ kind, enabled: enabled.includes(kind) }));
}


export class MockWatch implements WatchBridge {
  private readonly settings = new Map<string, WatchSettings>();
  private readonly events = new Map<string, WatchEvent[]>();
  /** Per Loupist, the assets with a stored decision (see `decidedIn`). */
  private readonly decided = new Map<string, Set<string>>();
  /** Loupists whose list follows the watchlist (nothing was installed). */
  private readonly derived = new Set<string>();
  private readonly listeners = new Set<(event: { loupistId: string }) => void>();
  /** Preview: Check now resolves only when this is false. */
  checkHold = false;
  checkFails = false;
  /** Alerts the next Check now finds. */
  nextAlerts: WatchEvent[] = [];
  readonly setCalls: WatchSettingSetRequest[] = [];
  readonly seenCalls: string[][] = [];

  constructor(private readonly watchlist: () => Promise<WatchlistEntry[]>) {}

  /** Installs a Loupist's watch settings (and its alerts, newest first). */
  install(loupistId: string, settings: Partial<WatchSettings>, events: WatchEvent[] = []) {
    const { undecidedAssetIds, ...rest } = clone(settings);
    const installed: WatchSettings = { loupistId, paused: false, consentOff: false, checking: false, watched: [], notWatched: [], undecidedAssetIds: [], ...rest };
    this.settings.set(loupistId, installed);
    this.derived.delete(loupistId);
    this.decided.delete(loupistId);
    // Without a list, watched assets are decided and the others are not, as after a first Watch all.
    if (undecidedAssetIds) this.decided.set(loupistId, new Set([...installed.watched, ...installed.notWatched].map(asset => asset.assetId).filter(id => !undecidedAssetIds.includes(id))));
    this.events.set(loupistId, clone(events));
    this.emit(loupistId);
  }

  private emit(loupistId: string) { for (const listener of this.listeners) listener({ loupistId }); }

  /**
   * Without installed settings, every followed equity, coin and futures market is listed
   * (every kind off until chosen) and, as the host's list, follows and unfollows show on the next read.
   */
  private async current(loupistId: string) {
    let settings = this.settings.get(loupistId);
    if (settings && !this.derived.has(loupistId)) return settings;
    const followed = (await this.watchlist()).filter(entry => !NOT_WATCHED.has(entry.market.trim().toUpperCase()));
    const known = new Map([...settings?.watched ?? [], ...settings?.notWatched ?? []].map(asset => [asset.assetId, asset]));
    const assets = followed.map(entry => known.get(mockAssetId(entry.symbol, entry.market))
      ?? { assetId: mockAssetId(entry.symbol, entry.market), symbol: entry.symbol, market: entry.market, name: entry.displayName?.trim() || entry.symbol, kinds: mockWatchKinds(entry.market, [], entry.symbol), revision: 1 });
    const watched = new Set(settings?.watched ?? []);
    // Updated in place: a running Check now holds this object.
    settings ??= { loupistId, paused: false, consentOff: false, checking: false, watched: [], notWatched: [], undecidedAssetIds: [] };
    settings.watched = assets.filter(asset => watched.has(asset));
    settings.notWatched = assets.filter(asset => !watched.has(asset));
    this.settings.set(loupistId, settings);
    this.derived.add(loupistId);
    return settings;
  }

  async watchSettingsList({ loupistId }: { loupistId: string }): Promise<WatchSettings> {
    const settings = await this.current(loupistId);
    const events = this.events.get(loupistId) ?? [];
    const withAlert = (asset: WatchAsset) => ({ ...asset, lastAlert: asset.lastAlert ?? events.find(event => event.assetId === asset.assetId) });
    // As the host: an asset is undecided until a switch is set or Watch all gives it defaults.
    const decided = this.decidedIn(loupistId, settings);
    const undecidedAssetIds = [...settings.watched, ...settings.notWatched].map(asset => asset.assetId).filter(id => !decided.has(id));
    return clone({ ...settings, watched: settings.watched.map(withAlert), notWatched: settings.notWatched.map(withAlert), undecidedAssetIds });
  }

  async watchSettingSet(request: WatchSettingSetRequest): Promise<WatchSettings> {
    this.setCalls.push(clone(request));
    const settings = await this.current(request.loupistId);
    const asset = [...settings.watched, ...settings.notWatched].find(item => item.assetId === request.assetId);
    if (!asset) throw new Error("This asset is not followed.");
    if (asset.revision !== request.expectedRevision) throw commandError("conflict", "The setting changed. Reload and try again.");
    const kind = asset.kinds.find(item => item.kind === request.kind);
    if (!kind) throw new Error("This kind does not apply to this asset.");
    this.decidedIn(request.loupistId, settings).add(asset.assetId);
    if (kind.needsConnection && request.enabled) throw new Error(`Turn on ${kind.needsConnection.name} first.`);
    kind.enabled = request.enabled;
    asset.revision += 1;
    // An asset is watched while any kind is on, and keeps its place in follow order.
    const on = asset.kinds.some(item => item.enabled);
    settings.watched = settings.watched.filter(item => item !== asset);
    settings.notWatched = settings.notWatched.filter(item => item !== asset);
    (on ? settings.watched : settings.notWatched).push(asset);
    this.emit(request.loupistId);
    return this.watchSettingsList({ loupistId: request.loupistId });
  }

  async watchPauseSet({ loupistId, paused }: { loupistId: string; paused: boolean }): Promise<WatchSettings> {
    const settings = await this.current(loupistId);
    settings.paused = paused;
    this.emit(loupistId);
    return this.watchSettingsList({ loupistId });
  }

  /**
   * Mirrors the host's "Watch all" (`apply_defaults_with_consent`): consent on, and the
   * class defaults for each followed asset with no decision yet; an asset the user already
   * decided on keeps its switches. Defaults are on whether or not their connection is, and
   * every watched kind still missing one is reported in `needsConnections`.
   */
  async watchDefaultsApply({ loupistId }: WatchDefaultsApplyRequest): Promise<WatchDefaultsApplyResult> {
    const settings = await this.current(loupistId);
    const decided = this.decidedIn(loupistId, settings);
    settings.consentOff = false;
    // Equity defaults; a class's kinds start off unless the Loupist was made from its template, which the mock does not know.
    const defaults = new Set<WatchKind>(["filings", "results", "dividends"]);
    for (const asset of [...settings.notWatched]) {
      if (decided.has(asset.assetId)) continue;
      decided.add(asset.assetId);
      for (const kind of asset.kinds) kind.enabled = defaults.has(kind.kind);
      asset.revision += 1;
      if (!asset.kinds.some(kind => kind.enabled)) continue;
      settings.notWatched = settings.notWatched.filter(item => item !== asset);
      settings.watched.push(asset);
    }
    const needs = new Map<string, WatchDefaultsApplyResult["needsConnections"][number]>();
    for (const asset of settings.watched) for (const kind of asset.kinds) {
      if (!kind.enabled || !kind.needsConnection) continue;
      const key = `${kind.kind}:${kind.needsConnection.id}`;
      const entry = needs.get(key) ?? { kind: kind.kind, connection: { ...kind.needsConnection, locked: Boolean(kind.needsConnection.locked) }, assetIds: [] };
      entry.assetIds.push(asset.assetId); needs.set(key, entry);
    }
    this.emit(loupistId);
    return { watchedAssets: settings.watched.length, needsConnections: [...needs.values()] };
  }

  /** Assets with a stored decision, as the host's `knowledge_watch_settings` rows: watched ones, and any switch the user set. */
  private decidedIn(loupistId: string, settings: WatchSettings) {
    let decided = this.decided.get(loupistId);
    if (!decided) { decided = new Set(settings.watched.map(asset => asset.assetId)); this.decided.set(loupistId, decided); }
    return decided;
  }

  async watchCheckNow({ loupistId }: { loupistId: string }): Promise<{ newAlerts: number; settings: WatchSettings }> {
    const settings = await this.current(loupistId);
    settings.checking = true;
    try {
      while (this.checkHold) await new Promise(resolve => setTimeout(resolve, 50));
      if (this.checkFails) throw new Error("Couldn't check now.");
      const found = this.nextAlerts.splice(0).map(event => ({ ...event, detectedAtMs: Date.now() }));
      this.events.set(loupistId, [...found, ...this.events.get(loupistId) ?? []]);
      settings.lastCheckedMs = Date.now();
      return { newAlerts: found.length, settings: await this.watchSettingsList({ loupistId }) };
    } finally {
      settings.checking = false;
      this.emit(loupistId);
    }
  }

  async watchEventsList(request: WatchEventsListRequest): Promise<WatchEvent[]> {
    return clone((this.events.get(request.loupistId) ?? [])
      .filter(event => request.sinceMs === undefined || event.detectedAtMs >= request.sinceMs)
      .sort((a, b) => b.detectedAtMs - a.detectedAtMs)
      .slice(0, request.limit));
  }

  async watchEventsSeen({ loupistId, eventIds }: { loupistId: string; eventIds: string[] }): Promise<void> {
    this.seenCalls.push([...eventIds]);
    const now = Date.now();
    for (const event of this.events.get(loupistId) ?? []) if (eventIds.includes(event.eventId)) event.seenAtMs ??= now;
  }

  onWatchChanged(handler: (event: { loupistId: string }) => void): () => void {
    this.listeners.add(handler);
    return () => { this.listeners.delete(handler); };
  }
}
