import type { LoupistBridge } from "../types";
import type { KnowledgePin } from "../contracts/knowledge";
import type { ComposerChip } from "./composerChips";
import { rankText } from "./composerChips";
import { entityLabel } from "../components/map/labels";
import { nodeCategory } from "../components/map/kinds";
import { marketFor } from "./marketStatus";

/** One row of the `$` menu: a followed asset, a company or asset on the Map, or a typed symbol. */
export type TickerOption = {
  key: string;
  source: "watchlist" | "map" | "free";
  label: string;
  symbol?: string;
  market?: string;
  /** Map names become a World pin instead of a ticker token. */
  pin?: KnowledgePin;
};

const SYMBOL_PATTERN = /^[A-Z0-9][A-Z0-9.^=-]{0,23}(?::[A-Z0-9_-]{1,24})?$/u;
const MAX_ROWS = 8;

/** Market codes that name a class of their own (Rust `class_from_market`): commodities, rates and FX, real, prediction and private assets. */
const ASSET_MARKETS = new Set(["CME", "COMEX", "NYMEX", "ICE", "FX", "FOREX", "RATES", "UST", "REAL", "POLYMARKET", "KALSHI", "PRIVATE"]);

/**
 * The marker a ticker chip takes, matching the Map: a company's shares are an Organization (the company color);
 * every other instrument, such as a coin, a future or a currency pair, is an Asset (the market color). The market
 * code decides, as it does first in Rust; an equity market keeps the company color.
 */
export function tickerMark(market: string | undefined): "company" | "asset" {
  const code = market?.trim().toUpperCase() ?? "";
  return marketFor(code) === "CRYPTO" || ASSET_MARKETS.has(code) ? "asset" : "company";
}

/** "AAPL" or "005930:KRX" typed after `$`, normalized, or null when it is not a symbol. */
export function typedSymbol(query: string): { symbol: string; market?: string } | null {
  const value = query.trim().toUpperCase();
  if (!value || !SYMBOL_PATTERN.test(value)) return null;
  const [symbol, market] = value.split(":");
  return { symbol: symbol!, market };
}

/** The `$` menu: followed assets first, then the Map's companies and assets, then "Use $NVDA". */
export async function searchTickers(bridge: LoupistBridge, loupistId: string, query: string, language: string): Promise<TickerOption[]> {
  const [watchlist, map] = await Promise.all([
    bridge.watchlistList().catch(() => []),
    bridge.knowledgeMap({ loupistId, maxNodes: 1000 }).catch(() => null),
  ]);
  const followed = watchlist.flatMap((entry): (TickerOption & { score: number })[] => {
    const label = entry.displayName?.trim() || entry.symbol;
    const score = rankText(query, [entry.symbol, label, entry.market]);
    return score ? [{ key: `watch:${entry.symbol}:${entry.market}`, source: "watchlist", label, symbol: entry.symbol, market: entry.market === "UNKNOWN" ? undefined : entry.market, score }] : [];
  }).sort((a, b) => b.score - a.score);
  const followedNames = new Set(followed.map((row) => row.label.toLowerCase()));
  const onMap = (map?.nodes ?? []).flatMap((node): (TickerOption & { score: number })[] => {
    if (node.kind !== "entity" || node.archived) return [];
    const category = nodeCategory(node);
    if (category !== "company" && category !== "market") return [];
    const label = entityLabel(node, language);
    if (followedNames.has(label.toLowerCase())) return [];
    const score = rankText(query, [label, node.label, ...(node.localNames ?? [])]);
    if (!score) return [];
    const pin: KnowledgePin | undefined = node.recordId ? { kind: "world_node", loupistId, recordId: node.recordId, objectId: node.id, label } : undefined;
    return pin ? [{ key: `map:${node.id}`, source: "map", label, pin, score: score * 10 + Math.min(node.degree, 9) }] : [];
  }).sort((a, b) => b.score - a.score);
  const rows: TickerOption[] = [...followed, ...onMap].slice(0, MAX_ROWS).map(({ score: _score, ...row }) => row);
  const typed = typedSymbol(query);
  if (typed && !rows.some((row) => row.symbol === typed.symbol && (!typed.market || row.market === typed.market))) {
    rows.push({ key: `free:${typed.symbol}:${typed.market ?? ""}`, source: "free", label: typed.symbol, symbol: typed.symbol, market: typed.market });
  }
  return rows;
}

/** The composer chip for a ticker row; it serializes to `$SYMBOL:MARKET (Name)`. */
export function companyChip(option: TickerOption): ComposerChip {
  const symbol = option.symbol!;
  const token = option.market ? `${symbol}:${option.market}` : symbol;
  const label = option.label && option.label !== symbol ? `${option.label} · ${symbol}` : symbol;
  return { key: `company:${token}`, kind: "company", id: token, label, text: option.label && option.label !== symbol ? option.label : undefined };
}

/** Ticker tokens a message carried, in order: `$AAPL:NASDAQ`, `$005930:KRX`, `$NVDA`. */
export function assetRefs(text: string): { symbol: string; market?: string; name?: string }[] {
  const refs: { symbol: string; market?: string; name?: string }[] = [];
  const seen = new Set<string>();
  // The composer writes a chip as `$SYMBOL:MARKET (Name)`; the name is the user's own label for it.
  for (const match of text.matchAll(/(?:^|[\s(])\$([A-Z0-9][A-Z0-9.^=-]{0,23})(?::([A-Z0-9_-]{1,24}))?(?: \(([^()\n]{1,120})\))?(?=$|[\s),.;!?])/gu)) {
    const key = `${match[1]}:${match[2] ?? ""}`;
    if (seen.has(key)) continue;
    seen.add(key);
    refs.push({ symbol: match[1]!, market: match[2], name: match[3] });
  }
  return refs;
}
