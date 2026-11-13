import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { TooltipProvider } from "../ui/tooltip";
import type { ChatThread, LoupistSummary } from "../../types";
import { MemoryPage } from "./MemoryPage";

async function fixture(setup?: (bridge: MockLoupistBridge, loupist: LoupistSummary) => Promise<void>) {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const [loupist] = await bridge.loupistList();
  await setup?.(bridge, loupist);
  const threads: ChatThread[] = [{ id: "chat-margin", loupist_id: loupist.id, title: "Hanul margin review", updated_at: "2026-10-28T00:00:00Z", use_memory: true, update_memory: true, messages: [] }];
  const handlers = { onOpenLoupistSettings: vi.fn(), onOpenChat: vi.fn(), onOpenMap: vi.fn(), onLoupistUpdated: vi.fn(), onOpenLink: vi.fn(), onOpenKnowledge: vi.fn(), onAsk: vi.fn() };
  const view = (initialItem?: string) => <TooltipProvider><MemoryPage bridge={bridge} loupist={loupist} threads={threads} initialItem={initialItem} {...handlers} /></TooltipProvider>;
  return { bridge, loupist, handlers, view, user: userEvent.setup() };
}

it("opens on Pages with your principles grouped in, shows where one was used, and asks your Loupist to revise it", async () => {
  let recordId = "";
  const { handlers, view, user } = await fixture(async (bridge, loupist) => {
    recordId = (await bridge.principleCreate({ loupistId: loupist.id, statement: "Prefer companies that publish segment margins.", origin: "settings" })).recordId;
    (bridge as unknown as { state: { memoryUsage: Record<string, unknown> } }).state.memoryUsage = { [loupist.id]: [{ recordId, usedCount: 4, lastUsedAtMs: Date.UTC(2026, 9, 28), usedIn: [{ chatId: "chat-margin", atMs: Date.UTC(2026, 9, 28) }] }] };
  });

  render(view());
  const page = await screen.findByRole("region", { name: /What .* knows/ });
  const tabs = within(page).getByRole("tablist");
  expect(within(tabs).getByRole("tab", { name: /Pages/ })).toHaveAttribute("aria-selected", "true");
  // Lens is Pages and History; what you follow lives on the Following page.
  expect(within(tabs).getAllByRole("tab").map(tab => tab.textContent)).toEqual([expect.stringMatching(/^Pages/), expect.stringMatching(/^History/)]);
  await user.click(await within(page).findByRole("button", { name: /Prefer companies that publish segment margins.*Principle, Used 4 times/ }));
  const reader = within(page).getByRole("complementary", { name: /Prefer companies that publish segment margins/ });
  await user.click(within(reader).getByRole("button", { name: "Hanul margin review" }));
  expect(handlers.onOpenChat).toHaveBeenCalledWith("chat-margin");
  await user.click(within(reader).getByRole("button", { name: "Ask Bot to revise" }));
  expect(handlers.onAsk).toHaveBeenCalledWith(expect.objectContaining({ recordId, kind: "lens" }));
});

it("says learning is paused when nothing new is saved, and Resume turns both switches back on", async () => {
  const { bridge, loupist, view, user } = await fixture(async (bridge, loupist) => {
    const scope = { kind: "project" as const, loupistId: loupist.id };
    const current = await bridge.configRead({ scope });
    await bridge.configUpdate({ scope, expectedRevision: current.revision, set: { chat: { use_memory: false, update_memory: false } }, unset: [] });
    await bridge.memoryLearningSettingsSet({ loupistId: loupist.id, learnMethods: false });
  });
  const learn = vi.spyOn(bridge, "memoryLearningSettingsSet");
  render(view());
  const banner = await screen.findByText(/Learning is paused/);
  await user.click(within(banner.closest("[role=status]")!).getByRole("button", { name: "Resume" }));
  expect(learn).toHaveBeenCalledWith({ loupistId: loupist.id, learnMethods: true });
  await waitFor(() => expect(screen.queryByText(/Learning is paused/)).toBeNull());
  expect(screen.queryByRole("switch", { name: "Remember new conversations" })).toBeNull();
  const config = await bridge.configRead({ scope: { kind: "project", loupistId: loupist.id } });
  expect(config.effective.chat.use_memory && config.effective.chat.update_memory).toBe(true);
});

it("shows the reset audit in History without treating it as an archived method", async () => {
  const { bridge, loupist, view, user } = await fixture();
  vi.spyOn(bridge, "knowledgeGrowthEvents").mockResolvedValue({ events: [{
    eventId: "reset-1", loupistId: loupist.id, atMs: Date.now(), kind: "lens_archived",
    actor: "user", recordId: null, subjectId: null, originChatId: null, originMessageId: null,
    origin: { kind: "memory_reset", counts: { principles: 0, pages: 1, sections: 0, names: 2, relationships: 1, sources: 1 } },
    summary: "Reset by you", beforeSummary: null, afterSummary: null,
  }], nextCursor: null });
  render(view());
  await user.click(await screen.findByRole("tab", { name: /History/ }));
  expect(await screen.findByRole("button", { name: /Reset by you/ })).toBeVisible();
});

