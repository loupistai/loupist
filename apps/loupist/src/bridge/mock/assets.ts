/**
 * Mock assets for the browser preview and renderer tests, after the thin-host
 * contract (`commands/assets/wire.rs`). Rust owns the real catalog, replay and
 * thesis; this keeps an app-level catalog of names with reference fields, each
 * Loupist's follows, pages and stored prices, derives reasons and lines from the
 * mock Journal and mirrors Undo's latest-move check.
 *
 * A replay is scripted: `installPrice` sets what the stored price and saved read
 * are and what the next replay does. Nothing here reaches a network.
 */
import type {
  AssetBridge, AssetEntitySetRequest, AssetFollowNameRequest, AssetIdRequest, AssetLine, AssetLookupCandidate, AssetLookupRequest, AssetLookupResult,
  AssetOpenRequest, AssetPage, AssetPricePoint, AssetReason, AssetReferenceFields, AssetReferenceOrigin, AssetRow, AssetStoredPrice,
  AssetVerdictUndoRequest, AssetsListRequest, PriceReadState, SavedPriceRead, StoredAssetQuote,
} from "../../contracts/assets";
import type { JournalRecord } from "../../contracts/knowledge";
import { commandError, rejectCommand } from "./commandError";

const clone = <T>(value: T): T => structuredClone(value);
const day = (ms: number) => new Date(ms).toISOString().slice(0, 10);
/** The Rust asset id shape: `ast_` and 16 lowercase hex digits. */
const ASSET_ID = /^ast_[0-9a-f]{16}$/;
/** The host refuses longer names and queries. */
const NAME_MAX = 200;
const QUERY_MAX = 128;

/** A stable mock id in the Rust shape from any key (a FIGI, a ticker or a name). */
export function mockAssetId(key: string) {
  const text = key.trim().toUpperCase();
  let a = 0x811c9dc5, b = 0x01000193;
  for (const char of text) {
    a = Math.imul(a ^ char.charCodeAt(0), 0x01000193) >>> 0;
    b = Math.imul(b ^ char.charCodeAt(0), 0x811c9dc5) >>> 0;
  }
  return `ast_${a.toString(16).padStart(8, "0")}${b.toString(16).padStart(8, "0")}`;
}

/** Rust trim uses Unicode White_Space, and the cap counts Unicode scalars. */
const trimmed = (value: string) => value.replace(/^\p{White_Space}+|\p{White_Space}+$/gu, "");
const same = (left: string | null | undefined, right: string | null | undefined) =>
  Boolean(left && right) && left!.trim().toLocaleLowerCase() === right!.trim().toLocaleLowerCase();

/** The listings the mock OpenFIGI answers with; a query matches a name or ticker that contains it. */
const LISTINGS: AssetLookupCandidate[] = [
  { figi: "BBG000BBJQV0", name: "NVIDIA CORP", ticker: "NVDA", exchange: "US", securityType: "Common Stock", marketSector: "Equity" },
  { figi: "BBG000BBJR79", name: "NVIDIA CORP", ticker: "NVDA", exchange: "UW", securityType: "Common Stock", marketSector: "Equity" },
  { figi: "BBG000B9XRY4", name: "APPLE INC", ticker: "AAPL", exchange: "US", securityType: "Common Stock", marketSector: "Equity" },
  { figi: "BBG000N9MNX3", name: "TESLA INC", ticker: "TSLA", exchange: "US", securityType: "Common Stock", marketSector: "Equity" },
  { figi: "BBG000BCQZS4", name: "SAMSUNG ELECTRONICS CO LTD", ticker: "005930", exchange: "KS", securityType: "Common Stock", marketSector: "Equity" },
  { figi: "BBG000GQVT51", name: "SK HYNIX INC", ticker: "000660", exchange: "KS", securityType: "Common Stock", marketSector: "Equity" },
  { figi: "BBG000BHTMY2", name: "SPDR S&P 500 ETF TRUST", ticker: "SPY", exchange: "US", securityType: "ETP", marketSector: "Equity" },
  { figi: "BBG00ZF5Y7T6", name: "Ridgeline Robotics", ticker: "RDGL", exchange: "US", securityType: "Common Stock", marketSector: "Equity" },
];

