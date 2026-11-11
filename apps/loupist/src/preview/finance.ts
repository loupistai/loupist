/**
 * Fictional results snapshots and watcher alerts for the design preview
 * (finance-earnings-snapshot.md §8, finance-watchers.md §8). Every value has a
 * dated source, like Rust's; names are fictional.
 */
import type { AssetEvidence, AssetPage } from "../contracts/assets";
import type { ChatMessage } from "../contracts/chat";
import type { EarningsCalc, EarningsMeasure, EarningsSnapshot, EarningsValue } from "../contracts/earnings";
import type { BriefingEarnings, RunSummaryItem, ScheduledRun, ScheduledTask } from "../contracts/scheduled";
import type { WatchAsset, WatchEvent, WatchSettings } from "../contracts/watch";
import { mockAssetId, mockChecks } from "../bridge/mock/assets";
import { mockWatchKinds } from "../bridge/mock/watch";
import { HANUL, PREVIEW_LOUPIST, type AssetFixture } from "./assets";
import { previewKo } from "./fixtures";

const DAY = 86_400_000;
/** YYYY-MM-DD `daysAgo` days before today (negative: ahead). */
const day = (daysAgo: number) => new Date(Date.now() - daysAgo * DAY).toISOString().slice(0, 10);
/** `hour`:`minute` on the day `daysAgo` days before today. */
const at = (daysAgo: number, hour: number, minute = 0) => { const time = new Date(Date.now() - daysAgo * DAY); time.setHours(hour, minute, 0, 0); return time.getTime(); };
/** 6:00 PM on the day `daysAgo` days before today. */
const evening = (daysAgo: number) => at(daysAgo, 18);
/** A Korean legal deadline Rust computes: 45 days after a quarter end, 90 after the fiscal year. */
const koreaDeadline = (periodEnd: string) => new Date(Date.parse(`${periodEnd}T00:00:00Z`) + (periodEnd.endsWith("-12-31") ? 90 : 45) * DAY).toISOString().slice(0, 10);

export const MORROW = { symbol: "MORR", market: "NASDAQ", name: "Morrow Components" } as const;
export const SOLSUP = { symbol: "990221", market: "KRX", get name() { return previewKo() ? "솔숲식품" : "Solsup Foods"; } };
/** Hanul Precision by its KRX name in Korean previews. */
const hanulName = () => (previewKo() ? "한울정밀" : "Hanul Precision");
export const MORROW_EVENT = "evt-morr-q3-2026";
export const SOLSUP_EVENT = "evt-solsup-q3-2026";
const morrowId = () => mockAssetId(MORROW.symbol, MORROW.market);
const solsupId = () => mockAssetId(SOLSUP.symbol, SOLSUP.market);
const hanulId = () => mockAssetId(HANUL.symbol, HANUL.market);

const SEC = (accession: string, file: string, title: string, date: string): AssetEvidence => ({ tool: "equity_fundamental_filings", provider: "sec", title, url: `https://www.sec.gov/Archives/edgar/data/990001/${accession}/${file}`, date });
const DART = (receipt: string, title: string, date: string): AssetEvidence => ({ tool: "dart_filing_search", provider: "opendart", title, url: `https://dart.fss.or.kr/dsaf001/main.do?rcpNo=${receipt}`, date });
const data = (tool: string, provider: string, title: string, date: string, id: string): AssetEvidence => ({ tool, provider, title, dataset: { dataset_id: id, revision: 1, digest: "e".repeat(64) }, pointer: "/structuredContent/results/0", date });

const change = (title: string, from: string, to: string, display: string, value: number, unit = "%"): EarningsCalc => ({
  title, display, value, unit,
  formulas: [unit === "pp" ? `${to} − ${from} = ${display}` : `(${to} − ${from}) ÷ ${from} = ${display}`],
  inputs: [{ name: "from", display: from }, { name: "to", display: to }],
});

