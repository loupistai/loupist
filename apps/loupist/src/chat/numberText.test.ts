import { afterEach, describe, expect, it } from "vitest";
import i18n from "@/i18n";
import type { ChatEvidenceRef, NumberCheck } from "../types";
import { evidenceFixture } from "../test/evidenceFixtures";
import { keepNumberParticles } from "./keepParticles";
import { calcView, dataRows, exactAmount, humanize, inputValue, plainLabel, printedValue, shownExactly } from "./numberText";
import { readNoun } from "./progressLabels";

const dart: ChatEvidenceRef = evidenceFixture({ id: "ev_dart", title: "Solsup Foods FY2025 statements", tool_name: "dart_financials", provider: "OpenDART", url: undefined,
  excerpt: JSON.stringify({ facts: [{ corp_name: "Solsup Foods", value: "452312000000", unit: "KRW" }, { corp_name: "Solsup Foods", value: "38104000000", unit: "KRW" }] }) });
const revenue: NumberCheck = { link_index: 1, text: "₩452.3bn", evidence_id: dart.id, kind: "data", status: "rounded", exact: "452312000000", pointer: "/facts/0/value",
  fields: [{ key: "corp_name", value: "Solsup Foods" }, { key: "canonical_concept", value: "revenue" }, { key: "period", value: "FY2025" }, { key: "value", value: "452312000000", matched: true }] };
/** What Rust ships with an input of a calculation whose `unit` was "KRW billions", and "KRW". */
const KRW_BILLIONS = { kind: "amount", currency: "KRW", scale: 9 } as const;
const KRW = { kind: "amount", currency: "KRW" } as const;
const margin: NumberCheck = { link_index: 0, text: "8.4%", evidence_id: "ev_calc", kind: "calculated", status: "matched",
  calc: { title: "Margin", formulas: ["Operating income / Revenue = 38.1 / 452.3 = 8.42%"], inputs: [
    { name: "profit", label: "Operating income", unit: KRW_BILLIONS, display: "38.1", evidence_id: dart.id, status: "rounded" },
    { name: "revenue", label: "Revenue", unit: KRW_BILLIONS, display: "452.3", evidence_id: dart.id, status: "rounded" }] } };
const growth: NumberCheck = { link_index: 2, text: "6.2%", evidence_id: "ev_growth", kind: "calculated", status: "matched",
  calc: { title: "Percent change", formulas: ["revenue FY2025 − revenue FY2024 = 452,312,000,000 − 425,900,000,000 = 26,412,000,000"], inputs: [
    { name: "from", label: "revenue FY2024", unit: KRW, display: "425,900,000,000", evidence_id: dart.id, status: "matched" },
    { name: "to", label: "revenue FY2025", unit: KRW, display: "452,312,000,000", evidence_id: dart.id, status: "matched" }] } };
const grid = (text: string, index: number): NumberCheck => ({ link_index: index, text, evidence_id: "ev_grid", kind: "calculated", status: "matched",
  calc: { title: "Reverse DCF grid", formulas: [], grid: [3, 3], inputs: [
    { name: "price", label: "price", unit: { kind: "price", currency: "KRW" }, display: "38,200" },
    { name: "shares", label: "shares", unit: { kind: "number" }, display: "34,560,000" },
    { name: "reference_growth_pct", label: "reference growth", unit: { kind: "percent" }, display: "5" }] } });
const checks = [margin, revenue, growth];

