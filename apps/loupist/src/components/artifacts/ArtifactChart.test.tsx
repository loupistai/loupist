import { render, screen } from "@testing-library/react";
import type { ChartDataset, ChartDocument } from "../../types";
import ArtifactChart from "./ArtifactChart";

vi.mock("../charts/FinancialChart", () => ({ default: () => <div>financial-engine</div> }));
vi.mock("../charts/AnalyticChart", () => ({ default: () => <div>analytic-engine</div> }));

const dataset: ChartDataset = {
  id: "dataset-1",
  columns: (["date", "open", "high", "low", "close"] as const).map((id) => ({ id, label: id, kind: id === "date" ? "date" as const : "number" as const })),
  rows: [["2026-08-01", 1, 3, 0, 2]],
  lineage: { kind: "agent_authored", upstream_receipts: [] },
  digest: "a".repeat(64),
};

const financial: ChartDocument = {
  dataset_id: dataset.id,
  dataset_digest: dataset.digest,
  view: { kind: "candlestick", symbol: "TEST", interval: "1d", time: "date", open: "open", high: "high", low: "low", close: "close", volume: null, turnover: null, price_precision: null },
  studies: [{ module_id: "EMA", calc_params: [20] }],
  drawings: [],
  custom_series: [],
  note: null,
};

it("routes financial documents to the KLine engine and analytic documents to the Flint engine", async () => {
  const view = render(<ArtifactChart chart={financial} dataset={dataset} theme="light" />);
  expect(screen.getByRole("figure", { name: "Financial chart" })).toHaveAccessibleDescription(expect.stringContaining("Latest close 2"));
  expect(await screen.findByText("financial-engine")).toBeVisible();
  view.unmount();
  const analytic: ChartDocument = { dataset_id: dataset.id, dataset_digest: dataset.digest, studies: [], drawings: [],
    view: { kind: "line", x: "date", y: ["close"], semantic_types: { date: "Date", close: "Price" }, title: "Close over time", color: null, subtitle: null }, custom_series: [], note: null };
  render(<ArtifactChart chart={analytic} dataset={dataset} theme="dark" />);
  expect(screen.getByText("Close over time")).toBeVisible();
  expect(await screen.findByText("analytic-engine")).toBeVisible();
});

it.each(["financial", "analytic", "unknown"])("rejects unsupported saved view %s before loading an engine", (kind) => {
  const chart = { ...financial, view: { kind } } as unknown as ChartDocument;
  render(<ArtifactChart chart={chart} dataset={dataset} theme="light" />);
  expect(screen.getByRole("alert")).toHaveTextContent(/unsupported/);
  expect(screen.queryByText("analytic-engine")).not.toBeInTheDocument();
  expect(screen.queryByText("financial-engine")).not.toBeInTheDocument();
});
