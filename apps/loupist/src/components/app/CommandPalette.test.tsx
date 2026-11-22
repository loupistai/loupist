import i18n from "@/i18n";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { renderHook } from "@testing-library/react";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { MOCK_LOUPISTS } from "../../bridge/mock/fixtures";
import type { ChatThread, LoupistSummary } from "../../types";
import type { AttentionItem } from "../../contracts/attention";
import type { KnowledgeMapNode, Principle } from "../../contracts/knowledge";
import type { InboxItem } from "../../contracts/scheduled";
import { useAppShortcuts } from "../../app/useAppShortcuts";
import { SHORTCUTS, shortcutKeys } from "../../app/shortcuts";
import { KeyboardSettings } from "../settings/KeyboardSettings";
import { AssetPageOpenerProvider, type OpenAssetPage } from "../assets/assetOpener";
import { CommandPalette, SEARCH_LOUPISTS, type PaletteAction } from "./CommandPalette";

const loupist = MOCK_LOUPISTS[0]!;
const thread = (id: string, title: string, updated: string): ChatThread => ({ id, loupist_id: loupist.id, title, updated_at: updated, use_memory: true, update_memory: true, messages: [] });

function renderPalette(extra: { attention?: AttentionItem[]; actions?: PaletteAction[] } = {}) {
  const handlers = { onOpenAttention: vi.fn(), onOpenThread: vi.fn(), onOpenMapNode: vi.fn(), onOpenPrinciple: vi.fn(), onOpenBriefing: vi.fn(), onOpenChange: vi.fn() };
  const threads = [thread("t1", "Hanul margin review", "2026-10-28T00:00:00Z"), thread("t2", "Inventory days", "2026-10-27T00:00:00Z"),
    thread("t3", "Supplier map", "2026-10-26T00:00:00Z"), thread("t4", "Dividend check", "2026-10-25T00:00:00Z")];
  render(<CommandPalette open bridge={new MockLoupistBridge({ delay_ms: 0 })} loupists={[loupist]} selectedLoupistId={loupist.id}
    threadsByLoupist={{ [loupist.id]: threads }} attention={extra.attention ?? []} actions={extra.actions ?? []} {...handlers} />);
  return handlers;
}

it("shows recent chats and actions with an empty query, filters as the user types, and runs the highlighted row with the keyboard", async () => {
  const user = userEvent.setup();
  const newChat = vi.fn();
  const handlers = renderPalette({ actions: [{ id: "new-chat", label: "New research", keys: "⌘N", run: newChat }] });
  const list = screen.getByRole("listbox", { name: "Results" });
  // Three recent chats, newest first, then the actions.
  expect(within(list).getAllByRole("option").map((row) => row.textContent)).toEqual([
    expect.stringContaining("Hanul margin review"), expect.stringContaining("Inventory days"), expect.stringContaining("Supplier map"), expect.stringContaining("New research"),
  ]);
  const input = screen.getByRole("combobox", { name: "Search" });
  await user.type(input, "dividend");
  expect(within(list).getAllByRole("option").map((row) => row.textContent)).toEqual([expect.stringContaining("Dividend check")]);
  await user.keyboard("{Enter}");
  expect(handlers.onOpenThread).toHaveBeenCalledWith(loupist.id, "t4");
  expect(handlers.onOpenChange).toHaveBeenCalledWith(false);
});

it("finds chats by their titles first, and by their Loupist's name only from two characters", async () => {
  const user = userEvent.setup();
  renderPalette();
  const input = screen.getByRole("combobox", { name: "Search" });
  const chats = () => within(screen.getByRole("listbox", { name: "Results" })).queryByRole("group", { name: "Research sessions" });
  const labels = () => within(chats()!).getAllByRole("option").map((row) => row.querySelector(".command-palette-label")?.textContent);
  // "y" is in "Quality Compounder", but one letter does not list every chat of that Loupist.
  await user.type(input, "y");
  expect(labels()).toEqual(["Inventory days"]);
  await user.clear(input);
  await user.type(input, "c");
  expect(labels()).toEqual(["Dividend check"]);
  // Two characters of the name find the Loupist's chats, after those whose title matches.
  await user.clear(input);
  await user.type(input, "er");
  expect(labels()).toEqual(["Supplier map", "Hanul margin review", "Inventory days", "Dividend check"]);
});

