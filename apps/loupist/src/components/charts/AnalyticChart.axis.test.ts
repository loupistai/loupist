import { compile, type TopLevelSpec } from "vega-lite";
import { parse, View } from "vega";
import i18n from "@/i18n";
import type { ChartColumn, ChartDocument } from "../../types";
import { assembleAnalyticSpec, describeAnalyticAxes } from "./analyticAdapter";

type AnalyticView = Exclude<ChartDocument["view"], { kind: "candlestick" }>;
type Bar = { x: number; y2: number; datum?: Record<string, unknown> };
type Drawn = {
  /** Each axis's shown labels, with the horizontal extent Vega gave each one. */
  ticks: Array<Array<{ value: unknown; text: string; x1: number; x2: number }>>;
  titles: string[];
  marks: string[];
  bars: Bar[];
  areas: number[];
  lines: number[];
  yDomain: number[];
  tooltip?: Record<string, string>;
  descriptions: string[];
};

/** Draws a spec headlessly and reads back what the reader sees: each axis's labels, axis titles, printed values and a mark's tooltip. */
async function draw(spec: Record<string, unknown>): Promise<Drawn> {
  const view = new View(parse(describeAnalyticAxes(compile(spec as unknown as TopLevelSpec).spec)), { renderer: "none" });
  try {
    await view.runAsync();
    const drawn: Drawn = { ticks: [], titles: [], marks: [], bars: [], areas: [], lines: [], descriptions: [], yDomain: view.scale("y").domain() };
    type Node = { role?: string; marktype?: string; items?: Array<Node & {
      datum?: Record<string, unknown>; text?: string; x?: number; y2?: number; opacity?: number; tooltip?: Record<string, string>; description?: string;
      bounds?: { x1: number; x2: number };
    }> };
    const visit = (node: Node | undefined) => {
      if (!node || typeof node !== "object") return;
      if (node.role === "axis-label") drawn.ticks.push((node.items ?? []).filter((item) => item.opacity !== 0).map((item) => ({
        value: item.datum?.value, text: item.text ?? "", x1: item.bounds?.x1 ?? 0, x2: item.bounds?.x2 ?? 0,
      })));
      else if (node.role === "axis-title") drawn.titles.push(...(node.items ?? []).map((item) => item.text ?? ""));
      else if (node.marktype === "text" && node.role === "mark") drawn.marks.push(...(node.items ?? []).map((item) => item.text ?? "").filter(Boolean));
      if (node.marktype === "rect" && node.role === "mark") drawn.bars.push(...(node.items ?? []).map((item) => ({
        x: item.x ?? 0, y2: item.y2 ?? 0, datum: item.datum,
      })));
      if (node.marktype === "area") drawn.areas.push(node.items?.[0]?.opacity ?? 1);
      if (node.marktype === "line") drawn.lines.push(node.items?.[0]?.opacity ?? 1);
      if (node.marktype && node.marktype !== "group" && node.marktype !== "text" && !node.role?.startsWith("axis")) drawn.tooltip ??= node.items?.find((item) => item.tooltip)?.tooltip;
      if (node.role === "mark") drawn.descriptions.push(...(node.items ?? []).flatMap(item => item.description ? [item.description] : []));
      for (const child of node.items ?? []) visit(child);
    };
    visit((view.scenegraph() as unknown as { root: Node }).root);
    return drawn;
  } finally {
    view.finalize();
  }
}

const chart = (view: Partial<AnalyticView>, rows: Array<Record<string, unknown>>, columns: ChartColumn[] = [], width = 520) => draw(assembleAnalyticSpec({
  kind: "bar", x: "year", y: ["revenue"], color: null, title: null, subtitle: null, semantic_types: {}, ...view,
} as AnalyticView, rows, "light", width, 360, columns));
const texts = (ticks: Drawn["ticks"][number]) => ticks.map((tick) => tick.text);

afterEach(async () => { await i18n.changeLanguage("en"); });

