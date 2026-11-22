import { useImperativeHandle, useState, type Ref } from "react";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { GrowthEvent, KnowledgeMap, KnowledgeMapEdge, KnowledgeMapNode } from "../../contracts/knowledge";
import type { KnowledgeLocation } from "../../types";
import { MapView } from "./MapView";
import { PreviewBridge } from "../../preview/fixtures";
import { prepareConflictsFixture } from "../../preview/fixtures/conflicts";

const fit = vi.hoisted(() => vi.fn());
vi.mock("./MapCanvas", () => ({ default: function Canvas({ nodes, controls, highlightIds, onSelect }: {
  nodes: KnowledgeMapNode[]; controls: Ref<{ fit: () => void; focus: () => void }>;
  highlightIds?: ReadonlySet<string>; onSelect: (node: KnowledgeMapNode) => void;
}) {
  useImperativeHandle(controls, () => ({ fit, focus: () => {} }), []);
  const neighbor = nodes.find(node => node.id === "asml");
  return <div data-testid="map-canvas" data-ids={nodes.map(node => node.id).sort().join(",")} data-highlight={[...(highlightIds ?? [])].join(",")}>
    {neighbor && <button onClick={() => onSelect(neighbor)}>Select ASML on canvas</button>}
  </div>;
} }));

const NOW = Date.now();
const entity = (id: string, label: string, extra: Partial<KnowledgeMapNode> = {}): KnowledgeMapNode => ({ id, label, kind: "entity", entityKind: "Organization", degree: 0, watched: false, ...extra });
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
  return { nodes, edges, counts, unmatchedWatchlist: [], worldRevisionId: "local-test", generatedAtMs: NOW };
}

const growth = (kind: GrowthEvent["kind"], subjectId: string, atMs: number, summary = ""): GrowthEvent => ({ eventId: `${kind}:${subjectId}`, loupistId: "loupist-quality", atMs, kind, subjectId, recordId: "world", originChatId: null, originMessageId: null, actor: "loupist", summary, beforeSummary: null, afterSummary: null });

async function setup({ events = [] as GrowthEvent[], lastSeenAtMs, initialView, entryId }: { events?: GrowthEvent[]; lastSeenAtMs?: number; entryId?: string; initialView?: KnowledgeLocation["view"] } = {}) {
  if (lastSeenAtMs !== undefined) localStorage.setItem("loupist.loupistVisit.v1", JSON.stringify({ "loupist-quality": { lastSeenAtMs } }));
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const catalog = await bridge.knowledgeCatalog({ loupistId: "loupist-quality" });
  vi.spyOn(bridge, "knowledgeMap").mockResolvedValue(localMap());
  const growthRead = vi.spyOn(bridge, "knowledgeGrowthEvents").mockImplementation(async (request) => ({ events: events.filter((item) => request.sinceMs === undefined || item.atMs >= request.sinceMs), nextCursor: null }));
  const onOpen = vi.fn();
  let current: KnowledgeLocation | undefined;
  function Harness() {
    const [location, setLocation] = useState<KnowledgeLocation>({ loupistId: catalog.loupistId, surface: "world", recordId: catalog.worlds[0]?.recordId, revisionId: catalog.worlds[0]?.revisionId, selection: entryId ? { kind: "node", id: entryId } : undefined, view: initialView });
    current = location;
    return <MapView initialAnchor={entryId} bridge={bridge} catalog={catalog} location={location} onPatch={setLocation} onOpen={onOpen} onAsk={vi.fn()} onAskInChat={vi.fn()} />;
  }
  const { unmount } = render(<Harness />);
  await screen.findByTestId("map-canvas");
  return { unmount, user: userEvent.setup(), growthRead, onOpen, location: () => current };
}
const drawn = () => screen.getByTestId("map-canvas").dataset.ids!.split(",");

beforeEach(() => localStorage.clear());

it("draws a highlighted path even when it lies outside the focal neighbourhood of a large World", async () => {
  await setup({ entryId: "tsmc", initialView: { highlightEntityIds: ["filler-3", "filler-4", "filler-5"] } });
  expect(drawn()).toEqual(["amat", "asml", "filler-3", "filler-4", "filler-5", "shin", "tsmc"]);
});

