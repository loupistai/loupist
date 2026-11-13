import { compile, type TopLevelSpec } from "vega-lite";
import { parse, View } from "vega";
import i18n from "@/i18n";
import type { ChartColumn, ChartDocument } from "../../types";
import { assembleAnalyticSpec } from "./analyticAdapter";

type AnalyticView = Exclude<ChartDocument["view"], { kind: "candlestick" }>;
type Drawn = { ticks: Array<Array<{ value: unknown; text: string }>>; titles: string[]; marks: string[]; bars: number[]; tooltip?: Record<string, string> };

/** Draws a spec headlessly and reads back what the reader sees: each axis's labels, axis titles, printed values and a mark's tooltip. */
async function draw(spec: Record<string, unknown>): Promise<Drawn> {
  const view = new View(parse(compile(spec as unknown as TopLevelSpec).spec), { renderer: "none" });
  try {
    await view.runAsync();
    const drawn: Drawn = { ticks: [], titles: [], marks: [], bars: [] };
    type Node = { role?: string; marktype?: string; items?: Array<Node & { datum?: { value?: unknown }; text?: string; x?: number; tooltip?: Record<string, string> }> };
    const visit = (node: Node | undefined) => {
      if (!node || typeof node !== "object") return;
      if (node.role === "axis-label") drawn.ticks.push((node.items ?? []).map((item) => ({ value: item.datum?.value, text: item.text ?? "" })));
      else if (node.role === "axis-title") drawn.titles.push(...(node.items ?? []).map((item) => item.text ?? ""));
      else if (node.marktype === "text" && node.role === "mark") drawn.marks.push(...(node.items ?? []).map((item) => item.text ?? "").filter(Boolean));
      if (node.marktype === "rect" && node.role === "mark") drawn.bars.push(...(node.items ?? []).map((item) => item.x ?? 0));
      if (node.marktype && node.marktype !== "group" && node.marktype !== "text" && !node.role?.startsWith("axis")) drawn.tooltip ??= node.items?.find((item) => item.tooltip)?.tooltip;
      for (const child of node.items ?? []) visit(child);
    };
    visit((view.scenegraph() as unknown as { root: Node }).root);
    return drawn;
  } finally {
    view.finalize();
  }
}

const chart = (view: Partial<AnalyticView>, rows: Array<Record<string, unknown>>, columns: ChartColumn[] = []) => draw(assembleAnalyticSpec({
  kind: "bar", x: "year", y: ["revenue"], color: null, title: null, subtitle: null, semantic_types: {}, ...view,
} as AnalyticView, rows, "light", 520, 360, columns));
const texts = (ticks: Drawn["ticks"][number]) => ticks.map((tick) => tick.text);

afterEach(async () => { await i18n.changeLanguage("en"); });

it.each([
  ["0.0000001", "0.0000002"],
  ["-0.00000025", "0.0000004"],
  ["2.5", "7.5"],
])("renders distinct quantitative ticks for decimal values %s and %s", async (first, second) => {
  const { ticks } = await chart({ kind: "line", x: "period", y: ["amount"], semantic_types: { period: "Category", amount: "Number" } },
    [{ period: "earlier", amount: first }, { period: "later", amount: second }]);
  const values = ticks.flat().filter((tick): tick is { value: number; text: string } => typeof tick.value === "number");
  expect(values.length).toBeGreaterThan(2);
  expect(new Set(values.map((tick) => tick.text)).size).toBe(values.length);
  for (const tick of values) {
    const displayed = Number(tick.text.replaceAll(",", "").replace("−", "-"));
    expect(Number.isFinite(displayed)).toBe(true);
    if (tick.value !== 0) expect(Math.abs((displayed - tick.value) / tick.value)).toBeLessThan(1e-8);
    else expect(displayed).toBe(0);
  }
});

// A company's revenue by year, as a model publishes it: years as numbers, no semantic types, won in the column id.
const revenue = [{ year: 2021, revenue_krw: 279.6e12 }, { year: 2022, revenue_krw: 302.2e12 }, { year: 2023, revenue_krw: 258.9e12 }, { year: 2024, revenue_krw: 300.9e12 }];
const revenueColumns = (label: string): ChartColumn[] => [{ id: "year", label: "", kind: "number" }, { id: "revenue_krw", label, kind: "number" }];

