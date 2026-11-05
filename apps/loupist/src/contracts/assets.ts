/**
 * Asset pages (Rust `knowledge/assets`, `commands/assets.rs`). Rust creates
 * pages, records dated observations, owns the read and its moves and decides
 * what is new; React renders what it returns and submits typed intent.
 */
import type { EarningsCalc } from "./earnings";

export type AssetClass = "equity" | "crypto" | "fund" | "commodity" | "rates_fx" | "real_asset" | "prediction" | "private";
export const ASSET_CLASSES: readonly AssetClass[] = ["equity", "crypto", "fund", "commodity", "rates_fx", "real_asset", "prediction", "private"];
export type AssetClassSource = "user" | "market" | "lookup" | "world" | "default";
/** "Holding up" is stored as `holding`; it never reads as ownership. */
export type Verdict = "holding" | "weakening" | "challenged" | "broken";

/** Where a row came from: a retained dataset row, an original URL, a Source, a Map claim or a cited turn result. */
export type AssetEvidence = {
  tool: string;
  provider?: string;
  title: string;
  url?: string;
  dataset?: { dataset_id: string; revision: number; digest: string };
  pointer?: string;
  recordId?: string;
  worldId?: string;
  citationUrl?: string;
  /** The evidence's own date (filing, publication or observation). */
  date?: string;
  retrievedAt?: string;
};

export type LineCrossing = {
  journalId: string;
  claim: string;
  metric: string;
  direction: "above" | "below";
  threshold: number;
  unit?: string;
  observed: number;
  crossed: boolean;
  setAtMs: number;
};

export type AssetMove = {
  moveId: string;
  assetId: string;
  from?: Verdict;
  to: Verdict;
  factDate: string;
  reason: string;
  evidence: AssetEvidence[];
  reasonId?: string;
  line?: LineCrossing;
  actor: "loupist" | "user";
  atMs: number;
  undone: boolean;
  undoesMoveId?: string;
  originChatId?: string;
  originMessageId?: string;
  seenAtMs?: number;
};

export type AssetReason = {
  journalId: string;
  text: string;
  createdAtMs: number;
  updatedAtMs: number;
  originChatId?: string;
  lastMove?: AssetMove;
};

export type AssetLine = {
  journalId: string;
  claim: string;
  metric: string;
  direction: "above" | "below";
  threshold: number;
  unit?: string;
  byDate?: string;
  createdAtMs: number;
  updatedAtMs: number;
  originChatId?: string;
  status: "not_checked" | "not_crossed" | "crossed";
  last?: { observed: number; factDate: string; evidence: AssetEvidence[] };
};

export type AssetRead = {
  verdict: Verdict;
  /** YYYY-MM-DD: the fact date of the move that set this step, or the first reason's day. */
  since: string;
  /** True while the read still stands where it started. */
  started: boolean;
  latestMove?: AssetMove;
  /** Set when the latest move is the Loupist's and can be undone. */
  undoMoveId?: string;
};

export type AssetGlanceKey = "lastFiling" | "lastResults" | "nextEarnings" | "dividend" | "price" | "drawdown" | "expenseRatio" | "level";
/**
 * `dividend` for a Korean equity (OpenDART `dart_dividends`): `{amount, currency, fiscalYear, recordDate?, recordInNotice?}`;
 * `recordInNotice` is true when the record date (배당기준일) could not be read and is only in the decision notice.
 */
export type AssetGlanceRow = { key: AssetGlanceKey; facts: Record<string, unknown>; date?: string; evidence: AssetEvidence[] };

export type AssetCheckFailure = { kind: string; connection: string; reason: "failed" | "needs_connection" };
export type AssetChecks = {
  /** Check kinds the last check read (the plan before the first check), as `assets:checks.kind.*` keys. */
  kinds: string[];
  /** Connections those reads came from; failed and switched-off ones are only in `failures`. */
  connections: string[];
  /** What this kind of asset would need that is not read yet. */
  notYet: string[];
  failures: AssetCheckFailure[];
  sourcesOnly: boolean;
};

/**
 * One row of the Korean investor-flows section (finance-korea-retail-disagreement.md §2.1).
 * Rust sums the daily rows through `calculate` and keeps the formula; flows are
 * not prices, so the renderer never gives them price colors.
 */
export type AssetFlowRow = {
  key: "foreign" | "institutions" | "individuals" | "shortSelling" | "lending";
  /** Net buying in won for foreign, institutions and individuals; a percent for short selling (share of volume) and lending (balance change). */
  value: number;
  unit: "KRW" | "%";
  /** The last day this row covers when it differs from the section's (lending balances settle two business days later). */
  to?: string;
  calc: EarningsCalc;
  evidence: AssetEvidence[];
};

/** Korea Investment investor flows over the last trading days, refreshed once a day after 18:30 in Seoul. */
export type AssetFlows = { days: number; to: string; rows: AssetFlowRow[] };

export type AssetPage = {
  assetId: string;
  symbol: string;
  market: string;
  name: string;
  class: AssetClass;
  classSource: AssetClassSource;
  entityId?: string;
  /** `watching`, `holding` or `none`; never a quantity. */
  watch: "watching" | "holding" | "none";
  reasons: AssetReason[];
  lines: AssetLine[];
  read?: AssetRead;
  /** Newest first. */
  history: AssetMove[];
  glance: AssetGlanceRow[];
  checks: AssetChecks;
  map?: { suppliers: number; customers: number };
  /** KRX equities with Korea Investment on: the latest investor-flows summary. */
  flows?: AssetFlows;
  refreshedAtMs?: number;
  createdAtMs: number;
  revision: number;
};

