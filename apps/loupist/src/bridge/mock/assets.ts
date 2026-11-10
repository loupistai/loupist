/**
 * Mock asset pages for the browser preview and renderer tests. Rust owns the
 * real model; this keeps installed fixtures, derives reasons and lines from the
 * mock Journal, and mirrors Undo's latest-move check.
 */
import type {
  AssetBridge, AssetClass, AssetDelta, AssetLine, AssetPage, AssetPageGetRequest, AssetReason, UpcomingEvent,
} from "../../contracts/assets";
import type { JournalRecord } from "../../contracts/knowledge";
import type { WatchlistEntry } from "../../contracts/userContext";
import { commandError } from "./commandError";

const clone = <T>(value: T): T => structuredClone(value);
const KOREAN = new Set(["KRX", "KOSPI", "KOSDAQ", "KONEX", "KS", "KQ"]);
const CRYPTO = new Set(["CRYPTO", "UPBIT", "BITHUMB", "BINANCE", "COINBASE", "KRAKEN", "BYBIT", "OKX"]);

/** A stable mock id in the Rust shape (`ast_` + 16 hex digits). */
export function mockAssetId(symbol: string, market: string) {
  const text = `${symbol.trim().toUpperCase()}:${market.trim().toUpperCase()}`;
  let a = 0x811c9dc5, b = 0x01000193;
  for (const char of text) {
    a = Math.imul(a ^ char.charCodeAt(0), 0x01000193) >>> 0;
    b = Math.imul(b ^ char.charCodeAt(0), 0x811c9dc5) >>> 0;
  }
  return `ast_${a.toString(16).padStart(8, "0")}${b.toString(16).padStart(8, "0")}`;
}

function classFor(market: string): AssetClass {
  const code = market.trim().toUpperCase();
  if (CRYPTO.has(code)) return "crypto";
  if (["CME", "COMEX", "NYMEX", "ICE"].includes(code)) return "commodity";
  if (["FX", "FOREX", "RATES", "UST"].includes(code)) return "rates_fx";
  if (code === "REAL") return "real_asset";
  if (["POLYMARKET", "KALSHI"].includes(code)) return "prediction";
  if (code === "PRIVATE") return "private";
  return "equity";
}

/** The plan's check kinds and what v1 does not read, as Rust reports them. */
export function mockChecks(assetClass: AssetClass, market: string): AssetPage["checks"] {
  const korean = KOREAN.has(market.trim().toUpperCase());
  switch (assetClass) {
    case "equity": return korean
      ? { kinds: ["filings", "results", "price"], connections: ["OpenDART", "KRX"], notYet: ["creditBalances", "shortSaleBalances", "lockUps"], failures: [], sourcesOnly: false }
      : { kinds: ["filings", "results", "price", "dividends"], connections: ["SEC EDGAR", "Market data platform"], notYet: ["estimateRevisions", "insiderTrades", "ownershipChanges"], failures: [], sourcesOnly: false };
    case "crypto": return { kinds: ["price", "drawdown"], connections: ["Market data platform"], notYet: ["tokenUnlocks", "tvl", "governance", "exchangeFlows"], failures: [], sourcesOnly: false };
    case "fund": return { kinds: ["price", "fees", "holdings"], connections: ["Market data platform"], notYet: ["fundFlows", "premiumDiscount"], failures: [], sourcesOnly: false };
    case "commodity": return { kinds: ["price"], connections: ["Market data platform"], notYet: ["positioning", "inventories"], failures: [], sourcesOnly: false };
    case "rates_fx": return { kinds: ["level"], connections: ["Market data platform"], notYet: ["centralBankCalendar", "auctions"], failures: [], sourcesOnly: false };
    case "prediction": return { kinds: [], connections: [], notYet: ["marketOdds"], failures: [], sourcesOnly: true };
    default: return { kinds: [], connections: [], notYet: ["valuationFeeds"], failures: [], sourcesOnly: true };
  }
}

const day = (ms: number) => new Date(ms).toISOString().slice(0, 10);

export class MockAssets implements AssetBridge {
  private readonly pages = new Map<string, AssetPage>();
  private readonly deltas = new Map<string, AssetDelta>();
  private readonly upcoming = new Map<string, UpcomingEvent[]>();
  /** Preview: Check now resolves only when this is false. */
  refreshHold = false;
  refreshFails = false;
  pageFails = false;
  undoFails = false;

  constructor(
    private readonly journals: (loupistId: string) => JournalRecord[],
    private readonly watchlist: () => Promise<WatchlistEntry[]>,
  ) {}

  private key(loupistId: string, assetId: string) { return `${loupistId}:${assetId}`; }

  /** Installs a page (and optionally its delta) for one Loupist. */
  install(loupistId: string, page: AssetPage, delta?: Omit<AssetDelta, "assetId" | "symbol" | "market" | "name" | "class">) {
    this.pages.set(this.key(loupistId, page.assetId), clone(page));
    if (delta) this.deltas.set(this.key(loupistId, page.assetId), { assetId: page.assetId, symbol: page.symbol, market: page.market, name: page.name, class: page.class, ...clone(delta) });
  }

