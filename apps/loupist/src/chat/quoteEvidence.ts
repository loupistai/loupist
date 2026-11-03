import type { ChatEvidenceRef } from "../types";

/** A saved quote row. Prices and symbols come only from fields that were present. */
export type QuoteRow = {
  symbol: string;
  name?: string;
  currency?: string;
  /** Canonical text of a finite JSON number, or a whole-string decimal. */
  price: string;
  /** Set only when the row itself carried `last_timestamp` or `quote_time`. */
  quoteTime?: string;
  /** JSON pointer of the result object when it lives in the parsed excerpt. */
  pointer?: string;
};

export type QuoteSelector = {
  pointer?: string;
  /** Exact ticker from the grid row. Never matched by comparing prices. */
  asset?: string;
  /** Exact ticker from a check field. Used only when it agrees with `asset`. */
  symbol?: string;
};

export type QuoteUnmatched = "pointer" | "asset" | "assetConflict" | "unspecified";

export type QuoteRead = {
  rows: QuoteRow[];
  focus: QuoteRow | null;
  unmatched?: QuoteUnmatched;
  /** The ticker a cell asked for when no single saved row had it. */
  requestedSymbol?: string;
  /** The saved excerpt did not parse, or the host marked it truncated. */
  partial: boolean;
  /** Bounded copy of the saved excerpt for the collapsed technical original. */
  original: string;
  originalShortened: boolean;
};

const MAX_SCAN = 200_000;
const MAX_ROWS = 40;
const ORIGINAL_LIMIT = 2_000;

/** OpenBB/yfinance quote tools. `krx_daily_quote` and other prose tools do not match. */
export const isQuoteTool = (tool: string) =>
  /(?:^|__)(?:equity_)?price_quote$/i.test(tool) || /(?:^|_)equity_price_quote$/i.test(tool);

const asRecord = (value: unknown): Record<string, unknown> | null =>
  value !== null && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : null;

const priceText = (value: unknown): string | null => {
  if (typeof value === "number" && Number.isFinite(value)) return JSON.stringify(value);
  if (typeof value === "string" && /^-?\d+(?:\.\d+)?$/u.test(value)) return value;
  return null;
};

const plain = (value: unknown): string | undefined =>
  typeof value === "string" && value.trim() ? value.trim() : undefined;

/** A quote clock only when the row stored one. Retrieval time is never reused. */
const quoteTimeOf = (record: Record<string, unknown>): string | undefined => {
  const raw = record.last_timestamp ?? record.quote_time;
  if (typeof raw === "string") {
    const value = raw.trim();
    return value && !Number.isNaN(Date.parse(value)) ? value : undefined;
  }
  if (typeof raw === "number" && Number.isFinite(raw) && raw >= 1_000_000_000) {
    const ms = raw < 1_000_000_000_000 ? raw * 1000 : raw;
    const date = new Date(ms);
    return Number.isNaN(date.getTime()) ? undefined : date.toISOString();
  }
  return undefined;
};

const quoteRow = (value: unknown, pointer?: string): QuoteRow | null => {
  const record = asRecord(value);
  if (!record) return null;
  const symbol = plain(record.symbol);
  const price = priceText(record.last_price);
  if (!symbol || !price) return null;
  return { symbol, name: plain(record.name), currency: plain(record.currency), price, quoteTime: quoteTimeOf(record), pointer };
};

const tryParse = (text: string): { ok: true; value: unknown } | { ok: false } => {
  try { return { ok: true, value: JSON.parse(text) as unknown }; }
  catch { return { ok: false }; }
};

type Basket = { items: unknown[]; base: string | null };

