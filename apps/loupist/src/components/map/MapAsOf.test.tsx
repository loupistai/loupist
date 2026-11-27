import { useState } from "react";
import { render, renderHook, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactNode } from "react";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { PreviewBridge } from "../../preview/fixtures";
import { chooseSelectOption, openSelect } from "../../test/selectOption";
import type { KnowledgeLocation } from "../../types";
import { MapView } from "./MapView";
import { WorldAsOfContext, useMapDetail } from "./useMapData";

vi.mock("./MapCanvas", () => ({ default: ({ nodes = [] }: { nodes?: { id: string }[] }) => <div data-testid="map-canvas" data-nodes={nodes.map(node => node.id).join(",")} /> }));

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(new Date(2026, 10, 26, 12));
});
afterEach(() => vi.useRealTimers());

async function setup() {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const catalog = await bridge.knowledgeCatalog({ loupistId: "loupist-quality" });
  const read = vi.spyOn(bridge, "knowledgeMap");
  function Harness() {
    const [location, setLocation] = useState<KnowledgeLocation>({ loupistId: catalog.loupistId, surface: "world", recordId: catalog.worlds[0]?.recordId, revisionId: catalog.worlds[0]?.revisionId });
    return <MapView bridge={bridge} catalog={catalog} location={location} onPatch={setLocation} onOpen={vi.fn()} onAsk={vi.fn()} onAddSource={vi.fn()} onAskInChat={vi.fn()} />;
  }
  render(<Harness />);
  await screen.findByText("37 names · 36 relationships");
  return { bridge, read, user: userEvent.setup() };
}

async function chooseDay(user: ReturnType<typeof userEvent.setup>, year: string, month: string, day: string) {
  await user.click(screen.getByRole("button", { name: "As of" }));
  const field = screen.getByRole("group", { name: "As-of date" });
  await chooseSelectOption(user, within(field).getByRole("combobox", { name: "Year" }), year);
  await chooseSelectOption(user, within(field).getByRole("combobox", { name: "Month" }), month);
  await chooseSelectOption(user, within(field).getByRole("combobox", { name: "Day" }), day);
}

it("shows today's World with no banner until a date is chosen", async () => {
  const { read } = await setup();
  expect(read).toHaveBeenLastCalledWith({ loupistId: "loupist-quality", maxNodes: 1000 });
  expect(screen.getByRole("button", { name: "As of" })).toHaveAttribute("aria-pressed", "false");
  expect(screen.queryByText(/Showing what was published by/)).toBeNull();
});

it("reads World as of a chosen date, names what it hides, and Today returns", async () => {
  const { bridge, read, user } = await setup();
  const expected = await bridge.knowledgeMap({ loupistId: "loupist-quality", asOf: "2026-09-10" });
  const hidden = expected.asOfReport!.facts;
  expect(hidden).toBeGreaterThan(1);
  await chooseDay(user, "2026", "Sep", "10");

  expect(await screen.findByText(`Showing what was published by September 10, 2026. ${hidden} later facts are hidden.`)).toBeVisible();
  expect(read).toHaveBeenLastCalledWith({ loupistId: "loupist-quality", maxNodes: 1000, asOf: "2026-09-10" });
  const drawn = screen.getByTestId("map-canvas").dataset.nodes!.split(",");
  expect(drawn.sort()).toEqual(expected.nodes.filter(node => node.kind === "entity").map(node => node.id).sort());
  expect(screen.getByRole("button", { name: "As of Sep 10, 2026" })).toHaveAttribute("aria-pressed", "true");

  // The field stays open for another change; Today closes it.
  await user.click(screen.getByRole("button", { name: "Today" }));
  expect(await screen.findByText("37 names · 36 relationships")).toBeVisible();
  expect(screen.queryByText(/Showing what was published by/)).toBeNull();
  expect(read).toHaveBeenLastCalledWith({ loupistId: "loupist-quality", maxNodes: 1000 });
});

it("shows the empty World with one line for a date before the first Source", async () => {
  const { user } = await setup();
  await chooseDay(user, "2026", "Aug", "31");
  const empty = await screen.findByTestId("map-empty");
  expect(empty).toHaveTextContent("Nothing in World was published by August 31, 2026.");
  expect(within(empty).queryByRole("button")).toBeNull();
  expect(screen.getByText(/Showing what was published by August 31, 2026\./)).toBeVisible();
  // A later date stays reachable from the empty World.
  expect(screen.getByRole("button", { name: "As of Aug 31, 2026" })).toBeEnabled();
});

it("offers no day after today", async () => {
  const { user } = await setup();
  await user.click(screen.getByRole("button", { name: "As of" }));
  const field = screen.getByRole("group", { name: "As-of date" });
  const years = await openSelect(user, within(field).getByRole("combobox", { name: "Year" }));
  expect(years.queryByRole("option", { name: "2027" })).toBeNull();
  await user.click(years.getByRole("option", { name: "2026" }));
  const months = await openSelect(user, within(field).getByRole("combobox", { name: "Month" }));
  expect(months.queryByRole("option", { name: "Dec" })).toBeNull();
  await user.click(months.getByRole("option", { name: "Nov" }));
  const days = await openSelect(user, within(field).getByRole("combobox", { name: "Day" }));
  expect(days.getByRole("option", { name: "26" })).toBeVisible();
  expect(days.queryByRole("option", { name: "27" })).toBeNull();
});

it("refuses a future date in the mock host with its own code", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  await expect(bridge.knowledgeMap({ loupistId: "loupist-quality", asOf: "2026-11-27" })).rejects.toMatchObject({ code: "as_of_future" });
});

it("leaves What's new out while a date is set, and offers it again on Today", async () => {
  const { user } = await setup();
  expect(screen.getByRole("button", { name: /What's new/ })).toBeVisible();
  await chooseDay(user, "2026", "Sep", "10");
  await screen.findByText(/Showing what was published by September 10, 2026\./);
  expect(screen.queryByRole("button", { name: /What's new/ })).toBeNull();
  await user.click(screen.getByRole("button", { name: "Today" }));
  expect(await screen.findByRole("button", { name: /What's new/ })).toBeVisible();
});

it("sends the view's date with every World read under it, so inspectors and highlights read one cutoff", async () => {
  const bridge = new PreviewBridge({ delay_ms: 0 });
  const query = vi.spyOn(bridge, "knowledgeWorldQuery");
  const location = { surface: "world" as const, loupistId: "loupist-quality", recordId: "preview-world", revisionId: "first" };
  const wrapper = ({ children }: { children: ReactNode }) => <WorldAsOfContext.Provider value="2026-03-03">{children}</WorldAsOfContext.Provider>;
  renderHook(() => useMapDetail(bridge, location, "preview-node-0"), { wrapper });
  await waitFor(() => expect(query).toHaveBeenCalledWith(expect.objectContaining({ centerNodeId: "preview-node-0", asOf: "2026-03-03" })));
  query.mockClear();
  renderHook(() => useMapDetail(bridge, location, "preview-node-0"));
  await waitFor(() => expect(query).toHaveBeenCalled());
  expect(query.mock.calls[0]![0]).not.toHaveProperty("asOf");
});