/** A page fixture: everything but the catalog row, which `follow` or `installAsset` owns. */
export type MockPageFixture = Partial<Omit<AssetPage, "asset" | "currentPrice" | "savedRead">>;

/** What the next replay of a saved read does. */
export type MockReplay = "ready" | "failed" | "needs_connection";

/** A scripted stored price: what the page shows now, its saved read, its chart and the next replay. */
export type MockPriceFixture = {
  price?: AssetStoredPrice | null;
  /** The saved read; `state` is what the page shows now. Null: no saved read yet. */
  savedRead?: SavedPriceRead | null;
  points?: AssetPricePoint[];
  /** What Check now does; default `ready` with `next` (or the same price). */
  replay?: MockReplay;
  /** The value and date the next successful replay states. */
  next?: Pick<AssetStoredPrice, "value" | "date">;
};

/** A saved read of one bundled tool, as `price_read_save` would leave it. */
export function mockSavedRead(state: Exclude<PriceReadState, "none">, tool = "equity_quote", history = false): SavedPriceRead {
  return {
    state,
    current: { tool, server: "openbb", arguments: { symbol: "NVDA", provider: "yfinance" }, fields: { value: "/results/0/last_price", date: "/results/0/last_timestamp", currency: "/results/0/currency", rows: null }, rolling: null },
    history: history ? { tool: "equity_price_historical", server: "openbb", arguments: { symbol: "NVDA", start_date: "2026-08-19", end_date: "2026-11-19", provider: "yfinance" },
      fields: { value: "/close", date: "/date", currency: null, rows: "/results" }, rolling: { fromArg: "start_date", toArg: "end_date", days: 92 } } : null,
  };
}

type Price = { price: AssetStoredPrice | null; savedRead: SavedPriceRead | null; points: AssetPricePoint[]; replay: MockReplay; next?: Pick<AssetStoredPrice, "value" | "date"> };

export class MockAssets implements AssetBridge {
  /** The catalog every Loupist shares; a row's `followedAtMs` is read per Loupist from `follows`. */
  private readonly rows = new Map<string, AssetRow>();
  /** When each Loupist followed each asset, by `loupistId:assetId`. */
  private readonly follows = new Map<string, number>();
  private readonly pages = new Map<string, MockPageFixture & { revision: number }>();
  private readonly prices = new Map<string, Price>();
  private readonly pendingSetups = new Set<string>();
  private readonly listeners = new Set<() => void>();
  private readonly undone = new Set<string>();

  /** Every submitted lookup query, in order. */
  readonly lookups: string[] = [];
  /** Delegated price setups the host would start: one per follow burst or Check now without a usable read. */
  readonly setups: { loupistId: string; assetIds: string[]; reason: "follow" | "check" }[] = [];
  lookupFails = false;
  /** Lookup answers arrive only while this is false. */
  lookupHold = false;
  /** Asset lookup is switched off for the Loupist: `asset_lookup` refuses with `connection_unavailable`. */
  lookupNeedsConnection = false;
  /** OpenFIGI answered 429: `asset_lookup` refuses with `rate_limited`. */
  lookupRateLimited = false;
  /** A saved OpenFIGI key the host could not read: the public route still answers. */
  lookupSavedKeyUnread = false;
  followFails = false;
  unfollowFails = false;
  pageFails = false;
  checkFails = false;
  /** Check now answers only while this is false. */
  checkHold = false;
  seriesFails = false;
  undoFails = false;

  constructor(
    private readonly journals: (loupistId: string) => JournalRecord[],
    private readonly admit: (loupistId: string) => void = () => {},
    /** The Loupists a fixture asset is followed by when it names none. */
    private readonly loupists: () => string[] = () => [],
  ) {}

  private key(loupistId: string, assetId: string) { return `${loupistId}:${assetId}`; }

  /** The catalog row as this Loupist reads it: followed since its own follow, or not at all. */
  private asRow(loupistId: string, row: AssetRow): AssetRow {
    return clone({ ...row, followedAtMs: this.follows.get(this.key(loupistId, row.assetId)) ?? null });
  }

  private followed(loupistId: string) {
    return [...this.rows.values()].filter(row => this.follows.has(this.key(loupistId, row.assetId)));
  }

