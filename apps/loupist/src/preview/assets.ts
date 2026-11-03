/**
 * Fictional asset pages for the design preview (finance-asset-page.md §8).
 * Every row has a date and a source, like Rust's.
 */
import type { AssetChangeRow, AssetDelta, AssetEvidence, AssetMove, AssetPage } from "../contracts/assets";
import type { ChatMessage } from "../contracts/chat";
import { mockAssetId, mockChecks } from "../bridge/mock/assets";
import type { PreviewBridge } from "./fixtures";

export const PREVIEW_LOUPIST = "loupist-quality";
const DAY = 86_400_000;
const day = (daysAgo: number) => new Date(Date.now() - daysAgo * DAY).toISOString().slice(0, 10);

const DART = (receipt: string): AssetEvidence => ({ tool: "dart_filing_search", provider: "opendart", title: "OpenDART", url: `https://dart.fss.or.kr/dsaf001/main.do?rcpNo=${receipt}`, dataset: { dataset_id: "dataset-preview", revision: 1, digest: "d".repeat(64) }, pointer: "/data/filings/0" });
const KRX = (date: string): AssetEvidence => ({ tool: "krx_daily_quote", provider: "krx", title: "KRX", dataset: { dataset_id: "dataset-krx", revision: 1, digest: "e".repeat(64) }, pointer: "/data/bars/0", date });
const MARKET = (date: string): AssetEvidence => ({ tool: "crypto_price_historical", provider: "coinbase", title: "crypto price historical · coinbase", dataset: { dataset_id: "dataset-crypto", revision: 1, digest: "f".repeat(64) }, pointer: "/structuredContent/results/0", date });

export type AssetFixture = { page: AssetPage; delta?: Omit<AssetDelta, "assetId" | "symbol" | "market" | "name" | "class"> };

/**
 * Hanul Precision as the rest of the preview follows it (`HANUL:KRX`), matched to
 * the first Map entity, which the asset scenarios name "Hanul Precision" (한울정밀).
 */
export const HANUL = { symbol: "HANUL", market: "KRX", entityId: "map-entity-00" } as const;

function hanulMove(kind: "weakening" | "crossed"): AssetMove {
  const fact = day(2);
  const assetId = mockAssetId(HANUL.symbol, HANUL.market);
  if (kind === "crossed") return {
    moveId: "preview-move-crossed", assetId, from: "weakening", to: "broken", factDate: fact,
    reason: "Crossed your line: One customer above 25% of revenue (27.4%, " + fact + ")",
    evidence: [{ ...DART("20261030000117"), title: "분기보고서 (2026.09)", date: fact }],
    line: { journalId: "preview-line-1", claim: "One customer above 25% of revenue", metric: "Largest customer share", direction: "above", threshold: 25, unit: "%", observed: 27.4, crossed: true, setAtMs: Date.now() - 32 * DAY },
    actor: "loupist", atMs: Date.now() - DAY, undone: false,
  };
  return {
    moveId: "preview-move-1", assetId, from: "holding", to: "weakening", factDate: fact,
    reason: "The quarterly report shows operating margin 14.8%, below the 16% your first reason needs.",
    evidence: [{ ...DART("20261030000117"), title: "분기보고서 (2026.09)", date: fact }],
    reasonId: "preview-reason-1", actor: "loupist", atMs: Date.now() - DAY, undone: false, originChatId: "preview-chat", originMessageId: "preview-answer",
  };
}

const row = (id: string, kind: AssetChangeRow["kind"], factDate: string, facts: Record<string, unknown>, evidence: AssetEvidence[], extra: Partial<AssetChangeRow> = {}): AssetChangeRow =>
  ({ id, kind, factDate, recordedAtMs: Date.now() - DAY, facts, evidence: evidence.map(item => ({ ...item, date: item.date ?? factDate })), new: true, material: true, ...extra });