const value = (rowId: string, kind: EarningsValue["kind"], amount: Partial<EarningsValue>, evidence: AssetEvidence, extra: Partial<EarningsValue> = {}): EarningsValue =>
  ({ rowId, kind, unit: "currency", currency: "USD", factDate: evidence.date ?? day(6), evidence: [evidence], ...amount, ...extra });

/** Morrow Components, Q3 2026: guidance range, consensus point, margin in points, new Q4 guidance. */
function morrowMeasures(withReported: boolean): EarningsMeasure[] {
  const release = SEC("000099000126000031", "morr-ex991.htm", "Morrow Components Q3 2026 results release (8-K)", day(6));
  const guidance = SEC("000099000126000022", "morr-ex991-q2.htm", "Morrow Components Q2 2026 results release (8-K)", day(91));
  const consensus = data("equity_estimates_forward_sales", "fmp", "Analyst consensus · FMP", day(7), "ds-morr-consensus");
  return [
    {
      key: "revenue", period: "Q3 2026", basis: "GAAP",
      expected: [
        value("row-rev-guide", "guidance", { low: 1.2e9, high: 1.25e9 }, guidance),
        value("row-rev-cons", "consensus", { point: 1.23e9 }, consensus, { provider: "FMP" }),
      ],
      reported: withReported ? [value("row-rev-rep", "reported", { point: 1.27e9 }, release, { stage: "preliminary" })] : [],
      differences: withReported ? [
        { kind: "range", position: "above", calc: change("Percent change", "1.25bn", "1.27bn", "+1.6%", 1.6) },
        { kind: "change", calc: change("Percent change", "1.23bn", "1.27bn", "+3.3%", 3.3) },
      ] : [],
    },
    {
      key: "operatingMargin", period: "Q3 2026",
      expected: [value("row-om-guide", "guidance", { point: 16, unit: "percent", currency: undefined }, guidance)],
      reported: withReported ? [value("row-om-rep", "reported", { point: 14.8, unit: "percent", currency: undefined }, release, { stage: "preliminary" })] : [],
      differences: withReported ? [{ kind: "change", calc: change("Difference in points", "16.0%", "14.8%", "−1.2 pp", -1.2, "pp") }] : [],
    },
    ...(withReported ? [{
      key: "revenue" as const, period: "Q4 2026", guidance: true,
      expected: [], reported: [value("row-q4-guide", "guidance", { low: 1.3e9, high: 1.35e9 }, release)],
      differences: [{ kind: "guidance" as const, change: "new" as const }],
    }] : []),
  ];
}

const morrowBase = (): Omit<EarningsSnapshot, "status" | "measures" | "chain"> => ({
  eventId: MORROW_EVENT, assetId: morrowId(), name: MORROW.name, symbol: MORROW.symbol, market: MORROW.market,
  period: "Q3 2026", reportDate: day(6), dateSource: "calendar",
  reactionWaiting: false, revisions: [], unchanged: false, consensusLate: false, consensusNeedsKey: false, noGuidance: false, preparing: false,
});

/** The price-implied range at the close the day before the report (a weekday). */
const implied = (closeDaysAgo = 7): NonNullable<EarningsSnapshot["implied"]> => ({
  lowPct: 6.1, highPct: 9.8, years: 5, price: 41.2, currency: "USD", closeDate: day(closeDaysAgo),
  calc: { title: "Reverse DCF grid", display: "6.1–9.8%", value: 9.8, unit: "%", formulas: ["Solve g where value = price × shares + net debt = 41.20 × 182.4M + 310M = 7.82bn"], inputs: [{ name: "cash_flow", display: "402M" }, { name: "price", display: "41.20" }, { name: "shares", display: "182.4M" }, { name: "net_debt", display: "310M" }] },
  evidence: [data("equity_price_historical", "nasdaq", "Daily closes · Nasdaq", day(closeDaysAgo), "ds-morr-close"), data("equity_fundamental_cash", "sec", "Cash flow statement · SEC", day(40), "ds-morr-cash")],
});