  /** Follows for this Loupist, keeping an earlier follow time. */
  private follow(loupistId: string, assetId: string, atMs: number) {
    const key = this.key(loupistId, assetId);
    if (!this.follows.has(key)) this.follows.set(key, atMs);
  }

  /** As removing a Loupist in Rust: its follows go with it. */
  forgetLoupist(loupistId: string) {
    for (const key of [...this.follows.keys()]) if (key.startsWith(`${loupistId}:`)) this.follows.delete(key);
  }

  onAssetsChanged(handler: () => void) {
    this.listeners.add(handler);
    return () => { this.listeners.delete(handler); };
  }
  /** As Rust's `assets-changed`: a unit event after a follow, an unfollow or a stored price change. */
  changed() { for (const handler of [...this.listeners]) handler(); }

  /**
   * Installs a catalog row, followed by `followedBy` (by default every Loupist the
   * mock has now; none with `followed: false`). A row with a ticker and no FIGI is
   * `imported` (an asset kept from before the thin host); one with a FIGI is `openfigi`.
   */
  installAsset(asset: { name: string; ticker?: string; exchange?: string; exchangeName?: string; figi?: string; securityType?: string; assetId?: string; followed?: boolean; followedBy?: string[]; origin?: AssetReferenceOrigin; createdAtMs?: number }): AssetRow {
    const reference = asset.ticker || asset.figi || asset.exchange || asset.exchangeName
      ? { figi: asset.figi ?? null, ticker: asset.ticker ?? null, exchange: asset.exchange ?? null, exchangeName: asset.exchangeName, securityType: asset.securityType ?? null, origin: asset.origin ?? (asset.figi ? "openfigi" as const : "imported" as const) }
      : null;
    const assetId = asset.assetId ?? mockAssetId(asset.figi ?? asset.ticker ?? asset.name);
    const createdAtMs = asset.createdAtMs ?? Date.now() - 40 * 86_400_000;
    this.rows.set(assetId, { assetId, name: asset.name, reference, followedAtMs: null, createdAtMs });
    const followers = asset.followed === false ? [] : asset.followedBy ?? this.loupists();
    for (const loupistId of followers) this.follow(loupistId, assetId, createdAtMs);
    return clone({ assetId, name: asset.name, reference, followedAtMs: followers.length ? createdAtMs : null, createdAtMs });
  }

  /** Installs a page's thesis, read and Map counts for one Loupist. */
  install(loupistId: string, assetId: string, page: MockPageFixture = {}) {
    const current = this.pages.get(this.key(loupistId, assetId));
    this.pages.set(this.key(loupistId, assetId), { ...current, ...clone(page), revision: page.revision ?? (current?.revision ?? 0) + 1 });
  }

  /** Installs a stored price, its saved read and chart points, and what the next replay does. */
  installPrice(loupistId: string, assetId: string, fixture: MockPriceFixture) {
    const current = this.prices.get(this.key(loupistId, assetId));
    this.prices.set(this.key(loupistId, assetId), {
      price: fixture.price === undefined ? current?.price ?? null : clone(fixture.price),
      savedRead: fixture.savedRead === undefined ? current?.savedRead ?? null : clone(fixture.savedRead),
      points: fixture.points ? clone(fixture.points) : current?.points ?? [],
      replay: fixture.replay ?? current?.replay ?? "ready",
      next: fixture.next ?? current?.next,
    });
  }

  finishSetup(loupistId: string, assetId: string) {
    this.pendingSetups.delete(this.key(loupistId, assetId));
    this.changed();
  }

  private row(assetId: string) {
    const row = this.rows.get(assetId);
    if (!row) rejectCommand("not_found", "Asset not found.");
    return row!;
  }

  private validId(request: { loupistId: unknown; assetId: unknown }) {
    if (typeof request.loupistId !== "string" || typeof request.assetId !== "string" || !ASSET_ID.test(request.assetId)) rejectCommand("invalid_request", "Invalid asset id.");
    this.admit(request.loupistId as string);
  }

  async assetsList({ loupistId }: AssetsListRequest): Promise<AssetRow[]> {
    this.admit(loupistId);
    return this.followed(loupistId).map(row => this.asRow(loupistId, row));
  }

