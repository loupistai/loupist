import type {
  UserContextBridge, WatchlistEntry, WatchlistAddRequest, HoldingsSnapshot, HoldingsCsvPreview, HoldingsSummary,
  CryptoFilePreview, WalletView, HoldingsPosition,
} from "../../contracts/userContext";
import { detectNetwork, looksLikeSecret, maskAddress } from "../../components/holdings/walletAddress";

const fail = (code: string, message: string): never => { throw Object.assign(new Error(message), { code }); };
const identity = (value: string) => {
  const normalized = value.trim().toUpperCase();
  if (!normalized) fail("invalid_request", "Invalid symbol or market.");
  return normalized;
};
const cryptoPosition = (symbol: string, quantity: string): HoldingsPosition => ({ symbol, market: "CRYPTO", displayName: null, quantity, costBasis: null, marketValue: null, currency: "" });

/** A Kraken ledger as Rust previews it: codes merged, cash and empty balances skipped. */
export const MOCK_KRAKEN_PREVIEW: Omit<CryptoFilePreview, "previewId"> = {
  format: "kraken_ledger", rowCount: 412, asOfMs: Date.parse("2026-10-31T08:30:00Z"),
  rows: [
    { symbol: "BTC", quantity: "0.4271", from: ["XXBT"] },
    { symbol: "ETH", quantity: "3.2", from: ["XETH", "ETH2.S"] },
    { symbol: "SOL", quantity: "41.07", from: ["SOL"] },
  ],
  skipped: [{ symbol: "USD", reason: "cash" }, { symbol: "EUR", reason: "cash" }, { symbol: "DOGE", reason: "zero" }],
  errors: [], mappingHeaders: null, tradesOnly: false,
};

/** Deterministic browser fixture; no filesystem, broker or blockchain access. */
export class MockUserContext implements UserContextBridge {
  private watchlist: WatchlistEntry[] = [];
  private previews = new Map<string, HoldingsCsvPreview>();
  private cryptoPreviews = new Map<string, CryptoFilePreview>();
  /** Latest snapshot per source; wallets keep one per address. */
  private latest = new Map<string, { source: HoldingsSnapshot["source"]; importedAtMs: number; positions: HoldingsPosition[] }>();
  private wallets: WalletView[] = [];
  /** Wallet ids whose next reads fail (the explorer did not answer). */
  readonly failingWallets = new Set<string>();

  constructor(private readonly hasKisAccountProfile: () => boolean = () => false) {}