it("starts each visit on All, ignores legacy saved scopes and requires a name for either neighbourhood", async () => {
  localStorage.setItem("loupist.mapVisit.v1", JSON.stringify({ "loupist-quality": { focalId: "tsmc", scope: "2" } }));
  const { user, unmount } = await setup();
  expect(drawn()).toHaveLength(78);
  for (const name of ["Direct connections", "One step further"]) {
    const button = screen.getByRole("button", { name });
    expect(button).toBeDisabled();
    expect(button).toHaveAccessibleDescription("Pick something first");
  }
  await user.click(screen.getByRole("button", { name: "Whole World" }));
  await user.click(within(screen.getByRole("region", { name: "Following" })).getByRole("button", { name: "TSMC" }));
  expect(drawn()).toHaveLength(4);
  unmount();
  await setup();
  expect(drawn()).toHaveLength(78);
  expect(screen.getByRole("button", { name: "Whole World" })).toBeVisible();
});

it("opens an entity link at one hop, widens to two, and clears the anchor on All while keeping details", async () => {
  const { user } = await setup({ entryId: "tsmc" });
  expect(screen.getByRole("button", { name: "Around TSMC" })).toBeVisible();
  expect(drawn()).toEqual(["amat", "asml", "shin", "tsmc"]);
  expect(screen.getByText("4 of 78 in World around TSMC")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "One step further" }));
  expect(drawn()).toEqual(["amat", "asml", "lam", "shin", "sumco", "tsmc", "zeiss"]);
  await user.click(screen.getByRole("button", { name: "All" }));
  expect(drawn()).toHaveLength(78);
  expect(screen.queryByText(/in World around TSMC/)).toBeNull();
  expect(screen.getByRole("button", { name: "Whole World" })).toBeVisible();
  expect(screen.getByRole("heading", { name: "TSMC" })).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Show its connections" }));
  expect(drawn()).toHaveLength(4);
});

it("re-centers from the Around menu and when a search result lies outside the drawn names", async () => {
  const { user } = await setup();
  await user.click(screen.getByRole("button", { name: "Whole World" }));
  // The followed names lead the Around menu under a heading that says only that.
  expect(within(await screen.findByRole("region", { name: "Following" })).getByRole("button", { name: "TSMC" })).toBeVisible();
  await user.click(await screen.findByRole("button", { name: "Pick in World…" }));
  expect(screen.getByText("Click a name in World to center on it.")).toBeVisible();
  await user.keyboard("{Escape}");
  expect(screen.queryByText("Click a name in World to center on it.")).toBeNull();
  await user.type(screen.getByRole("combobox"), "Lonely");
  await user.click(await screen.findByRole("option", { name: /Lonely Corp/ }));
  expect(await screen.findByRole("button", { name: "Around Lonely Corp" })).toBeVisible();
  expect(drawn()).toEqual(["lonely"]);
  expect(screen.getByText("Lonely Corp has no relationships yet. Ask about it in Chat.")).toBeVisible();
});

it("centers Direct connections on the open entity and on a search result already in view", async () => {
  const { user } = await setup();
  const search = async (name: string) => {
    const field = screen.getByRole("combobox");
    await user.clear(field);
    await user.type(field, name);
    await user.click(await screen.findByRole("option", { name: new RegExp(name) }));
    expect(await screen.findByRole("heading", { name })).toBeVisible();
  };
  await user.click(screen.getByRole("button", { name: "All" }));
  await search("Zeiss");
  expect(screen.getByRole("button", { name: "Direct connections" })).toHaveAttribute("aria-pressed", "true");
  expect(screen.getByRole("button", { name: "Around Zeiss" })).toBeVisible();
  expect(drawn()).toEqual(["asml", "zeiss"]);
  await user.click(screen.getByRole("button", { name: "One step further" }));
  fit.mockClear();
  await search("ASML");
  await waitFor(() => expect(fit).toHaveBeenCalled());
  expect(screen.getByRole("button", { name: "Around ASML" })).toBeVisible();
  expect(drawn()).toEqual(["asml", "tsmc", "zeiss"]);

  await user.click(screen.getByRole("button", { name: "More actions" }));
  await user.click(screen.getByRole("menuitemcheckbox", { name: "List view" }));
  await user.click(within(screen.getByRole("table", { name: "List view" })).getByRole("button", { name: "TSMC" }));
  expect(await screen.findByRole("heading", { name: "TSMC" })).toBeVisible();
  expect(screen.getByRole("button", { name: "Around ASML" })).toBeVisible();
});