  async assetLookup({ loupistId, query }: AssetLookupRequest): Promise<AssetLookupResult> {
    this.admit(loupistId);
    const text = trimmed(query);
    if (!text || [...text].length > QUERY_MAX) return rejectCommand("invalid_request", "Search text must be 1 to 128 characters.");
    this.lookups.push(text);
    while (this.lookupHold) await new Promise(resolve => setTimeout(resolve, 20));
    if (this.lookupNeedsConnection) throw commandError("connection_unavailable", "Asset lookup is turned off for this Loupist.");
    if (this.lookupRateLimited) throw commandError("rate_limited", "OpenFIGI asked to wait.");
    if (this.lookupFails) throw commandError("provider_unavailable", "OpenFIGI did not answer.");
    const needle = text.toLocaleLowerCase();
    const candidates = LISTINGS.filter(item => item.name.toLocaleLowerCase().includes(needle) || item.ticker?.toLocaleLowerCase() === needle).map(clone);
    return { candidates, savedKeyUnread: this.lookupSavedKeyUnread };
  }

  async assetFollowName(request: AssetFollowNameRequest): Promise<AssetRow> {
    const { loupistId } = request;
    if (Object.keys(request).some(key => !["loupistId", "name", "reference"].includes(key)) || typeof loupistId !== "string" || typeof request.name !== "string") {
      return rejectCommand("invalid_request", "Invalid follow request.");
    }
    this.admit(loupistId);
    const name = trimmed(request.name);
    if (!name || [...name].length > NAME_MAX || /[\p{Cc}\p{Cs}]/u.test(name)) return rejectCommand("invalid_request", "Name must be 1 to 200 characters without control characters.");
    if (this.followFails) throw commandError("internal", "Couldn't follow.");
    const reference: AssetReferenceFields | null = request.reference ?? null;
    // Of several matches, one this Loupist follows wins, then the lowest ID.
    const rows = [...this.rows.values()].map(row => this.asRow(loupistId, row)).sort((a, b) =>
      Number(a.followedAtMs === null) - Number(b.followedAtMs === null) || a.assetId.localeCompare(b.assetId));
    const listing = reference && rows.find(({ reference: stored }) => {
      if (!stored) return false;
      if (reference.figi != null) return stored.figi === reference.figi;
      return reference.ticker != null && stored.ticker === reference.ticker
        && (stored.exchange ?? null) === (reference.exchange ?? null);
    });
    const lower = name.toLowerCase();
    const existing = listing ?? rows.find(row => (!reference || !row.reference) && trimmed(row.name).toLowerCase() === lower);
    const savedReference: AssetRow["reference"] = existing?.reference
      ?? (reference && { ...reference, origin: reference.figi != null ? "openfigi" : "user" });
    const tickerKey = reference?.ticker ? `listing:${reference.ticker}:${reference.exchange ?? ""}` : null;
    const now = Date.now();
    const row: AssetRow = existing
      ? { ...existing, reference: savedReference, followedAtMs: null }
      : { assetId: mockAssetId(reference?.figi ?? tickerKey ?? `name:${name}`), name, reference: savedReference, followedAtMs: null, createdAtMs: now };
    this.rows.set(row.assetId, row);
    this.follow(loupistId, row.assetId, now);
    this.startSetup(loupistId, row.assetId, "follow");
    this.changed();
    return this.asRow(loupistId, row);
  }

  async assetFollow(request: AssetIdRequest): Promise<AssetRow> {
    this.validId(request);
    if (this.followFails) throw commandError("internal", "Couldn't follow.");
    const row = this.row(request.assetId);
    this.follow(request.loupistId, row.assetId, Date.now());
    this.startSetup(request.loupistId, row.assetId, "follow");
    this.changed();
    return this.asRow(request.loupistId, row);
  }

  async assetUnfollow(request: AssetIdRequest): Promise<void> {
    this.validId(request);
    if (this.unfollowFails) throw commandError("internal", "Couldn't stop following.");
    this.row(request.assetId);
    this.follows.delete(this.key(request.loupistId, request.assetId));
    this.changed();
  }