  async watchlistList() { return structuredClone(this.watchlist); }
  async holdingsSummary(): Promise<HoldingsSummary> {
    const kinds = { stocks: new Map<string, number>(), crypto: new Map<string, number>() };
    const sources: HoldingsSummary["sources"] = [];
    for (const snapshot of this.latest.values()) {
      const kind = snapshot.source === "crypto_file" || snapshot.source === "wallet" ? kinds.crypto : kinds.stocks;
      for (const position of snapshot.positions) kind.set(`${position.symbol}:${position.market}`, Math.max(kind.get(`${position.symbol}:${position.market}`) ?? 0, snapshot.importedAtMs));
      if (snapshot.source !== "wallet") sources.push({ source: snapshot.source, importedAtMs: snapshot.importedAtMs, count: snapshot.positions.length });
    }
    const count = (items: Map<string, number>, source: (s: HoldingsSnapshot["source"]) => boolean) => {
      const dates = [...this.latest.values()].filter(snapshot => source(snapshot.source)).map(snapshot => snapshot.importedAtMs);
      return dates.length ? { count: items.size, importedAtMs: Math.max(...dates) } : null;
    };
    return {
      sources,
      stocks: count(kinds.stocks, source => source === "csv" || source === "kis"),
      crypto: count(kinds.crypto, source => source === "crypto_file" || source === "wallet"),
    };
  }
  private remember(snapshot: HoldingsSnapshot, key: string = snapshot.source) {
    this.latest.set(key, { source: snapshot.source, importedAtMs: snapshot.importedAtMs, positions: structuredClone(snapshot.positions) });
    return snapshot;
  }
  async watchlistAdd(request: WatchlistAddRequest) {
    const symbol = identity(request.symbol), market = identity(request.market);
    const displayName = request.displayName?.trim() || null;
    const existing = this.watchlist.find((entry) => entry.symbol === symbol && entry.market === market);
    if (existing) { existing.displayName = displayName ?? existing.displayName; return structuredClone(existing); }
    const entry = { symbol, market, displayName, addedAtMs: Date.now() };
    this.watchlist.push(entry);
    return structuredClone(entry);
  }
  async watchlistRemove(request: { symbol: string; market: string }) {
    const symbol = identity(request.symbol), market = identity(request.market);
    this.watchlist = this.watchlist.filter((entry) => entry.symbol !== symbol || entry.market !== market);
  }
  async holdingsImportCsv({ path }: { path: string }) {
    if (!path.trim()) fail("invalid_request", "Choose a CSV file.");
    const preview: HoldingsCsvPreview = {
      previewId: newId(),
      mapping: { symbol: "symbol", market: "market", quantity: "quantity", costBasis: "avg cost", marketValue: "value", currency: "currency" },
      positions: [{ symbol: "AAPL", market: "NASDAQ", displayName: "Apple", quantity: "2.5", costBasis: "150.125", marketValue: "500", currency: "USD" }],
      errors: [], rowCount: 1,
    };
    this.previews.set(preview.previewId, structuredClone(preview));
    return preview;
  }
  /** The file name picks the fixture: "trades" refuses, "columns" asks for a mapping, anything else is a Kraken ledger. */
  async holdingsImportCryptoFile({ path }: { path: string }) {
    if (!path.trim()) fail("invalid_request", "Choose a CSV file.");
    const base = { previewId: newId(), format: null, rowCount: 0, asOfMs: null, rows: [], skipped: [], errors: [], mappingHeaders: null, tradesOnly: false } satisfies CryptoFilePreview;
    const preview: CryptoFilePreview = /trades/i.test(path) ? { ...base, rowCount: 38, tradesOnly: true }
      : /columns/i.test(path) ? { ...base, rowCount: 3, mappingHeaders: ["Token", "Holding", "Note"] }
        : { ...structuredClone(MOCK_KRAKEN_PREVIEW), previewId: base.previewId };
    this.cryptoPreviews.set(preview.previewId, structuredClone(preview));
    return preview;
  }
  async holdingsMapColumns({ previewId, asset, balance }: { previewId: string; asset: string; balance: string }) {
    const preview = this.cryptoPreviews.get(previewId);
    if (!preview || preview.tradesOnly) return fail("invalid_request", "CSV preview is missing or expired.");
    if (!asset || !balance || asset === balance) return fail("invalid_request", "The asset and balance columns must differ.");
    const mapped: CryptoFilePreview = { ...preview, format: "mapped", mappingHeaders: null, rows: [{ symbol: "BTC", quantity: "0.25", from: ["BTC"] }, { symbol: "ETH", quantity: "1.5", from: ["ETH"] }] };
    this.cryptoPreviews.set(previewId, structuredClone(mapped));
    return mapped;
  }
  async holdingsCommitCsv({ previewId }: { previewId: string }) {
    const cryptoPreview = this.cryptoPreviews.get(previewId);
    if (cryptoPreview) {
      if (cryptoPreview.tradesOnly || cryptoPreview.mappingHeaders || cryptoPreview.errors.length || !cryptoPreview.rows.length) return fail("invalid_request", "This file has no balances to import.");
      this.cryptoPreviews.delete(previewId);
      return this.remember({ snapshotId: newId(), source: "crypto_file", importedAtMs: Date.now(), currency: "", note: null, positions: cryptoPreview.rows.map(row => cryptoPosition(row.symbol, row.quantity)) });
    }
    const preview = this.previews.get(previewId);
    if (!preview) return fail("invalid_request", "CSV preview is missing or already committed.");
    this.previews.delete(previewId);
    return this.remember({ snapshotId: newId(), source: "csv", importedAtMs: Date.now(), currency: "USD", note: null, positions: preview.positions } satisfies HoldingsSnapshot);
  }
  async holdingsImportKis(): Promise<HoldingsSnapshot> {
    if (!this.hasKisAccountProfile()) return fail("needs_account_profile", "Add the optional account profile to the Korea Investment connection to import holdings.");
    return this.remember({
      snapshotId: newId(), source: "kis", importedAtMs: Date.now(), currency: "KRW", note: null,
      positions: [{ symbol: "005930", market: "KRX", displayName: "삼성전자", quantity: "10", costBasis: "65000.125", marketValue: "720000", currency: "KRW" }],
    });
  }
  async walletsList() { return structuredClone(this.wallets); }
  private read(wallet: WalletView) {
    const now = Date.now();
    wallet.lastReadAtMs = now;
    if (this.failingWallets.has(wallet.id)) { wallet.status = "failed"; return; }
    wallet.status = "ok";
    wallet.lastGoodReadAtMs = now;
    const coin = wallet.family === "bitcoin" ? "BTC" : wallet.family === "solana" ? "SOL" : "ETH";
    wallet.unlistedCount = wallet.family === "evm" ? 4 : 0;
    this.latest.set(`wallet:${wallet.id}`, { source: "wallet", importedAtMs: now, positions: [cryptoPosition(coin, "1")] });
  }
  async walletAdd({ address, name }: { address: string; name?: string | null }) {
    if (looksLikeSecret(address)) return fail("secret_refused", "This looks like a recovery phrase or private key. Loupist never needs it.");
    const family = detectNetwork(address);
    if (!family) return fail("unsupported_address", "Not a supported address.");
    if (this.wallets.length >= 10) return fail("wallet_limit", "You can add up to 10 wallets.");
    const value = address.trim();
    if (this.wallets.some(wallet => wallet.address.toLowerCase() === value.toLowerCase())) return fail("conflict", "This wallet is already added.");
    const wallet: WalletView = {
      id: `wallet-${newId()}`, family, masked: maskAddress(value), address: value, name: name?.trim() || null,
      addedAtMs: Date.now(), lastReadAtMs: null, lastGoodReadAtMs: null, status: "unread", unlistedCount: 0,
    };
    this.wallets.push(wallet);
    this.read(wallet);
    return structuredClone(this.wallets);
  }
  async walletRename({ walletId, name }: { walletId: string; name?: string | null }) {
    const wallet = this.wallets.find(item => item.id === walletId) ?? fail("invalid_request", "Wallet is not on this computer.");
    wallet.name = name?.trim() || null;
    return structuredClone(wallet);
  }
  async walletRemove({ walletId }: { walletId: string }) {
    this.wallets = this.wallets.filter(wallet => wallet.id !== walletId);
    this.latest.delete(`wallet:${walletId}`);
  }
  async walletRefresh(request: { walletId?: string } = {}) {
    for (const wallet of this.wallets) if (!request.walletId || wallet.id === request.walletId) this.read(wallet);
    return structuredClone(this.wallets);
  }
}

function newId() { return crypto.randomUUID(); }
