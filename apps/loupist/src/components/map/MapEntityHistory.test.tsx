import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { WorldChangesRequest, WorldChangesView } from "../../contracts/knowledge";
import type { LoupistBridge } from "../../types";
import { HISTORY_PAGE, MapEntityHistory } from "./MapEntityHistory";
import { WorldAsOfContext } from "./useMapData";

const change = (claimId: string, proposition: string, published: string | null, kind = "added") => ({
  kind, claimId, proposition, relation: "SUPPLIES", validAt: null, invalidAt: null, status: "active",
  source: { title: "Annual report", publicationTime: published, readArgs: { scope: "source", recordId: `source-${claimId}` } },
});
const page = (changes: unknown[], next: string | null): WorldChangesView => ({
  mode: "since", pair: null, from: null, to: null, endedByTo: [], newInTo: [], restated: [], notRestated: [], conflicts: [],
  window: null, pending: 0, failed: 0, changes, lens: [], firstRun: false, capped: false, next,
});
const stub = (read: (request: WorldChangesRequest) => Promise<WorldChangesView>) => ({ knowledgeWorldChanges: vi.fn(read) }) as unknown as LoupistBridge & { knowledgeWorldChanges: ReturnType<typeof vi.fn> };

it("lists the entity's feed on the published axis, dated, and opens a row's claim", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const read = vi.spyOn(bridge, "knowledgeWorldChanges");
  const onOpenClaim = vi.fn();
  render(<MapEntityHistory bridge={bridge} loupistId="loupist-quality" entityId="map-entity-00" name="Apple" onOpenClaim={onOpenClaim} />);
  const history = await screen.findByRole("region", { name: "History" });
  const rows = await within(history).findAllByRole("listitem");
  expect(rows.length).toBeGreaterThan(1);
  expect(read).toHaveBeenCalledWith(expect.objectContaining({ axis: "published", about: [{ node: "map-entity-00" }], since: "1990-01-01", limit: HISTORY_PAGE }));
  expect(rows[0]).toHaveTextContent(/Added · Published \w{3} \d{1,2}, 2026/);
  await userEvent.click(within(rows[0]!).getByRole("button"));
  expect(onOpenClaim).toHaveBeenCalledWith("map-claim-00");
});

it("pages with the feed's cursor, showing each row once", async () => {
  const bridge = stub(async request => request.since === "cursor-2"
    ? page([change("c2", "Hanul Foods supplies Ostra Retail", "2025-04-02"), change("c3", "Hanul Foods ended its supply deal with Vela GmbH", "2025-06-30", "ended")], null)
    : page([change("c1", "Hanul Foods opened a plant in Querétaro", "2024-11-05"), change("c2", "Hanul Foods supplies Ostra Retail", "2025-04-02")], "cursor-2"));
  render(<MapEntityHistory bridge={bridge} loupistId="loupist" entityId="hanul" name="Hanul Foods" onOpenClaim={vi.fn()} />);
  await screen.findByText("Hanul Foods opened a plant in Querétaro");
  await userEvent.click(screen.getByRole("button", { name: "Show more" }));
  await screen.findByText("Hanul Foods ended its supply deal with Vela GmbH");
  expect(screen.getAllByText("Hanul Foods supplies Ostra Retail")).toHaveLength(1);
  expect(screen.getAllByRole("listitem")).toHaveLength(3);
  expect(screen.getAllByRole("listitem")[2]).toHaveTextContent("Ended");
  expect(screen.queryByRole("button", { name: "Show more" })).toBeNull();
});

it("reads the feed of World as of its date from the host", async () => {
  const bridge = stub(async request => page(request.asOf === "2024-03-31" ? [change("c1", "Sakura Denki listed on a second exchange", "2023-02-14")] : [
    change("c1", "Sakura Denki listed on a second exchange", "2023-02-14"),
    change("c2", "Sakura Denki named a new chief executive", "2024-09-01"),
  ], null));
  render(<WorldAsOfContext.Provider value="2024-03-31"><MapEntityHistory bridge={bridge} loupistId="loupist" entityId="sakura" name="Sakura Denki" onOpenClaim={vi.fn()} /></WorldAsOfContext.Provider>);
  await screen.findByText("Sakura Denki listed on a second exchange");
  expect(bridge.knowledgeWorldChanges).toHaveBeenCalledWith(expect.objectContaining({ asOf: "2024-03-31", axis: "published" }));
  expect(screen.queryByText("Sakura Denki named a new chief executive")).toBeNull();
});

it("in the mock, a date before a relationship was published leaves it out of History", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const today = await bridge.knowledgeWorldChanges({ loupistId: "loupist-quality", since: "1990-01-01", axis: "published", about: [{ node: "map-entity-00" }] });
  const before = await bridge.knowledgeWorldChanges({ loupistId: "loupist-quality", since: "1990-01-01", axis: "published", about: [{ node: "map-entity-00" }], asOf: "1990-01-01" });
  expect(today.changes.length).toBeGreaterThan(0);
  expect(before.changes).toEqual([]);
  await expect(bridge.knowledgeWorldChanges({ loupistId: "loupist-quality", since: "1990-01-01", asOf: "2999-01-01" })).rejects.toMatchObject({ code: "as_of_future" });
});

it("says when nothing is known about the name, by the date when one is set", async () => {
  const bridge = stub(async request => page(request.asOf ? [] : [change("c1", "Lumen Bank opened a branch", "2025-01-10")], null));
  const view = render(<MapEntityHistory bridge={stub(async () => page([], null))} loupistId="loupist" entityId="lumen" name="Lumen Bank" onOpenClaim={vi.fn()} />);
  expect(await screen.findByText("Nothing has been published about Lumen Bank yet.")).toBeVisible();
  view.unmount();
  render(<WorldAsOfContext.Provider value="2024-12-31"><MapEntityHistory bridge={bridge} loupistId="loupist" entityId="lumen" name="Lumen Bank" onOpenClaim={vi.fn()} /></WorldAsOfContext.Provider>);
  expect(await screen.findByText("Nothing was published about Lumen Bank by December 31, 2024.")).toBeVisible();
});

it("shows loading only after 300 ms, and an error with Try again", async () => {
  vi.useFakeTimers();
  try {
    let fail!: (error: unknown) => void;
    const bridge = stub(() => new Promise((_, reject) => { fail = reject; }));
    render(<MapEntityHistory bridge={bridge} loupistId="loupist" entityId="lumen" name="Lumen Bank" onOpenClaim={vi.fn()} />);
    expect(screen.queryByText("Loading history…")).toBeNull();
    await act(async () => { vi.advanceTimersByTime(300); });
    expect(screen.getByText("Loading history…")).toBeVisible();
    await act(async () => { fail({ code: "unavailable" }); });
    expect(screen.getByRole("alert")).toHaveTextContent("History could not be read.");
    bridge.knowledgeWorldChanges.mockResolvedValueOnce(page([change("c1", "Lumen Bank opened a branch", "2025-01-10")], null));
    await act(async () => { screen.getByRole("button", { name: "Try again" }).click(); });
    expect(screen.getByText("Lumen Bank opened a branch")).toBeVisible();
    expect(bridge.knowledgeWorldChanges).toHaveBeenCalledTimes(2);
  } finally {
    vi.useRealTimers();
  }
});
