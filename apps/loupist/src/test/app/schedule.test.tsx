import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { ChatEvidenceRef, ConflictGrid, InboxChangedEvent, InboxItem, InboxOpenEvent, ScheduledRun } from "../../types";
import { GRID_SCHEMA } from "../../contracts/chat";
import { scheduledRunFixture } from "../scheduledFixtures";
import { mainTab, openApp, openLoupistSwitcher, selectedLoupistButton } from "../renderApp";

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

async function openScheduleFromResearch(bridge: MockLoupistBridge, user: ReturnType<typeof userEvent.setup>) {
  const title = "Apple revenue research";
  await bridge.chatCreate({ loupist_id: "loupist-quality", title });
  await openApp(bridge);
  await user.click(await screen.findByRole("button", { name: title }));
  await screen.findByRole("heading", { name: title });
  const composer = screen.getByRole("textbox", { name: "Message Loupist" });
  fireEvent.change(composer, { target: { value: "Keep my research draft" } });
  await user.click(await mainTab(/Schedule/));
}

it("offers a built-in briefing, a filings watch and a custom task in the empty Schedule, with the account note after all three", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const enable = vi.spyOn(bridge, "briefingSettingsSet");
  const user = userEvent.setup();
  const send = vi.spyOn(bridge, "chatSend");
  await openScheduleFromResearch(bridge, user);
  const heading = await screen.findByRole("heading", { name: "Nothing is scheduled yet" });
  const choices = within(heading.parentElement!);
  expect(choices.getByText("Loupist can run work for you at set times and post the result in Bot.")).toBeVisible();
  const cards = choices.getAllByRole("heading", { level: 3 });
  expect(cards.map(node => node.textContent)).toEqual(["Morning briefing", "Evening briefing", "Watch for filings and results", "Your own task"]);
  const actionInCard = (heading: string, name: string) => within(choices.getByRole("heading", { name: heading, level: 3 }).parentElement!).getByRole("button", { name });
  const turnOn = actionInCard("Morning briefing", "Turn on");
  const turnOnEvening = actionInCard("Evening briefing", "Turn on");
  const turnOnWatch = actionInCard("Watch for filings and results", "Turn on");
  const askBot = actionInCard("Your own task", "Ask Bot");
  const note = choices.getByText(/Scheduled tasks use your AI account/);
  for (const action of [turnOn, turnOnEvening, turnOnWatch, askBot]) {
    expect(action.compareDocumentPosition(note) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  }
  expect(note.closest("header")).toBeNull();
  expect(screen.getAllByText(/Scheduled tasks use your AI account/)).toHaveLength(1);
  await user.click(turnOn);
  expect(send).not.toHaveBeenCalled();
  expect(enable).toHaveBeenCalledExactlyOnceWith({ loupistId: "loupist-quality", enabled: true });
  const row = await taskRow(/^Briefing/);
  expect(within(row).getByText("Built in")).toBeVisible();
  await user.click(within(row).getByRole("button"));
  expect(within(await screen.findByRole("article", { name: "Briefing" })).getByText("Built in")).toBeVisible();
});

it("opens Bot from an empty Schedule with the custom example focused and unsent", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const send = vi.spyOn(bridge, "chatSend");
  const user = userEvent.setup();
  await openScheduleFromResearch(bridge, user);
  await user.click(await screen.findByRole("button", { name: "Ask Bot" }));
  const composer = await screen.findByRole("textbox", { name: "Message Loupist" });
  expect(composer).toHaveValue("Every Monday at 8, summarize semiconductor news.");
  expect(composer).toHaveFocus();
  expect(screen.getByRole("button", { name: "Bot" })).toHaveAttribute("aria-current", "page");
  expect(send).not.toHaveBeenCalled();
  await user.click(screen.getByRole("button", { name: "Apple revenue research" }));
  expect(await screen.findByRole("textbox", { name: "Message Loupist" })).toHaveValue("Keep my research draft");
});

it("opens a run from the task history, shows its sections, and marks it read after a second", async () => {
  const { bridge, user } = await fixture();
  await openApp(bridge);
  await openFirstRun(user);
  expect(await screen.findByText("+3 in World · 2 relationships · 0 conflicts")).toBeVisible();
  expect(screen.getByText("Hanul Precision supplies Morrow Components")).toBeVisible();
  expect(screen.getByRole("button", { name: /To check by .*: Inventory days below 50/ })).toBeVisible();
  expect(bridge.inboxMarkRead).not.toHaveBeenCalled();
  await waitFor(() => expect(bridge.inboxMarkRead).toHaveBeenCalledWith({ loupistId: "loupist-quality", runIds: ["briefing-run"] }), { timeout: 2000 });
  await user.click(screen.getByText("What was checked"));
  expect(screen.getByText("Companies: NVDA")).toBeVisible();
});

