import userEvent from "@testing-library/user-event";
import { render, screen, within } from "@testing-library/react";
import type { ChartDataset, ChatArtifactView } from "../../types";
import { ArtifactContent } from "./ArtifactContent";

vi.mock("./ArtifactChart", () => ({
  default: () => <div>financial-engine</div>,
}));

const chartDataset: ChartDataset = {
  id: "dataset-1",
  columns: [{ id: "date", label: "Date", kind: "date" }, { id: "close", label: "Close", kind: "number" }],
  rows: [["2026-08-01", 2]],
  lineage: { kind: "agent_authored", upstream_receipts: [] },
  digest: "a".repeat(64),
};

const chartView: ChatArtifactView = {
  artifact: {
    id: "artifact-1",
    chat_session_id: "session-1",
    kind: "chart",
    title: "KOSPI chart",
    current_revision: 1,
    created_at_ms: 1_754_000_000_000,
    updated_at_ms: 1_754_000_000_000,
  },
  revision: {
    artifact_id: "artifact-1",
    revision: 1,
    payload: {
      kind: "chart",
      schema: "loupist-chart/1",
      chart: {
        dataset_id: chartDataset.id,
        dataset_digest: chartDataset.digest,
        view: { kind: "candlestick", symbol: "KOSPI", interval: "1d", time: "date", open: "close", high: "close", low: "close", close: "close" },
        studies: [],
        drawings: [],
      },
    },
    digest: "b".repeat(64),
    source_message_id: "message-1",
    created_at_ms: 1_754_000_000_000,
  },
  chart_dataset: chartDataset,
};

describe("ArtifactContent", () => {
  it("recovers from a clipboard failure", async () => {
    const user = userEvent.setup();
    const view: ChatArtifactView = {
      artifact: { ...chartView.artifact, kind: "markdown", title: "Research note" },
      revision: { ...chartView.revision, payload: { kind: "markdown", schema: "loupist-markdown/1", markdown: "# Research note\n\nSaved findings." } },
    };
    const writeText = vi.spyOn(navigator.clipboard, "writeText").mockRejectedValueOnce(new Error("Clipboard blocked")).mockResolvedValue();
    render(<ArtifactContent view={view} theme="light" />);
    await user.click(screen.getByRole("button", { name: "Copy" }));
    expect(await screen.findByRole("alert")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Copy" }));
    expect(await screen.findByRole("button", { name: "Copied" })).toBeVisible();
    expect(writeText).toHaveBeenLastCalledWith("# Research note\n\nSaved findings.");
  });
  it("pages and copies native chart rows without asking the agent, resets on a revision, and selects a compact reference", async () => {
    const user = userEvent.setup();
    const onAsk = vi.fn();
    const writeText = vi.spyOn(navigator.clipboard, "writeText").mockResolvedValue();
    const view = { ...chartView, chart_dataset: { ...chartDataset, rows: Array.from({ length: 60 }, (_, i) => [`2026-08-${i + 1}`, i]) } };
    const { rerender } = render(<ArtifactContent view={view} theme="light" onAsk={onAsk} />);
    await user.click(screen.getByRole("tab", { name: "Data" }));
    expect(within(screen.getByRole("table")).getAllByRole("row")).toHaveLength(26);
    await user.click(screen.getByRole("button", { name: "Next data rows" }));
    expect(screen.getByText("26–50 of 60")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Copy rows" }));
    expect(writeText.mock.calls[0]?.[0]).toContain("2026-08-26\t25");
    expect(writeText.mock.calls[0]?.[0]).not.toContain("2026-08-1\t0");
    expect(onAsk).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Ask about this" }));
    expect(onAsk).toHaveBeenCalledWith({ artifact_id: "artifact-1", revision: 1, kind: "chart", title: "KOSPI chart", digest: "b".repeat(64) });
    rerender(<ArtifactContent view={{ ...view, revision: { ...view.revision, revision: 2 } }} theme="dark" onAsk={onAsk} askDisabled />);
    expect(await screen.findByText("financial-engine")).toBeVisible();
    expect(screen.getByRole("button", { name: "Ask about this" })).toBeDisabled();
    await user.click(screen.getByRole("tab", { name: "Data" }));
    expect(screen.getByText("1–25 of 60")).toBeVisible();
  });

});


it("rejects an unsupported schema even when its view kind is still supported", () => {
  const view = structuredClone(chartView);
  (view.revision.payload as { schema: string }).schema = "loupist-chart/2";
  render(<ArtifactContent view={view} theme="light" />);
  expect(screen.getByRole("alert")).toHaveTextContent(/unsupported/);
  expect(screen.queryByText("financial-engine")).not.toBeInTheDocument();
});
