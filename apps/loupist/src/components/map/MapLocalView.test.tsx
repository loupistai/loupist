import { useState } from "react";
import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { GrowthEvent, KnowledgeMap, KnowledgeMapEdge, KnowledgeMapNode } from "../../contracts/knowledge";
import type { KnowledgeLocation } from "../../types";
import { MapView } from "./MapView";

vi.mock("./MapCanvas", () => ({ default: ({ nodes = [], highlightIds }: { nodes?: { id: string }[]; highlightIds?: ReadonlySet<string> }) => <div data-testid="map-canvas" data-ids={nodes.map((node) => node.id).sort().join(",")} data-highlight={[...(highlightIds ?? [])].join(",")} /> }));

const NOW = Date.now();
const entity = (id: string, label: string, extra: Partial<KnowledgeMapNode> = {}): KnowledgeMapNode => ({ id, label, kind: "entity", entityKind: "Organization", degree: 0, watched: false, held: false, ...extra });
const claim = (source: string, target: string): KnowledgeMapEdge => ({ id: `claim-${source}-${target}`, kind: "claim", source, target, relation: "SUPPLIES", status: "active" });

/** TSMC (watched) with three suppliers, each with one supplier of its own, plus 70 unrelated names. */
function localMap(): KnowledgeMap {
  const nodes = [entity("tsmc", "TSMC", { watched: true }), entity("asml", "ASML"), entity("amat", "Applied Materials"), entity("shin", "Shin-Etsu"),
    entity("zeiss", "Zeiss"), entity("lam", "Lam Research"), entity("sumco", "SUMCO"), entity("lonely", "Lonely Corp")];
  for (let index = 0; index < 70; index++) nodes.push(entity(`filler-${index}`, `Filler ${index}`));
  const edges = [claim("asml", "tsmc"), claim("amat", "tsmc"), claim("shin", "tsmc"), claim("zeiss", "asml"), claim("lam", "amat"), claim("sumco", "shin")];
  for (let index = 1; index < 70; index++) edges.push(claim(`filler-${index - 1}`, `filler-${index}`));
  for (const edge of edges) for (const id of [edge.source, edge.target]) nodes.find((node) => node.id === id)!.degree++;
  const counts = Object.fromEntries(["entity", "lens_perspective", "lens_principle", "lens_method", "journal", "claim", "lens_link", "applies_to", "about"].map((kind) => [kind, { total: kind === "entity" ? nodes.length : kind === "claim" ? edges.length : 0, shown: kind === "entity" ? nodes.length : kind === "claim" ? edges.length : 0, truncated: false }])) as KnowledgeMap["counts"];
  return { nodes, edges, counts, unmatchedHoldings: [], unmatchedWatchlist: [], worldRevisionId: "local-test", generatedAtMs: NOW };
}

const growth = (kind: GrowthEvent["kind"], subjectId: string, atMs: number, summary = ""): GrowthEvent => ({ eventId: `${kind}:${subjectId}`, loupistId: "loupist-quality", atMs, kind, subjectId, recordId: "world", originChatId: null, originMessageId: null, actor: "loupist", summary, beforeSummary: null, afterSummary: null });

async function setup({ events = [] as GrowthEvent[], lastSeenAtMs, initialView }: { events?: GrowthEvent[]; lastSeenAtMs?: number; initialView?: KnowledgeLocation["view"] } = {}) {
  if (lastSeenAtMs !== undefined) localStorage.setItem("loupist.loupistVisit.v1", JSON.stringify({ "loupist-quality": { lastSeenAtMs } }));
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const catalog = await bridge.knowledgeCatalog({ loupistId: "loupist-quality" });
  vi.spyOn(bridge, "knowledgeMap").mockResolvedValue(localMap());
  const growthRead = vi.spyOn(bridge, "knowledgeGrowthEvents").mockImplementation(async (request) => ({ events: events.filter((item) => request.sinceMs === undefined || item.atMs >= request.sinceMs), nextCursor: null }));
  const onOpen = vi.fn();
  let current: KnowledgeLocation | undefined;
  function Harness() {
    const [location, setLocation] = useState<KnowledgeLocation>({ loupistId: catalog.loupistId, surface: "world", recordId: catalog.worlds[0]?.recordId, revisionId: catalog.worlds[0]?.revisionId, view: initialView });
    current = location;
    return <MapView bridge={bridge} catalog={catalog} location={location} onPatch={setLocation} onOpen={onOpen} onAsk={vi.fn()} onAskInChat={vi.fn()} />;
  }
  render(<Harness />);
  await screen.findByTestId("map-canvas");
  return { user: userEvent.setup(), growthRead, onOpen, location: () => current };
}
const drawn = () => screen.getByTestId("map-canvas").dataset.ids!.split(",");

beforeEach(() => localStorage.clear());