  /** A follow burst (the same Loupist within a second) shares one setup; an asset with a usable read needs none. */
  private startSetup(loupistId: string, assetId: string, reason: "follow" | "check") {
    const key = this.key(loupistId, assetId);
    const read = this.prices.get(key)?.savedRead;
    if (this.pendingSetups.has(key) || read?.state === "ready" || read?.state === "needs_connection") return;
    this.pendingSetups.add(key);
    this.changed();
    const last = this.setups.at(-1);
    if (reason === "follow" && last?.reason === "follow" && last.loupistId === loupistId) { if (!last.assetIds.includes(assetId)) last.assetIds.push(assetId); return; }
    this.setups.push({ loupistId, assetIds: [assetId], reason });
  }

  private view(loupistId: string, assetId: string): AssetPage {
    const row = this.row(assetId);
    const fixture = this.pages.get(this.key(loupistId, assetId));
    const price = this.prices.get(this.key(loupistId, assetId));
    const entries = this.journals(loupistId).filter(entry => !entry.archived && entry.thesis?.assetId === assetId);
    const reasons: AssetReason[] = [...fixture?.reasons ?? []];
    const lines: AssetLine[] = [...fixture?.lines ?? []];
    for (const entry of entries) {
      if (entry.thesis?.role === "reason" && !reasons.some(item => item.journalId === entry.journalId)) {
        reasons.push({ journalId: entry.journalId, text: entry.judgment, createdAtMs: entry.createdAtMs, updatedAtMs: entry.updatedAtMs, originChatId: entry.originChatId ?? undefined });
      }
      if (entry.thesis?.role === "line" && entry.check && !lines.some(item => item.journalId === entry.journalId)) {
        lines.push({ journalId: entry.journalId, claim: entry.check.claim, metric: entry.check.metric, direction: entry.check.direction, threshold: entry.check.threshold, unit: entry.check.unit, byDate: entry.check.byDate, createdAtMs: entry.createdAtMs, updatedAtMs: entry.updatedAtMs, status: "not_checked" });
      }
    }
    const read = fixture?.read ?? (reasons.length ? { verdict: "holding" as const, since: day(Math.min(...reasons.map(item => item.createdAtMs))), started: true } : null);
    return clone({
      asset: this.asRow(loupistId, row),
      priceSetupRunning: this.pendingSetups.has(this.key(loupistId, assetId)),
      currentPrice: price?.price ?? null,
      savedRead: price?.savedRead ?? null,
      entityId: fixture?.entityId ?? null,
      reasons, lines,
      read: reasons.length ? read : null,
      history: fixture?.history ?? [],
      map: fixture?.entityId ? fixture.map ?? null : null,
      revision: fixture?.revision ?? 1,
    });
  }

  async assetPageGet(request: AssetOpenRequest): Promise<AssetPage> {
    const { loupistId } = request;
    this.admit(loupistId);
    if (this.pageFails) throw new Error("Couldn't open this page.");
    if (request.entityId) {
      for (const row of this.rows.values()) {
        const page = this.view(loupistId, row.assetId);
        if (page.entityId === request.entityId && page.read) return page;
      }
      throw commandError("not_found", "Asset page was not found.");
    }
    if (request.assetId) {
      if (!ASSET_ID.test(request.assetId)) return rejectCommand("invalid_request", "Invalid asset id.");
      if (this.rows.has(request.assetId)) return this.view(loupistId, request.assetId);
    }
    // A saved link: by id, else by exact reference ticker, else the asset is not tracked.
    const ticker = request.ticker?.trim();
    const matches = ticker ? [...this.rows.values()].filter(row => same(row.reference?.ticker, ticker)) : [];
    if (matches.length > 1) throw commandError("ambiguous_asset", "More than one asset has this ticker.");
    if (matches.length === 1) return this.view(loupistId, matches[0]!.assetId);
    throw commandError("unknown_asset", "This asset is not tracked.");
  }

