import type { ChartDocument } from "../../types";
import { assembleAnalyticSpec } from "./analyticAdapter";

type AnalyticView = Exclude<ChartDocument["view"], { kind: "candlestick" }>;

const earningsView: AnalyticView = {
  kind: "line", x: "period", y: ["revenue_krw_trillion", "operating_profit_krw_trillion"], color: null, title: "연결 매출과 영업이익", subtitle: null,
  semantic_types: { period: "Category", revenue_krw_trillion: "Currency", operating_profit_krw_trillion: "Currency" },
};
const earningsRows = [
  { period: "2025 Q2", revenue_krw_trillion: 74.6, operating_profit_krw_trillion: 4.7 },
  { period: "2026 Q1", revenue_krw_trillion: 85, operating_profit_krw_trillion: 8 },
  { period: "2026 Q2", revenue_krw_trillion: 95, operating_profit_krw_trillion: 12 },
];
const values = (spec: ReturnType<typeof assembleAnalyticSpec>) => (spec.data as { values: Array<Record<string, unknown>> }).values;

it.each<AnalyticView["kind"]>(["line", "area", "bar", "scatter"])("fits a single-Y %s chart to the pane and keeps its title in the host header", (kind) => {
  const spec = assembleAnalyticSpec({ kind, x: "date", y: ["close"], color: null, semantic_types: { date: "Date", close: "Price" }, title: "Close over time", subtitle: null },
    [{ date: "2026-08-01", close: 2 }, { date: "2026-08-02", close: 3 }], "light", 400, 320);
  expect(spec).toMatchObject({ width: 400, height: 320, autosize: { type: "fit", contains: "padding" } });
  expect(JSON.stringify(spec)).toContain('"field":"close"');
  expect(JSON.stringify(spec)).not.toContain("Close over time");
});

it("keeps both earnings series visible from zero", () => {
  const spec = assembleAnalyticSpec(earningsView, earningsRows, "dark", 520, 880);
  expect(JSON.stringify(spec)).toContain('"zero":true');
  expect(values(spec).map((row) => row.__flint_series_key).sort()).toEqual([
    ...Array(3).fill("operating_profit_krw_trillion"), ...Array(3).fill("revenue_krw_trillion"),
  ]);
});

it("uses dataset labels on the axes and legend without merging equally named series", () => {
  const spec = assembleAnalyticSpec(earningsView, earningsRows, "dark", 380, 500, [
    { id: "period", label: "회계기간", kind: "string" },
    { id: "revenue_krw_trillion", label: "금액 (조 원)", kind: "number" },
    { id: "operating_profit_krw_trillion", label: "금액 (조 원)", kind: "number" },
  ]);
  const layers = spec.layer as Array<{ encoding: Record<string, Record<string, unknown>> }>;
  expect(layers[0]!.encoding.x).toHaveProperty("title", "회계기간");
  expect(layers[0]!.encoding.color!.legend).toMatchObject({ labelExpr: expect.stringContaining("금액 (조 원)") });
  expect(new Set(values(spec).map((row) => row.__flint_series_key)).size).toBe(2);
  expect(values(spec).map((row) => row.__flint_series_value)).toEqual([74.6, 4.7, 85, 8, 95, 12]);
});