it.each([
  ["en", "Month", "Shipments", "Jan", "Month axis", "Shipments axis, from 0 to ", "Bar"],
  ["ko", "월", "출하량", "1월", "월 축", "출하량 축, 0부터 ", "막대"],
])("writes %s descriptions into the accessible chart SVG", async (language, x, y, first, xAxis, yAxis, role) => {
  await i18n.changeLanguage(language);
  const spec = assembleAnalyticSpec({
    kind: "bar", x: "month", y: ["shipments"], color: null, title: null, subtitle: null, semantic_types: {},
  }, [{ month: first, shipments: 18 }, { month: "02", shipments: 25 }], "light", 520, 360,
    [{ id: "month", label: x, kind: "string" }, { id: "shipments", label: y, kind: "number" }]);
  const view = new View(parse(describeAnalyticAxes(compile(spec as unknown as TopLevelSpec).spec)), { renderer: "none" });
  try {
    const svg = new DOMParser().parseFromString(await view.toSVG(), "image/svg+xml");
    const labels = [...svg.querySelectorAll("[aria-label]")].map(node => node.getAttribute("aria-label"));
    expect(labels).toContain(xAxis);
    expect(labels.some(label => label?.startsWith(yAxis) && label.includes(String(view.scale("y").domain().at(-1))))).toBe(true);
    const marks = [...svg.querySelectorAll(`[aria-roledescription="${role}"]`)].map(node => node.getAttribute("aria-label"));
    expect(marks).toContain(`${x}: ${first}, ${y}: 18`);
    expect(labels.join(" ")).not.toMatch(/[XY]-axis for a /u);
  } finally {
    view.finalize();
  }
});

it.each([
  ["en", "Revenue (USD millions)", "Margin (%)", "Revenue (USD)", "$40"],
  ["ko", "매출 (백만 달러)", "이익률 (%)", "매출 (달러)", "40달러"],
])("uses the %s axis units in mark descriptions, value labels and tooltips", async (language, scaled, percent, unscaled, amount) => {
  await i18n.changeLanguage(language);
  const columns = (label: string): ChartColumn[] => [{ id: "year", label: "", kind: "number" }, { id: "revenue", label, kind: "number" }];
  const rows = [{ year: 2023, revenue: 40 }, { year: 2024, revenue: 48 }];
  for (const kind of ["bar", "line", "area", "scatter"] as const) {
    const result = await chart({ kind, semantic_types: { year: "Number" } }, rows, columns(scaled));
    expect(result.titles).toContain(scaled);
    expect(result.descriptions.some(description => description.includes(`${scaled}: 40`))).toBe(true);
    expect(result.descriptions.some(description => description.includes(amount))).toBe(false);
    expect(result.tooltip).toMatchObject({ [scaled]: "40" });
    if (kind === "bar") expect(result.marks).toEqual(expect.arrayContaining(["40", "48"]));
  }
  const margin = await chart({ semantic_types: { revenue: "Percentage" } }, rows, columns(percent));
  expect(margin.titles).toContain(percent);
  expect(margin.descriptions.some(description => description.includes(`${percent}: 40`))).toBe(true);
  expect(margin.tooltip).toMatchObject({ [percent]: "40" });
  const dollars = await chart({}, rows, columns(unscaled));
  expect(dollars.descriptions.some(description => description.includes(`${unscaled}: ${amount}`))).toBe(true);
  expect(dollars.tooltip).toMatchObject({ [unscaled]: amount });
});

const fiscalRevenue = [
  { company: "Apple", fy2023: 383_285, fy2024: 391_035 },
  { company: "Microsoft", fy2023: 211_915, fy2024: 245_122 },
  { company: "Alphabet", fy2023: 307_394, fy2024: 350_018 },
];
const revenueByYear = fiscalRevenue.flatMap(({ company, fy2023, fy2024 }) => [
  { company, year: "FY2023", revenue: fy2023 },
  { company, year: "FY2024", revenue: fy2024 },
]);

it("draws the saved C-008 comparison with six side-by-side bars on a single-value axis", async () => {
  const { bars, yDomain, marks, tooltip } = await chart({ x: "company", color: "year" }, revenueByYear, [], 1_000);
  expect(bars).toHaveLength(6);
  const groups = fiscalRevenue.map(({ company }) => bars.filter((bar) => bar.datum?.company === company));
  for (const group of groups) expect(new Set(group.map((bar) => bar.x)).size).toBe(2);
  expect(new Set(bars.map((bar) => bar.y2)).size).toBe(1);
  expect(yDomain[0]).toBe(0);
  expect(yDomain[1]).toBeGreaterThanOrEqual(391_035);
  expect(yDomain[1]).toBeLessThan(450_000);
  expect(marks.filter((label) => label === "391,035")).toHaveLength(1);
  expect(tooltip).toEqual({ Company: "Apple", Year: "FY2023", Revenue: "383,285" });
});

it("stacks the comparison only when the view explicitly requests it", async () => {
  const { bars, yDomain } = await chart({ x: "company", color: "year", stack: true }, revenueByYear);
  expect(bars).toHaveLength(6);
  expect(new Set(bars.map((bar) => bar.x)).size).toBe(3);
  expect(new Set(bars.map((bar) => bar.y2)).size).toBeGreaterThan(1);
  expect(yDomain[1]).toBeGreaterThanOrEqual(774_320);
});

