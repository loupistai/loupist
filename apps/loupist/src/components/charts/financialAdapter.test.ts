import { drawingOverlays, indicatorConfig, periodFor } from "./financialAdapter";

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
  expect(drawingOverlays({ kind: "annotation", points: [{ timestamp: "2026-08-01", value: 100 }], label: "earnings" })).toEqual([
    expect.objectContaining({ name: "simpleAnnotation", lock: true, extendData: "earnings" }),
  ]);
});

it("computes measure labels from the two points", () => {
  const [overlay] = drawingOverlays({ kind: "measure", points: [{ timestamp: "2026-08-01", value: 100 }, { timestamp: "2026-08-02", value: 110 }] });
  expect((overlay!.extendData as (points: unknown) => string[])(overlay!.points)).toEqual(["+10.00", "+10.00%", "1d"]);
});