it("opens a large Map around the watched company at one hop and widens to two hops and the whole Map", async () => {
  const { user } = await setup();
  expect(screen.getByRole("button", { name: "Around TSMC" })).toBeVisible();
  expect(drawn()).toEqual(["amat", "asml", "shin", "tsmc"]);
  expect(screen.getByText("4 of 78 on the map around TSMC")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Links of links" }));
  expect(drawn()).toEqual(["amat", "asml", "lam", "shin", "sumco", "tsmc", "zeiss"]);
  await user.click(screen.getByRole("button", { name: "All" }));
  expect(drawn()).toHaveLength(78);
  expect(screen.queryByText(/on the map around TSMC/)).toBeNull();
});

it("re-centers from the Around menu and when a search result lies outside the drawn names", async () => {
  const { user } = await setup();
  await user.click(screen.getByRole("button", { name: "Around TSMC" }));
  await user.click(await screen.findByRole("button", { name: "Pick on the map…" }));
  expect(screen.getByText("Click a name on the map to center on it.")).toBeVisible();
  await user.keyboard("{Escape}");
  expect(screen.queryByText("Click a name on the map to center on it.")).toBeNull();
  await user.type(screen.getByRole("combobox"), "Lonely");
  await user.click(await screen.findByRole("option", { name: /Lonely Corp/ }));
  expect(await screen.findByRole("button", { name: "Around Lonely Corp" })).toBeVisible();
  expect(drawn()).toEqual(["lonely"]);
  expect(screen.getByText("Lonely Corp has no relationships yet. Ask about it in Chat.")).toBeVisible();
});

it("moves between neighbors, re-centers and steps back with the keyboard", async () => {
  const { user } = await setup();
  const stage = screen.getByRole("group", { name: /Map around TSMC/ });
  act(() => stage.focus());
  await user.keyboard("{ArrowRight}");
  const first = screen.getByTestId("map-canvas").dataset.highlight!;
  expect(["asml", "amat", "shin"]).toContain(first);
  await user.keyboard("{ArrowRight}");
  expect(screen.getByTestId("map-canvas").dataset.highlight).not.toBe(first);
  await user.keyboard("{Shift>}{Enter}{/Shift}");
  expect(screen.queryByRole("button", { name: "Around TSMC" })).toBeNull();
  expect(drawn()).toContain("tsmc");
  await user.keyboard("{Backspace}");
  expect(screen.getByRole("button", { name: "Around TSMC" })).toBeVisible();
  await user.keyboard("2");
  expect(screen.getByRole("button", { name: "Links of links" })).toHaveAttribute("aria-pressed", "true");
  await user.keyboard("0");
  expect(drawn()).toHaveLength(78);
});

it("lists what changed since the last visit by day and focuses a name from its row", async () => {
  const lastSeenAtMs = NOW - 3 * 86_400_000;
  const { user, growthRead } = await setup({ lastSeenAtMs, events: [
    growth("entity_added", "zeiss", NOW - 60_000, "Zeiss"),
    growth("claim_added", "claim-zeiss-asml", NOW - 60_000, "Zeiss supplies ASML"),
    growth("entity_added", "lonely", NOW - 86_400_000 - 60_000, "Lonely Corp"),
    growth("schema_extended", "kinds", NOW - 60_000),
  ] });
  const toggle = await screen.findByRole("button", { name: "What's new: 3 changes since your last visit" });
  expect(growthRead).toHaveBeenCalledWith(expect.objectContaining({ sinceMs: lastSeenAtMs + 1 }));
  await user.click(toggle);
  const drawer = await screen.findByRole("dialog", { name: "What's new" });
  expect(within(drawer).getByRole("button", { name: "Since last visit" })).toHaveAttribute("aria-pressed", "true");
  expect(await within(drawer).findByText(/→ now · \+2 on the map · 1 relationship$/)).toBeVisible();
  expect(within(drawer).getAllByRole("heading", { level: 3 }).map((heading) => heading.textContent)).toEqual(["Today", "Yesterday"]);
  await user.click(within(drawer).getByText("Zeiss").closest("button")!);
  expect(await screen.findByRole("heading", { name: "Zeiss", level: 2 })).toBeVisible();
  expect(screen.getByRole("button", { name: "Around Zeiss" })).toBeVisible();
});

it("says nothing is new and still lets the user widen the range", async () => {
  const { user } = await setup({ lastSeenAtMs: NOW - 86_400_000, events: [growth("entity_added", "zeiss", NOW - 10 * 86_400_000, "Zeiss")] });
  expect(screen.getByRole("button", { name: "What's new" })).toBeVisible();
  await user.click(screen.getByRole("button", { name: "What's new" }));
  const drawer = await screen.findByRole("dialog", { name: "What's new" });
  expect(await within(drawer).findByText(/^Nothing new since /)).toBeVisible();
  await user.click(within(drawer).getByRole("button", { name: "30 days" }));
  expect((await within(drawer).findByText("Zeiss")).closest("button")).toBeVisible();
  await user.keyboard("{Escape}");
  expect(screen.queryByRole("dialog", { name: "What's new" })).toBeNull();
  expect(screen.getByRole("button", { name: "What's new" })).toHaveFocus();
});

it("opens What's new when the morning message asks for it, then clears the request", async () => {
  const { location } = await setup({ lastSeenAtMs: NOW - 86_400_000, initialView: { whatsNew: true } });
  expect(await screen.findByRole("dialog", { name: "What's new" })).toBeVisible();
  expect(location()?.view?.whatsNew).toBeUndefined();
});
