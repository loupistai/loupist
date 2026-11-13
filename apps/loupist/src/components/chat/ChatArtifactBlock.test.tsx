import { act, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { openSpy } from "../../test/open";
import { OpenProvider } from "../navigation/Open";
import type { ReactNode } from "react";
import type { ChartDataset, ChatArtifactRef, ChatArtifactView, LoupistBridge } from "../../types";
import { ChatArtifactBlock } from "./ChatArtifactBlock";
import { ChatExecutionProvider } from "./ChatExecutionContext";

// The stand-in chart leaves a snapshot the way the real engines do: a 320 × 160 CSS-pixel drawing.
vi.mock("../artifacts/ArtifactChart", async () => {
  const { useChartSnapshotSlot } = await import("../charts/chartSnapshot");
  return { default: function Chart({ compact }: { compact?: boolean }) {
    const slot = useChartSnapshotSlot();
    if (slot) slot.current = async (scale: number) => {
      const canvas = document.createElement("canvas");
      canvas.width = 320 * scale;
      canvas.height = 160 * scale;
      return { canvas, scale };
    };
    return <div>{compact ? "compact-chart" : "full-chart"}</div>;
  } };
});

const dataset: ChartDataset = {
  id: "dataset-1",
  columns: [{ id: "quarter", label: "Quarter", kind: "string" }, { id: "margin", label: "Margin", kind: "number" }],
  rows: [["Q2", 16.2], ["Q3", 14.8]],
  lineage: { kind: "agent_authored", upstream_receipts: [] },
  digest: "a".repeat(64),
};
const ref: ChatArtifactRef = { artifact_id: "chart-1", revision: 2, kind: "chart", title: "Operating margin", digest: "b".repeat(64) };
const view: ChatArtifactView = {
  artifact: { id: "chart-1", chat_session_id: "chat", kind: "chart", title: "Operating margin", current_revision: 2, created_at_ms: 0, updated_at_ms: 0 },
  revision: {
    artifact_id: "chart-1", revision: 2, digest: "b".repeat(64), source_message_id: "answer", created_at_ms: 0,
    payload: { kind: "chart", schema: "loupist-chart/1", chart: { dataset_id: dataset.id, dataset_digest: dataset.digest, studies: [], drawings: [],
      view: { kind: "bar", x: "quarter", y: ["margin"], semantic_types: { quarter: "Period", margin: "Percent" }, color: null, title: null, subtitle: null }, custom_series: [], note: null } },
  },
  chart_dataset: dataset,
};

function withScope(children: ReactNode, read = vi.fn().mockResolvedValue(view), exportSave = vi.fn().mockResolvedValue("/Users/me/chart.png")) {
  const bridge = { chatArtifactRead: read, exportSave } as unknown as LoupistBridge;
  return <ChatExecutionProvider value={{ bridge, loupistId: "loupist", threadId: "chat", surface: "session", theme: "light", active: true }}>{children}</ChatExecutionProvider>;
}

it("reserves the card with its title and Preparing while the chart is being built, on still ruled paper, without actions", () => {
  const { container } = render(withScope(<ChatArtifactBlock block={{ kind: "building", key: "b", title: "Operating margin", stepId: "step" }} onAsk={vi.fn()} />));
  const card = screen.getByRole("region", { name: "Chart: Operating margin" });
  expect(card).toHaveAttribute("aria-busy", "true");
  expect(card).toHaveAttribute("data-state", "building");
  expect(screen.getByRole("status")).toHaveTextContent("Preparing");
  expect(container.querySelector(".chat-artifact-block-ruled")).toHaveAttribute("aria-hidden", "true");
  expect(container.querySelector("[class*='skeleton'], [class*='shimmer']")).toBeNull();
  expect(screen.queryByRole("button")).toBeNull();
});

it("fills the same card in place with the compact chart, copy and ask", async () => {
  const onAsk = vi.fn();
  const read = vi.fn().mockResolvedValue(view);
  const writeText = vi.spyOn(navigator.clipboard, "writeText").mockResolvedValue();
  render(withScope(<ChatArtifactBlock block={{ kind: "ready", key: "r", artifact: ref, updating: false }} onAsk={onAsk} />, read));
  expect(await screen.findByText("compact-chart")).toBeVisible();
  expect(read).toHaveBeenCalledWith("loupist", "chat", "chart-1", 2);
  expect(screen.getByRole("region", { name: "Chart: Operating margin" })).toHaveAttribute("data-state", "ready");
  expect(screen.queryByRole("button", { name: /Expand/ })).toBeNull();
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Copy data" })); });
  expect(writeText).toHaveBeenCalledWith("Quarter\tMargin\nQ2\t16.2\nQ3\t14.8");
  expect(screen.getByRole("button", { name: "Copied" })).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "Ask about Operating margin" }));
  expect(onAsk).toHaveBeenCalledWith({ artifact_id: "chart-1", revision: 2, kind: "chart", title: "Operating margin", digest: "b".repeat(64) });
});

it("disables Ask while a reply is running and explains a missing chart with a retry", async () => {
  const read = vi.fn().mockRejectedValueOnce(new Error("gone")).mockResolvedValue(view);
  render(withScope(<ChatArtifactBlock block={{ kind: "ready", key: "r", artifact: ref, updating: false }} onAsk={vi.fn()} askDisabled />, read));
  expect(await screen.findByRole("alert")).toHaveTextContent("This chart is no longer available.");
  fireEvent.click(screen.getByRole("button", { name: "Try again" }));
  expect(await screen.findByText("compact-chart")).toBeVisible();
  expect(screen.getByRole("button", { name: "Ask about Operating margin" })).toBeDisabled();
});

