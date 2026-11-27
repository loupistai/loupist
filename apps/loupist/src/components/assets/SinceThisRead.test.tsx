import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { AssetPage } from "../../contracts/assets";
import type { WorldChangesView } from "../../contracts/knowledge";
import type { LoupistBridge } from "../../types";
import { SinceThisRead } from "./SinceThisRead";

const row = (kind: string, proposition: string, index = 0) => ({
  kind,
  claimId: `${kind}-${index}`,
  proposition,
  validAt: { precision: "date", value: "2024-01-15" },
  invalidAt: kind === "ended" ? { precision: "date", value: "2026-02-01" } : null,
  source: { title: `${kind} source`, publicationTime: "2026-02-28", readArgs: { recordId: `rec-${kind}`, revisionId: "rev", spanId: `span-${kind}` } },
});

function page(overrides: Record<string, unknown> = {}): AssetPage {
  return {
    entityId: "node-1",
    asset: { assetId: "ast_1" },
    read: { since: "2026-03-01", latestMove: { factDate: "2026-03-01" } },
    sinceRead: {
      counts: { ended: 9, added: 1, conflict: 1 },
      lens: [],
      pending: 0,
      failed: 0,
      capped: false,
      next: "",
      changes: [
        ...Array.from({ length: 9 }, (_, index) => row("ended", `Ended fact ${index + 1}`, index)),
        row("added", "A supplier was added."),
        row("conflict", "Two filings name different shares."),
      ],
    },
    ...overrides,
  } as unknown as AssetPage;
}

const feed = (lens: unknown[]): WorldChangesView => ({
  mode: "since", pair: null, from: null, to: null, endedByTo: [], newInTo: [], restated: [], notRestated: [], conflicts: [],
  window: null, pending: 2, failed: 0, changes: [row("added", "Learned in the window.")], lens, firstRun: false, capped: false, next: null,
});

it("lists what changed since the latest move and opens the passage", async () => {
  const knowledgeWorldChanges = vi.fn();
  const onOpenSource = vi.fn();
  const user = userEvent.setup();
  render(<SinceThisRead bridge={{ knowledgeWorldChanges } as unknown as LoupistBridge} loupistId="loupist" page={page()} name="Hanul" onOpenMap={vi.fn()} onOpenSource={onOpenSource} />);
  const block = screen.getByRole("region", { name: "Since this read" });
  expect(knowledgeWorldChanges).not.toHaveBeenCalled();
  expect(within(block).getByRole("button", { name: "Since Mar 1, 2026" })).toHaveAttribute("aria-pressed", "true");
  expect(within(block).queryByText("Ended fact 9")).toBeNull();
  await user.click(within(block).getByRole("button", { name: "Show all 9" }));
  expect(within(block).getByText("Ended fact 9")).toBeVisible();
  expect(within(block).getByRole("region", { name: "Added" })).toHaveTextContent("A supplier was added.");
  expect(within(block).getByRole("region", { name: "Disagreements" })).toHaveTextContent("Two filings name different shares.");
  await user.click(within(block).getByRole("button", { name: "added source" }));
  expect(onOpenSource).toHaveBeenCalledWith("rec-added");
});

it("reads another window from the change feed, including Lens sections still being read", async () => {
  const knowledgeWorldChanges = vi.fn(async () => feed([{ recordId: "lens-1", title: "Hanul page", sectionKey: "risks", state: "superseded" }]));
  const user = userEvent.setup();
  render(<SinceThisRead bridge={{ knowledgeWorldChanges } as unknown as LoupistBridge} loupistId="loupist" page={page()} name="Hanul" onOpenMap={vi.fn()} onOpenSource={vi.fn()} />);
  await user.click(screen.getByRole("button", { name: "30 days" }));
  const block = await screen.findByRole("region", { name: "Since this read" });
  expect(knowledgeWorldChanges).toHaveBeenCalledWith(expect.objectContaining({
    loupistId: "loupist", axis: "recorded", about: [{ asset: "ast_1" }], since: expect.stringMatching(/^\d{4}-\d{2}-\d{2}$/),
  }));
  expect(await within(block).findByText("Learned in the window.")).toBeVisible();
  expect(within(block).getByText("Hanul page · risks")).toBeVisible();
  expect(within(block).getByRole("status")).toHaveTextContent("World is still reading 2 Sources about Hanul.");
});

it("does not say nothing changed while Sources are still being read", () => {
  const bridge = { knowledgeWorldChanges: vi.fn() } as unknown as LoupistBridge;
  const view = render(<SinceThisRead bridge={bridge} loupistId="loupist" page={page({
    sinceRead: { counts: {}, lens: [], pending: 3, failed: 0, capped: false, next: "", changes: [] },
  })} name="Hanul" onOpenMap={vi.fn()} onOpenSource={vi.fn()} />);
  expect(screen.getByRole("status")).toHaveTextContent("World is still reading 3 Sources about Hanul.");
  expect(screen.queryByText(/Nothing changed in World/)).toBeNull();
  view.rerender(<SinceThisRead bridge={bridge} loupistId="loupist" page={page({
    sinceRead: { counts: {}, lens: [], pending: 0, failed: 0, capped: false, next: "", changes: [] },
  })} name="Hanul" onOpenMap={vi.fn()} onOpenSource={vi.fn()} />);
  expect(screen.getByText(/Nothing changed in World/)).toBeVisible();
});

it("asks for a World name when the asset is not linked, and says Recent changes before any read", async () => {
  const knowledgeWorldChanges = vi.fn(async () => feed([]));
  const onOpenMap = vi.fn();
  const user = userEvent.setup();
  const view = render(<SinceThisRead bridge={{ knowledgeWorldChanges } as unknown as LoupistBridge} loupistId="loupist" page={page({ entityId: null })} name="Hanul" onOpenMap={onOpenMap} onOpenSource={vi.fn()} />);
  expect(screen.getByRole("region", { name: "Since this read" })).toHaveTextContent("Link Hanul to a World name to see what changed.");
  expect(knowledgeWorldChanges).not.toHaveBeenCalled();
  await user.click(screen.getByRole("button", { name: "Open World" }));
  expect(onOpenMap).toHaveBeenCalledOnce();
  view.rerender(<SinceThisRead bridge={{ knowledgeWorldChanges } as unknown as LoupistBridge} loupistId="loupist" page={page({ read: null, sinceRead: undefined })} name="Hanul" onOpenMap={onOpenMap} onOpenSource={vi.fn()} />);
  expect(await screen.findByRole("region", { name: "Recent changes" })).toBeVisible();
  expect(screen.queryByRole("button", { name: /^Since [A-Z]/ })).toBeNull();
  await act(async () => {});
  expect(knowledgeWorldChanges).toHaveBeenCalledWith(expect.objectContaining({ axis: "recorded", about: [{ asset: "ast_1" }] }));
});

it("offers Try again when a window fails to load", async () => {
  const knowledgeWorldChanges = vi.fn().mockRejectedValueOnce(new Error("offline")).mockResolvedValue(feed([]));
  const user = userEvent.setup();
  render(<SinceThisRead bridge={{ knowledgeWorldChanges } as unknown as LoupistBridge} loupistId="loupist" page={page()} name="Hanul" onOpenMap={vi.fn()} onOpenSource={vi.fn()} />);
  await user.click(screen.getByRole("button", { name: "30 days" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Could not load what changed.");
  await user.click(screen.getByRole("button", { name: "Try again" }));
  expect(await screen.findByText("Learned in the window.")).toBeVisible();
});