export type MorrowState = "upcoming" | "preparing" | "before" | "after" | "revised" | "late" | "none";

/** Morrow's captures: 6:00 PM the evening before its Oct 22 report, and the morning after the release. */
const MORROW_BEFORE = () => evening(7);
const MORROW_AFTER = () => at(5, 6, 30);

/** One results card state for Morrow Components. */
export function morrowSnapshot(state: MorrowState): EarningsSnapshot {
  const base = morrowBase();
  if (state === "upcoming" || state === "preparing") return { ...base, status: "upcoming", reportDate: day(-5), captureAtMs: evening(-4), measures: [], chain: [], preparing: state === "preparing" };
  if (state === "before") return { ...base, status: "before_captured", reportDate: day(-1), before: { capturedAtMs: evening(0) - DAY }, measures: morrowMeasures(false), implied: implied(1), chain: [] };
  if (state === "late") return { ...base, status: "before_captured", reportDate: day(0), before: { capturedAtMs: Date.now() - 3_600_000, lateAsOf: day(1) }, consensusLate: true,
    measures: morrowMeasures(false).map(measure => ({ ...measure, expected: measure.expected.filter(item => item.kind !== "consensus") })), chain: [] };
  if (state === "none") return { ...base, status: "before_captured", reportDate: day(-2), before: { capturedAtMs: evening(-1) - DAY }, noGuidance: true, consensusNeedsKey: true, measures: [], chain: [] };
  const release = SEC("000099000126000031", "morr-ex991.htm", "Morrow Components Q3 2026 results release (8-K)", day(6));
  const tenQ = SEC("000099000126000034", "morr-20260927.htm", "Morrow Components quarterly report (10-Q)", day(1));
  return {
    ...base, status: "final", before: { capturedAtMs: MORROW_BEFORE() }, after: { capturedAtMs: MORROW_AFTER(), date: day(6) },
    measures: morrowMeasures(true), implied: implied(),
    reaction: { from: 41.2, to: 39.92, currency: "USD", fromDate: day(7), toDate: day(5), calc: change("Percent change", "41.20", "39.92", "−3.1%", -3.1), evidence: [data("equity_price_historical", "nasdaq", "Daily closes · Nasdaq", day(5), "ds-morr-close")] },
    // The run writes in the reader's language; the quoted span stays as the transcript says it.
    tone: { text: previewKo() ? "마진에는 신중했어요 — “pricing pressure persisted”" : "cautious on margins — “pricing pressure persisted”", spans: ["pricing pressure persisted"], evidence: [{ tool: "equity_fundamental_transcript", provider: "fmp", title: "Q3 2026 earnings call transcript", date: day(6), dataset: { dataset_id: "ds-morr-call", revision: 1, digest: "f".repeat(64) } }] },
    chain: [
      { stage: "release", filed: true, date: day(6), form: "8-K", evidence: [release] },
      { stage: "quarterly", filed: true, date: day(1), form: "10-Q", evidence: [tenQ] },
    ],
    revisions: state === "revised" ? [{ key: "revenue", from: { point: 1.27e9, unit: "currency", currency: "USD" }, to: { point: 1.262e9, unit: "currency", currency: "USD" }, calc: change("Percent change", "1.27bn", "1.262bn", "−0.6%", -0.6), evidence: [tenQ] }] : [],
    unchanged: state === "after",
  };
}

