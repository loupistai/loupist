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
import { SHORTCUTS } from "../../app/shortcuts";
import { KeyboardSettings } from "../settings/KeyboardSettings";
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
  const list = screen.getByRole("listbox", { name: "Command palette" });
  // Three recent chats, newest first, then the actions.
  expect(within(list).getAllByRole("option").map((row) => row.textContent)).toEqual([
    expect.stringContaining("Hanul margin review"), expect.stringContaining("Inventory days"), expect.stringContaining("Supplier map"), expect.stringContaining("New research"),
  ]);
  const input = screen.getByRole("combobox", { name: "Command palette" });
  await user.type(input, "dividend");
  expect(within(list).getAllByRole("option").map((row) => row.textContent)).toEqual([expect.stringContaining("Dividend check")]);
  await user.keyboard("{Enter}");
  expect(handlers.onOpenThread).toHaveBeenCalledWith(loupist.id, "t4");
  expect(handlers.onOpenChange).toHaveBeenCalledWith(false);
});

it("moves through rows with the arrow keys and folds long groups behind More", async () => {
  const user = userEvent.setup();
  const attention: AttentionItem[] = Array.from({ length: 6 }, (_, index) => ({ kind: "checkDue", loupistId: loupist.id, loupistName: loupist.name, recordId: `j${index}`, title: `Check ${index}`, dueDate: "2000-01-03", atMs: index }));
  const handlers = renderPalette({ attention });
  const list = screen.getByRole("listbox", { name: "Command palette" });
  const group = within(list).getByRole("group", { name: "Needs attention" });
  // Four rows, then "More from Needs attention".
  expect(within(group).getAllByRole("option")).toHaveLength(5);
  fireEvent.click(within(group).getByRole("option", { name: /More from Needs attention/ }));
  expect(within(within(list).getByRole("group", { name: "Needs attention" })).getAllByRole("option")).toHaveLength(6);
  const input = screen.getByRole("combobox", { name: "Command palette" });
  await user.click(input);
  await user.keyboard("{ArrowDown}{Enter}");
  expect(handlers.onOpenAttention).toHaveBeenCalledWith(attention[1]);
});

it("searches names on the active Loupist's Map and its principles", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const map = await bridge.knowledgeMap({ loupistId: loupist.id, maxNodes: 1000 });
  const name = map.nodes.find((node) => node.kind === "entity" && !node.archived);
  if (!name) return;
  const handlers = { onOpenAttention: vi.fn(), onOpenThread: vi.fn(), onOpenMapNode: vi.fn(), onOpenPrinciple: vi.fn(), onOpenBriefing: vi.fn(), onOpenChange: vi.fn() };
  render(<CommandPalette open bridge={bridge} loupists={[loupist]} selectedLoupistId={loupist.id} threadsByLoupist={{}} attention={[]} actions={[]} {...handlers} />);
  await user.type(screen.getByRole("combobox", { name: "Command palette" }), name.label.slice(0, 4));
  const group = await waitFor(() => within(screen.getByRole("listbox", { name: "Command palette" })).getByRole("group", { name: "Map" }));
  fireEvent.click(within(group).getAllByRole("option")[0]!);
  expect(handlers.onOpenMapNode).toHaveBeenCalledWith(loupist.id, expect.objectContaining({ kind: "entity" }));
});

describe("search across Loupists", () => {
  const [active, other, offline] = MOCK_LOUPISTS as [LoupistSummary, LoupistSummary, LoupistSummary];
  const entity = (id: string, label: string): KnowledgeMapNode => ({ id, label, kind: "entity", entityKind: "Organization", degree: 1, watched: false, held: false });
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
    await user.type(screen.getByRole("combobox", { name: "Command palette" }), "hanul");
    const list = screen.getByRole("listbox", { name: "Command palette" });
    const names = await waitFor(() => {
      const group = within(list).getByRole("group", { name: "Map" });
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
    await user.type(screen.getByRole("combobox", { name: "Command palette" }), "hanul");
    fireEvent.click(await screen.findByRole("option", { name: /More from Map/ }));
    const names = () => within(within(screen.getByRole("listbox", { name: "Command palette" })).getByRole("group", { name: "Map" })).getAllByRole("option");
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

it("lists every shortcut the app handles on Settings › Keyboard", () => {
  render(<KeyboardSettings />);
  const rows = screen.getAllByRole("row").slice(1);
  expect(rows).toHaveLength(SHORTCUTS.length);
  expect(screen.getByRole("row", { name: /Command palette/ })).toBeInTheDocument();
});
