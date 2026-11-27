import { drawingOverlays, fittedRange, formatChartPrice, indicatorConfig, periodFor, ticksThatFit } from "./financialAdapter";

it("maps only explicit bounded intervals instead of silently changing them", () => {
  expect(periodFor("15m")).toEqual({ type: "minute", span: 15 });
  expect(periodFor("1wk")).toEqual({ type: "week", span: 1 });
  for (const interval of ["quarterly", "0d", "10000m"]) expect(() => periodFor(interval)).toThrow("Unsupported financial chart interval");
});

it("places price studies on the candle pane and keeps empty parameters on module defaults", () => {
  expect(indicatorConfig({ module_id: "VOL", calc_params: [] })).toEqual({ value: { name: "VOL" }, isStack: false });
  expect(indicatorConfig({ module_id: "EMA", calc_params: [10, 20] })).toEqual({ value: { name: "EMA", paneId: "candle_pane", calcParams: [10, 20] }, isStack: true });
  expect(indicatorConfig({ module_id: "DMI", calc_params: [14, 6] })).toEqual({ value: { name: "DMI", calcParams: [14, 6] }, isStack: false });
});

it("maps persisted agent drawings to locked overlays and labels them once", () => {
  const [segment, label] = drawingOverlays({
    kind: "segment", label: "breakout", color: "#2563EB", line_width: 2, line_style: "dashed",
    points: [{ timestamp: "2026-08-01", value: 100 }, { timestamp: 1_786_492_800, value: 110 }],
  });
  expect(segment).toEqual({
    name: "segment", lock: true, styles: { line: { color: "#2563EB", size: 2, style: "dashed" } },
    points: [{ timestamp: Date.parse("2026-08-01"), value: 100 }, { timestamp: 1_786_492_800_000, value: 110 }],
  });
  expect(label).toEqual({ name: "simpleAnnotation", points: [{ timestamp: 1_786_492_800_000, value: 110 }], lock: true, extendData: "breakout" });
  expect(drawingOverlays({ kind: "annotation", points: [{ timestamp: "2026-08-01", value: 100 }], label: "earnings", color: null, line_width: null, line_style: null })).toEqual([
    expect.objectContaining({ name: "simpleAnnotation", lock: true, extendData: "earnings" }),
  ]);
});

it("formats a price with the chart's fraction digits", () => {
  expect(formatChartPrice(155.3800048828125, 2)).toBe("155.38");
  expect(formatChartPrice(155.3800048828125, 4)).toBe("155.3800");
  expect(formatChartPrice(2, 2)).toBe("2.00");
});

it.each([
  { width: 400, bars: 251, data: 251, to: 250, label: 72 },
  { width: 320, bars: 30, data: 251, to: 250, label: 64 },
  { width: 480, bars: 10, data: 251, to: 40, label: 80 },
])("fits every bar of the selected range and leaves room for the right label ($bars bars)", ({ width, bars, data, to, label }) => {
  const view = fittedRange(width, bars, data, to, label);
  expect(view).not.toBeNull();
  const gutter = label / 2 + 4;
  const space = view!.barSpace;
  const coord = width - (data + view!.offsetRight / space - to - 0.5) * space;
  expect(space * bars).toBeLessThanOrEqual(width + 0.01);
  expect(coord).toBeCloseTo(width - gutter, 5);
  expect(coord + label / 2).toBeLessThanOrEqual(width + 0.01);
});

it("keeps the rightmost axis label inside the plot", () => {
  const width = 400;
  const textWidth = (text: string) => text.length * 8;
  const ticks = [
    { coord: 40, value: 1, text: "Jan" },
    { coord: 200, value: 2, text: "Jun" },
    { coord: 398, value: 3, text: "Sep 2026" },
  ];
  const fitted = ticksThatFit(ticks, width, textWidth);
  const last = fitted.at(-1)!;
  const half = textWidth(last.text) / 2;
  expect(last.text).toBe("Sep 2026");
  expect(last.coord - half).toBeGreaterThanOrEqual(0);
  expect(last.coord + half).toBeLessThanOrEqual(width);
  expect(fitted.filter((tick) => tick.text === "Jan")).toHaveLength(1);
});

it("computes measure labels from the two points", () => {
  const [overlay] = drawingOverlays({ kind: "measure", points: [{ timestamp: "2026-08-01", value: 100 }, { timestamp: "2026-08-02", value: 110 }], color: null, line_width: null, line_style: null, label: null });
  expect((overlay!.extendData as (points: unknown) => string[])(overlay!.points)).toEqual(["+10.00", "+10.00%", "1d"]);
});