it("opens straight on an item from a receipt or a saved chip", async () => {
  let recordId = "";
  const { view } = await fixture(async (bridge, loupist) => {
    recordId = (await bridge.principleCreate({ loupistId: loupist.id, statement: "Treat one customer above 40% of revenue as a concentration risk.", origin: "feedback", originChatId: "chat-margin", originMessageId: "answer-1" })).recordId;
  });
  render(view(recordId));
  const reader = await screen.findByRole("complementary", { name: /Treat one customer above 40%/ });
  expect(within(reader).getByText(/You said “Not helpful”/)).toBeVisible();
});

it("pauses and resumes a principle and a method through the host", async () => {
  let principleId = "";
  const { bridge, loupist, view, user } = await fixture(async (bridge, loupist) => {
    principleId = (await bridge.principleCreate({ loupistId: loupist.id, statement: "Check cash before profit.", origin: "settings" })).recordId;
  });
  const pause = vi.spyOn(bridge, "knowledgeLensPause");
  render(view(principleId));
  const rule = await screen.findByRole("complementary", { name: "Check cash before profit." });
  await user.click(within(rule).getByRole("button", { name: "Pause" }));
  await user.click(await within(rule).findByRole("button", { name: "Resume" }));
  await waitFor(() => expect(pause).toHaveBeenLastCalledWith({ loupistId: loupist.id, recordId: principleId, paused: false }));
  const catalog = await bridge.knowledgeCatalog({ loupistId: loupist.id });
  const method = catalog.lenses.find(item => item.lensKind === "method")!;
  await user.click(await screen.findByRole("button", { name: new RegExp(method.title) }));
  const reader = await screen.findByRole("complementary", { name: method.title });
  await user.click(within(reader).getByRole("button", { name: "Pause" }));
  expect(pause).toHaveBeenLastCalledWith({ loupistId: loupist.id, recordId: method.recordId, paused: true });
  await user.click(await within(reader).findByRole("button", { name: "Resume" }));
  await waitFor(() => expect(pause).toHaveBeenLastCalledWith({ loupistId: loupist.id, recordId: method.recordId, paused: false }));
});

it("groups pages by kind and narrows them with search", async () => {
  const { view, user } = await fixture();
  render(view());
  await screen.findByRole("region", { name: /What .* knows/ });
  const groups = await screen.findAllByRole("region", { name: /^(Topics|Ideas|Methods|Your principles|Perspective)$/ });
  expect(groups.map(group => group.getAttribute("aria-label"))).toEqual(["Topics", "Ideas", "Methods", "Your principles", "Perspective"]);
  const header = within(groups[0]!).getByRole("heading", { level: 3 });
  expect(header.children).toHaveLength(3);
  expect(header.children[0]).toHaveTextContent(/^Topics$/);
  expect(header.children[1]).toHaveAttribute("aria-hidden", "true");
  expect(header.children[2]).toHaveTextContent(/^1 page$/);
  expect(within(groups[0]!).getByRole("button", { name: /^TCP, Topic/ })).toBeVisible();
  expect(within(groups[1]!).getByRole("button", { name: /QUIC could displace TCP/ })).toBeVisible();
  await user.type(screen.getByRole("searchbox", { name: "Search what it knows" }), "head-of-line");
  expect(await screen.findByRole("button", { name: /^TCP, Topic/ })).toBeVisible();
  expect(screen.queryByRole("button", { name: /Compare independent accounts/ })).toBeNull();
  await user.clear(screen.getByRole("searchbox", { name: "Search what it knows" }));
  await user.type(screen.getByRole("searchbox", { name: "Search what it knows" }), "zzzz");
  expect(await screen.findByText("No matching pages")).toBeVisible();
});

it("shows the home Thread as Bot in Used in and in History, never its stored title", async () => {
  let recordId = "";
  const { bridge, loupist, handlers, user } = await fixture(async (bridge, loupist) => {
    recordId = (await bridge.principleCreate({ loupistId: loupist.id, statement: "Check cash before profit.", origin: "settings" })).recordId;
    (bridge as unknown as { state: { memoryUsage: Record<string, unknown> } }).state.memoryUsage = { [loupist.id]: [{ recordId, usedCount: 1, lastUsedAtMs: Date.UTC(2026, 9, 28), usedIn: [{ chatId: "home-chat", atMs: Date.UTC(2026, 9, 28) }] }] };
  });
  vi.spyOn(bridge, "knowledgeGrowthEvents").mockResolvedValue({ events: [{
    eventId: "filed-1", loupistId: loupist.id, atMs: Date.UTC(2026, 9, 28), kind: "lens_revised", actor: "loupist", recordId: "lens-subject-tcp", subjectId: null,
    originChatId: "home-chat", originMessageId: "answer-1", summary: "TCP", beforeSummary: null, afterSummary: null,
  }], nextCursor: null });
  const threads = [{ id: "home-chat", loupist_id: loupist.id, home: true, title: "Thread", updated_at: "2026-10-28T00:00:00Z", use_memory: true, update_memory: true, messages: [] }];
  render(<TooltipProvider><MemoryPage bridge={bridge} loupist={loupist} threads={threads} initialItem={recordId} {...handlers} /></TooltipProvider>);
  const reader = await screen.findByRole("complementary", { name: "Check cash before profit." });
  await user.click(within(reader).getByRole("button", { name: "Bot" }));
  expect(handlers.onOpenChat).toHaveBeenCalledWith("home-chat");
  expect(within(reader).queryByText("Thread")).toBeNull();
  await user.click(screen.getByRole("tab", { name: /History/ }));
  expect(await screen.findByRole("button", { name: /Filed after: Bot/ })).toBeVisible();
  expect(screen.queryByText(/Thread/)).toBeNull();
});

