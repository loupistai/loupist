import { fireEvent, render, screen, waitFor, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ChartDataset, ChartDocument } from "../../types";

const { instance, init, dispose, registerOverlay, registerIndicator } = vi.hoisted(() => {
  const instance = {
    setDataLoader: vi.fn(), setSymbol: vi.fn(), setPeriod: vi.fn(), setTimezone: vi.fn(), setStyles: vi.fn(), setLocale: vi.fn(),
    createIndicator: vi.fn(), createOverlay: vi.fn().mockReturnValue("agent-drawing"), removeOverlay: vi.fn(), getOverlays: vi.fn(() => []),
    overrideOverlay: vi.fn(), zoomAtCoordinate: vi.fn(), scrollToRealTime: vi.fn(), resize: vi.fn(), setPaneOptions: vi.fn(), overrideYAxis: vi.fn(),
  };
  return { instance, init: vi.fn(() => instance), dispose: vi.fn(), registerOverlay: vi.fn(), registerIndicator: vi.fn() };
});

vi.mock("klinecharts", () => ({ init, dispose, registerOverlay, registerIndicator, registerLocale: vi.fn() }));
vi.mock("@klinecharts/extension", () => ({ rect: { name: "rect" }, arrow: { name: "arrow" }, measure: { name: "measure" }, fibonacciExtension: { name: "fibonacciExtension" } }));

import FinancialChart from "./FinancialChart";

const dataset: ChartDataset = {
  id: "dataset-1",
  columns: (["date", "open", "high", "low", "close"] as const).map((id) => ({ id, label: id, kind: id === "date" ? "date" as const : "number" as const })),
  rows: [["2026-08-01", 100, 105, 98, 103], ["2026-08-02", 103, 108, 101, 106]],
  lineage: { kind: "agent_authored", upstream_receipts: [] },
  digest: "a".repeat(64),
};

const chart: ChartDocument = {
  dataset_id: dataset.id,
  dataset_digest: dataset.digest,
  view: { kind: "candlestick", symbol: "TEST", interval: "1d", time: "date", open: "open", high: "high", low: "low", close: "close", volume: null, turnover: null, price_precision: null },
  studies: [{ module_id: "SAR", calc_params: [] }],
  drawings: [{ kind: "horizontal_line", points: [{ timestamp: "2026-08-01", value: 101 }], label: "support", color: null, line_width: null, line_style: null }],
  custom_series: [],
  note: null,
};

it("keeps original drawings locked until the user enables editing", async () => {
  render(<FinancialChart chart={chart} dataset={dataset} theme="light" />);

  await waitFor(() => expect(instance.createOverlay).toHaveBeenCalledWith(expect.objectContaining({ name: "horizontalStraightLine", lock: true })));
  expect(instance.createOverlay).toHaveBeenCalledWith(expect.objectContaining({ name: "simpleAnnotation", lock: true, extendData: "support" }));
  expect(instance.createIndicator).toHaveBeenCalledWith({ name: "SAR", paneId: "candle_pane" }, true);
  // Drawing tools sit behind "Draw" so the toolbar stays one row.
  const user = userEvent.setup();
  await user.click(screen.getByRole("button", { name: "Draw" }));
  await user.click(screen.getByRole("menuitem", { name: "Edit drawings" }));
  expect(instance.overrideOverlay).toHaveBeenCalledWith({ lock: false });
  await user.click(screen.getByRole("button", { name: "Draw" }));
  expect(screen.getByRole("menuitem", { name: "Stop editing drawings" })).toBeInTheDocument();
  await user.click(screen.getByRole("menuitem", { name: "Trend" }));
  expect(instance.createOverlay).toHaveBeenCalledWith(expect.objectContaining({ name: "segment", lock: false }));
});

it("draws saved custom series using exact timestamp-bound data and named panes", async () => {
  const withSeries = { ...chart, custom_series: [{ id: "equity", label: "Equity", column: "equity", pane: "equity", color: "#123456" }] };
  const input = { ...dataset, columns: [...dataset.columns, { id: "equity", label: "Equity", kind: "number" as const }], rows: [["2026-08-01", 100, 105, 98, 103, null], ["2026-08-02", 103, 108, 101, 106, 1009]] };
  render(<FinancialChart chart={withSeries} dataset={input} theme="light" />);
  expect(instance.createIndicator).toHaveBeenCalledWith(expect.objectContaining({ paneId: "equity", shortName: "Equity", extendData: { [Date.parse("2026-08-02")]: 1009 } }), true);
  const definition = registerIndicator.mock.calls[0][0];
  expect(definition.calc([{ timestamp: Date.parse("2026-08-01") }, { timestamp: Date.parse("2026-08-02") }], { extendData: { [Date.parse("2026-08-02")]: 1009 } })).toEqual([{ value: undefined }, { value: 1009 }]);
});

it("selects a concrete range without a model call", () => {
  const onAskRange = vi.fn();
  render(<FinancialChart chart={chart} dataset={dataset} theme="light" onAskRange={onAskRange} />);
  fireEvent.change(screen.getByLabelText("Range start"), { target: { value: "1" } });
  fireEvent.click(screen.getByRole("button", { name: "Ask about range" }));
  expect(onAskRange).toHaveBeenCalledWith({ start: "2026-08-02", end: "2026-08-02" });
});

