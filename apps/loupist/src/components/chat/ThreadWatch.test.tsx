import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { previewEarningsMessage, previewWatchMessage, SOLSUP_EVENT, solsupSnapshot } from "../../preview/finance";
import type { LoupistSummary } from "../../types";
import { AssetPageOpenerProvider } from "../assets/assetOpener";
import { ThreadEarnings } from "./ThreadEarnings";
import { ThreadWatch } from "./ThreadWatch";

async function setup() {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const [loupist] = await bridge.loupistList() as LoupistSummary[];
  return { bridge, loupist: loupist! };
}

it("names five events in one message, each with a source, then +n more to Schedule › Watching", async () => {
  const { bridge, loupist } = await setup();
  const onOpenSchedule = vi.fn();
  const open = vi.fn();
  render(<AssetPageOpenerProvider value={open}><ThreadWatch message={previewWatchMessage()} bridge={bridge} loupistId={loupist.id} loupistName="Quality" onOpenSchedule={onOpenSchedule} /></AssetPageOpenerProvider>);
  const message = screen.getByRole("article", { name: "Alerts from Watching" });
  const events = within(message).getAllByRole("listitem");
  expect(events).toHaveLength(5);
  for (const event of events) expect(event.querySelector(".loupist-citation")).not.toBeNull();
  // Each alert leads with its fact date, newest first; a periodic report reads as one.
  expect(events[0]).toHaveTextContent(/^[A-Z][a-z]{2} \d{1,2} · Morrow Components filed a quarterly report \(10-Q\)\./);
  // Newest fact first: yesterday's results date moves above the insiders found two days ago.
  expect(events[1]).toHaveTextContent(/Solsup Foods reports Q3 2026 results on/);
  expect(events[2]).toHaveTextContent(/3 insiders bought shares/);
  // Actions name their asset.
  await userEvent.click(within(events[0]!).getByRole("button", { name: "What changed · Morrow Components" }));
  expect(open).toHaveBeenCalledWith(expect.objectContaining({ assetId: previewWatchMessage().watch!.events[0]!.assetId }));
  await userEvent.click(within(message).getByRole("button", { name: "+1 more" }));
  expect(onOpenSchedule).toHaveBeenCalledWith("watching");
});

it("Prepare before and after captures expectations for that results event", async () => {
  const { bridge, loupist } = await setup();
  bridge.earnings.install(loupist.id, { ...solsupSnapshot(), eventId: SOLSUP_EVENT, status: "upcoming", measures: [], chain: [] });
  const prepare = vi.spyOn(bridge, "earningsPrepare");
  const open = vi.fn();
  render(<AssetPageOpenerProvider value={open}><ThreadWatch message={previewWatchMessage()} bridge={bridge} loupistId={loupist.id} loupistName="Quality" /></AssetPageOpenerProvider>);
  await userEvent.click(screen.getByRole("button", { name: "Prepare before and after · Solsup Foods" }));
  await waitFor(() => expect(prepare).toHaveBeenCalledWith({ loupistId: loupist.id, assetId: solsupSnapshot().assetId, eventId: SOLSUP_EVENT }));
  await waitFor(() => expect(open).toHaveBeenCalledWith(expect.objectContaining({ section: "results", eventId: SOLSUP_EVENT })));
});

it("the alerts are seen after a second on screen", async () => {
  vi.useFakeTimers();
  try {
    const { bridge, loupist } = await setup();
    render(<ThreadWatch message={previewWatchMessage()} bridge={bridge} loupistId={loupist.id} loupistName="Quality" />);
    expect(bridge.watch.seenCalls).toHaveLength(0);
    await act(async () => { vi.advanceTimersByTime(1000); });
    expect(bridge.watch.seenCalls).toEqual([previewWatchMessage().watch!.event_ids]);
  } finally { vi.useRealTimers(); }
});

it("a results message shows two measures and opens the card; a failed capture offers Try again", async () => {
  const { bridge, loupist } = await setup();
  const open = vi.fn();
  render(<AssetPageOpenerProvider value={open}>
    <ThreadEarnings message={previewEarningsMessage("reported")} bridge={bridge} loupistId={loupist.id} loupistName="Quality" />
    <ThreadEarnings message={previewEarningsMessage("failed")} bridge={bridge} loupistId={loupist.id} loupistName="Quality" />
  </AssetPageOpenerProvider>);
  const [reported, failed] = screen.getAllByRole("article");
  expect(within(reported!).getByText("Morrow Components reported Q3 2026 results.")).toBeVisible();
  expect(within(reported!).getAllByRole("row")).toHaveLength(2);
  await userEvent.click(within(reported!).getByRole("button", { name: "See before and after · Morrow Components" }));
  expect(open).toHaveBeenCalledWith(expect.objectContaining({ section: "results", eventId: previewEarningsMessage().earnings!.event_id }));
  expect(within(failed!).getByText("Couldn't capture expectations for Morrow Components before results.")).toBeVisible();
  const prepare = vi.spyOn(bridge, "earningsPrepare");
  await userEvent.click(within(failed!).getByRole("button", { name: "Try again · Morrow Components" }));
  await waitFor(() => expect(prepare).toHaveBeenCalled());
});