it("says when nothing matches and what to try, as a status outside the results", async () => {
  const user = userEvent.setup();
  renderPalette();
  const status = screen.getByRole("status");
  expect(status).toBeEmptyDOMElement();
  const input = screen.getByRole("combobox", { name: "Search" });
  await user.type(input, "zzqx");
  expect(status).toHaveTextContent("Nothing matches. Try another word, a ticker or a session title.");
  expect(screen.queryByRole("listbox")).toBeNull();
  expect(input).toHaveAttribute("aria-expanded", "false");
  await user.clear(input);
  expect(status).toBeEmptyDOMElement();
  expect(screen.getByRole("listbox", { name: "Results" })).toBeVisible();
});

it("says nothing needs attention when opened on that group with nothing waiting", () => {
  render(<CommandPalette open focusGroup="attention" bridge={new MockLoupistBridge({ delay_ms: 0 })} loupists={[loupist]} selectedLoupistId={loupist.id}
    threadsByLoupist={{}} attention={[]} actions={[]} onOpenAttention={vi.fn()} onOpenThread={vi.fn()} onOpenMapNode={vi.fn()} onOpenPrinciple={vi.fn()} onOpenBriefing={vi.fn()} onOpenChange={vi.fn()} />);
  expect(screen.getByRole("status")).toHaveTextContent("Nothing needs attention right now.");
});

it("moves through rows with the arrow keys and folds long groups behind More", async () => {
  const user = userEvent.setup();
  const attention: AttentionItem[] = Array.from({ length: 6 }, (_, index) => ({ kind: "checkDue", loupistId: loupist.id, loupistName: loupist.name, recordId: `j${index}`, title: `Check ${index}`, dueDate: "2000-01-03", atMs: index }));
  const handlers = renderPalette({ attention });
  const list = screen.getByRole("listbox", { name: "Results" });
  const group = within(list).getByRole("group", { name: "Needs attention" });
  // Four rows, then "More from Needs attention".
  expect(within(group).getAllByRole("option")).toHaveLength(5);
  fireEvent.click(within(group).getByRole("option", { name: /More from Needs attention/ }));
  expect(within(within(list).getByRole("group", { name: "Needs attention" })).getAllByRole("option")).toHaveLength(6);
  const input = screen.getByRole("combobox", { name: "Search" });
  await user.click(input);
  await user.keyboard("{ArrowDown}{Enter}");
  expect(handlers.onOpenAttention).toHaveBeenCalledWith(attention[1]);
});

it("searches names on the active Loupist's World and its principles", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const map = await bridge.knowledgeMap({ loupistId: loupist.id, maxNodes: 1000 });
  const name = map.nodes.find((node) => node.kind === "entity" && !node.archived);
  if (!name) return;
  const handlers = { onOpenAttention: vi.fn(), onOpenThread: vi.fn(), onOpenMapNode: vi.fn(), onOpenPrinciple: vi.fn(), onOpenBriefing: vi.fn(), onOpenChange: vi.fn() };
  render(<CommandPalette open bridge={bridge} loupists={[loupist]} selectedLoupistId={loupist.id} threadsByLoupist={{}} attention={[]} actions={[]} {...handlers} />);
  await user.type(screen.getByRole("combobox", { name: "Search" }), name.label.slice(0, 4));
  const group = await waitFor(() => within(screen.getByRole("listbox", { name: "Results" })).getByRole("group", { name: "World" }));
  fireEvent.click(within(group).getAllByRole("option")[0]!);
  expect(handlers.onOpenMapNode).toHaveBeenCalledWith(loupist.id, expect.objectContaining({ kind: "entity" }));
});

