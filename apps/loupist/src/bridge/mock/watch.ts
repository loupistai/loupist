/**
 * Mock local watchers for the browser preview and renderer tests. Rust owns
 * cadence, detection, defaults and delivery; this keeps installed settings and
 * events and mirrors the revision check on each switch.
 */
import type {
  WatchAsset, WatchBridge, WatchEvent, WatchEventsListRequest, WatchKind, WatchSettingSetRequest, WatchSettings,
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
    this.settings.set(loupistId, { loupistId, paused: false, consentOff: false, checking: false, watched: [], notWatched: [], ...clone(settings) });
    this.events.set(loupistId, clone(events));
    this.emit(loupistId);
  }

  private emit(loupistId: string) { for (const listener of this.listeners) listener({ loupistId }); }

  /** Without installed settings, every followed equity, coin and futures market is listed with every kind off. */
  private async current(loupistId: string) {
    let settings = this.settings.get(loupistId);
    if (!settings) {
      const followed = (await this.watchlist()).filter(entry => !NOT_WATCHED.has(entry.market.trim().toUpperCase()));
      settings = {
        loupistId, paused: false, consentOff: false, checking: false, watched: [],
        notWatched: followed.map(entry => ({ assetId: mockAssetId(entry.symbol, entry.market), symbol: entry.symbol, market: entry.market, name: entry.displayName?.trim() || entry.symbol, kinds: mockWatchKinds(entry.market, [], entry.symbol), revision: 1 })),
      };
      this.settings.set(loupistId, settings);
    }
    return settings;
  }

  async watchSettingsList({ loupistId }: { loupistId: string }): Promise<WatchSettings> {
    const settings = await this.current(loupistId);
    const events = this.events.get(loupistId) ?? [];
    const withAlert = (asset: WatchAsset) => ({ ...asset, lastAlert: asset.lastAlert ?? events.find(event => event.assetId === asset.assetId) });
    return clone({ ...settings, watched: settings.watched.map(withAlert), notWatched: settings.notWatched.map(withAlert) });
  }

  async watchSettingSet(request: WatchSettingSetRequest): Promise<WatchSettings> {
    this.setCalls.push(clone(request));
    const settings = await this.current(request.loupistId);
    const asset = [...settings.watched, ...settings.notWatched].find(item => item.assetId === request.assetId);
    if (!asset) throw new Error("This asset is not followed.");
    if (asset.revision !== request.expectedRevision) throw commandError("conflict", "The setting changed. Reload and try again.");
    const kind = asset.kinds.find(item => item.kind === request.kind);
    if (!kind) throw new Error("This kind does not apply to this asset.");
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

  /** Mirrors the host's "Watch all": consent on, class defaults for assets with every kind off. */
  async watchDefaultsApply({ loupistId }: { loupistId: string }): Promise<WatchSettings> {
    const settings = await this.current(loupistId);
    settings.consentOff = false;
    const defaults = new Set<WatchKind>(["filings", "results", "dividends"]);
    for (const asset of [...settings.notWatched]) {
      for (const kind of asset.kinds) if (defaults.has(kind.kind) && !kind.needsConnection) kind.enabled = true;
      if (!asset.kinds.some(kind => kind.enabled)) continue;
      asset.revision += 1;
      settings.notWatched = settings.notWatched.filter(item => item !== asset);
      settings.watched.push(asset);
    }
    this.emit(loupistId);
    return this.watchSettingsList({ loupistId });
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
