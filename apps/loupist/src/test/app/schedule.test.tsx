import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { ChatEvidenceRef, ConflictGrid, InboxChangedEvent, InboxItem, InboxOpenEvent, ScheduledRun } from "../../types";
import { GRID_SCHEMA } from "../../contracts/chat";
import { scheduledRunFixture } from "../scheduledFixtures";
import { mainTab, openApp, selectedLoupistButton } from "../renderApp";

const context = {
  symbols: ["NVDA"], principlesChecked: 2, newFacts: 14, disputedEntityIds: [],
  changed: { names: 3, relations: 2, conflicts: 0, rows: [{ kind: "claim_added", subjectId: "c1", summary: "Hanul Precision supplies Morrow Components" }] },
  ahead: [{ journalId: "j1", title: "Inventory days below 50", byDate: "2026-11-04" }],
};

async function fixture(run: Partial<ScheduledRun> = {}) {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const settings = await bridge.briefingSettingsSet({ loupistId: "loupist-quality", enabled: true });
  const briefingRun = scheduledRunFixture({ kind: "briefing", task_id: settings.task!.id, task_title: "Briefings", run_id: "briefing-run", status: "succeeded", summary: "Revenue grew while margins narrowed.", thread_id: null, started_at_ms: Date.now(), finished_at_ms: Date.now(), briefing_context: context, ...run });
  vi.spyOn(bridge, "scheduledTaskRuns").mockResolvedValue([briefingRun]);
  vi.spyOn(bridge, "inboxMarkRead").mockResolvedValue();
  return { bridge, settings, run: briefingRun, user: userEvent.setup() };
}

async function scheduledWork() {
  return screen.findByRole("region", { name: "Scheduled work" });
}

async function taskRow(name: RegExp | string) {
  const button = await within(await scheduledWork()).findByRole("button", { name });
  return button.closest("li")!;
}

async function openFirstRun(user: ReturnType<typeof userEvent.setup>) {
  await user.click(await mainTab(/Schedule/));
  await user.click(within(await scheduledWork()).getByRole("button", { name: /^Briefing/ }));
  const history = await screen.findByRole("list", { name: /Runs of/ });
  await user.click(within(history).getAllByRole("button")[0]!);
}

it("opens a run from the task history, shows its sections, and marks it read after a second", async () => {
  const { bridge, user } = await fixture();
  await openApp(bridge);
  await openFirstRun(user);
  expect(await screen.findByText("+3 on the map · 2 relationships · 0 conflicts")).toBeVisible();
  expect(screen.getByText("Hanul Precision supplies Morrow Components")).toBeVisible();
  expect(screen.getByRole("button", { name: /To check by .*: Inventory days below 50/ })).toBeVisible();
  expect(bridge.inboxMarkRead).not.toHaveBeenCalled();
  await waitFor(() => expect(bridge.inboxMarkRead).toHaveBeenCalledWith({ loupistId: "loupist-quality", runIds: ["briefing-run"] }), { timeout: 2000 });
  await user.click(screen.getByText("What was checked"));
  expect(screen.getByText("Companies: NVDA")).toBeVisible();
});

it("lists tasks, cadence and run history without manual scheduling controls", async () => {
  const { bridge, user } = await fixture();
  await openApp(bridge);
  await user.click(await mainTab(/Schedule/));
  const row = await taskRow(/^Briefing/);
  expect(within(row).queryByRole("button", { name: /Actions for/ })).toBeNull();
  expect(within(await scheduledWork()).queryByRole("textbox")).toBeNull();
  await user.click(within(row).getByRole("button"));
  expect(await screen.findByRole("list", { name: /Runs of Briefing/ })).toBeVisible();
});

it("updates the tab and Loupist badge on inbox-changed, and routes inbox-open to the run on its Loupist", async () => {
  const { bridge, run } = await fixture(); const listeners: Array<(event: InboxChangedEvent) => void> = []; let open!: (event: InboxOpenEvent) => void;
  vi.spyOn(bridge, "onInboxChanged").mockImplementation(handler => { listeners.push(handler); return () => {}; });
  vi.spyOn(bridge, "onInboxOpen").mockImplementation(handler => { open = handler; return () => {}; });
  const item: InboxItem = { runId: run.run_id, taskId: run.task_id, kind: "briefing", title: "Briefings", status: "succeeded", summary: run.summary, startedAtMs: run.started_at_ms, finishedAtMs: run.finished_at_ms ?? null, readAtMs: null, threadId: null, messageId: null, skippedTools: [], briefingContext: context };
  vi.spyOn(bridge, "inboxList").mockResolvedValue({ items: [item], nextCursor: null });
  const counts = vi.spyOn(bridge, "inboxUnreadCounts").mockResolvedValue([{ loupistId: "loupist-quality", count: 2 }]);
  await openApp(bridge);
  const profile = within(screen.getByRole("navigation", { name: "Loupists" })).getByRole("button", { name: "Quality Compounder" });
  await waitFor(() => expect(profile).toHaveAccessibleDescription("2 unread briefings"));
  expect(within(profile).getByText("2")).toBeVisible();
  counts.mockResolvedValue([{ loupistId: "loupist-quality", count: 3 }]);
  await act(async () => listeners.forEach(fn => fn({loupistId:"loupist-quality"})));
  await waitFor(() => expect(profile).toHaveAccessibleDescription("3 unread briefings"));
  expect(within(profile).getByText("3")).toBeVisible();
  const other = (await bridge.loupistList()).find(loupist => loupist.id !== "loupist-quality")!;
  await act(async () => open({ loupistId: other.id, runId: item.runId }));
  expect(await screen.findByRole("heading", { name: "Briefing", level: 2 })).toBeVisible();
  expect(selectedLoupistButton()).toHaveTextContent(other.name);
  expect(bridge.inboxList).toHaveBeenCalledWith(expect.objectContaining({ loupistId: other.id }));
});