it("keeps an off-overview search result and its exact connections after closing its details", async () => {
  const bridge = new PreviewBridge({ delay_ms: 0 });
  await prepareConflictsFixture(bridge);
  const catalog = await bridge.knowledgeCatalog({ loupistId: "loupist-quality" });
  const full = await bridge.knowledgeMap({ loupistId: catalog.loupistId });
  const [first, second] = full.nodes;
  vi.spyOn(bridge, "knowledgeMap").mockResolvedValue({ ...full, nodes: [second!], edges: [] });
  function Harness() {
    const [location, setLocation] = useState<KnowledgeLocation>({ loupistId: catalog.loupistId, surface: "world", recordId: "preview-world", revisionId: "preview-revision" });
    return <MapView bridge={bridge} catalog={catalog} location={location} onPatch={setLocation} onOpen={vi.fn()} onAsk={vi.fn()} />;
  }
  render(<Harness />);
  const user = userEvent.setup();
  await user.type(await screen.findByRole("combobox"), first!.label);
  await user.click(await screen.findByRole("option", { name: new RegExp(first!.label) }));
  await waitFor(() => expect(drawn()).toEqual(expect.arrayContaining([first!.id, second!.id])));
  await user.click(screen.getByRole("button", { name: "Close inspector" }));
  expect(screen.getByRole("button", { name: `Around ${first!.label}` })).toBeVisible();
  expect(drawn()).toEqual(expect.arrayContaining([first!.id, second!.id]));
});

it("selects a node on the whole World without scoping until its action or Pick in World is used", async () => {
  const { user } = await setup();
  await user.click(screen.getByRole("button", { name: "Select ASML on canvas" }));
  expect(screen.getByRole("heading", { name: "ASML" })).toBeVisible();
  expect(drawn()).toHaveLength(78);
  await user.click(screen.getByRole("button", { name: "Show its connections" }));
  expect(drawn()).toEqual(["asml", "tsmc", "zeiss"]);
  await user.click(screen.getByRole("button", { name: "All" }));
  await user.click(screen.getByRole("button", { name: "Whole World" }));
  expect(screen.getByRole("button", { name: "ASML" })).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Pick in World…" }));
  await user.click(screen.getByRole("button", { name: "Select ASML on canvas" }));
  expect(drawn()).toEqual(["asml", "tsmc", "zeiss"]);
});

it("replays the whole World and restores the neighbourhood when replay closes", async () => {
  const { user } = await setup({ entryId: "tsmc" });
  await user.click(screen.getByRole("button", { name: "More actions" }));
  await user.click(screen.getByRole("menuitemcheckbox", { name: "Replay" }));
  expect(drawn()).toHaveLength(78);
  await user.click(screen.getByRole("button", { name: "Exit replay" }));
  expect(drawn()).toHaveLength(4);
});

it("includes the searched anchor when a filter would hide it", async () => {
  const { user } = await setup();
  await user.click(screen.getByRole("button", { name: "Filter" }));
  await user.click(screen.getByLabelText("Only what I follow"));
  await user.keyboard("{Escape}");
  await user.type(screen.getByRole("combobox"), "ASML");
  await user.click(await screen.findByRole("option", { name: /ASML/ }));
  expect(screen.getByRole("button", { name: "Around ASML" })).toBeVisible();
  expect(drawn()).toEqual(["asml", "tsmc"]);
});

it("moves between neighbors, re-centers and steps back with the keyboard", async () => {
  const { user } = await setup({ entryId: "tsmc" });
  const stage = screen.getByRole("group", { name: /World around TSMC/ });
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
  expect(screen.getByRole("button", { name: "One step further" })).toHaveAttribute("aria-pressed", "true");
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
  expect(await within(drawer).findByText(/→ now · \+2 in World · 1 relationship$/)).toBeVisible();
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