it("shows the chart alone in an answer card, with drawing and range tools left to the panel", async () => {
  render(<FinancialChart chart={{ ...chart, studies: [...chart.studies, { module_id: "VOL", calc_params: [] }] }} dataset={dataset} theme="light" onAskRange={vi.fn()} compact />);
  // Volume keeps the library's calculation but takes localized legend titles.
  await waitFor(() => expect(instance.createIndicator).toHaveBeenCalledWith(expect.objectContaining({ name: "VOL", paneId: "study-VOL", shortName: "Volume" }), false));
  const volume = instance.createIndicator.mock.calls.find(([value]) => (value as { name: string }).name === "VOL")![0] as { figures: Array<{ key: string; title: string }> };
  expect(volume.figures.map((figure) => figure.title)).toEqual(["MA5: ", "MA10: ", "MA20: ", "Volume: "]);
  expect(instance.setPaneOptions).toHaveBeenCalledWith({ id: "study-VOL", height: 48 });
  expect(instance.createIndicator).toHaveBeenCalledWith({ name: "SAR", paneId: "candle_pane" }, true);
  expect(screen.getByLabelText("Interactive financial chart")).toBeInTheDocument();
  expect(screen.queryByRole("toolbar")).toBeNull();
  expect(screen.queryByRole("button", { name: "Ask about range" })).toBeNull();
  // Moving averages have no value for the first bars: study values show only under the pointer, and a gap
  // reads as a dash instead of the library's "n/a" (B75).
  const styles = (init.mock.calls.at(-1) as unknown as [HTMLElement, { styles: { indicator: { tooltip: { showRule: string; legend: { defaultValue: string } } }; candle: { tooltip: { legend: { defaultValue: string } } } } }])[1].styles;
  expect(styles.indicator.tooltip).toMatchObject({ showRule: "follow_cross", legend: { defaultValue: "–" } });
  expect(styles.candle.tooltip.legend.defaultValue).toBe("–");
});

it("keeps price axis labels out from under the last-price tag", () => {
  render(<FinancialChart chart={chart} dataset={dataset} theme="light" />);
  const { createTicks } = instance.overrideYAxis.mock.calls.at(-1)![0] as { createTicks: (params: { defaultTicks: { value: string; text: string; coord: number }[] }) => { text: string }[] };
  // A short pane puts the library's ticks 13 px apart; the last close is 106.00, which the tag covers.
  const ticks = ([["108.00", 10], ["107.00", 23], ["106.00", 36], ["105.00", 49], ["104.00", 62]] as const).map(([value, coord]) => ({ value, text: value, coord }));
  expect(createTicks({ defaultTicks: ticks }).map((tick) => tick.text)).toEqual(["108.00", "104.00"]);
});

it("gives a panel chart's study panes a quarter of its height, so a short dock keeps room for the bars", () => {
  const height = vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(320);
  render(<FinancialChart chart={{ ...chart, studies: [{ module_id: "VOL", calc_params: [] }] }} dataset={dataset} theme="light" />);
  expect(instance.setPaneOptions).toHaveBeenLastCalledWith({ id: "study-VOL", height: 80 });
  height.mockRestore();
});

it("undoes a completed local drawing", async () => {
  const user = userEvent.setup();
  render(<FinancialChart chart={chart} dataset={dataset} theme="light" />);
  await user.click(screen.getByRole("button", { name: "Draw" }));
  await user.click(screen.getByRole("menuitem", { name: "Trend" }));
  const overlay = instance.createOverlay.mock.calls.at(-1)![0];
  act(() => overlay.onDrawEnd());
  await user.click(screen.getByRole("button", { name: "Draw" }));
  await user.click(screen.getByRole("menuitem", { name: "Undo drawing" }));
  expect(instance.removeOverlay).toHaveBeenCalledWith();
});

it("updates candle and change-marker colors from locale tokens without losing the chart", async () => {
  const root = document.documentElement;
  root.style.setProperty("--gg-color-price-up", "#17794a");
  root.style.setProperty("--gg-color-price-down", "#c2382f");
  const view = render(<FinancialChart chart={chart} dataset={dataset} theme="light" />);
  const initializations = init.mock.calls.length;
  expect(instance.setStyles).toHaveBeenLastCalledWith(expect.objectContaining({ candle: expect.objectContaining({ bar: expect.objectContaining({ upColor: "#17794a", downColor: "#c2382f" }) }) }));
  await act(async () => {
    root.style.setProperty("--gg-color-price-up", "#c2382f");
    root.style.setProperty("--gg-color-price-down", "#2d6aa8");
    root.dataset.priceColors = "red-up";
  });
  await waitFor(() => expect(instance.setStyles).toHaveBeenLastCalledWith(expect.objectContaining({ candle: expect.objectContaining({ bar: expect.objectContaining({ upColor: "#c2382f", downColor: "#2d6aa8" }) }) })));
  expect(init.mock.calls.length).toBe(initializations);
  view.unmount();
  root.style.removeProperty("--gg-color-price-up");
  root.style.removeProperty("--gg-color-price-down");
  delete root.dataset.priceColors;
});