it("continues in the run's own Chat at the selected answer", async () => {
  const { bridge, user } = await fixture({ thread_id: "briefing-chat", message_id: "briefing-answer" });
  const select = bridge.loupistSelect.bind(bridge);
  vi.spyOn(bridge, "loupistSelect").mockImplementation(async id => {
    const workspace = await select(id);
    workspace.threads.push({ id: "briefing-chat", loupist_id: id, title: "Briefings Chat", use_memory: true, update_memory: true, updated_at: new Date().toISOString(), messages: [{ id: "briefing-answer", role: "assistant", content: "The specific briefing answer.", status: "complete", created_at: new Date().toISOString() }] });
    return workspace;
  });
  await openApp(bridge); await openFirstRun(user);
  expect(await screen.findByText("The specific briefing answer.")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Continue in Chat →" }));
  expect(await screen.findByRole("heading", { name: "Briefings Chat" })).toBeVisible();
});

it("prints the briefing as the reader shows it, for Save as PDF, without its controls (X9)", async () => {
  const { bridge, user } = await fixture({ thread_id: "briefing-chat", message_id: "briefing-answer" });
  const select = bridge.loupistSelect.bind(bridge);
  vi.spyOn(bridge, "loupistSelect").mockImplementation(async id => {
    const workspace = await select(id);
    workspace.threads.push({ id: "briefing-chat", loupist_id: id, title: "Briefings Chat", use_memory: true, update_memory: true, updated_at: new Date().toISOString(), messages: [{ id: "briefing-answer", role: "assistant", content: "The specific briefing answer.", status: "complete", created_at: new Date().toISOString() }] });
    return workspace;
  });
  const print = vi.spyOn(window, "print").mockImplementation(() => undefined);
  await openApp(bridge); await openFirstRun(user);
  expect(await screen.findByText("The specific briefing answer.")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Print or save as PDF…" }));
  await waitFor(() => expect(print).toHaveBeenCalledTimes(1));
  const paper = document.querySelector<HTMLElement>(".print-sheet")!;
  expect(paper).toHaveTextContent("+3 on the map · 2 relationships · 0 conflicts");
  expect(paper).toHaveTextContent("Inventory days below 50");
  expect(paper).toHaveTextContent("The specific briefing answer.");
  expect(paper).toHaveTextContent("Companies: NVDA");
  expect(paper).toHaveTextContent("AI can be wrong. Not a licensed financial adviser.");
  expect(paper).not.toHaveTextContent("Continue in Chat");
  expect(paper).not.toHaveTextContent("Print or save as PDF");
  expect(paper.querySelector("button")).toBeNull();
  expect(document.title).toMatch(/^Briefing · /);
  act(() => { window.dispatchEvent(new Event("afterprint")); });
  expect(document.querySelector(".print-sheet")).toBeNull();
  print.mockRestore();
});

it("opens the Map with its conflicting evidence filter from the reader link", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const map = await bridge.knowledgeMap({ loupistId: "loupist-quality" });
  const edge = map.edges.find(edge => edge.kind === "claim")!;
  edge.status = "disputed";
  const ids = [edge.source, edge.target];
  const settings = await bridge.briefingSettingsSet({ loupistId: "loupist-quality", enabled: true });
  vi.spyOn(bridge, "knowledgeMap").mockResolvedValue(map);
  vi.spyOn(bridge, "scheduledTaskRuns").mockResolvedValue([scheduledRunFixture({ kind: "briefing", task_id: settings.task!.id, status: "succeeded", thread_id: null, briefing_context: { ...context, disputedEntityIds: ids, changed: { ...context.changed, conflicts: 1 } } })]);
  vi.spyOn(bridge, "inboxMarkRead").mockResolvedValue();
  const user = userEvent.setup();
  await openApp(bridge); await openFirstRun(user);
  await user.click(await screen.findByRole("button", { name: "See conflicting evidence on the Map (2)" }));
  await user.click(await screen.findByRole("button", { name: "Filter" }));
  expect(screen.getByRole("checkbox", { name: "Only where sources disagree" })).toBeChecked();
});

it("shows a task on an account at its plan limit or needing reconnect as waiting, not failed", async () => {
  const { bridge, user } = await fixture();
  bridge.setProviderStatus("openai", { notice: { kind: "plan_limit", at_ms: Date.now() } });
  await openApp(bridge);
  await user.click(await mainTab(/Schedule/));
  const briefing = await taskRow(/^Briefing/);
  expect(briefing).toHaveTextContent("Waiting: ChatGPT plan at its limit");
  act(() => bridge.setProviderStatus("openai", { notice: null, state: "needs_reconnect" }));
  await waitFor(() => expect(briefing).toHaveTextContent("Waiting: sign-in needs reconnect"));
});

it("offers See on the Map from a conflict card in the briefing reader", async () => {
  const { bridge, user } = await fixture({ thread_id: "briefing-chat", message_id: "briefing-answer" });
  const evidence: ChatEvidenceRef = { id: "ev_dart", title: "Solsup Foods FY2025 statements", kind: "original", provider: "OpenDART", tool_name: "dart_financials", retrieved_at: "2026-03-20T06:00:00Z", published_at: "2026-03-18", excerpt: "", excerpt_truncated: false, content_sha256: "c".repeat(64), warnings: [] };
  const conflict: ConflictGrid = {
    layout: "conflict", measure: { label: "revenue", entity_label: "Solsup Foods", period: "FY2025" },
    reports: [{ evidence_id: evidence.id, says: "₩452.3bn", basis: "Consolidated, final", dated: "2026-03-18", check: { link_index: 0, text: "₩452.3bn", evidence_id: evidence.id, kind: "data", status: "rounded" } }],
    reason: "preliminary_vs_final", explanation: "The news report carried the preliminary figure.", used: 0, used_reason: "The filed figure is final.",
    world_entity_id: "ent:solsup", map_link: true,
  };
  const select = bridge.loupistSelect.bind(bridge);
  vi.spyOn(bridge, "loupistSelect").mockImplementation(async id => {
    const workspace = await select(id);
    workspace.threads.push({ id: "briefing-chat", loupist_id: id, title: "Briefings Chat", use_memory: true, update_memory: true, updated_at: new Date().toISOString(), messages: [
      { id: "briefing-answer", role: "assistant", content: "Sources disagree on FY2025 revenue.", status: "complete", created_at: new Date().toISOString(), artifact_refs: [{ artifact_id: "conflict", revision: 1, kind: "grid", title: "Solsup revenue", digest: "b".repeat(64) }] },
    ] });
    return workspace;
  });
  vi.spyOn(bridge, "chatArtifactRead").mockResolvedValue({
    artifact: { id: "conflict", chat_session_id: "briefing-chat", kind: "grid", title: "Solsup revenue", current_revision: 1, created_at_ms: 1, updated_at_ms: 1 },
    revision: { artifact_id: "conflict", revision: 1, payload: { kind: "grid", schema: GRID_SCHEMA, grid: conflict }, digest: "b".repeat(64), source_message_id: "briefing-answer", created_at_ms: 1 },
    evidence_refs: [evidence],
  });
  await openApp(bridge); await openFirstRun(user);
  expect(await screen.findByText("Sources disagree on FY2025 revenue.")).toBeVisible();
  await user.click(await screen.findByRole("button", { name: "See on the Map" }));
  await user.click(await screen.findByRole("button", { name: "Filter" }));
  expect(screen.getByRole("checkbox", { name: "Only where sources disagree" })).toBeChecked();
});

it("opens a file a briefing answer links beside the briefing's own Chat, not the Chat tab's", async () => {
  const { bridge, user } = await fixture({ thread_id: "briefing-chat", message_id: "briefing-answer" });
  const select = bridge.loupistSelect.bind(bridge);
  vi.spyOn(bridge, "loupistSelect").mockImplementation(async id => {
    const workspace = await select(id);
    workspace.threads.push({ id: "briefing-chat", loupist_id: id, title: "Briefings Chat", use_memory: true, update_memory: true, updated_at: new Date().toISOString(), messages: [
      { id: "briefing-answer", role: "assistant", content: "The table is in [the report](reports/summary.md).", status: "complete", created_at: new Date().toISOString() },
    ] });
    return workspace;
  });
  await openApp(bridge); await openFirstRun(user);
  await user.click(await screen.findByRole("link", { name: "the report" }));
  expect(await screen.findByRole("heading", { name: "Briefings Chat" })).toBeVisible();
  expect(await screen.findByRole("tab", { name: "Files" })).toHaveAttribute("aria-selected", "true");
});