/** Hanul Precision: Weakening, two reasons, one line, a filing with section counts, results, Map and price rows. */
export function hanulFixture(state: "weakening" | "crossed" = "weakening"): AssetFixture {
  const move = hanulMove(state);
  const assetId = mockAssetId(HANUL.symbol, HANUL.market);
  const written = Date.now() - 32 * DAY;
  const weakening = hanulMove("weakening");
  const history = state === "crossed" ? [move, weakening] : [move];
  const page: AssetPage = {
    assetId, symbol: HANUL.symbol, market: HANUL.market, name: "Hanul Precision", class: "equity", classSource: "default", watch: "watching", entityId: HANUL.entityId,
    reasons: [
      { journalId: "preview-reason-1", text: "Margins recover once the new line qualifies.", createdAtMs: written, updatedAtMs: written, originChatId: "preview-chat", lastMove: weakening },
      { journalId: "preview-reason-2", text: "Morrow stays the only HBM substrate supplier.", createdAtMs: written, updatedAtMs: written },
    ],
    lines: [{
      journalId: "preview-line-1", claim: "One customer above 25% of revenue", metric: "Largest customer share", direction: "above", threshold: 25, unit: "%",
      createdAtMs: written, updatedAtMs: written, status: state === "crossed" ? "crossed" : "not_crossed",
      last: { observed: state === "crossed" ? 27.4 : 21, factDate: day(2), evidence: [{ ...DART("20261030000117"), title: "분기보고서 (2026.09)", date: day(2) }] },
    }],
    read: { verdict: state === "crossed" ? "broken" : "weakening", since: day(2), started: false, latestMove: move, undoMoveId: move.moveId },
    history,
    glance: [
      { key: "lastFiling", facts: { form: "분기보고서", title: "분기보고서 (2026.09)" }, date: day(2), evidence: [{ ...DART("20261030000117"), date: day(2) }] },
      { key: "lastResults", facts: { period: "Q3 2026" }, date: day(2), evidence: [{ ...DART("20261030000117"), date: day(2) }] },
      { key: "dividend", facts: { amount: 300, currency: "KRW", exDate: day(-60) }, date: day(20), evidence: [{ ...DART("20261009000045"), date: day(20) }] },
      { key: "price", facts: { change: -3.1, from: 41200, to: 39920, fromDate: day(5), currency: "KRW" }, date: day(1), evidence: [KRX(day(5)), KRX(day(1))] },
    ],
    checks: mockChecks("equity", "KRX"),
    map: { suppliers: 2, customers: 3 },
    refreshedAtMs: Date.now() - 2 * 3_600_000, createdAtMs: written, revision: 4,
  };
  const delta = {
    entityId: HANUL.entityId, verdict: page.read!.verdict, verdictSince: day(2), latestMove: move, moved: true, crossed: state === "crossed",
    rows: [
      ...(state === "crossed" ? [row("preview-row-crossed", "thesis", day(2), { from: "weakening", to: "broken", reason: move.reason, actor: "loupist", undone: false, line: move.line }, move.evidence)] : []),
      row("preview-row-filing", "filing", day(2), { form: "분기보고서", title: "분기보고서 (2026.09)" }, [DART("20261030000117")], { sections: [{ section: "risk", added: 2, removed: 1, changed: 1 }, { section: "business", added: 0, removed: 0, changed: 3 }] }),
      row("preview-row-results", "results", day(2), { period: "Q3 2026", metric: "operating_margin", value: 14.8, unit: "%", prior: 16.2 }, [{ ...DART("20261030000117"), tool: "dart_financials", pointer: "/data/facts/12" }]),
      row("preview-row-map", "map", day(3), { summary: "Morrow Components supplies Hanul Precision", disputed: false }, [{ tool: "world", title: "Morrow Components supplies Hanul Precision", worldId: "map-claim-1" }]),
      row("preview-row-price", "price", day(1), { change: -3.1, from: 41200, to: 39920, fromDate: day(5), currency: "KRW" }, [KRX(day(5)), KRX(day(1))], { new: false, material: false }),
    ],
  };
  return { page, delta };
}

/** Morrow Components with no thesis yet. */
export function noThesisFixture(): AssetFixture {
  const assetId = mockAssetId("MORR", "NASDAQ");
  return { page: {
    assetId, symbol: "MORR", market: "NASDAQ", name: "Morrow Components", class: "equity", classSource: "lookup", watch: "watching",
    reasons: [], lines: [], history: [],
    glance: [{ key: "lastFiling", facts: { form: "10-Q", title: "Quarterly report" }, date: day(4), evidence: [{ tool: "equity_fundamental_filings", provider: "sec", title: "SEC EDGAR", url: "https://www.sec.gov/Archives/edgar/data/990001/000099000126000011/morr-20260927.htm", date: day(4) }] }],
    checks: mockChecks("equity", "NASDAQ"), createdAtMs: Date.now() - 10 * DAY, refreshedAtMs: Date.now() - 3_600_000, revision: 2,
  }, delta: { moved: false, crossed: false, rows: [
    row("preview-morr-filing", "filing", day(4), { form: "10-Q", title: "Quarterly report" }, [{ tool: "equity_fundamental_filings", provider: "sec", title: "SEC EDGAR", url: "https://www.sec.gov/Archives/edgar/data/990001/000099000126000011/morr-20260927.htm" }], { sections: [] }),
  ] } };
}