describe("search across Loupists", () => {
  const [active, other, offline] = MOCK_LOUPISTS as [LoupistSummary, LoupistSummary, LoupistSummary];
  const entity = (id: string, label: string): KnowledgeMapNode => ({ id, label, kind: "entity", entityKind: "Organization", degree: 1, watched: false });
  const handlers = () => ({ onOpenAttention: vi.fn(), onOpenThread: vi.fn(), onOpenMapNode: vi.fn(), onOpenPrinciple: vi.fn(), onOpenBriefing: vi.fn(), onOpenChange: vi.fn() });

  async function searchBridge(names: Record<string, KnowledgeMapNode[]>) {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const base = await bridge.knowledgeMap({ loupistId: active.id, maxNodes: 1000 });
    const reads = vi.spyOn(bridge, "knowledgeMap").mockImplementation(async ({ loupistId }) => ({ ...base, nodes: names[loupistId] ?? [] }));
    vi.spyOn(bridge, "principleList").mockImplementation(async ({ loupistId }) => loupistId === other.id
      ? [{ recordId: "p-other", statement: "Hanul capex needs a payback date", archived: false } as Principle] : []);
    vi.spyOn(bridge, "inboxList").mockImplementation(async ({ loupistId }) => ({ nextCursor: null, items: loupistId === other.id
      ? [{ runId: "run-other", kind: "briefing", status: "succeeded", title: "Hanul weekly", startedAtMs: Date.UTC(2026, 9, 27), finishedAtMs: Date.UTC(2026, 9, 27) } as InboxItem] : [] }));
    return { bridge, reads };
  }

  it("lists each kind's results by Loupist, the active one first, and opens another Loupist's result with that Loupist", async () => {
    const user = userEvent.setup();
    const { bridge, reads } = await searchBridge({ [active.id]: [entity("a-hanul", "Hanul Precision")], [other.id]: [entity("o-hanul", "Hanul Holdings")], [offline.id]: [entity("x-hanul", "Hanul Offline")] });
    const opened = handlers();
    const loupists = [other, { ...offline, availability: { status: "workspace_unavailable" as const } }, active];
    render(<CommandPalette open bridge={bridge} loupists={loupists} selectedLoupistId={active.id} threadsByLoupist={{}} attention={[]} actions={[]} {...opened} />);
    await user.type(screen.getByRole("combobox", { name: "Search" }), "hanul");
    const list = screen.getByRole("listbox", { name: "Results" });
    const names = await waitFor(() => {
      const group = within(list).getByRole("group", { name: "World" });
      expect(within(group).getAllByRole("option")).toHaveLength(2);
      return group;
    });
    // The active Loupist first; each row names its Loupist. A folder that cannot be opened is not read.
    expect(within(names).getAllByRole("option").map((row) => row.textContent)).toEqual([`Hanul Precision${active.name}`, `Hanul Holdings${other.name}`]);
    expect(reads.mock.calls.map(([request]) => request.loupistId)).toEqual([active.id, other.id]);
    expect(within(within(list).getByRole("group", { name: "Principles" })).getByRole("option")).toHaveTextContent(`Hanul capex needs a payback date${other.name}`);
    expect(within(within(list).getByRole("group", { name: "Briefings" })).getByRole("option")).toHaveTextContent(`Hanul weekly${other.name} · Oct 27`);
    fireEvent.click(within(names).getByRole("option", { name: /Hanul Holdings/ }));
    expect(opened.onOpenMapNode).toHaveBeenCalledWith(other.id, expect.objectContaining({ id: "o-hanul" }));
    expect(opened.onOpenChange).toHaveBeenCalledWith(false);
  });

  it(`reads at most ${SEARCH_LOUPISTS} Loupists, and one that has not answered holds back no other`, async () => {
    const user = userEvent.setup();
    const many = Array.from({ length: SEARCH_LOUPISTS + 2 }, (_, index): LoupistSummary => ({ ...other, id: `loupist-${index}`, name: `Loupist ${index}` }));
    const { bridge, reads } = await searchBridge(Object.fromEntries(many.map((loupist) => [loupist.id, [entity(`${loupist.id}-hanul`, `Hanul ${loupist.name}`)]])));
    const answer = reads.getMockImplementation()!;
    reads.mockImplementation((request) => request.loupistId === "loupist-0" ? new Promise(() => undefined) : answer(request));
    render(<CommandPalette open bridge={bridge} loupists={many} selectedLoupistId="loupist-3" threadsByLoupist={{}} attention={[]} actions={[]} {...handlers()} />);
    await user.type(screen.getByRole("combobox", { name: "Search" }), "hanul");
    fireEvent.click(await screen.findByRole("option", { name: /More from World/ }));
    const names = () => within(within(screen.getByRole("listbox", { name: "Results" })).getByRole("group", { name: "World" })).getAllByRole("option");
    // The active Loupist leads; the one that has not answered and the two past the cap are missing.
    await waitFor(() => expect(names().map((row) => row.textContent)).toEqual([3, 1, 2, 4, 5, 6, 7].map((index) => `Hanul Loupist ${index}Loupist ${index}`)));
    expect(reads).toHaveBeenCalledTimes(SEARCH_LOUPISTS);
  });
});

it("opens the palette with ⌘K while the narrow navigation sheet is open", () => {
  const onPalette = vi.fn();
  renderHook(() => useAppShortcuts({ onSettings: vi.fn(), onNewChat: vi.fn(), onPalette, canCreateChat: true }));
  const sheet = document.createElement("div");
  sheet.setAttribute("role", "dialog");
  sheet.setAttribute("data-state", "open");
  sheet.setAttribute("data-sidebar", "sidebar");
  document.body.append(sheet);
  fireEvent.keyDown(window, { key: "k", metaKey: true });
  expect(onPalette).toHaveBeenCalledTimes(1);
  const dialog = document.createElement("div");
  dialog.setAttribute("role", "dialog");
  dialog.setAttribute("data-state", "open");
  document.body.append(dialog);
  fireEvent.keyDown(window, { key: "k", metaKey: true });
  expect(onPalette).toHaveBeenCalledTimes(1);
  sheet.remove();
  dialog.remove();
});

