import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { WorldChangesView } from "../../contracts/knowledge";
import type { LoupistBridge } from "../../types";
import { dateValue } from "../../knowledge/presentation";
import { ReportChanges } from "./ReportChanges";

type ChangeClaim = WorldChangesView["endedByTo"][number];
type ChangeSource = NonNullable<WorldChangesView["to"]>;

const claim = (claimId: string, proposition: string): ChangeClaim => ({
  claimId, proposition, relation: "stated", status: "active",
  validAt: { precision: "date", value: "2024-01-15" }, invalidAt: null,
});

const source = (recordId: string, title: string, publicationTime: string, pending = false): ChangeSource => ({
  recordId, title, publicationTime, episodes: 3, pending,
});

function comparison(pending = false): WorldChangesView {
  const ended = Array.from({ length: 9 }, (_, index) => claim(`ended-${index}`, `Ended fact ${index + 1}`));
  return {
    mode: "between",
    pair: { from: "rec-from", to: "rec-to" },
    from: source("rec-from", "Q3 report", "2025-11-01"),
    to: source("rec-to", "Q4 report", "2026-02-28", pending),
    endedByTo: ended,
    newInTo: [claim("new-1", "A new customer is above a quarter of revenue.")],
    restated: [claim("both-1", "The substrate supplier is unchanged.")],
    notRestated: [claim("quiet-1", "The old capacity line was not repeated.")],
    conflicts: [{ claimId: "both-1", withClaimId: "other-1" }],
    window: null,
    pending: pending ? 1 : 0,
    failed: 0,
    changes: [],
    lens: [],
    firstRun: false,
    capped: false,
    next: null,
  };
}

const empty = (): WorldChangesView => ({ ...comparison(), pair: null, from: null, to: null, endedByTo: [], newInTo: [], restated: [], notRestated: [], conflicts: [] });

function harness(result: WorldChangesView | Error) {
  const knowledgeWorldChanges = vi.fn(async () => {
    if (result instanceof Error) throw result;
    return result;
  });
  const onOpenSource = vi.fn();
  const bridge = { knowledgeWorldChanges } as unknown as LoupistBridge;
  return { bridge, knowledgeWorldChanges, onOpenSource };
}

it("groups the latest report against the one before it", async () => {
  const { bridge, knowledgeWorldChanges, onOpenSource } = harness(comparison());
  const user = userEvent.setup();
  render(<ReportChanges bridge={bridge} loupistId="loupist" assetId="ast_1" onOpenSource={onOpenSource} />);
  const card = await screen.findByRole("region", { name: "Changes in the latest report" });
  expect(knowledgeWorldChanges).toHaveBeenCalledWith({ loupistId: "loupist", between: { asset: "ast_1" } });
  expect(card).toHaveTextContent(`Q3 report · ${dateValue("2025-11-01")}`);
  expect(card).toHaveTextContent(`Q4 report · ${dateValue("2026-02-28")}`);
  expect(within(card).getByRole("region", { name: "Ended by this report" })).toHaveTextContent("Ended fact 1");
  expect(within(card).queryByText("Ended fact 9")).toBeNull();
  await user.click(within(card).getByRole("button", { name: "Show all" }));
  expect(within(card).getByText("Ended fact 9")).toBeVisible();
  expect(within(card).getByRole("region", { name: "New in this report" })).toHaveTextContent("A new customer is above a quarter of revenue.");
  expect(within(card).getByRole("region", { name: "Sources disagree" })).toHaveTextContent("The substrate supplier is unchanged.");
  expect(within(card).queryByText("The old capacity line was not repeated.")).toBeNull();
  await user.click(within(card).getByRole("button", { name: /Not repeated in this report/ }));
  expect(within(card).getByText("The old capacity line was not repeated.")).toBeVisible();
  expect(card).not.toHaveTextContent(/ended these|Ended fact 9 ended/i);
  expect(card).toHaveTextContent("Also stated in both: 1");
  expect(card).not.toHaveTextContent("both-1");
  await user.click(within(card).getAllByRole("button", { name: "Q4 report" })[0]!);
  expect(onOpenSource).toHaveBeenCalledWith("rec-to");
});

it("renders nothing when the asset has no report pair", async () => {
  const { bridge } = harness(empty());
  const { container } = render(<ReportChanges bridge={bridge} loupistId="loupist" assetId="ast_1" onOpenSource={vi.fn()} />);
  await act(async () => {});
  expect(container).toBeEmptyDOMElement();
  expect(screen.queryByRole("heading", { name: "Changes in the latest report" })).toBeNull();
});

it("says World is still reading the newer report", async () => {
  const { bridge } = harness(comparison(true));
  render(<ReportChanges bridge={bridge} loupistId="loupist" assetId="ast_1" onOpenSource={vi.fn()} />);
  const card = await screen.findByRole("region", { name: "Changes in the latest report" });
  expect(within(card).getByRole("status")).toHaveTextContent("World is still reading this report.");
  expect(within(card).getByText("Ended fact 1")).toBeVisible();
});

it("shows loading only after a moment, then nothing when there is no pair", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  try {
    let settle: (value: WorldChangesView) => void = () => {};
    const knowledgeWorldChanges = vi.fn(() => new Promise<WorldChangesView>(resolve => { settle = resolve; }));
    const bridge = { knowledgeWorldChanges } as unknown as LoupistBridge;
    const view = render(<ReportChanges bridge={bridge} loupistId="loupist" assetId="ast_1" onOpenSource={vi.fn()} />);
    expect(screen.queryByRole("status")).toBeNull();
    await act(async () => { await vi.advanceTimersByTimeAsync(300); });
    expect(screen.getByRole("status")).toHaveTextContent("Loading the comparison…");
    await act(async () => { settle(empty()); });
    expect(view.container).toBeEmptyDOMElement();
  } finally { vi.useRealTimers(); }
});

it("offers Try again when the comparison fails", async () => {
  const knowledgeWorldChanges = vi.fn()
    .mockRejectedValueOnce(new Error("offline"))
    .mockResolvedValueOnce(comparison());
  const bridge = { knowledgeWorldChanges } as unknown as LoupistBridge;
  render(<ReportChanges bridge={bridge} loupistId="loupist" assetId="ast_1" onOpenSource={vi.fn()} />);
  expect(await screen.findByRole("alert")).toHaveTextContent("Could not compare these reports.");
  await userEvent.setup().click(screen.getByRole("button", { name: "Try again" }));
  expect(await screen.findByRole("region", { name: "Changes in the latest report" })).toBeVisible();
});