/** Corvid Token: price and drawdown; token data is not checked yet. */
export function cryptoFixture(): AssetFixture {
  const assetId = mockAssetId("CRVD", "CRYPTO");
  const written = Date.now() - 20 * DAY;
  return { page: {
    assetId, symbol: "CRVD", market: "CRYPTO", name: "Corvid Token", class: "crypto", classSource: "market", watch: "holding",
    reasons: [{ journalId: "preview-crypto-reason", text: "Fees from the settlement network keep growing faster than token issuance.", createdAtMs: written, updatedAtMs: written }],
    lines: [], read: { verdict: "holding", since: day(20), started: true }, history: [],
    glance: [
      { key: "price", facts: { change: 12.6, from: 1.27, to: 1.43, fromDate: day(7), currency: "USD" }, date: day(0), evidence: [MARKET(day(7)), MARKET(day(0))] },
      { key: "drawdown", facts: { change: -18.4, high: 1.75, highDate: day(40) }, date: day(0), evidence: [MARKET(day(40)), MARKET(day(0))] },
    ],
    checks: mockChecks("crypto", "CRYPTO"), createdAtMs: written, refreshedAtMs: Date.now() - 600_000, revision: 3,
  }, delta: { verdict: "holding", verdictSince: day(20), moved: false, crossed: false, rows: [
    row("preview-crypto-price", "price", day(0), { change: 12.6, from: 1.27, to: 1.43, fromDate: day(7), currency: "USD" }, [MARKET(day(7)), MARKET(day(0))], { new: false }),
  ] } };
}

/** Saebyeok Robotics, a private company: Sources only. */
export function privateFixture(): AssetFixture {
  const assetId = mockAssetId("SAEBYEOK", "PRIVATE");
  return { page: {
    assetId, symbol: "SAEBYEOK", market: "PRIVATE", name: "Saebyeok Robotics", class: "private", classSource: "market", watch: "watching",
    reasons: [], lines: [], history: [], glance: [], checks: mockChecks("private", "PRIVATE"), createdAtMs: Date.now() - 5 * DAY, revision: 1,
  }, delta: { moved: false, crossed: false, rows: [
    row("preview-private-source", "source", day(3), { title: "Saebyeok Robotics Series B announcement" }, [{ tool: "source", title: "Saebyeok Robotics Series B announcement", recordId: "preview-sources" }]),
  ] } };
}

/**
 * One failed source and one connection that is off: OpenDART answered for
 * filings but not for results, and KRX is switched off. As in Rust, the checks
 * line lists only what the last check read.
 */
export function partialFixture(): AssetFixture {
  const fixture = hanulFixture();
  fixture.page.checks = { ...fixture.page.checks, kinds: ["filings"], connections: ["OpenDART"], failures: [
    { kind: "results", connection: "OpenDART", reason: "failed" },
    { kind: "price", connection: "KRX", reason: "needs_connection" },
  ] };
  return fixture;
}

export async function installAsset(bridge: PreviewBridge, fixture: AssetFixture, visitedDaysAgo?: number) {
  await bridge.watchlistAdd({ symbol: fixture.page.symbol, market: fixture.page.market, displayName: fixture.page.name });
  bridge.assets.install(PREVIEW_LOUPIST, fixture.page, fixture.delta);
  if (visitedDaysAgo !== undefined) {
    try {
      localStorage.setItem("loupist.assetVisit.v1", JSON.stringify({ [`${PREVIEW_LOUPIST}:${fixture.page.assetId}`]: Date.now() - visitedDaysAgo * DAY }));
    } catch { /* storage unavailable: a first look */ }
  }
}

/** The home Thread's crossed-line message, as Rust posts it. */
export function previewLineAlert(): ChatMessage {
  const assetId = mockAssetId(HANUL.symbol, HANUL.market);
  return {
    id: "preview-alert", role: "assistant", status: "complete", host_composed: true, created_at: new Date(Date.now() - 3_600_000).toISOString(),
    content: "Hanul Precision crossed the line you set: One customer above 25% of revenue — 27.4%. Thesis marked Broken.",
    alert: { asset_id: assetId, move_id: "preview-move-crossed", name: "Hanul Precision", symbol: HANUL.symbol, market: HANUL.market, claim: "One customer above 25% of revenue", value: "27.4%", set_at_ms: Date.now() - 32 * DAY, fact_date: day(2), source_url: "https://dart.fss.or.kr/dsaf001/main.do?rcpNo=20261030000117", source_title: "분기보고서 (2026.09)" },
  };
}
