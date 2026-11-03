import { render, screen, waitFor, within } from "@testing-library/react";
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
  const handlers = { onBack: vi.fn(), onOpenLoupistSettings: vi.fn(), onOpenChat: vi.fn(), onOpenMap: vi.fn(), onLoupistUpdated: vi.fn(), onOpenLink: vi.fn(), onOpenKnowledge: vi.fn(), onAsk: vi.fn() };
  const view = (initialItem?: string) => <TooltipProvider><MemoryPage bridge={bridge} loupist={loupist} threads={threads} initialItem={initialItem} {...handlers} /></TooltipProvider>;
  return { bridge, loupist, handlers, view, user: userEvent.setup() };
}

it("opens on the principles the user taught, shows where one was used, and edits it through Rust", async () => {
  let recordId = "";
  const { handlers, view, user } = await fixture(async (bridge, loupist) => {
    recordId = (await bridge.principleCreate({ loupistId: loupist.id, statement: "Prefer companies that publish segment margins.", origin: "settings" })).recordId;
    (bridge as unknown as { state: { memoryUsage: Record<string, unknown> } }).state.memoryUsage = { [loupist.id]: [{ recordId, usedCount: 4, lastUsedAtMs: Date.UTC(2026, 9, 28), usedIn: [{ chatId: "chat-margin", atMs: Date.UTC(2026, 9, 28) }] }] };
  });

  render(view());
  const page = await screen.findByRole("region", { name: "Research principles" });
  const tabs = within(page).getByRole("tablist");
  expect(within(tabs).getByRole("tab", { name: /Your rules/ })).toHaveAttribute("aria-selected", "true");
  await user.click(await within(page).findByRole("button", { name: /Prefer companies that publish segment margins.*Rule, used 4 times/ }));
  const reader = within(page).getByRole("complementary", { name: /Prefer companies that publish segment margins/ });
  await user.click(within(reader).getByRole("button", { name: "Hanul margin review" }));
  expect(handlers.onOpenChat).toHaveBeenCalledWith("chat-margin");
  await user.click(within(reader).getByRole("button", { name: "Ask Bot to revise" }));
  expect(handlers.onAsk).toHaveBeenCalledWith(expect.objectContaining({ recordId, kind: "lens" }));
});

it("starts a new Loupist on About you with follows, holdings as a count only, and no principles yet", async () => {
  const { bridge, view, user } = await fixture(async (bridge) => {
    await bridge.watchlistAdd({ symbol: "AAPL", market: "NASDAQ", displayName: "Apple" });
    const preview = await bridge.holdingsImportCsv({ path: "/tmp/holdings.csv" });
    await bridge.holdingsCommitCsv({ previewId: preview.previewId });
  });
  const remove = vi.spyOn(bridge, "watchlistRemove");
  vi.spyOn(bridge, "principleList").mockResolvedValue([]);
  vi.spyOn(bridge, "learningReviewList").mockResolvedValue([]);
  vi.spyOn(bridge, "knowledgeGrowthEvents").mockResolvedValue({ events: [], nextCursor: null });
  render(view());
  await user.click(await screen.findByRole("tab", { name: /About you/ }));
  expect(screen.getByText("Apple · AAPL")).toBeVisible();
  expect(screen.getByText(/^Stocks and funds: 1 asset · .* · Crypto: none/)).toBeVisible();
  expect(screen.getByRole("button", { name: "Update holdings" })).toBeVisible();
  // Quantities and values never reach this page.
  expect(document.body).not.toHaveTextContent("2.5");
  expect(document.body).not.toHaveTextContent("500");
  await user.click(screen.getByRole("button", { name: "Stop following Apple" }));
  expect(remove).toHaveBeenCalledWith({ symbol: "AAPL", market: "NASDAQ" });
  await user.click(screen.getByRole("tab", { name: /History/ }));
  expect(await screen.findByText("No history yet.")).toBeVisible();
  await user.click(screen.getByRole("tab", { name: /Your rules/ }));
  expect(screen.getByText(/^Example: /)).toBeVisible();
  expect(screen.queryByRole("button", { name: /^Example/ })).toBeNull();
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
  await user.click(screen.getByRole("tab", { name: /About you/ }));
  expect(screen.queryByRole("switch", { name: "Remember new chats" })).toBeNull();
  const config = await bridge.configRead({ scope: { kind: "project", loupistId: loupist.id } });
  expect(config.effective.chat.use_memory && config.effective.chat.update_memory).toBe(true);
});

it("shows the reset audit in History without treating it as an archived method", async () => {
  const { bridge, loupist, view, user } = await fixture();
  vi.spyOn(bridge, "knowledgeGrowthEvents").mockResolvedValue({ events: [{
    eventId: "reset-1", loupistId: loupist.id, atMs: Date.now(), kind: "lens_archived",
    actor: "user", recordId: null, subjectId: null, originChatId: null, originMessageId: null,
    origin: { kind: "memory_reset", counts: { principles: 0, methods: 1, names: 2, relationships: 1, sources: 1 } },
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
  await user.click(screen.getByRole("tab", { name: /Ways of working/ }));
  await user.click(await screen.findByRole("button", { name: new RegExp(method.title) }));
  const reader = await screen.findByRole("complementary", { name: method.title });
  await user.click(within(reader).getByRole("button", { name: "Pause" }));
  expect(pause).toHaveBeenLastCalledWith({ loupistId: loupist.id, recordId: method.recordId, paused: true });
  await user.click(await within(reader).findByRole("button", { name: "Resume" }));
  await waitFor(() => expect(pause).toHaveBeenLastCalledWith({ loupistId: loupist.id, recordId: method.recordId, paused: false }));
});

it("returns to the originating method after opening its Source", async () => {
  const { bridge, loupist, handlers, view, user } = await fixture();
  const catalog = await bridge.knowledgeCatalog({ loupistId: loupist.id });
  const method = catalog.lenses.find(item => item.lensKind === "method")!;
  const source = catalog.sources[0];
  vi.spyOn(bridge, "knowledgeLensChanges").mockResolvedValue([{
    changeId: "change-source", recordId: method.recordId, newRevisionId: method.revisionId!, rationale: "Supported by the original source.", createdAtMs: 1, changeKind: "create", actor: "loupist",
    inputs: { references: [{ kind: "source", recordId: source.recordId, revisionId: source.revisionId!, objectId: source.recordId }] },
  }]);
  render(view(method.recordId));
  await screen.findByRole("heading", { name: method.title });
  await user.click(screen.getByText("Saved basis"));
  await user.click(await screen.findByRole("button", { name: "Source 1" }));
  expect(await screen.findByRole("button", { name: "Back to method" })).toBeVisible();
  expect(handlers.onOpenKnowledge).not.toHaveBeenCalled();
  await user.click(screen.getByRole("button", { name: "Back to method" }));
  expect(await screen.findByRole("heading", { name: method.title })).toBeVisible();
  expect(screen.getByRole("tab", { name: /Ways of working/, selected: true })).toBeVisible();
});