/** Solsup Foods, Q3 2026: 잠정실적 filed, the quarterly report due by its legal deadline. */
export function solsupSnapshot(): EarningsSnapshot {
  const preliminary = DART("20261023000231", "연결재무제표기준영업(잠정)실적(공정공시)", day(5));
  const guidance = DART("20260811000144", "기업설명회(IR)개최", day(78));
  const won = (rowId: string, kind: EarningsValue["kind"], point: number, evidence: AssetEvidence, extra: Partial<EarningsValue> = {}) => value(rowId, kind, { point, currency: "KRW" }, evidence, extra);
  return {
    eventId: SOLSUP_EVENT, assetId: solsupId(), name: SOLSUP.name, symbol: SOLSUP.symbol, market: SOLSUP.market,
    period: "Q3 2026", reportDate: day(5), dateSource: "ir_notice", status: "reported",
    before: { capturedAtMs: evening(6) }, after: { capturedAtMs: at(5, 16, 30), date: day(5) },
    measures: [
      { key: "operatingIncome", period: "Q3 2026", basis: "consolidated", expected: [won("row-oi-guide", "guidance", 115e9, guidance)], reported: [won("row-oi-rep", "reported", 121.2e9, preliminary, { stage: "preliminary" })], differences: [{ kind: "change", calc: change("Percent change", "1,150억", "1,212억", "+5.4%", 5.4) }] },
      { key: "revenue", period: "Q3 2026", basis: "consolidated", expected: [], reported: [won("row-rev-rep", "reported", 1.248e12, preliminary, { stage: "preliminary" })], differences: [] },
    ],
    reaction: { from: 51200, to: 50120, currency: "KRW", fromDate: day(6), toDate: day(2), calc: change("Percent change", "51,200", "50,120", "−2.1%", -2.1), evidence: [data("krx_daily_quote", "krx", "KRX", day(2), "ds-solsup-close")] },
    reactionWaiting: false,
    chain: [
      { stage: "preliminary", filed: true, date: day(5), form: "공정공시", evidence: [preliminary] },
      { stage: "quarterly", filed: false, dueBy: koreaDeadline("2026-09-30"), evidence: [] },
    ],
    revisions: [], unchanged: false, consensusLate: false, consensusNeedsKey: false, noGuidance: false, preparing: false,
  };
}

/** Solsup Foods' asset page: equity on KRX, followed, with its next results due by the legal deadline. */
export function solsupFixture(): AssetFixture {
  return { page: {
    assetId: solsupId(), symbol: SOLSUP.symbol, market: SOLSUP.market, name: SOLSUP.name, class: "equity", classSource: "market", watch: "watching",
    reasons: [], lines: [], history: [],
    glance: [
      { key: "lastFiling", facts: { title: "연결재무제표기준영업(잠정)실적(공정공시)" }, date: day(5), evidence: [DART("20261023000231", "OpenDART", day(5))] },
      { key: "lastResults", facts: { period: "Q3 2026" }, date: day(5), evidence: [DART("20261023000231", "OpenDART", day(5))] },
    ],
    checks: mockChecks("equity", "KRX"), createdAtMs: Date.now() - 40 * DAY, refreshedAtMs: Date.now() - 3_600_000, revision: 2,
  } satisfies AssetPage, delta: { moved: false, crossed: false, rows: [] } };
}

/** The results message Rust posts into the Thread after the after-capture commits. */
export function previewEarningsMessage(outcome: "reported" | "failed" = "reported"): ChatMessage {
  const snapshot = morrowSnapshot("after");
  return {
    id: `preview-earnings-${outcome}`, role: "assistant", status: "complete", host_composed: true, created_at: new Date(Date.now() - 2 * 3_600_000).toISOString(),
    content: outcome === "reported" ? "Morrow Components reported Q3 2026 results." : "Couldn't capture expectations for Morrow Components before results.",
    earnings: { asset_id: snapshot.assetId, event_id: snapshot.eventId, name: MORROW.name, symbol: MORROW.symbol, market: MORROW.market, period: "Q3 2026", outcome, side: outcome === "failed" ? "before" : undefined, measures: outcome === "reported" ? snapshot.measures.slice(0, 2) : [] },
  };
}

