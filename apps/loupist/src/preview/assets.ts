/**
 * Fictional assets for the design preview after the thin host: a name with optional
 * reference fields, the page's thesis, and the one stored price a saved read replays
 * (value, currency, stated date, provider), or why there is none. Every thesis move has
 * a date and a source, like Rust's.
 */
import type { AssetEvidence, AssetMove, AssetPricePoint, AssetStoredPrice } from "../contracts/assets";
import type { ChatMessage } from "../contracts/chat";
import { mockAssetId, mockSavedRead, type MockAssets, type MockPageFixture, type MockPriceFixture } from "../bridge/mock/assets";
import type { PreviewBridge } from "./fixtures";

export const PREVIEW_LOUPIST = "loupist-quality";
const DAY = 86_400_000;
const day = (daysAgo: number) => new Date(Date.now() - daysAgo * DAY).toISOString().slice(0, 10);

const DART = (receipt: string, date: string): AssetEvidence => ({ tool: "dart_filing_search", provider: "opendart", title: "분기보고서 (2026.09)", url: `https://dart.fss.or.kr/dsaf001/main.do?rcpNo=${receipt}`, date });

/** One asset to install: its catalog row, and optionally its page and stored price for the preview Loupist. */
export type AssetFixture = {
  asset: Parameters<MockAssets["installAsset"]>[0] & { assetId: string };
  page?: MockPageFixture;
  price?: MockPriceFixture;
};

export function installAsset(bridge: PreviewBridge, fixture: AssetFixture) {
  bridge.assets.installAsset(fixture.asset);
  if (fixture.page) bridge.assets.install(PREVIEW_LOUPIST, fixture.asset.assetId, fixture.page);
  if (fixture.price) bridge.assets.installPrice(PREVIEW_LOUPIST, fixture.asset.assetId, fixture.price);
}

/** The last weekday before today. */
function lastWeekday() {
  for (let back = 1; ; back += 1) {
    const at = new Date(Date.now() - back * DAY);
    if (at.getUTCDay() !== 0 && at.getUTCDay() !== 6) return at.toISOString().slice(0, 10);
  }
}

/**
 * A plausible run of weekday values ending on `end` at `last`: a seeded random walk
 * with a little drift, scaled so the final value is exactly `last`.
 */
export function previewPoints({ end = lastWeekday(), last, days, seed = 7, volatility = 0.018, digits = 2 }: { end?: string; last: number; days: number; seed?: number; volatility?: number; digits?: number }): AssetPricePoint[] {
  const dates: string[] = [];
  for (let at = Date.parse(`${end}T00:00:00Z`); dates.length < days; at -= DAY) {
    const weekday = new Date(at).getUTCDay();
    if (weekday !== 0 && weekday !== 6) dates.unshift(new Date(at).toISOString().slice(0, 10));
  }
  let state = seed >>> 0 || 1;
  const random = () => { state = (Math.imul(state, 1_103_515_245) + 12_345) >>> 0; return state / 4_294_967_296; };
  const walk = [1];
  for (let index = 1; index < dates.length; index += 1) walk.push(walk[index - 1]! * (1 + 0.0006 + (random() - 0.5) * 2 * volatility));
  const scale = last / walk.at(-1)!;
  const factor = 10 ** digits;
  return dates.map((date, index) => ({ date, value: Math.round(walk[index]! * scale * factor) / factor }));
}

/**
 * Hanul Precision as the rest of the preview follows it, matched to the first Map entity,
 * which the scenarios name "Hanul Precision" (한울정밀).
 */
export const HANUL = { name: "Hanul Precision", ticker: "HNP", exchange: "KS", entityId: "map-entity-00" } as const;
export const HANUL_ID = mockAssetId(HANUL.name);

function hanulMove(kind: "weakening" | "crossed"): AssetMove {
  const fact = day(2);
  if (kind === "crossed") return {
    moveId: "preview-move-crossed", assetId: HANUL_ID, from: "weakening", to: "broken", factDate: fact,
    reason: "Crossed your line: One customer above 25% of revenue (27.4%, " + fact + ")",
    evidence: [DART("20261030000117", fact)],
    line: { journalId: "preview-line-1", claim: "One customer above 25% of revenue", metric: "Largest customer share", direction: "above", threshold: 25, unit: "%", observed: 27.4, crossed: true, setAtMs: Date.now() - 32 * DAY },
    actor: "loupist", atMs: Date.now() - DAY, undone: false,
  };
  return {
    moveId: "preview-move-1", assetId: HANUL_ID, from: "holding", to: "weakening", factDate: fact,
    reason: "The quarterly report shows operating margin 14.8%, below the 16% your first reason needs.",
    evidence: [DART("20261030000117", fact)],
    reasonId: "preview-reason-1", actor: "loupist", atMs: Date.now() - DAY, undone: false, originChatId: "preview-chat", originMessageId: "preview-answer",
  };
}

/** A stored price as a replay of a bundled read leaves it. */
const stored = (value: number, date: string, currency: string | null, provider: string): AssetStoredPrice =>
  ({ value, date, currency, provider, sourceUrl: null, dataset: { dataset_id: `preview-${provider}`, revision: 1, digest: "preview" } });