it("opens the palette with ⌘K and focuses the composer with a bare slash", () => {
  const onPalette = vi.fn();
  renderHook(() => useAppShortcuts({ onSettings: vi.fn(), onNewChat: vi.fn(), onPalette, canCreateChat: true }));
  fireEvent.keyDown(window, { key: "k", metaKey: true });
  expect(onPalette).toHaveBeenCalledTimes(1);
  const composer = document.createElement("textarea");
  composer.setAttribute("data-chat-composer", "");
  document.body.append(composer);
  fireEvent.keyDown(document.body, { key: "/" });
  expect(document.activeElement).toBe(composer);
  // A slash typed into a field stays in that field.
  const field = document.createElement("input");
  document.body.append(field);
  field.focus();
  fireEvent.keyDown(field, { key: "/" });
  expect(document.activeElement).toBe(field);
  composer.remove();
  field.remove();
});

it.each([{ platform: "MacIntel", mac: true }, { platform: "Win32", mac: false }])("lists the shortcuts supported on $platform in Settings › Keyboard", ({ platform, mac }) => {
  const reportedPlatform = vi.spyOn(navigator, "platform", "get").mockReturnValue(platform);
  try {
    render(<KeyboardSettings />);
    expect(screen.getAllByRole("row").slice(1)).toHaveLength(SHORTCUTS.filter(shortcut => shortcutKeys(shortcut, mac)).length);
    expect(screen.getByRole("row", { name: /^Search/ })).toBeInTheDocument();
    expect(screen.getByRole("row", { name: /^Bot · Following · World · Lens · Schedule/ })).toHaveTextContent(mac ? "⌘5" : "Ctrl+5");
    const hide = screen.queryByRole("row", { name: /^Hide Loupist/ });
    if (mac) expect(hide).toHaveTextContent("⌘H");
    else expect(hide).toBeNull();
  } finally {
    reportedPlatform.mockRestore();
  }
});

it("lists two follows of one ticker as two rows, each opening its own asset by id", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const first = `ast_${"1".repeat(16)}`, second = `ast_${"2".repeat(16)}`;
  bridge.assets.installAsset({ assetId: first, name: "Hanul Precision", ticker: "HNP", exchange: "KRX" });
  bridge.assets.installAsset({ assetId: second, name: "Hanul Works", ticker: "HNP", exchange: "KRX" });
  const openAsset = vi.fn<OpenAssetPage>();
  render(<AssetPageOpenerProvider value={openAsset}>
    <CommandPalette open bridge={bridge} loupists={[loupist]} selectedLoupistId={loupist.id} threadsByLoupist={{}} attention={[]} actions={[]}
      onOpenAttention={vi.fn()} onOpenThread={vi.fn()} onOpenMapNode={vi.fn()} onOpenPrinciple={vi.fn()} onOpenBriefing={vi.fn()} onOpenChange={vi.fn()} />
  </AssetPageOpenerProvider>);
  await user.type(screen.getByRole("combobox", { name: "Search" }), "hanul");
  const group = await screen.findByRole("group", { name: "Following" });
  const rows = within(group).getAllByRole("option");
  expect(rows.map((row) => [row.dataset.paletteRow, row.querySelector(".command-palette-label")?.textContent])).toEqual([
    [`following:${first}`, "Hanul Precision"], [`following:${second}`, "Hanul Works"]]);
  await user.click(rows[1]!);
  expect(openAsset).toHaveBeenCalledExactlyOnceWith({ loupistId: loupist.id, assetId: second, name: "Hanul Works" });
});