/** A briefing's results rows: one reported event and one ahead with expectations captured. */
export function previewBriefingEarnings(): BriefingEarnings[] {
  const after = morrowSnapshot("after");
  return [
    { assetId: after.assetId, eventId: after.eventId, name: MORROW.name, symbol: MORROW.symbol, market: MORROW.market, period: "Q3 2026", reportDate: after.reportDate, status: "final", captured: true, measures: after.measures.slice(0, 2) },
    { assetId: solsupId(), eventId: SOLSUP_EVENT, name: SOLSUP.name, symbol: SOLSUP.symbol, market: SOLSUP.market, period: "Q3 2026", reportDate: day(-6), status: "before_captured", captured: true, measures: [] },
  ];
}

/** The automatic results task Rust creates, with two past captures. */
export function previewEarningsTask(): { task: ScheduledTask; runs: ScheduledRun[] } {
  const now = Date.now();
  const task: ScheduledTask = {
    kind: "earnings", id: "preview-earnings", loupist_id: PREVIEW_LOUPIST, revision: 3, title: "Results before and after", prompt: "Host capture",
    schedule: { kind: "interval", every_minutes: 60 }, target: { kind: "existing_chat", thread_id: `home-${PREVIEW_LOUPIST}` },
    model_profile_id: "", thinking_level: "", run_options: {}, use_memory: true, update_memory: true, status: "active",
    next_run_at_ms: evening(-5), last_run_at_ms: MORROW_AFTER(), created_at_ms: now - 40 * DAY, updated_at_ms: MORROW_AFTER(),
    last_error: null,
    session_thread_id: null,
    awaiting_confirmation: false,
  };
  const run = (id: string, atMs: number, item: RunSummaryItem): ScheduledRun => ({
    briefing_context: null, kind: "earnings", summary: "", summary_items: [item], read_at_ms: atMs + 60_000, message_id: null, skipped_tools: [], id: `${id}-row`,
    task_id: task.id, loupist_id: PREVIEW_LOUPIST, task_revision: 3, task_title: task.title, thread_id: null, run_id: id,
    scheduled_for_ms: atMs, started_at_ms: atMs, finished_at_ms: atMs + 4_000, status: "succeeded",
    error: null,
  });
  return { task, runs: [
    run("preview-earnings-after", MORROW_AFTER(), { kind: "earnings_reported", name: MORROW.name, period: "Q3 2026", count: 3 }),
    run("preview-earnings-before", MORROW_BEFORE(), { kind: "earnings_expected", name: MORROW.name, period: "Q3 2026", count: 3 }),
  ] };
}

const asset = (symbol: string, market: string, name: string, enabled: WatchAsset["kinds"][number]["kind"][], extra: Partial<WatchAsset> = {}): WatchAsset =>
  ({ assetId: mockAssetId(symbol, market), symbol, market, name, kinds: mockWatchKinds(market, enabled, symbol), revision: 2, ...extra });

/** Hanul, Morrow and Solsup watched; Bluehaven followed but not watched here. Korean flows need Korea Investment. */
export function previewWatchSettings(options: { paused?: boolean; consentOff?: boolean } = {}): Partial<WatchSettings> {
  const needsKis = (item: WatchAsset): WatchAsset => ({ ...item, kinds: item.kinds.map(kind => kind.kind === "flows" ? { ...kind, enabled: false, needsConnection: { id: "koreainvestment.market-data", name: "Korea Investment" } } : kind) });
  return {
    paused: options.paused ?? false, consentOff: options.consentOff ?? false, checking: false, lastCheckedMs: Date.now() - 18 * 60_000,
    watched: [
      needsKis(asset(HANUL.symbol, HANUL.market, hanulName(), ["filings", "results", "dividends"])),
      asset(MORROW.symbol, MORROW.market, MORROW.name, ["filings", "results", "insiders", "dividends"]),
      needsKis(asset(SOLSUP.symbol, SOLSUP.market, SOLSUP.name, ["filings", "results", "holders", "dividends"])),
    ],
    notWatched: [asset("BLUEH", "NYSE", "Bluehaven Memory", [])],
  };
}