it("groups separate y columns too", async () => {
  const { bars, yDomain } = await chart({ x: "company", y: ["fy2023", "fy2024"], stack: false }, fiscalRevenue);
  expect(new Set(bars.map((bar) => bar.x)).size).toBe(6);
  expect(yDomain[1]).toBeLessThan(450_000);
});

it("keeps date and value labels in the grouped-bar tooltip", async () => {
  const { ticks, tooltip } = await chart({ x: "date", color: "case", semantic_types: { date: "Date" } }, [
    { date: "2024-01-31", case: "Base", revenue: 40 },
    { date: "2024-01-31", case: "High", revenue: 60 },
  ]);
  expect(texts(ticks[0]!)).toEqual(["Jan 31"]);
  expect(tooltip).toEqual({ Date: "Jan 31", Case: "Base", Revenue: "40" });
});

it.each([12, 40])("keeps both bars in all %s groups when labels would collide", async (count) => {
  const rows = Array.from({ length: count }, (_, index) => ["FY2023", "FY2024"].map((year, offset) => ({
    company: `Company ${index + 1}`, year, revenue: 300_000 + offset * 70_000,
  }))).flat();
  const { bars, ticks, marks } = await chart({ x: "company", color: "year" }, rows);
  expect(new Set(bars.map((bar) => bar.x)).size).toBe(count * 2);
  expect(ticks[0]!.length).toBeGreaterThan(1);
  expect(ticks[0]!.length).toBeLessThan(count);
  expect(marks).toHaveLength(0);
});

it("overlays translucent areas with solid lines and keeps the single-value extent", async () => {
  const { areas, lines, yDomain } = await chart({ kind: "area", y: ["earlier", "later"] }, [
    { year: 2023, earlier: 80, later: 70 }, { year: 2024, earlier: 90, later: 100 },
  ]);
  expect(areas).toHaveLength(2);
  expect(areas.every((opacity) => opacity > 0 && opacity < 1)).toBe(true);
  expect(lines).toEqual([1, 1]);
  expect(yDomain).toEqual([0, 100]);
});

it.each([
  ["0.0000001", "0.0000002"],
  ["-0.00000025", "0.0000004"],
  ["2.5", "7.5"],
])("renders distinct quantitative ticks for decimal values %s and %s", async (first, second) => {
  const { ticks } = await chart({ kind: "line", x: "period", y: ["amount"], semantic_types: { period: "Category", amount: "Number" } },
    [{ period: "earlier", amount: first }, { period: "later", amount: second }]);
  const values = ticks.flat().filter((tick): tick is Drawn["ticks"][number][number] & { value: number } => typeof tick.value === "number");
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
    if (kind === "bar") expect(new Set(drawn.bars.map((bar) => Math.round(bar.x))).size).toBe(4);
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

it.each([
  // The side panel (460 px) and the answer column, in both languages.
  ["ko", 420, ["1월 1일", "4월 1일", "7월 1일", "10월 1일"]],
  ["en", 420, ["Jan 1", "Apr 1", "Jul 1", "Oct 1"]],
  ["ko", 900, ["12월 1일", "1월 1일", "2월 1일", "3월 1일", "4월 1일", "5월 1일", "6월 1일", "7월 1일", "8월 1일", "9월 1일", "10월 1일", "11월 1일"]],
])("gives a year of daily prices as many date labels as the width holds, none touching (%s, %s px)", async (language, width, labels) => {
  await i18n.changeLanguage(language);
  const rows: Array<Record<string, unknown>> = [];
  for (let day = Date.UTC(2025, 10, 24); day <= Date.UTC(2026, 10, 23); day += 86_400_000) {
    if ([0, 6].includes(new Date(day).getUTCDay())) continue;
    rows.push({ date: new Date(day).toISOString().slice(0, 10), close: 500_000 + rows.length * 3_000 });
  }
  const { ticks } = await chart({ kind: "line", x: "date", y: ["close"], semantic_types: { date: "Date", close: "Price" } }, rows,
    [{ id: "date", label: "Date", kind: "date" }, { id: "close", label: "Close", kind: "number" }] as ChartColumn[], width);
  expect(texts(ticks[0]!)).toEqual(labels);
  for (const [left, right] of ticks[0]!.slice(1).map((tick, index) => [ticks[0]![index]!, tick] as const)) expect(right.x1 - left.x2).toBeGreaterThanOrEqual(8);
});