  async assetPriceCheck(request: AssetIdRequest): Promise<AssetPage> {
    this.validId(request);
    this.row(request.assetId);
    if (!this.follows.has(this.key(request.loupistId, request.assetId))) {
      return this.view(request.loupistId, request.assetId);
    }
    while (this.checkHold) await new Promise(resolve => setTimeout(resolve, 20));
    if (this.checkFails) throw new Error("Couldn't check now.");
    const key = this.key(request.loupistId, request.assetId);
    if (this.pendingSetups.has(key)) return this.view(request.loupistId, request.assetId);
    const price = this.prices.get(key);
    if (!price?.savedRead || price.savedRead.state === "none") {
      this.startSetup(request.loupistId, request.assetId, "check");
      return this.view(request.loupistId, request.assetId);
    }
    const wasFailed = price.savedRead.state === "failed";
    if (price.replay === "ready") {
      const base = price.price;
      if (base || price.next) {
        price.price = { ...(base ?? { currency: null, currencySource: "none", provider: "yfinance", sourceUrl: null, dataset: { dataset_id: "mock-replay", revision: 1, digest: "mock" } }), ...(price.next ?? { value: base!.value, date: base!.date }) };
        const point = { date: price.price.date.slice(0, 10), value: price.price.value };
        price.points = [...price.points.filter(item => item.date !== point.date), point];
      }
      price.savedRead = { ...price.savedRead, state: "ready" };
    } else {
      // A failed replay keeps the last good price and marks the read.
      price.savedRead = { ...price.savedRead, state: price.replay };
      if (wasFailed && price.replay === "failed") this.startSetup(request.loupistId, request.assetId, "check");
    }
    this.changed();
    return this.view(request.loupistId, request.assetId);
  }

  async assetPriceSeries(request: AssetIdRequest): Promise<AssetPricePoint[]> {
    this.validId(request);
    this.row(request.assetId);
    if (this.seriesFails) throw new Error("Couldn't read the chart.");
    return clone(this.prices.get(this.key(request.loupistId, request.assetId))?.points ?? []);
  }

  async watchlistQuotes({ loupistId }: AssetsListRequest): Promise<StoredAssetQuote[]> {
    this.admit(loupistId);
    return this.followed(loupistId).flatMap(row => {
      const price = this.prices.get(this.key(loupistId, row.assetId))?.price;
      return price ? [{ assetId: row.assetId, price: clone(price) }] : [];
    });
  }

  async assetPageSeen(request: AssetIdRequest) { this.validId(request); }

  async assetEntitySet(request: AssetEntitySetRequest): Promise<AssetPage> {
    this.validId(request);
    this.row(request.assetId);
    const key = this.key(request.loupistId, request.assetId);
    const fixture = this.pages.get(key) ?? { revision: 1 };
    this.pages.set(key, { ...fixture, entityId: request.entityId, map: request.entityId ? fixture.map : null, revision: fixture.revision + 1 });
    return this.view(request.loupistId, request.assetId);
  }

  async assetVerdictUndo(request: AssetVerdictUndoRequest): Promise<AssetPage> {
    this.validId(request);
    const key = this.key(request.loupistId, request.assetId);
    const page = this.pages.get(key);
    if (!page) throw new Error("Asset page not found.");
    if (this.undoFails) throw new Error("Couldn't undo.");
    const history = page.history ?? [];
    const latest = history[0];
    // As in Rust: the move itself is the conflict check, not the page revision.
    if (!latest || latest.moveId !== request.moveId || this.undone.has(latest.moveId)) throw commandError("conflict", "A newer change was made; reopen the page.");
    if (latest.actor !== "loupist" || latest.undone) throw new Error("Only the latest change can be undone.");
    this.undone.add(latest.moveId);
    latest.undone = true;
    const restored = latest.from ?? "holding";
    const undo = { ...latest, moveId: `${latest.moveId}-undo`, from: latest.to, to: restored, actor: "user" as const, undone: false, undoesMoveId: latest.moveId, reason: "Undone by you", evidence: [], line: undefined, atMs: Date.now() };
    page.history = [undo, ...history];
    page.read = { verdict: restored, since: latest.factDate, started: false, latestMove: undo };
    page.lines = (page.lines ?? []).map(line => line.status === "crossed" && latest.line?.journalId === line.journalId ? { ...line, status: "not_crossed" } : line);
    page.revision += 1;
    return this.view(request.loupistId, request.assetId);
  }
}