const event = (eventId: string, assetId: string, name: string, symbol: string, market: string, kind: WatchEvent["kind"], factDate: string, facts: Record<string, unknown>, evidence: AssetEvidence[], extra: Partial<WatchEvent> = {}): WatchEvent =>
  ({ eventId, assetId, name, symbol, market, kind, factDate, detectedAtMs: Date.now() - 3_600_000, facts, evidence, ...extra });

/** Six alerts from the last checks, newest first. */
export function previewWatchEvents(extra: Partial<WatchEvent> = {}): WatchEvent[] {
  return [
    event("watch-morr-10q", morrowId(), MORROW.name, MORROW.symbol, MORROW.market, "filings", day(1), { form: "10-Q", title: "Quarterly report" }, [SEC("000099000126000034", "morr-20260927.htm", "SEC EDGAR", day(1))], extra),
    event("watch-morr-insiders", morrowId(), MORROW.name, MORROW.symbol, MORROW.market, "insiders", day(2), { count: 3, from: day(13), to: day(2) }, [data("equity_ownership_insider_trading", "sec", "Insider trading · SEC", day(2), "ds-morr-form4")], extra),
    event("watch-solsup-results", solsupId(), SOLSUP.name, SOLSUP.symbol, SOLSUP.market, "results", day(1), { date: day(-6), period: "Q3 2026", eventId: SOLSUP_EVENT }, [DART("20261027000412", "기업설명회(IR)개최", day(1))], extra),
    event("watch-hanul-holder", hanulId(), hanulName(), HANUL.symbol, HANUL.market, "holders", day(2), { holder: "Saebit Asset Management", ratio: "5.2%", before: "4.6%" }, [DART("20261030000118", "주식등의대량보유상황보고서", day(2))], extra),
    event("watch-hanul-dividend", hanulId(), hanulName(), HANUL.symbol, HANUL.market, "dividends", day(6), { amount: 300, currency: "KRW", recordDate: day(-5), declared: true }, [DART("20261022000077", "현금ㆍ현물배당결정", day(6))], extra),
    event("watch-solsup-flows", solsupId(), SOLSUP.name, SOLSUP.symbol, SOLSUP.market, "flows", day(1), { direction: "in", to: day(1) }, [data("kis_read", "kis", "Investor trading by stock · Korea Investment", day(1), "ds-solsup-flows")], extra),
  ];
}

/** The Thread message for one check batch: five events, then "+1 more". */
export function previewWatchMessage(): ChatMessage {
  const events = previewWatchEvents({ deliveredIn: "message" });
  return {
    id: "preview-watch", role: "assistant", status: "complete", host_composed: true, created_at: new Date(Date.now() - 50 * 60_000).toISOString(),
    content: "Morrow Components filed its Q3 report.",
    watch: { event_ids: events.slice(0, 5).map(item => item.eventId), events: events.slice(0, 5), more: 1 },
  };
}

/** Installs the watch settings and alerts for the preview Loupist. */
export function installWatch(bridge: { watch: { install: (loupistId: string, settings: Partial<WatchSettings>, events?: WatchEvent[]) => void } }, settings = previewWatchSettings(), events = previewWatchEvents()) {
  bridge.watch.install(PREVIEW_LOUPIST, settings, events);
}

// W16.12: class watchers (finance-crypto-and-templates.md §3.3).

/** Corvid Token, the preview's coin, by the preview asset page's id. */
const CORVID = { symbol: "CRVD", market: "CRYPTO", name: "Corvid Token" } as const;
const DEFILLAMA = { id: "defillama.tvl", name: "DefiLlama" };
const EIA = { id: "eia.energy", name: "U.S. Energy Information Administration" };