describe("number popover words", () => {
  afterEach(async () => { await i18n.changeLanguage("en"); });

  it("names a calculation's inputs by the label Rust shipped and prints them in its unit", async () => {
    const view = calcView(margin, checks);
    expect(view.title).toBe("Operating margin · FY2025");
    expect(view.inputs.map((input) => [input.label, input.value])).toEqual([["Operating income", "₩38.1bn"], ["Revenue", "₩452.3bn"]]);
    expect(view.formulas).toEqual(margin.calc!.formulas);
    await i18n.changeLanguage("ko");
    const ko = calcView(margin, checks);
    expect(ko.title).toBe("영업이익률 · FY2025");
    expect(ko.inputs.map((input) => [input.label, input.value])).toEqual([["영업이익", "381억 원"], ["매출", "4,523억 원"]]);
    expect(ko.formulas).toEqual(["영업이익 / 매출 = 38.1 / 452.3 = 8.42%"]);
  });

  it("guesses nothing the host did not say: no label is read from the formula, no currency from the evidence", () => {
    // The formula says "Operating income"; the input's own label decides what the popover calls it.
    const relabelled = { ...margin, calc: { ...margin.calc!, inputs: [{ ...margin.calc!.inputs[0]!, label: "EBIT" }, margin.calc!.inputs[1]!] } };
    expect(calcView(relabelled, checks).inputs[0]!.label).toBe("EBIT");
    // An amount the request gave no unit for prints as the number, even beside a won-denominated source.
    const bare = { ...margin, calc: { ...margin.calc!, inputs: [{ ...margin.calc!.inputs[1]!, unit: { kind: "amount" as const } }] } };
    expect(calcView(bare, checks).inputs[0]!.value).toBe("452.3");
    // An answer saved before inputs carried labels names them by their input.
    const older = { ...margin, calc: { ...margin.calc!, inputs: [{ name: "operating_cash_flow", display: "17.5" } as unknown as NonNullable<NumberCheck["calc"]>["inputs"][number]] } };
    expect(calcView(older, [older]).inputs[0]).toMatchObject({ label: "Operating cash flow", value: "17.5" });
  });

  it("prints each kind of unit Rust ships", async () => {
    expect(inputValue("2.5", { kind: "points" })).toBe("2.5pp");
    expect(inputValue("40", { kind: "basis_points" })).toBe("40bp");
    expect(inputValue("12.5", { kind: "multiple" })).toBe("12.5x");
    expect(inputValue("1,200", { kind: "number", text: "stores" })).toBe("1,200 stores");
    expect(inputValue("402", { kind: "amount", currency: "USD", scale: 6 })).toBe("$402m");
    expect(inputValue("4,523", { kind: "amount", scale: 8 })).toBe("452.3bn");
    expect(inputValue("8.4", { kind: "percent" })).toBe("8.4%");
    expect(inputValue("n/a", { kind: "percent" })).toBe("n/a");
    await i18n.changeLanguage("ko");
    expect(inputValue("2.5", { kind: "points" })).toBe("2.5%p");
    expect(inputValue("12.5", { kind: "multiple" })).toBe("12.5배");
    expect(inputValue("38,200", { kind: "price", currency: "KRW" })).toBe("38,200원");
  });

  it("keeps a label's period and spans two periods in the title", async () => {
    await i18n.changeLanguage("ko");
    const view = calcView(growth, checks);
    expect(view.title).toBe("변화율 · FY2024 → FY2025");
    expect(view.inputs.map((input) => [input.label, input.value])).toEqual([["매출 FY2024", "4,259억 원"], ["매출 FY2025", "4,523억 원"]]);
    expect(view.formulas[0]).toMatch(/^매출 FY2025 − 매출 FY2024 = 452,312,000,000/u);
  });

  it("shows a grid's range instead of one value, and prices, counts and percents as such", async () => {
    const cells = [grid("7.8%", 1), grid("4.1%", 0)];
    const view = calcView(cells[0]!, cells);
    expect(view.title).toBe("Value at different growth and discount rates");
    expect(view.value).toBe("4.1%–7.8%");
    expect(view.inputs.map((input) => [input.label, input.value])).toEqual([["Share price", "₩38,200"], ["Shares outstanding", "34,560,000"], ["Reference growth", "5%"]]);
    expect(calcView(cells[0]!, [cells[0]!]).value).toBeUndefined();
    await i18n.changeLanguage("ko");
    expect(calcView(cells[0]!, cells).inputs.map((input) => input.label)).toEqual(["주가", "발행주식 수", "기준 성장률"]);
  });

  it("words a data row: known fields by name, amounts with grouping and currency, no repeated company", async () => {
    const rows = dataRows(revenue, dart, checks, "Solsup Foods · FY2025");
    expect(rows.map((row) => [row.label, row.value])).toEqual([["Item", "Revenue"], ["Period", "FY2025"], ["Amount", "₩452,312,000,000"]]);
    expect(exactAmount(revenue, dart, checks)).toBe("₩452,312,000,000");
    await i18n.changeLanguage("ko");
    expect(dataRows(revenue, dart, checks, "Solsup Foods · FY2025").map((row) => [row.label, row.value])).toEqual([["항목", "매출"], ["기간", "FY2025"], ["금액", "4,523억 1,200만 원"]]);
    expect(dataRows({ ...revenue, fields: [{ key: "net_change_ratio", value: "0.5" }] }, dart, checks, "x")[0]!.label).toBe("Net change ratio");
  });

  it("humanizes unknown keys and leaves unknown labels as written", () => {
    expect(humanize("fs_div_code")).toBe("Fs div code");
    expect(plainLabel("adjusted EBIT")).toEqual({ text: "adjusted EBIT" });
  });

  it("names as-of reads as nouns, with their target", async () => {
    expect(readNoun("kis_read")).toBe("market data");
    expect(readNoun("krx_daily_quote", { target: "990221" })).toBe("daily prices for 990221");
    expect(readNoun("mcp__openbb__equity_profile")).toBe("company profile");
    expect(readNoun("custom_tool", { connection: "Broker feed" })).toBe("Broker feed");
    await i18n.changeLanguage("ko");
    expect(readNoun("krx_daily_quote", { target: "990221" })).toBe("990221 일별 시세");
    expect(readNoun("dart_company_lookup")).toBe("기업 조회");
  });

  it("keeps a Korean particle with the number link it follows", () => {
    const link = { type: "element" as const, tagName: "a", properties: { href: "#evidence-ev" }, children: [{ type: "text" as const, value: "8.4%" }] };
    const tree = { type: "root" as const, children: [{ type: "element" as const, tagName: "p", properties: {}, children: [{ type: "text" as const, value: "이익률은 " }, link, { type: "text" as const, value: "였고, 매출은" }] }] };
    keepNumberParticles()(tree);
    const paragraph = tree.children[0]!;
    expect(paragraph.children.map((child) => child.type === "text" ? child.value : `${child.tagName}.${String((child.properties as Record<string, unknown>).className ?? "")}`)).toEqual(["이익률은 ", "span.number-keep", ", 매출은"]);
    const group = paragraph.children[1] as { children: unknown[] };
    expect(group.children).toEqual([link, { type: "text", value: "였고" }]);
  });
});

describe("printed values", () => {
  it("reads scale words in English and Korean", () => {
    expect(printedValue("₩452.3bn")).toBe(452_300_000_000);
    expect(printedValue("$1.27B")).toBe(1_270_000_000);
    expect(printedValue("4,523억 원")).toBe(452_300_000_000);
    expect(printedValue("1조 2,345억 원")).toBe(1_234_500_000_000);
    expect(printedValue("8.42%")).toBe(8.42);
    expect(printedValue("452,300,000,000 KRW")).toBe(452_300_000_000);
    expect(printedValue("about 5 apples")).toBeUndefined();
  });

  it("does not call a number rounded when it shows the source value exactly", () => {
    expect(shownExactly({ text: "4,523억 원", exact: "452300000000" })).toBe(true);
    expect(shownExactly({ text: "₩452.3bn", exact: "452300000000" })).toBe(true);
    expect(shownExactly({ text: "₩452.3bn", exact: "452312000000" })).toBe(false);
  });
});