/** Hanul Precision: Weakening (or Broken by a crossed line), two reasons, one line, a Map match and a won price with three months of points. */
export function hanulFixture(state: "weakening" | "crossed" = "weakening"): AssetFixture {
  const move = hanulMove(state);
  const written = Date.now() - 32 * DAY;
  const weakening = hanulMove("weakening");
  const points = previewPoints({ last: 39_920, days: 64, seed: 20_261_113, volatility: 0.021, digits: -1 });
  return {
    asset: { assetId: HANUL_ID, name: HANUL.name, ticker: HANUL.ticker, exchange: HANUL.exchange, figi: "BBG000PREV01", securityType: "Common Stock", createdAtMs: written },
    page: {
      entityId: HANUL.entityId, map: { suppliers: 2, customers: 3 },
      reasons: [
        { journalId: "preview-reason-1", text: "Margins recover once the new line qualifies.", createdAtMs: written, updatedAtMs: written, originChatId: "preview-chat", lastMove: weakening },
        { journalId: "preview-reason-2", text: "Morrow stays the only HBM substrate supplier.", createdAtMs: written, updatedAtMs: written },
      ],
      lines: [{
        journalId: "preview-line-1", claim: "One customer above 25% of revenue", metric: "Largest customer share", direction: "above", threshold: 25, unit: "%",
        createdAtMs: written, updatedAtMs: written, status: state === "crossed" ? "crossed" : "not_crossed",
        last: { observed: state === "crossed" ? 27.4 : 21, factDate: day(2), evidence: [DART("20261030000117", day(2))] },
      }],
      read: { verdict: state === "crossed" ? "broken" : "weakening", since: day(2), started: false, latestMove: move, undoMoveId: move.moveId },
      history: state === "crossed" ? [move, weakening] : [move],
    },
    price: { price: stored(39_920, points.at(-1)!.date, "KRW", "krx"), savedRead: mockSavedRead("ready", "krx_daily_quote"), points },
  };
}

/**
 * NVIDIA with a saved read and a history read: a US-dollar price whose provider stated a
 * time, and a year of points from the history read.
 */
export function nvidiaFixture(): AssetFixture {
  const assetId = mockAssetId("BBG000BBJQV0");
  const points = previewPoints({ last: 160, days: 262, seed: 3 });
  const at = new Date(Date.now() - 20 * 60_000).toISOString().replace(/\.\d{3}Z$/u, "Z");
  return {
    asset: { assetId, name: "NVIDIA CORP", ticker: "NVDA", exchange: "US", figi: "BBG000BBJQV0", securityType: "Common Stock" },
    price: { price: stored(160, at, "USD", "yfinance"), savedRead: mockSavedRead("ready", "equity_quote", true), points: [...points.slice(0, -1), { date: at, value: 160 }], next: { value: 160.42, date: at } },
  };
}

/** Corvane Systems, just followed from a listing: no saved read yet, so no price, no chart and no thesis. */
export function noThesisFixture(): AssetFixture {
  return { asset: { assetId: mockAssetId("BBG000PREV02"), name: "Corvane Systems", ticker: "CRVN", exchange: "US", figi: "BBG000PREV02", securityType: "Common Stock" } };
}

/** A price kept from an earlier replay after the latest one failed. */
export function failedFixture(): AssetFixture {
  const fixture = hanulFixture();
  return { ...fixture, price: { ...fixture.price!, savedRead: mockSavedRead("failed", "krx_daily_quote"), replay: "failed" } };
}

/** A price kept after the grant its saved read needs was switched off. */
export function needsConnectionFixture(): AssetFixture {
  const fixture = nvidiaFixture();
  return { ...fixture, price: { ...fixture.price!, savedRead: mockSavedRead("needs_connection"), replay: "needs_connection" } };
}

/** A fund whose result states no currency: the price shows without one, and no display-currency line. */
export function noCurrencyFixture(): AssetFixture {
  const assetId = mockAssetId("BBG000PREV03");
  const points = previewPoints({ last: 12.84, days: 30, seed: 11, volatility: 0.006 });
  return {
    asset: { assetId, name: "Harbor Fund", ticker: "HRBF", exchange: "US", figi: "BBG000PREV03", securityType: "Open-End Fund" },
    price: { price: stored(12.84, points.at(-1)!.date, null, "fmp"), savedRead: mockSavedRead("ready", "fund_nav"), points },
  };
}

/** An asset followed by its name alone, with one reason and a long name: no reference fields, no price yet. */
export const NAME_ONLY = "Seongsu-dong office building stake (through the family partnership)";
export function nameOnlyFixture(): AssetFixture {
  const assetId = mockAssetId(`name:${NAME_ONLY}`);
  const written = Date.now() - 12 * DAY;
  return {
    asset: { assetId, name: NAME_ONLY, createdAtMs: written },
    page: { reasons: [{ journalId: "preview-local-reason", text: "Rent covers the loan even if one floor stays empty.", createdAtMs: written, updatedAtMs: written }] },
  };
}

/** The Thread's post when a line the user set was crossed. */
export function previewLineAlert(): ChatMessage {
  return {
    id: "preview-alert", role: "assistant", status: "complete", host_composed: true, created_at: new Date(Date.now() - 3_600_000).toISOString(),
    content: "Hanul Precision crossed the line you set: One customer above 25% of revenue — 27.4%. Thesis marked Broken.",
    alert: { asset_id: HANUL_ID, move_id: "preview-move-crossed", name: HANUL.name, symbol: HANUL.ticker, market: HANUL.exchange, claim: "One customer above 25% of revenue", value: "27.4%", set_at_ms: Date.now() - 32 * DAY, fact_date: day(2), source_url: "https://dart.fss.or.kr/dsaf001/main.do?rcpNo=20261030000117", source_title: "분기보고서 (2026.09)" },
  };
}