/**
 * `insiders`, `holders` and `flows` come from the watchers' routes (finance-watchers.md); their facts match the watch event's.
 * `capital` is one Korean capital decision (주요사항보고) read from OpenDART; its facts are `AssetCapitalFacts`.
 */
export type AssetChangeKind = "filing" | "results" | "earnings" | "dividend" | "price" | "holdings" | "fees" | "positioning" | "rates" | "map" | "source" | "thesis" | "insiders" | "holders" | "flows" | "capital";

/**
 * The share count a capital decision may add: as the filing states it, or as
 * Rust calculated it (new shares ÷ shares before). For bonds, `at` says whether
 * it is the filed share at the starting price or the most shares at the reset
 * floor (face amount ÷ floor over the shares outstanding the filing implies).
 */
export type AssetDilution = { value: number; display: string; basis: "filing" | "calculated"; at?: "price" | "floor"; calc?: EarningsCalc };

/**
 * A `capital` row's facts. Amounts and prices are won; dates are `YYYY-MM-DD`.
 * OpenDART's rights-offering decision carries no issue price or payment date,
 * so for a rights offering they are null and the row says they are in the notice.
 */
export type AssetCapitalFacts = {
  decision: "rights_offering" | "convertible_bonds" | "bonds_with_warrants" | "buyback";
  filingId: string;
  currency: "KRW";
  /** Rights offering: new common shares and how they are offered. */
  shares?: number;
  method?: "shareholders" | "third_party" | "public" | "other";
  price?: number | null;
  paymentDate?: string | null;
  /** Bonds: face amount, conversion or exercise price and the reset floor (최저 조정가액); buyback: the planned amount. */
  amount?: number;
  conversionPrice?: number;
  floor?: number | null;
  /** Buyback: the last day of the buying period. */
  by?: string;
  dilution?: AssetDilution;
};
export type AssetSectionCount = { section: "risk" | "mdna" | "business"; added: number; removed: number; changed: number };

export type AssetChangeRow = {
  id: string;
  kind: AssetChangeKind;
  factDate: string;
  recordedAtMs: number;
  facts: Record<string, unknown>;
  evidence: AssetEvidence[];
  /** Compared sections of an annual or quarterly filing; empty when not compared yet. */
  sections?: AssetSectionCount[];
  /** Recorded after the chosen date. */
  new: boolean;
  /** Worth a Thread or briefing line. */
  material: boolean;
};

export type AssetDelta = {
  assetId: string;
  symbol: string;
  market: string;
  name: string;
  class: AssetClass;
  /** The World entity the page matched (Map inspector). */
  entityId?: string;
  verdict?: Verdict;
  verdictSince?: string;
  /** The move behind the current read; absent while it stands where it started. */
  latestMove?: AssetMove;
  rows: AssetChangeRow[];
  moved: boolean;
  crossed: boolean;
};

export type AssetPageGetRequest = { loupistId: string; assetId?: string; symbol?: string; market?: string; name?: string; sinceMs?: number };
export type AssetDeltaRequest = { loupistId: string; assetId?: string; sinceMs: number };
export type AssetRefreshRequest = { loupistId: string; assetId: string; sinceMs?: number };
export type AssetClassSetRequest = { loupistId: string; assetId: string; class: AssetClass; sinceMs?: number };
/** `moveId` is the move the user saw as the latest; a newer move makes Undo a conflict (a refresh does not). */
export type AssetVerdictUndoRequest = { loupistId: string; assetId: string; moveId: string; sinceMs?: number };
export type AssetPageSeenRequest = { loupistId: string; assetId: string };
export type UpcomingEventsRequest = { loupistId: string };
/** The page an upcoming event belongs to, named in the reader's language by `localAssetName`. */
export type UpcomingAsset = { assetId: string; symbol: string; market: string; name: string; entityId?: string };
/**
 * One row of the morning message's event strip, from Rust (soonest first, at
 * most five, today through 14 days ahead). Results carry where their date comes
 * from: a legal deadline is a bound, never an expected date.
 */
export type UpcomingEvent =
  | { kind: "results"; date: string; asset: UpcomingAsset; dateSource: "calendar" | "ir_notice" | "legal_deadline"; eventId: string; period: string }
  | { kind: "exDividend" | "recordDate"; date: string; asset: UpcomingAsset }
  | { kind: "check"; date: string; journalId: string; claim: string; asset?: UpcomingAsset };

export interface AssetBridge {
  assetPageGet(request: AssetPageGetRequest): Promise<AssetPage>;
  assetDelta(request: AssetDeltaRequest): Promise<AssetDelta[]>;
  assetRefresh(request: AssetRefreshRequest): Promise<AssetPage>;
  assetClassSet(request: AssetClassSetRequest): Promise<AssetPage>;
  assetVerdictUndo(request: AssetVerdictUndoRequest): Promise<AssetPage>;
  assetPageSeen(request: AssetPageSeenRequest): Promise<void>;
  /** The morning message's event strip; read-only, no model call. */
  upcomingEvents(request: UpcomingEventsRequest): Promise<UpcomingEvent[]>;
}

/** Where the asset page opens and which section it scrolls to ("results": the results card, or the event's row in Earlier results). */
export type AssetPageTarget = { loupistId: string; assetId?: string; symbol?: string; market?: string; name?: string; section?: "history" | "results"; eventId?: string };