/** One parsed results array. Inner `content[].text` is parsed only when that string is complete JSON. */
const selectBasket = (root: unknown): Basket | null => {
  const record = asRecord(root);
  if (!record) return null;
  const candidates: Basket[] = [];
  const structured = asRecord(record.structuredContent);
  if (structured && Array.isArray(structured.results)) candidates.push({ items: structured.results, base: "/structuredContent/results" });
  if (Array.isArray(record.results)) candidates.push({ items: record.results, base: "/results" });
  if (Array.isArray(record.content)) {
    for (const item of record.content) {
      const text = asRecord(item)?.text;
      if (typeof text !== "string") continue;
      const inner = tryParse(text);
      const results = inner.ok ? asRecord(inner.value)?.results : undefined;
      if (Array.isArray(results)) candidates.push({ items: results, base: null });
    }
  }
  return candidates.find((candidate) => candidate.items.some((item) => quoteRow(item))) ?? candidates[0] ?? null;
};

const looksLikeQuoteJson = (excerpt: string) => {
  const trimmed = excerpt.trimStart();
  if (!trimmed.startsWith("{") && !trimmed.startsWith("[")) return false;
  return excerpt.includes('"content"') || excerpt.includes('"last_price"') || excerpt.includes('"results"');
};

const pointerHits = (pointer: string, path: string) => pointer === path || pointer.startsWith(`${path}/`);

const focusOf = (rows: readonly QuoteRow[], selector?: QuoteSelector): Pick<QuoteRead, "focus" | "unmatched" | "requestedSymbol"> => {
  if (!selector) return { focus: null };
  const pointer = selector.pointer?.trim();
  if (pointer) {
    const hits = rows.filter((row) => row.pointer && pointerHits(pointer, row.pointer));
    if (hits.length) {
      const focus = hits.reduce((best, row) => (row.pointer!.length > (best.pointer?.length ?? 0) ? row : best));
      // A supplied ticker that names a different row does not replace the pointed row.
      return { focus };
    }
    // A pointer that does not resolve must not fall through to another symbol.
    return { focus: null, unmatched: "pointer" };
  }
  const asset = plain(selector.asset);
  const symbol = plain(selector.symbol);
  if (asset && symbol && asset !== symbol) return { focus: null, unmatched: "assetConflict" };
  const key = asset ?? symbol;
  if (key) {
    const hits = rows.filter((row) => row.symbol === key);
    return hits.length === 1 ? { focus: hits[0]! } : { focus: null, unmatched: "asset", requestedSymbol: key };
  }
  return { focus: null, unmatched: "unspecified" };
};

const bounded = (excerpt: string) => ({
  original: excerpt.length > ORIGINAL_LIMIT ? excerpt.slice(0, ORIGINAL_LIMIT) : excerpt,
  originalShortened: excerpt.length > ORIGINAL_LIMIT,
});

/**
 * A price-quote tool's saved basket.
 * Other tools, articles and prose stay on their own readers.
 * A quote tool whose excerpt is not valid JSON returns a receipt with no rows.
 */
export function quoteEvidence(evidence: ChatEvidenceRef, selector?: QuoteSelector): QuoteRead | null {
  if (!isQuoteTool(evidence.tool_name)) return null;
  const limits = bounded(evidence.excerpt);
  const parsed = evidence.excerpt.length > MAX_SCAN ? { ok: false as const } : tryParse(evidence.excerpt);
  if (!parsed.ok) {
    if (evidence.excerpt.length <= MAX_SCAN && !looksLikeQuoteJson(evidence.excerpt)) return null;
    return { rows: [], focus: null, unmatched: selector ? "unspecified" : undefined, partial: true, ...limits };
  }
  const basket = selectBasket(parsed.value);
  if (!basket) return null;
  const rows = basket.items.flatMap((item, index) => {
    const row = quoteRow(item, basket.base ? `${basket.base}/${index}` : undefined);
    return row ? [row] : [];
  }).slice(0, MAX_ROWS);
  return {
    rows,
    ...focusOf(rows, selector),
    partial: evidence.excerpt_truncated || basket.items.length > MAX_ROWS,
    ...limits,
  };
}