describe("Save as PNG… (X9)", () => {
  const PNG_SIGNATURE = [137, 80, 78, 71, 13, 10, 26, 10];
  beforeEach(() => {
    vi.spyOn(HTMLCanvasElement.prototype, "toDataURL").mockReturnValue(`data:image/png;base64,${btoa(String.fromCharCode(...PNG_SIGNATURE))}`);
  });
  afterEach(() => vi.restoreAllMocks());

  it("saves the drawn chart as a 2× PNG with its footer through Rust export_save", async () => {
    const exportSave = vi.fn().mockResolvedValue("/Users/me/Operating margin.png");
    const drawn: HTMLCanvasElement[] = [];
    const toDataURL = vi.mocked(HTMLCanvasElement.prototype.toDataURL);
    toDataURL.mockImplementation(function (this: HTMLCanvasElement) { drawn.push(this); return `data:image/png;base64,${btoa(String.fromCharCode(...PNG_SIGNATURE))}`; });
    render(withScope(<ChatArtifactBlock block={{ kind: "ready", key: "r", artifact: ref, updating: false }} />, undefined, exportSave));
    await screen.findByText("compact-chart");
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Save as PNG…" })); });
    expect(exportSave).toHaveBeenCalledExactlyOnceWith({ suggestedName: "Operating margin.png", kind: "png", bytes: PNG_SIGNATURE });
    // The file is the chart at twice its size, with a footer band under it.
    expect(drawn).toHaveLength(1);
    expect(drawn[0]!.width).toBe(640);
    expect(drawn[0]!.height).toBeGreaterThan(320);
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("treats a cancelled dialog as nothing, and a failed save as a retryable error", async () => {
    const exportSave = vi.fn().mockResolvedValueOnce(null).mockRejectedValueOnce(new Error("disk full"));
    render(withScope(<ChatArtifactBlock block={{ kind: "ready", key: "r", artifact: ref, updating: false }} />, undefined, exportSave));
    await screen.findByText("compact-chart");
    const save = screen.getByRole("button", { name: "Save as PNG…" });
    await act(async () => { fireEvent.click(save); });
    expect(screen.queryByRole("alert")).toBeNull();
    await act(async () => { fireEvent.click(save); });
    expect(screen.getByRole("alert")).toHaveTextContent("Couldn’t save the picture. Try again.");
    expect(exportSave).toHaveBeenCalledTimes(2);
  });

  it("keeps a failed save from the narrow card's … menu on the card after the menu closes", async () => {
    const user = userEvent.setup();
    const exportSave = vi.fn().mockRejectedValue(new Error("disk full"));
    render(withScope(<ChatArtifactBlock block={{ kind: "ready", key: "r", artifact: ref, updating: false }} />, undefined, exportSave));
    await screen.findByText("compact-chart");
    await user.click(screen.getByRole("button", { name: "More actions" }));
    await user.click(await screen.findByRole("menuitem", { name: "Save as PNG…" }));
    expect(screen.queryByRole("menu")).toBeNull();
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Couldn’t save the picture. Try again.");
    // Under 720 px the action bar is hidden; the message is not inside it.
    expect(alert.closest(".chat-artifact-block-actions")).toBeNull();
    expect(exportSave).toHaveBeenCalledOnce();
  });
});

it("shows a failed Copy data outside the action bar a narrow card hides", async () => {
  vi.spyOn(navigator.clipboard, "writeText").mockRejectedValue(new Error("denied"));
  render(withScope(<ChatArtifactBlock block={{ kind: "ready", key: "r", artifact: ref, updating: false }} />));
  await screen.findByText("compact-chart");
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Copy data" })); });
  const alert = await screen.findByRole("alert");
  expect(alert.closest(".chat-artifact-block-actions")).toBeNull();
  expect(alert.closest("section")).toHaveAttribute("aria-label", "Chart: Operating margin");
});

it("says when a chart could not be built and offers the question again", () => {
  const onAskAgain = vi.fn();
  render(withScope(<ChatArtifactBlock block={{ kind: "failed", key: "f", title: "Operating margin", stepId: "step" }} onAskAgain={onAskAgain} />));
  expect(screen.getByRole("alert")).toHaveTextContent("Couldn't build this chart.");
  fireEvent.click(screen.getByRole("button", { name: "Ask again" }));
  expect(onAskAgain).toHaveBeenCalledOnce();
});

it("draws the older answer's revision, keeps the update note, and opens the latest on request", async () => {
  const onOpen = openSpy();
  const onAsk = vi.fn();
  const original = { ...view, revision: { ...view.revision, revision: 1, digest: "c".repeat(64) } };
  const read = vi.fn().mockImplementation((_loupist, _chat, _artifact, revisionId) => Promise.resolve(revisionId === 1 ? original : view));
  render(<OpenProvider open={onOpen}>{withScope(<ChatArtifactBlock block={{ kind: "ready", key: "r", artifact: { ...ref, revision: 1 }, updating: false }} onAsk={onAsk} />, read)}</OpenProvider>);
  expect(await screen.findByText("Updated since this answer.")).toBeVisible();
  expect(await screen.findByText("compact-chart")).toBeVisible();
  expect(read).toHaveBeenCalledWith("loupist", "chat", "chart-1", 1);
  expect(screen.getByRole("region", { name: "Chart: Operating margin" })).toHaveAttribute("data-state", "revised");
  expect(screen.getByRole("button", { name: "Copy data" })).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: /Ask about/ }));
  expect(onAsk).toHaveBeenCalledWith({ ...ref, revision: 1, digest: "c".repeat(64) });
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Open latest" })); });
  expect(onOpen).toHaveBeenCalledWith({ kind: "artifact", artifact: ref });
});