it("lists a Korean listing by the name its page shows in Korean, found by either name", async () => {
  await i18n.changeLanguage("ko");
  try {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const assetId = `ast_${"3".repeat(16)}`;
    bridge.assets.installAsset({ assetId, name: "Hanul Precision", ticker: "HNP", exchange: "KS" });
    const map = await bridge.knowledgeMap({ loupistId: loupist.id, maxNodes: 1000 });
    vi.spyOn(bridge, "knowledgeMap").mockResolvedValue({ ...map, nodes: [{ id: "e-hanul", kind: "entity", label: "Hanul Precision", localNames: ["한울정밀"], entityKind: "Organization", degree: 1, watched: false }] });
    const openAsset = vi.fn<OpenAssetPage>();
    render(<AssetPageOpenerProvider value={openAsset}>
      <CommandPalette open bridge={bridge} loupists={[loupist]} selectedLoupistId={loupist.id} threadsByLoupist={{}} attention={[]} actions={[]}
        onOpenAttention={vi.fn()} onOpenThread={vi.fn()} onOpenMapNode={vi.fn()} onOpenPrinciple={vi.fn()} onOpenBriefing={vi.fn()} onOpenChange={vi.fn()} />
    </AssetPageOpenerProvider>);
    const input = screen.getByRole("combobox");
    await user.type(input, "hanul");
    await waitFor(() => expect(within(screen.getByRole("group", { name: "관심 자산" })).getByRole("option").querySelector(".command-palette-label")).toHaveTextContent("한울정밀"));
    await user.clear(input);
    await user.type(input, "한울");
    expect(within(screen.getByRole("group", { name: "관심 자산" })).getByRole("option")).toHaveTextContent("한울정밀");
    await user.click(within(screen.getByRole("group", { name: "관심 자산" })).getByRole("option"));
    expect(openAsset).toHaveBeenCalledExactlyOnceWith({ loupistId: loupist.id, assetId, name: "한울정밀" });
  } finally { await i18n.changeLanguage("en"); }
});

it("captions a follow by its ticker only when its reference has one", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const local = await bridge.assetFollowName({ loupistId: loupist.id, name: "Harbor Fund" });
  bridge.assets.installAsset({ name: "Harbor Logistics", ticker: "HRBR", exchange: "NYSE", exchangeName: "New York Stock Exchange" });
  const openAsset = vi.fn<OpenAssetPage>();
  render(<AssetPageOpenerProvider value={openAsset}>
    <CommandPalette open bridge={bridge} loupists={[loupist]} selectedLoupistId={loupist.id} threadsByLoupist={{}} attention={[]} actions={[]}
      onOpenAttention={vi.fn()} onOpenThread={vi.fn()} onOpenMapNode={vi.fn()} onOpenPrinciple={vi.fn()} onOpenBriefing={vi.fn()} onOpenChange={vi.fn()} />
  </AssetPageOpenerProvider>);
  await user.type(screen.getByRole("combobox", { name: "Search" }), "harbor");
  const group = await screen.findByRole("group", { name: "Following" });
  const rows = within(group).getAllByRole("option");
  expect(rows.map((row) => [row.querySelector(".command-palette-label")?.textContent, row.querySelector("small")?.textContent])).toEqual([
    ["Harbor Fund", undefined], ["Harbor Logistics", "HRBR · New York Stock Exchange"]]);
  await user.click(rows[0]!);
  expect(openAsset).toHaveBeenCalledExactlyOnceWith({ loupistId: loupist.id, assetId: local.assetId, name: "Harbor Fund" });
});

it("names an untitled research session as the sidebar does", () => {
  render(<CommandPalette open bridge={new MockLoupistBridge({ delay_ms: 0 })} loupists={[loupist]} selectedLoupistId={loupist.id}
    threadsByLoupist={{ [loupist.id]: [thread("t9", "", "2026-10-29T00:00:00Z")] }} attention={[]} actions={[]}
    onOpenAttention={vi.fn()} onOpenThread={vi.fn()} onOpenMapNode={vi.fn()} onOpenPrinciple={vi.fn()} onOpenBriefing={vi.fn()} onOpenChange={vi.fn()} />);
  const list = screen.getByRole("listbox", { name: "Results" });
  expect(within(list).getAllByRole("option")[0]).toHaveTextContent("Untitled research session");
});

it("groups the home thread as Bot when searching by Loupist name", async () => {
  const user = userEvent.setup();
  const openThread = vi.fn();
  const home = { ...thread("home", "Bot", "2026-10-28T00:00:00Z"), home: true };
  render(<CommandPalette open bridge={new MockLoupistBridge({ delay_ms: 0 })} loupists={[loupist]} selectedLoupistId={loupist.id}
    threadsByLoupist={{ [loupist.id]: [home] }} attention={[]} actions={[]} onOpenAttention={vi.fn()} onOpenThread={openThread}
    onOpenMapNode={vi.fn()} onOpenPrinciple={vi.fn()} onOpenBriefing={vi.fn()} onOpenChange={vi.fn()} />);
  await user.type(screen.getByRole("combobox"), loupist.name);
  expect(within(screen.getByRole("group", { name: "Bot" })).getByRole("option")).toHaveTextContent(loupist.name);
  expect(screen.queryByRole("group", { name: "Research sessions" })).toBeNull();
  await user.keyboard("{Enter}");
  expect(openThread).toHaveBeenCalledWith(loupist.id, home.id);
});
