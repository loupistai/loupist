import { describe, expect, it } from "vitest";
import type { AssetsGrid, ChatEvidenceRef, NumberCheck } from "../types";
import { currencyOf, gridCounts, gridCsv, gridDecimals, gridMarkdown, gridTsv, nextSort, shownAmount, shownCheck, sortedRows } from "./grid";

const DART: ChatEvidenceRef = {
  id: "ev_dart", title: "Solsup Foods FY2025 statements", kind: "original", provider: "OpenDART", tool_name: "dart_financials",
  retrieved_at: "2026-03-20T06:00:00Z", published_at: "2026-03-18", url: "https://opendart.fss.or.kr/dsaf001/main.do?rcpNo=20260318990221",
  excerpt: "", excerpt_truncated: false, content_sha256: "c".repeat(64), warnings: [],
};
const MARGIN: ChatEvidenceRef = { ...DART, id: "ev_margin", title: "Margin", kind: "derived", provider: undefined, url: undefined, tool_name: "calculate", calc_operation: "margin", published_at: undefined };

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

  it("names the source of a citation target a note carries in an export, never its raw id", () => {
    const cited: AssetsGrid = { ...grid, notes: [`Revenue from #evidence-${DART.id}; margin [as filed](#evidence-${MARGIN.id}); old #evidence-ev_gone.`],
      cells: [grid.cells[0]!, [{ empty: "not_reported", note: `Separate only (#evidence-${DART.id}).` }, grid.cells[1]![1]!], grid.cells[2]!] };
    const markdown = gridMarkdown("Dividend payers", cited, [DART, MARGIN]);
    expect(markdown).toContain("Revenue from OpenDART; margin as filed; old source.");
    expect(markdown).not.toContain("#evidence-");
    const csv = gridCsv(cited, [DART, MARGIN]);
    expect(csv).toContain("Separate only (OpenDART).");
    expect(csv).not.toContain("#evidence-");
  });

  it("rounds a money value's long decimal tail to two places and leaves every other value as written", () => {
    // KRW in 조, 억 and 만.
    expect(shownAmount("₩32.765719조")).toBe("₩32.77조");
    expect(shownAmount("-₩7.730313조")).toBe("-₩7.73조");
    expect(shownAmount("₩66.192960조")).toBe("₩66.19조");
    expect(shownAmount("4,523.4567억 원")).toBe("4,523.46억 원");
    expect(shownAmount("1,234.999만 원")).toBe("1,235만 원");
    expect(shownAmount("12.3456만 원")).toBe("12.35만 원");
    // Already sensible, or several numbers: as written.
    expect(shownAmount("₩452.3bn")).toBe("₩452.3bn");
    expect(shownAmount("4,523억 원")).toBe("4,523억 원");
    expect(shownAmount("1조 2,345억 원")).toBe("1조 2,345억 원");
    expect(shownAmount("₩32.77조")).toBe("₩32.77조");
    // USD and other currencies.
    expect(shownAmount("$394.3281bn")).toBe("$394.33bn");
    expect(shownAmount("$1.20bn")).toBe("$1.20bn");
    expect(shownAmount("USD 1,234.5678")).toBe("USD 1,234.57");
    expect(shownAmount("€12.5m")).toBe("€12.5m");
    expect(shownAmount("¥1,050.000bn")).toBe("¥1,050bn");
    // Not money: a percent or a multiple keeps the model's digits.
    expect(shownAmount("8.4231%")).toBe("8.4231%");
    expect(shownAmount("1.2345×")).toBe("1.2345×");
  });

  it("shows a rounded tail in the cell, the copy and the Markdown table, and keeps the exact value in the chip", () => {
    const exact = { ...data("₩32.765719조", "matched"), exact: "32765719000000" };
    const cell = { value: "₩32.765719조", evidence_id: DART.id, check: exact, sort: 32.765719e12 };
    expect(shownCheck(cell)).toEqual({ ...exact, text: "₩32.77조", status: "rounded" });
    const plain = grid.cells[0]![0]! as Extract<AssetsGrid["cells"][number][number], { value: string }>;
    expect(shownCheck(plain)).toBe(plain.check);
    const long: AssetsGrid = { ...grid, cells: [[cell, grid.cells[0]![1]!], grid.cells[1]!, grid.cells[2]!] };
    expect(gridTsv(long).split("\n")[1]).toBe("Solsup Foods\t₩32.77조\t8.42%");
    expect(gridMarkdown("Margins", long, [DART, MARGIN])).toContain("| Solsup Foods | ₩32.77조[^1] | 8.42%[^2] |");
  });

  it("shows the values it rounds in one column with one number of decimals, and every written value as written", () => {
    const cell = (value: string, status: NumberCheck["status"] = "matched") => ({ value, evidence_id: DART.id, check: { ...data(value, status), exact: value }, sort: 0 });
    const column = (values: string[]): AssetsGrid => ({ ...grid, rows: values.map((_, index) => ({ symbol: `99000${index}`, market: "KRX", label: `Row ${index}` })),
      columns: [grid.columns[0]!], cells: values.map((value) => [cell(value)]) });
    // KRW: "₩43.600123조" rounds to 43.6 but its neighbour needed two decimals, so it keeps "43.60".
    const krw = column(["₩47.214321조", "₩43.600123조", "₩12.30조"]);
    expect(gridDecimals(krw)).toEqual([2]);
    expect(gridTsv(krw).split("\n").slice(1).map((line) => line.split("\t")[1])).toEqual(["₩47.21조", "₩43.60조", "₩12.30조"]);
    expect(gridMarkdown("Revenue", krw, [DART])).toContain("| Row 1 | ₩43.60조[^2] |");
    // The chip says the shown "₩43.60조" was rounded and keeps the exact source value; a written value is unchanged.
    expect(shownCheck(krw.cells[1]![0] as never, 2)).toMatchObject({ text: "₩43.60조", status: "rounded", exact: "₩43.600123조" });
    expect(shownCheck(krw.cells[2]![0] as never, 2)).toBe((krw.cells[2]![0] as { check: NumberCheck }).check);
    // A value the model wrote with fewer decimals stays as written even when its neighbours show two.
    expect(shownAmount("₩452.3bn", 2)).toBe("₩452.3bn");
    expect(shownAmount("4,523억 원", 2)).toBe("4,523억 원");
    // USD, and a column whose rounded values all need one decimal or none: no padding beyond what they need.
    expect(gridTsv(column(["$394.3281bn", "-$12.6004bn"])).split("\n").slice(1).map((line) => line.split("\t")[1])).toEqual(["$394.33bn", "-$12.60bn"]);
    expect(gridDecimals(column(["$1.2004bn", "$3.0001bn"]))).toEqual([1]);
    expect(gridTsv(column(["$1.2004bn", "$3.0001bn"])).split("\n").slice(1).map((line) => line.split("\t")[1])).toEqual(["$1.2bn", "$3.0bn"]);
    // Not money: percents and multiples neither count nor change.
    expect(gridDecimals(column(["8.4231%", "1.2345×"]))).toEqual([0]);
  });

  it("names the currency a printed amount uses", () => {
    expect(currencyOf("₩452.3bn")).toBe("KRW");
    expect(currencyOf("4,523억 원")).toBe("KRW");
    expect(currencyOf("$1.2bn")).toBe("USD");
    expect(currencyOf("USD 1.2bn")).toBe("USD");
    expect(currencyOf("1.2×")).toBe("");
  });
});