it.each([
  ["ko", "매출", ["0", "50조", "100조", "150조", "200조", "250조", "300조", "350조"], "매출 (원)", "302조 2,000억 원", "302.2조"],
  ["en", "Revenue", ["0", "50tn", "100tn", "150tn", "200tn", "250tn", "300tn", "350tn"], "Revenue (KRW)", "₩302,200,000,000,000", "302.2tn"],
])("abbreviates a %s revenue axis, names the currency once, and keeps years as years", async (language, label, yTicks, title, exact, printed) => {
  await i18n.changeLanguage(language);
  for (const kind of ["bar", "line"] as const) {
    const drawn = await chart({ kind, y: ["revenue_krw"] }, revenue, revenueColumns(label));
    const [x, y] = drawn.ticks;
    expect(texts(x!)).toEqual(["2021", "2022", "2023", "2024"]);
    expect(texts(y!)).toEqual(yTicks);
    expect(drawn.titles).toContain(title);
    // One bar per year, side by side: a year is a category, not a quantity on a zero-based scale.
    if (kind === "bar") expect(drawn.marks).toContain(printed);
    if (kind === "bar") expect(new Set(drawn.bars.map(Math.round)).size).toBe(4);
    // The exact stored value stays one hover away.
    const tooltip = (await chart({ kind, y: ["revenue_krw"] }, [revenue[1]!], revenueColumns(label))).tooltip;
    expect(Object.values(tooltip ?? {}).map(String)).toEqual(["2022", exact]);
  }
});

it("abbreviates US dollars and negative values, with ticks that read apart", async () => {
  const rows = [{ year: 2022, net_income_usd: -1.2e9 }, { year: 2023, net_income_usd: 0.4e9 }, { year: 2024, net_income_usd: 1.35e9 }];
  const { ticks, titles } = await chart({ kind: "bar", y: ["net_income_usd"] }, rows, [{ id: "year", label: "Year", kind: "number" }, { id: "net_income_usd", label: "Net income", kind: "number" }]);
  const y = texts(ticks[1]!);
  expect(y).toContain("0");
  expect(y.some((text) => /^-\d.*bn$/u.test(text))).toBe(true);
  expect(y.every((text) => text === "0" || /bn$/u.test(text))).toBe(true);
  expect(new Set(y).size).toBe(y.length);
  expect(titles).toContain("Net income (USD)");
});

it.each(["ko", "en"])("keeps prices, ratios and percentages in plain digits with their decimals (%s)", async (language) => {
  await i18n.changeLanguage(language);
  const prices = await chart({ kind: "line", x: "day", y: ["close"], semantic_types: { day: "Category", close: "Price" } },
    [{ day: "Mon", close: 71_000.5 }, { day: "Tue", close: 73_400 }]);
  expect(texts(prices.ticks[1]!).every((text) => /^[\d,]+$/u.test(text))).toBe(true);
  expect(Object.values((await chart({ kind: "line", x: "day", y: ["close"], semantic_types: { close: "Price" } }, [{ day: "Mon", close: 71_000.5 }])).tooltip ?? {})).toContain("71,000.5");
  const margin = await chart({ kind: "bar", x: "year", y: ["margin"], semantic_types: { margin: "Percentage" } },
    [{ year: 2023, margin: 0.123 }, { year: 2024, margin: -0.05 }]);
  expect(texts(margin.ticks[1]!)).toContain("0.05");
  expect(texts(margin.ticks[1]!)).toContain("-0.05");
  expect(margin.marks).toEqual(expect.arrayContaining(["0.123", "-0.05"]));
});

it.each([
  ["ko", ["2024Q3", "2024Q4", "FY2025"], ["1월 31일", "2월 29일", "3월 31일"]],
  ["en", ["2024Q3", "2024Q4", "FY2025"], ["Jan 31", "Feb 29", "Mar 31"]],
])("prints fiscal periods as given and dates in the app's format (%s)", async (language, periods, dates) => {
  await i18n.changeLanguage(language);
  const quarters = await chart({ x: "period", y: ["sales"], semantic_types: { period: "Quarter" } },
    [{ period: "2024Q3", sales: 4.1e12 }, { period: "2024Q4", sales: 4.4e12 }, { period: "FY2025", sales: 17.2e12 }]);
  expect(texts(quarters.ticks[0]!)).toEqual(periods);
  const daily = await chart({ kind: "line", x: "date", y: ["close"], semantic_types: { date: "Date", close: "Price" } },
    [{ date: "2024-01-31", close: 10 }, { date: "2024-02-29", close: 11 }, { date: "2024-03-31", close: 12 }]);
  expect(texts(daily.ticks[0]!)).toEqual(dates);
  // Identifiers stay as written, even when they look like numbers.
  const codes = await chart({ x: "code", y: ["sales"], semantic_types: { code: "ID" } }, [{ code: 5930, sales: 1 }, { code: 660, sales: 2 }]);
  expect(texts(codes.ticks[0]!).sort()).toEqual(["5930", "660"]);
});
