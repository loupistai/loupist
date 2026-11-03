import { describe, expect, it } from "vitest";
import type { AssetsGrid, ChatEvidenceRef, NumberCheck } from "../types";
import { currencyOf, gridCounts, gridCsv, gridMarkdown, gridTsv, nextSort, sortedRows } from "./grid";

const DART: ChatEvidenceRef = {
  id: "ev_dart", title: "Solsup Foods FY2025 statements", kind: "original", provider: "OpenDART", tool_name: "dart_financials",
  retrieved_at: "2026-03-20T06:00:00Z", published_at: "2026-03-18", url: "https://opendart.fss.or.kr/dsaf001/main.do?rcpNo=20260318990221",
  excerpt: "", excerpt_truncated: false, content_sha256: "c".repeat(64), warnings: [],
};
const MARGIN: ChatEvidenceRef = { ...DART, id: "ev_margin", title: "Margin", kind: "derived", provider: undefined, url: undefined, tool_name: "calculate", published_at: undefined };

const data = (text: string, status: NumberCheck["status"] = "rounded"): NumberCheck => ({ link_index: 0, text, evidence_id: DART.id, kind: "data", status });
const margin: NumberCheck = {
  link_index: 1, text: "8.42%", evidence_id: MARGIN.id, kind: "calculated", status: "matched",
  calc: { title: "Operating margin", formulas: ["Operating income / Revenue = 38.1 / 452.3 = 8.42%"], inputs: [
    { name: "profit", label: "Operating income", unit: { kind: "amount", currency: "KRW", scale: 9 }, display: "38.1", evidence_id: DART.id, status: "rounded" },
    { name: "revenue", label: "Revenue", unit: { kind: "amount", currency: "KRW", scale: 9 }, display: "452.3", evidence_id: DART.id, status: "rounded" },
  ] },
};

const grid: AssetsGrid = {
  layout: "assets",
  subtitle: "FY2025 unless noted",
  notes: ["Payout uses net income attributable to owners."],
  rows: [
    { symbol: "990221", market: "KRX", label: "Solsup Foods" },
    { symbol: "990552", market: "KRX", label: "Iseul Logistics" },
    { symbol: "990777", market: "KRX", label: "Hanbit Chem" },
  ],
  columns: [
    { id: "revenue", label: "Revenue", period: "FY2025", basis: "Consolidated", unit: "amount" },
    { id: "margin", label: "Operating margin", period: "FY2025", unit: "percent" },
  ],
  cells: [
    [{ value: "₩452.3bn", evidence_id: DART.id, check: data("₩452.3bn"), sort: 452.3e9 }, { value: "8.42%", evidence_id: MARGIN.id, check: margin, sort: 8.42 }],
    [{ empty: "not_reported", note: "Separate statements only." }, { value: "5.92%", evidence_id: MARGIN.id, check: { ...margin, text: "5.92%", status: "differs", nearest: "6.10%" }, sort: 5.92 }],
    [{ value: "₩286.5bn", evidence_id: DART.id, check: data("₩286.5bn"), sort: 286.5e9 }, { empty: "unavailable" }],
  ],
};

describe("grid answers", () => {
  it("cycles a column's sort high to low, low to high, then the original order, with empty cells last", () => {
    let sort = nextSort(null, 0);
    expect(sort).toEqual({ column: 0, direction: "desc" });
    expect(sortedRows(grid, sort)).toEqual([0, 2, 1]);
    sort = nextSort(sort, 0);
    expect(sortedRows(grid, sort)).toEqual([2, 0, 1]);
    sort = nextSort(sort, 0);
    expect(sort).toBeNull();
    expect(sortedRows(grid, sort)).toEqual([0, 1, 2]);
    // Another column starts over at high to low; the empty cell stays last either way.
    expect(sortedRows(grid, nextSort({ column: 0, direction: "asc" }, 1))).toEqual([0, 1, 2]);
    expect(sortedRows(grid, { column: 1, direction: "asc" })).toEqual([1, 0, 2]);
  });

  it("counts linked cells and the numbers to check", () => {
    expect(gridCounts(grid)).toEqual({ linked: 4, toCheck: 1 });
  });

  it("copies tab-separated rows with empty cells blank", () => {
    expect(gridTsv(grid).split("\n")).toEqual([
      "Asset\tRevenue\tOperating margin",
      "Solsup Foods\t₩452.3bn\t8.42%",
      "Iseul Logistics\t\t5.92%",
      "Hanbit Chem\t₩286.5bn\t",
    ]);
  });

  it("exports one CSV row per cell with its source or formula, and the notice last", () => {
    const csv = gridCsv(grid, [DART, MARGIN]);
    expect(csv.startsWith("﻿")).toBe(true);
    const lines = csv.slice(1).trimEnd().split("\r\n");
    expect(lines[0]).toBe("asset,symbol,market,measure,period,basis,value,unit,currency,status,source,source_date,source_url_or_formula");
    expect(lines).toHaveLength(1 + 3 * 2 + 1);
    expect(lines[1]).toBe(`Solsup Foods,990221,KRX,Revenue,FY2025,Consolidated,₩452.3bn,amount,KRW,rounded,OpenDART,2026-03-18,${DART.url}`);
    // A calculated cell carries its formula with the inputs substituted and the inputs' source.
    expect(lines[2]).toBe("Solsup Foods,990221,KRX,Operating margin,FY2025,,8.42%,percent,,matched,OpenDART,2026-03-18,Operating margin = 38.1 / 452.3 = 8.42%");
    // An empty cell puts its reason in status.
    expect(lines[3]).toBe("Iseul Logistics,990552,KRX,Revenue,FY2025,Consolidated,,amount,,not_reported,,,Separate statements only.");
    expect(lines[4]).toContain(",differs,");
    expect(lines.at(-1)).toBe('Note,,,,,,,,,,"Made with Loupist. AI can be wrong. Not a licensed financial adviser.",,');
    // Holdings never reach a grid, so no export can carry a quantity.
    expect(csv).not.toMatch(/quantity|shares held/iu);
  });

  it("exports a Markdown table with a footnote per value, the notes and the notice", () => {
    const markdown = gridMarkdown("Dividend payers", grid, [DART, MARGIN]);
    const lines = markdown.split("\n");
    expect(lines[0]).toBe("# Dividend payers");
    expect(lines).toContain("| Asset | Revenue · FY2025 | Operating margin · FY2025 |");
    expect(lines).toContain("| Solsup Foods | ₩452.3bn[^1] | 8.42%[^2] |");
    expect(lines).toContain("| Iseul Logistics | Not reported | 5.92%[^3] |");
    expect(lines).toContain("| Hanbit Chem | ₩286.5bn[^4] | Couldn't get |");
    expect(markdown).toContain(`[^1]: Solsup Foods FY2025 statements — 2026-03-18 — ${DART.url}`);
    expect(markdown).toContain("[^2]: Operating margin = 38.1 / 452.3 = 8.42% (calculated in Loupist; inputs: OpenDART, 2026-03-18)");
    expect(markdown).toContain("Payout uses net income attributable to owners.");
    expect(lines.filter(Boolean).at(-1)).toBe("_Made with Loupist. AI can be wrong. Not a licensed financial adviser._");
  });

  it("names the currency a printed amount uses", () => {
    expect(currencyOf("₩452.3bn")).toBe("KRW");
    expect(currencyOf("4,523억 원")).toBe("KRW");
    expect(currencyOf("$1.2bn")).toBe("USD");
    expect(currencyOf("USD 1.2bn")).toBe("USD");
    expect(currencyOf("1.2×")).toBe("");
  });
});