it("returns to the page after opening a Source from its basis chip", async () => {
  const { handlers, view, user } = await fixture();
  render(view("lens-subject-tcp"));
  await screen.findByRole("heading", { name: "TCP" });
  const overview = (await screen.findByRole("heading", { name: "Overview" })).closest("section")!;
  await user.click(within(overview).getByRole("button", { name: /^Source · / }));
  expect(await screen.findByRole("button", { name: "Back to page" })).toBeVisible();
  expect(handlers.onOpenKnowledge).not.toHaveBeenCalled();
  await user.click(screen.getByRole("button", { name: "Back to page" }));
  expect(await screen.findByRole("heading", { name: "TCP" })).toBeVisible();
  expect(screen.getByRole("tab", { name: /Pages/, selected: true })).toBeVisible();
});

it("never takes focus for the page it opens by itself, and focuses the page you open", async () => {
  const { view, user } = await fixture();
  render(<><button type="button">Elsewhere</button>{view()}</>);
  const elsewhere = screen.getByRole("button", { name: "Elsewhere" });
  elsewhere.focus();
  expect(await screen.findByRole("heading", { level: 2, name: "TCP" })).toBeVisible();
  expect(elsewhere).toHaveFocus();
  await user.click(screen.getByRole("button", { name: /^QUIC could displace TCP for mobile traffic, Idea/ }));
  await waitFor(() => expect(screen.getByRole("heading", { level: 2, name: /QUIC could displace TCP/ })).toHaveFocus());
});


it.each(["memoryLearningSettingsSet", "configUpdate"] as const)("reports a failed %s when resuming learning", async method => {
  const { bridge, view, user } = await fixture(async (bridge, loupist) => {
    const scope = { kind: "project" as const, loupistId: loupist.id };
    const current = await bridge.configRead({ scope });
    await bridge.configUpdate({ scope, expectedRevision: current.revision, set: { chat: { use_memory: false, update_memory: false } }, unset: [] });
    await bridge.memoryLearningSettingsSet({ loupistId: loupist.id, learnMethods: false });
  });
  vi.spyOn(bridge, method).mockRejectedValueOnce(new Error("write failed"));
  render(view());
  await user.click(await screen.findByRole("button", { name: "Resume" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't resume learning. Check the learning settings and try again.");
});


it("debounces Lens search while the user types and cancels a cleared query", async () => {
  const { bridge, view } = await fixture();
  const search = vi.spyOn(bridge, "knowledgeLensSearch").mockResolvedValue([]);
  const rendered = render(view());
  const input = await screen.findByRole("searchbox");
  vi.useFakeTimers();
  try {
    fireEvent.change(input, { target: { value: "cash" } });
    fireEvent.change(input, { target: { value: "cash flow" } });
    expect(search).not.toHaveBeenCalled();
    await act(async () => vi.advanceTimersByTimeAsync(200));
    expect(search).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ query: "cash flow" }));
    fireEvent.change(input, { target: { value: "margin" } });
    fireEvent.change(input, { target: { value: "" } });
    await act(async () => vi.advanceTimersByTimeAsync(200));
    expect(search).toHaveBeenCalledTimes(1);
    rendered.unmount();
  } finally { vi.useRealTimers(); }
});

it("offers + Add principle on an empty Lens and opens the principle it saves", async () => {
  const { bridge, loupist, view, user } = await fixture();
  // A new Loupist: no methods or pages yet.
  const catalog = bridge.knowledgeCatalog.bind(bridge);
  vi.spyOn(bridge, "knowledgeCatalog").mockImplementation(async request => ({ ...await catalog(request), lenses: [] }));
  for (const principle of await bridge.principleList({ loupistId: loupist.id })) await bridge.principleArchive({ loupistId: loupist.id, recordId: principle.recordId, archived: true });
  render(view());
  const page = await screen.findByRole("region", { name: /What .* knows/ });
  await within(page).findByText(/Nothing here yet/);
  await user.click(within(page).getByRole("button", { name: "+ Add principle" }));
  await user.type(within(page).getByRole("textbox", { name: "Statement" }), "Check capacity costs before trusting a margin recovery.");
  await user.click(within(page).getByRole("button", { name: "Save" }));
  expect(await within(page).findByRole("complementary", { name: /Check capacity costs before trusting a margin recovery/ })).toBeVisible();
  expect((await bridge.principleList({ loupistId: loupist.id })).map(item => item.statement)).toContain("Check capacity costs before trusting a margin recovery.");
});