  private withJournal(loupistId: string, page: AssetPage): AssetPage {
    const entries = this.journals(loupistId).filter(entry => !entry.archived && entry.thesis?.assetId === page.assetId);
    const reasons: AssetReason[] = [...page.reasons];
    const lines: AssetLine[] = [...page.lines];
    for (const entry of entries) {
      if (entry.thesis?.role === "reason" && !reasons.some(item => item.journalId === entry.journalId)) {
        reasons.push({ journalId: entry.journalId, text: entry.judgment, createdAtMs: entry.createdAtMs, updatedAtMs: entry.updatedAtMs, originChatId: entry.originChatId ?? undefined });
      }
      if (entry.thesis?.role === "line" && entry.check && !lines.some(item => item.journalId === entry.journalId)) {
        lines.push({ journalId: entry.journalId, claim: entry.check.claim, metric: entry.check.metric, direction: entry.check.direction, threshold: entry.check.threshold, unit: entry.check.unit, byDate: entry.check.byDate, createdAtMs: entry.createdAtMs, updatedAtMs: entry.updatedAtMs, status: "not_checked" });
      }
    }
    const read = page.read ?? (reasons.length ? { verdict: "holding" as const, since: day(Math.min(...reasons.map(item => item.createdAtMs))), started: true } : undefined);
    return { ...page, reasons, lines, read: reasons.length ? read : undefined };
  }

  async assetPageGet(request: AssetPageGetRequest): Promise<AssetPage> {
    if (this.pageFails) throw new Error("Couldn't open this page.");
    const assetId = request.assetId ?? mockAssetId(request.symbol ?? "", request.market ?? "");
    const key = this.key(request.loupistId, assetId);
    let page = this.pages.get(key);
    if (!page) {
      if (!request.symbol || !request.market) throw new Error("Asset page not found.");
      const followed = (await this.watchlist()).find(item => item.symbol === request.symbol && item.market === request.market);
      const assetClass = classFor(request.market);
      page = {
        assetId, symbol: request.symbol, market: request.market,
        name: followed?.displayName?.trim() || request.name?.trim() || request.symbol,
        class: assetClass, classSource: assetClass === "equity" ? "default" : "market",
        watch: followed ? "watching" : "none",
        reasons: [], lines: [], history: [], glance: [], checks: mockChecks(assetClass, request.market),
        createdAtMs: Date.now(), revision: 1,
      };
      this.pages.set(key, page);
    }
    return clone(this.withJournal(request.loupistId, page));
  }

  async assetDelta(request: { loupistId: string; assetId?: string; sinceMs: number }): Promise<AssetDelta[]> {
    return [...this.deltas.entries()]
      .filter(([key, delta]) => key.startsWith(`${request.loupistId}:`) && (!request.assetId || delta.assetId === request.assetId))
      .map(([, delta]) => clone(delta));
  }

  async assetRefresh(request: { loupistId: string; assetId: string; sinceMs?: number }): Promise<AssetPage> {
    while (this.refreshHold) await new Promise(resolve => setTimeout(resolve, 50));
    if (this.refreshFails) throw new Error("Couldn't check now.");
    const page = this.pages.get(this.key(request.loupistId, request.assetId));
    if (!page) throw new Error("Asset page not found.");
    page.refreshedAtMs = Date.now();
    page.revision += 1;
    return clone(this.withJournal(request.loupistId, page));
  }

  async assetClassSet(request: { loupistId: string; assetId: string; class: AssetClass }): Promise<AssetPage> {
    const page = this.pages.get(this.key(request.loupistId, request.assetId));
    if (!page) throw new Error("Asset page not found.");
    Object.assign(page, { class: request.class, classSource: "user", checks: mockChecks(request.class, page.market), revision: page.revision + 1 });
    return clone(this.withJournal(request.loupistId, page));
  }

  async assetVerdictUndo(request: { loupistId: string; assetId: string; moveId: string }): Promise<AssetPage> {
    const page = this.pages.get(this.key(request.loupistId, request.assetId));
    if (!page) throw new Error("Asset page not found.");
    if (this.undoFails) throw new Error("Couldn't undo.");
    const latest = page.history[0];
    // As in Rust: the move itself is the conflict check, not the page revision.
    if (!latest || latest.moveId !== request.moveId) throw commandError("conflict", "A newer change was made; reopen the page.");
    if (latest.actor !== "loupist" || latest.undone) throw new Error("Only the latest change can be undone.");
    latest.undone = true;
    const restored = latest.from ?? "holding";
    const undo = { ...latest, moveId: `${latest.moveId}-undo`, from: latest.to, to: restored, actor: "user" as const, undone: false, undoesMoveId: latest.moveId, reason: "Undone by you", evidence: [], line: undefined, atMs: Date.now() };
    page.history = [undo, ...page.history];
    page.read = { verdict: restored, since: latest.factDate, started: false, latestMove: undo };
    page.lines = page.lines.map(line => line.status === "crossed" && latest.line?.journalId === line.journalId ? { ...line, status: "not_crossed" } : line);
    page.revision += 1;
    return clone(this.withJournal(request.loupistId, page));
  }

  async assetPageSeen(_request: { loupistId: string; assetId: string }) {}

  /** Installs the event strip as Rust would order it (soonest first); it keeps at most five. */
  installUpcoming(loupistId: string, events: UpcomingEvent[]) { this.upcoming.set(loupistId, clone(events)); }

  async upcomingEvents(request: { loupistId: string }): Promise<UpcomingEvent[]> {
    return clone((this.upcoming.get(request.loupistId) ?? []).slice(0, 5));
  }

}