it("lists tasks, cadence and run history without row menus or inline editing", async () => {
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
  const item: InboxItem = { runId: run.run_id, taskId: run.task_id, kind: "briefing", title: "Briefings", status: "succeeded", summary: run.summary, startedAtMs: run.started_at_ms, finishedAtMs: run.finished_at_ms ?? null, readAtMs: null, threadId: null, messageId: null, skippedTools: [], briefingContext: context, error: null };
  vi.spyOn(bridge, "inboxList").mockResolvedValue({ items: [item], nextCursor: null });
  const counts = vi.spyOn(bridge, "inboxUnreadCounts").mockResolvedValue([{ loupistId: "loupist-quality", count: 2 }]);
  await openApp(bridge);
  // The switcher at the top of the sidebar counts each Loupist's unread results.
  const profile = () => within(openLoupistSwitcher()).getByRole("menuitemradio", { name: "Quality Compounder" });
  await waitFor(() => expect(profile()).toHaveAccessibleDescription("2 unread scheduled results"));
  expect(within(profile()).getByText("2")).toBeVisible();
  counts.mockResolvedValue([{ loupistId: "loupist-quality", count: 3 }]);
  await act(async () => listeners.forEach(fn => fn({loupistId:"loupist-quality"})));
  await waitFor(() => expect(profile()).toHaveAccessibleDescription("3 unread scheduled results"));
  expect(within(profile()).getByText("3")).toBeVisible();
  fireEvent.keyDown(openLoupistSwitcher(), { key: "Escape" });
  const other = (await bridge.loupistList()).find(loupist => loupist.id !== "loupist-quality")!;
  await act(async () => open({ loupistId: other.id, runId: item.runId }));
  expect(await screen.findByRole("heading", { name: "Briefing", level: 2 })).toBeVisible();
  expect(selectedLoupistButton()).toHaveTextContent(other.name);
  expect(bridge.inboxList).toHaveBeenCalledWith(expect.objectContaining({ loupistId: other.id }));
});

it.each([false, true])("opens the selected answer and describes whether this session was continued (%s)", async continued => {
  const { bridge, user, run } = await fixture({ thread_id: "briefing-chat", message_id: "briefing-answer" });
  const select = bridge.loupistSelect.bind(bridge);
  vi.spyOn(bridge, "loupistSelect").mockImplementation(async id => {
    const workspace = await select(id);
    workspace.threads.push({ id: "briefing-chat", loupist_id: id, title: "Briefings Chat", use_memory: true, update_memory: true, updated_at: new Date().toISOString(), messages: [
      { id: "scheduled-request", role: "user", content: "Prepare the briefing", status: "complete", created_at: new Date(run.started_at_ms).toISOString(),
        scheduled: { run_id: run.run_id, task_id: run.task_id, task_title: run.task_title, task_kind: run.kind } },
      { id: "briefing-answer", role: "assistant", content: "The specific briefing answer.", status: "complete", created_at: new Date().toISOString() }] });
    if (continued) workspace.threads.at(-1)!.messages.push({ id: "reply", role: "user", status: "complete", content: "What changed?", created_at: new Date().toISOString() });
    return workspace;
  });
  await openApp(bridge); await openFirstRun(user);
  expect(await screen.findByText("The specific briefing answer.")).toBeVisible();
  expect(Boolean(screen.queryByText("Sending a message adds this session to Research sessions."))).toBe(!continued);
  await user.click(screen.getByRole("button", { name: continued ? "Open the session" : "Continue in Chat →" }));
  expect(await screen.findByRole("heading", { name: "Briefings Chat" })).toBeVisible();
  const session = screen.getByRole("button", { name: "Briefings Chat" });
  if (continued) {
    expect(session).toHaveAccessibleDescription("Started by a schedule");
    await user.click(await screen.findByRole("button", { name: "From Schedule: Briefing" }));
    expect(await screen.findByRole("article", { name: "Briefing" })).toBeVisible();
  } else {
    expect(session).not.toHaveAccessibleDescription("Started by a schedule");
    expect(screen.queryByText("From Schedule: Briefing")).toBeNull();
  }
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
  expect(paper).toHaveTextContent("+3 in World · 2 relationships · 0 conflicts");
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

it("opens World with its conflicting evidence filter from the reader link", async () => {
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
  await user.click(await screen.findByRole("button", { name: "See conflicting evidence in World (2)" }));
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

it("offers Show in World from a conflict card in the briefing reader", async () => {
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
    chart_dataset: null,
  });
  await openApp(bridge); await openFirstRun(user);
  expect(await screen.findByText("Sources disagree on FY2025 revenue.")).toBeVisible();
  await user.click(await screen.findByRole("button", { name: "Show in World" }));
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
