import { evidenceFixture } from "../test/evidenceFixtures";
import { quoteEvidence } from "./quoteEvidence";

const rows = [
  { symbol: "AAPL", name: "Apple", currency: "USD", last_price: 271.4 },
  { symbol: "MSFT", name: "Microsoft", currency: "USD", last_price: 415.2 },
  { symbol: "AAPL", name: "Apple duplicate", currency: "USD", last_price: 271.4 },
];

const nested = (items: unknown[]) => JSON.stringify({
  content: [{ type: "text", text: JSON.stringify({ results: items }) }],
});

const basket = (excerpt: string, extra: Parameters<typeof evidenceFixture>[0] = {}) => evidenceFixture({
  id: "quote",
  title: "Equity price quote",
  tool_name: "mcp__openbb__equity_price_quote",
  provider: "Yahoo Finance",
  url: "https://finance.example/quotes",
  retrieved_at: "2026-10-11T15:04:00Z",
  published_at: undefined,
  excerpt,
  ...extra,
});

describe("quoteEvidence", () => {
  it("reads a nested content text basket and does not invent a quote time from retrieval", () => {
    const read = quoteEvidence(basket(nested(rows.slice(0, 2))));
    expect(read?.rows.map((row) => [row.symbol, row.price, row.currency, row.name, row.quoteTime])).toEqual([
      ["AAPL", "271.4", "USD", "Apple", undefined],
      ["MSFT", "415.2", "USD", "Microsoft", undefined],
    ]);
    expect(read?.focus).toBeNull();
    expect(read?.partial).toBe(false);
  });

  it("selects the pointer's row, including a field under that row, and refuses a string blob", () => {
    const excerpt = JSON.stringify({
      structuredContent: { results: rows.slice(0, 2) },
      content: [{ type: "text", text: JSON.stringify({ results: rows.slice(0, 2) }) }],
    });
    const read = quoteEvidence(basket(excerpt), { pointer: "/structuredContent/results/0/last_price", asset: "MSFT" });
    expect(read?.focus).toMatchObject({ symbol: "AAPL", price: "271.4", pointer: "/structuredContent/results/0" });
    const blob = quoteEvidence(basket(excerpt), { pointer: "/content/0/text", asset: "MSFT" });
    expect(blob?.focus).toBeNull();
    expect(blob?.unmatched).toBe("pointer");
  });

  it("does not substitute another symbol when the pointed row has no price", () => {
    const excerpt = JSON.stringify({ results: [{ symbol: "AAPL" }, rows[1]] });
    const read = quoteEvidence(basket(excerpt), { pointer: "/results/0/last_price", asset: "MSFT" });
    expect(read?.focus).toBeNull();
    expect(read?.unmatched).toBe("pointer");
    expect(read?.rows.map((row) => row.symbol)).toEqual(["MSFT"]);
  });

  it("uses an exact unique ticker and refuses price equality, duplicates, and disagreeing hints", () => {
    const excerpt = nested(rows);
    expect(quoteEvidence(basket(excerpt), { asset: "MSFT" })?.focus?.symbol).toBe("MSFT");
    const samePrice = quoteEvidence(basket(excerpt), {});
    expect(samePrice?.focus).toBeNull();
    expect(samePrice?.rows).toHaveLength(3);
    expect(quoteEvidence(basket(excerpt), { asset: "AAPL" })?.unmatched).toBe("asset");
    expect(quoteEvidence(basket(nested(rows.slice(0, 2))), { asset: "MSFT", symbol: "AAPL" })?.unmatched).toBe("assetConflict");
  });

  it("keeps a quote timestamp only when the row stored one", () => {
    const excerpt = nested([{ ...rows[0], last_timestamp: "2026-10-11T14:30:00Z", date: "2020-01-01" }]);
    expect(quoteEvidence(basket(excerpt), { asset: "AAPL" })?.focus?.quoteTime).toBe("2026-10-11T14:30:00Z");
  });

  it("does not read prices out of a truncated quote payload", () => {
    const payload = JSON.stringify({ content: [{ type: "text", text: JSON.stringify({ results: [...rows.slice(0, 2), { symbol: "NVDA", name: "NVIDIA", currency: "USD", last_price: 131.2 }] }) }] });
    const excerpt = payload.slice(0, payload.indexOf("NVDA") + 2);
    const read = quoteEvidence(basket(excerpt, { excerpt_truncated: true }));
    expect(read?.rows).toEqual([]);
    expect(read?.partial).toBe(true);
    expect(read?.original).toContain("AAPL");
  });

  it("stays a receipt for a known quote tool with no complete row, and leaves other evidence alone", () => {
    expect(quoteEvidence(basket("{\"results\":[{\"symbol\":\"AAPL\"}]}", { excerpt_truncated: true }))?.rows).toEqual([]);
    const cut = nested(rows.slice(0, 2)).slice(0, 40);
    expect(quoteEvidence(basket(cut, { tool_name: "web_fetch" }))).toBeNull();
    expect(quoteEvidence(basket("No prices were returned."))).toBeNull();
    expect(quoteEvidence(evidenceFixture({ tool_name: "krx_daily_quote", url: undefined, excerpt: "KOSPI close 2650" }))).toBeNull();
    expect(quoteEvidence(evidenceFixture())).toBeNull();
  });
});
