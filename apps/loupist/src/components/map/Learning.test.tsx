import { useState } from "react";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { PreviewBridge, prepareLearningFixture } from "../../preview/fixtures";
import { MapInspector } from "./MapInspector";
import { MapView } from "./MapView";
import { isNewMethod } from "./learningState";
import type { KnowledgeLocation } from "../../types";

vi.mock("./MapCanvas", () => ({ default: ({ selectedId, learnedIds, learningGlowIds }: { selectedId?: string; learnedIds?: Set<string>; learningGlowIds?: Set<string> }) => <div data-testid="learning-canvas" data-selected={selectedId} data-new={[...(learnedIds ?? [])].join(",")} data-glow={[...(learningGlowIds ?? [])].join(",")} /> }));
async function fixture() {
  const bridge = new PreviewBridge({ delay_ms: 0 });
  const review = await prepareLearningFixture(bridge);
  const catalog = await bridge.knowledgeCatalog({ loupistId: "loupist-quality" });
  const map = await bridge.knowledgeMap({ loupistId: "loupist-quality" });
  const node = map.nodes.find(node => node.recordId === review.lensRecordId)!;
  const location: KnowledgeLocation = { loupistId: catalog.loupistId, surface: "world", recordId: catalog.worlds[0].recordId, revisionId: catalog.worlds[0].revisionId, selection: { kind: "node", id: node.id } };
  return { bridge, review, catalog, map, node, location };
}

it("Keep clears the recent ring and stays acknowledged across a Map remount", async () => {
  const { bridge, catalog, location, node } = await fixture();
  const list = bridge.learningReviewList.bind(bridge);
  bridge.learningReviewList = async request => (await list(request)).map(review => ({ ...review, createdAtMs: review.createdAtMs - 60000 }));
  function Harness() { const [where, setWhere] = useState(location); return <MapView bridge={bridge} catalog={catalog} location={where} onPatch={setWhere} onOpen={setWhere} onAsk={vi.fn()} />; }
  const first = render(<Harness />);
  await screen.findByRole("button", { name: "Keep" });
  expect((await screen.findByTestId("learning-canvas")).getAttribute("data-new")?.split(",")).toContain(node.id);
  await userEvent.click(screen.getByRole("button", { name: "Keep" }));
  expect(screen.getByTestId("learning-canvas").getAttribute("data-new")?.split(",")).not.toContain(node.id);
  first.unmount();
  render(<Harness />);
  expect(await screen.findByRole("button", { name: "Kept" })).toBeDisabled();
  expect(screen.getByTestId("learning-canvas").getAttribute("data-new")?.split(",")).not.toContain(node.id);
});

it("requires inline confirmation before calling the archive bridge", async () => {
  const { bridge, map, node, location, catalog } = await fixture();
  const archive = vi.spyOn(bridge, "knowledgeLensArchive").mockResolvedValue();
  const close = vi.fn(), changed = vi.fn();
  render(<MapInspector bridge={bridge} map={map} node={node} location={location} catalog={catalog} onSelect={vi.fn()} onClose={close} onChanged={changed} onOpen={vi.fn()} onAsk={vi.fn()} />);
  await userEvent.click(await screen.findByRole("button", { name: "Archive" }));
  expect(screen.getByText("Archive this way of working? Your Loupist will stop using it.")).toBeVisible();
  expect(archive).not.toHaveBeenCalled();
  await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(archive).not.toHaveBeenCalled();
  await userEvent.click(screen.getByRole("button", { name: "Archive" }));
  await userEvent.click(screen.getByRole("button", { name: "Archive" }));
  expect(archive).toHaveBeenCalledWith({ loupistId: "loupist-quality", recordId: node.recordId, archived: true });
  expect(changed).toHaveBeenCalledOnce(); expect(close).toHaveBeenCalledOnce();
});

it("shows the latest revision summary and earlier changes for a learned method", async () => {
  const { bridge, map, node, location, catalog } = await fixture();
  render(<MapInspector bridge={bridge} map={map} node={node} location={location} catalog={catalog} onSelect={vi.fn()} onClose={vi.fn()} onOpen={vi.fn()} onAsk={vi.fn()} />);
  expect(await screen.findByText(/Confirm recovery with operating cash flow/)).toBeVisible();
  expect(screen.getByText(/Changed because: Margin recovery alone/)).toBeVisible();
  await userEvent.click(screen.getByRole("button", { name: "Earlier changes (1)" }));
  expect(screen.getByText(/Three companies reported temporary qualification costs/)).toBeVisible();
});

it("keeps recent learning scoped to seven days and acknowledges each refinement separately", () => {
  const node = { kind: "lens_method" as const, recordId: "method", learnedAtMs: 1000 };
  expect(isNewMethod(node, new Map(), 1000 + 7 * 86400000)).toBe(false);
  expect(isNewMethod(node, new Map([["method", 1000]]), 1001)).toBe(false);
  expect(isNewMethod({ ...node, learnedAtMs: 1001 }, new Map([["method", 1000]]), 1001)).toBe(true);
});