/** A coin in a Crypto Loupist: moves and the fall from the high on; value locked waits for DefiLlama. */
export function previewCryptoWatchSettings(): Partial<WatchSettings> {
  return {
    paused: false, consentOff: false, checking: false, lastCheckedMs: Date.now() - 25 * 60_000,
    watched: [{
      assetId: mockAssetId(CORVID.symbol, CORVID.market), symbol: CORVID.symbol, market: CORVID.market, name: CORVID.name, revision: 3,
      kinds: [
        { kind: "cryptoMove", enabled: true },
        { kind: "drawdown", enabled: true },
        { kind: "tvl", enabled: false, needsConnection: DEFILLAMA },
      ],
    }],
    notWatched: [],
  };
}

/** Gold, WTI crude and copper in a Commodities Loupist; the petroleum report needs EIA. */
export function previewCommodityWatchSettings(): Partial<WatchSettings> {
  const futures = (symbol: string, market: string, name: string, extra: Partial<WatchAsset> = {}): WatchAsset =>
    ({ ...asset(symbol, market, name, ["futuresMove", "positioning", "oilInventory"]), ...extra });
  const gold = futures("GC", "COMEX", previewKo() ? "금" : "Gold");
  const crude = futures("CL", "NYMEX", previewKo() ? "WTI 원유" : "WTI crude");
  return {
    paused: false, consentOff: false, checking: false, lastCheckedMs: Date.now() - 40 * 60_000,
    watched: [
      gold,
      { ...crude, kinds: crude.kinds.map(kind => kind.kind === "oilInventory" ? { ...kind, enabled: false, needsConnection: EIA } : kind) },
      futures("HG", "COMEX", previewKo() ? "구리" : "Copper"),
    ],
    notWatched: [],
  };
}

const CFTC_REPORT = (date: string): AssetEvidence => ({ tool: "cftc_cot", provider: "cftc", title: "CFTC Commitments of Traders", url: "https://www.cftc.gov/MarketReports/CommitmentsofTraders/index.htm", dataset: { dataset_id: "ds-cot", revision: 1, digest: "c".repeat(64) }, pointer: "/structuredContent/results/2", date });

/** What the class watchers found: a positioning flip and two weekly moves (commodities), a coin's fall from its high. */
export function previewCommodityWatchEvents(): WatchEvent[] {
  const name = (english: string, korean: string) => (previewKo() ? korean : english);
  return [
    event("watch-gc-positioning", mockAssetId("GC", "COMEX"), name("Gold", "금"), "GC", "COMEX", "positioning", day(5), { direction: "short", net: -11_400, before: 6_200, to: day(5), market: "GOLD - COMMODITY EXCHANGE INC." }, [CFTC_REPORT(day(5))]),
    event("watch-hg-move", mockAssetId("HG", "COMEX"), name("Copper", "구리"), "HG", "COMEX", "futuresMove", day(1), { window: "week", change: 0.062, from: day(8), to: day(1), close: 5.21 }, [data("derivatives_futures_historical", "yfinance", "derivatives futures historical · yfinance", day(1), "ds-hg-price")]),
    event("watch-cl-move", mockAssetId("CL", "NYMEX"), name("WTI crude", "WTI 원유"), "CL", "NYMEX", "futuresMove", day(2), { window: "week", change: -0.054, from: day(9), to: day(2), close: 66.4 }, [data("derivatives_futures_historical", "yfinance", "derivatives futures historical · yfinance", day(2), "ds-cl-price")]),
  ];
}

export function previewCryptoWatchEvents(): WatchEvent[] {
  return [
    event("watch-crvd-drawdown", mockAssetId(CORVID.symbol, CORVID.market), CORVID.name, CORVID.symbol, CORVID.market, "drawdown", day(3), { level: -0.2, drawdown: -0.214, high: 1.75, highDate: day(40), close: 1.375, to: day(3) }, [data("crypto_price_historical", "coinbase", "crypto price historical · coinbase", day(3), "dataset-crypto")]),
  ];
}
