import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import i18n from "../../i18n";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { MOCK_LOUPISTS } from "../../bridge/mock/fixtures";
import type { KnowledgeMap } from "../../contracts/knowledge";
import type { InboxChangedEvent } from "../../contracts/scheduled";
import type { ChatMessage, LoupistWorkspace, ScheduledRun, ScheduledTaskListItem } from "@/types";
import { inboxItemOf, ScheduleView } from "./ScheduleView";

const loupist = MOCK_LOUPISTS[0]!;
const running: ScheduledRun = {
  quiet: false,
  briefing_context: null, kind: "custom", summary: "", read_at_ms: null, message_id: null, skipped_tools: [], id: "run-row", task_id: "task-1",
  loupist_id: loupist.id, task_revision: 1, task_title: "Chip news", thread_id: "chat-run", run_id: "run-1", scheduled_for_ms: 0,
  started_at_ms: Date.now(), finished_at_ms: null, status: "running", error: null,
};
const task = (latest: ScheduledRun) => ({
  id: "task-1", loupist_id: loupist.id, kind: "custom", title: "Chip news", revision: 1, status: "active", awaiting_confirmation: false,
  next_run_at_ms: null, last_run_at_ms: latest.started_at_ms, created_at_ms: 0, updated_at_ms: 0, last_error: null, session_thread_id: null,
  schedule: { kind: "once", at_ms: 0 }, latest_run: latest,
}) as unknown as ScheduledTaskListItem;

it("lists an unreadable task in the default filter without offering task controls", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const unreadable: ScheduledTaskListItem = { ...task(running), id: "damaged-check", prompt: "", status: "paused", last_error: "unreadable_task", latest_run: null };
  vi.spyOn(bridge, "scheduledTasksList").mockResolvedValue([task(running), unreadable]);
  const history = vi.spyOn(bridge, "scheduledTaskRuns");
  render(<ScheduleView bridge={bridge} loupist={loupist} theme="light" initialTaskId={unreadable.id} open={vi.fn()} onOpenChat={vi.fn()} />);

  const row = (await screen.findByText("Unreadable scheduled task")).closest("li")!;
  expect(row).toHaveTextContent("damaged-check");
  expect(within(row).queryByRole("button")).toBeNull();
  expect(await screen.findByRole("button", { name: /Chip news/ })).toBeVisible();
  expect(history).not.toHaveBeenCalled();
});

it("shows the answer of a run left open while it runs once the run finishes", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  let run = running;
  vi.spyOn(bridge, "scheduledTasksList").mockImplementation(async () => [task(run)]);
  vi.spyOn(bridge, "scheduledTaskRuns").mockImplementation(async () => [run]);
  const answer: ChatMessage = { id: "answer-1", role: "assistant", status: "complete", content: "Chip demand held up this week.", created_at: new Date().toISOString() };
  const workspace = await bridge.loupistSelect(loupist.id);
  vi.spyOn(bridge, "loupistSelect").mockImplementation(async () => ({ ...workspace, threads: [{ ...workspace.threads[0]!, id: "chat-run", home: false, messages: [answer] }] }) as LoupistWorkspace);
  const markRead = vi.spyOn(bridge, "inboxMarkRead");
  let changed: ((event: InboxChangedEvent) => void) | undefined;
  vi.spyOn(bridge, "onInboxChanged").mockImplementation((handler) => { changed = handler; return () => undefined; });
  render(<ScheduleView bridge={bridge} loupist={loupist} theme="light" open={vi.fn()} onOpenChat={vi.fn()} />);

  const runningSection = await screen.findByRole("region", { name: "Running now" });
  await user.click(within(runningSection).getByRole("button", { name: "Open" }));
  expect(await screen.findByText("Running this research…", { selector: "p" })).toBeVisible();

  run = { ...running, status: "succeeded", memory_save_skip: "deadline", message_id: answer.id, finished_at_ms: Date.now(), summary: "Chip demand held up this week.",
    discarded_memory_items: [{ kind: "note", title: "Weekly chip demand" }],
  };
  await act(async () => { changed?.({ loupistId: loupist.id }); });
  expect(await screen.findByText("Chip demand held up this week.", { selector: ".message-content p" })).toBeVisible();
  expect(screen.getByText("Not saved to Memory: another conversation was still running after 10 minutes.")).toBeVisible();
  expect(screen.getByText("What this answer says it saved was not kept.")).toBeVisible();
  expect(screen.getByRole("list", { name: "Not kept" })).toHaveTextContent("Weekly chip demand");
  const reader = document.querySelector(".briefings-reader")!;
  expect(reader.querySelector("header")).toHaveTextContent(loupist.name);
  expect(reader.querySelector(".message-meta time")).toBeNull();
  expect(reader.querySelector(".message-meta")?.textContent ?? "").not.toContain(loupist.name);
  // Read once it has been on screen for a second.
  await vi.waitFor(() => expect(markRead).toHaveBeenCalledWith({ loupistId: loupist.id, runIds: ["run-1"] }), { timeout: 2_000 });
});

it("labels a task's prompt in its detail, and hides one that only repeats the title", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const finished: ScheduledRun = { ...running, status: "succeeded", finished_at_ms: Date.now() };
  const same = { ...task(finished), prompt: "Chip news" } as ScheduledTaskListItem;
  const asked = { ...task(finished), id: "task-2", title: "Morning review", model_profile_id: "", model_selection: (await bridge.modelCatalogGet()).default_selection, prompt: "Summarize new Hanul Precision filings." } as ScheduledTaskListItem;
  vi.spyOn(bridge, "scheduledTasksList").mockResolvedValue([same, asked]);
  vi.spyOn(bridge, "scheduledTaskRuns").mockResolvedValue([]);
  render(<ScheduleView bridge={bridge} loupist={loupist} theme="light" open={vi.fn()} onOpenChat={vi.fn()} />);

  await user.click(await screen.findByRole("button", { name: /Morning review/ }));
  const detail = await screen.findByRole("article", { name: "Morning review" });
  expect(within(detail).getByText("Model: GPT-5.6 Luna")).toBeVisible();
  expect(within(detail).queryByText("Built in")).toBeNull();
  expect(within(detail).getByRole("heading", { level: 3, name: "Prompt" })).toBeVisible();
  expect(within(detail).getByText("Summarize new Hanul Precision filings.")).toBeVisible();

  await user.click(within(detail).getByRole("button", { name: /Schedule/ }));
  await user.click(await screen.findByRole("button", { name: /^Chip news/ }));
  const repeated = await screen.findByRole("article", { name: "Chip news" });
  expect(within(repeated).queryByRole("heading", { name: "Prompt" })).toBeNull();
  expect(within(repeated).getAllByText("Chip news")).toHaveLength(1);
});

it("lists no Watching item and names a briefing's followed tickers by their saved names", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  bridge.assets.installAsset({ name: "NVIDIA", ticker: "NVDA", exchange: "US", figi: "BBG000BBJQV0" });
  const briefing: ScheduledRun = {
    ...running, kind: "briefing", task_id: "briefing", task_title: "Briefings", status: "succeeded", finished_at_ms: Date.now(), thread_id: null, summary: "A quiet day.",
    skipped_tools: [{ name: "FRED", reason: "keychain_permission_required" }],
    briefing_context: { symbols: ["NVDA", "TSM"], principlesChecked: 0, newFacts: 0, disputedEntityIds: [], changed: { names: 0, relations: 0, conflicts: 0, rows: [] }, ahead: [] },
  };
  vi.spyOn(bridge, "scheduledTasksList").mockResolvedValue([{ ...task(briefing), id: "briefing", kind: "briefing", title: "Briefings" } as ScheduledTaskListItem]);
  vi.spyOn(bridge, "inboxList").mockResolvedValue({ items: [inboxItemOf(briefing)], nextCursor: null });
  render(<ScheduleView bridge={bridge} loupist={loupist} theme="light" open={vi.fn()} onOpenChat={vi.fn()} initialRunId="run-1" />);

  expect(await screen.findByText("NVIDIA · TSM")).toBeVisible();
  expect(screen.getByText("A quiet day.")).toBeVisible();
  expect(screen.getByText("Not used: FRED (macOS permission needed for the saved key)")).toBeVisible();
  expect(screen.queryByRole("list", { name: "Watching" })).toBeNull();
  expect(screen.queryByRole("button", { name: /^Watching/ })).toBeNull();
});

it("names a changed relationship's relation in the interface language", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const map = {
    nodes: [
      { id: "tesla", label: "Tesla, Inc.", localNames: ["테슬라"], kind: "entity", entityKind: "Organization", degree: 1, watched: false },
      { id: "nasdaq", label: "Nasdaq Global Select Market", kind: "entity", entityKind: "Organization", degree: 1, watched: false },
    ],
    edges: [{ id: "c1", kind: "claim", source: "tesla", target: "nasdaq", relation: "LISTS_ON", status: "active" }],
    counts: {}, unmatchedWatchlist: [], worldRevisionId: "rev", generatedAtMs: 0,
  } as unknown as KnowledgeMap;
  vi.spyOn(bridge, "knowledgeMap").mockResolvedValue(map);
  const briefing: ScheduledRun = {
    ...running, kind: "briefing", task_id: "briefing", task_title: "Briefings", status: "succeeded", finished_at_ms: Date.now(), thread_id: null, summary: "A quiet day.",
    briefing_context: { symbols: [], principlesChecked: 0, newFacts: 0, disputedEntityIds: [], changed: { names: 0, relations: 1, conflicts: 0, rows: [{ kind: "claim_added", subjectId: "c1", summary: "Tesla, Inc. lists on Nasdaq" }] }, ahead: [] },
  };
  vi.spyOn(bridge, "scheduledTasksList").mockResolvedValue([{ ...task(briefing), id: "briefing", kind: "briefing", title: "Briefings" } as ScheduledTaskListItem]);
  vi.spyOn(bridge, "inboxList").mockResolvedValue({ items: [inboxItemOf(briefing)], nextCursor: null });
  render(<ScheduleView bridge={bridge} loupist={loupist} theme="light" open={vi.fn()} onOpenChat={vi.fn()} initialRunId="run-1" />);
  expect(await screen.findByText("Tesla, Inc. → Nasdaq Global Select Market (lists on)")).toBeVisible();
  await act(async () => { await i18n.changeLanguage("ko"); });
  try {
    expect(await screen.findByText("테슬라 → Nasdaq Global Select Market (상장 거래소)")).toBeVisible();
  } finally {
    await act(async () => { await i18n.changeLanguage("en"); });
  }
});

describe("the built-in briefing beside a custom task", () => {
  async function scheduleWithCustomTask() {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const finished: ScheduledRun = { ...running, status: "succeeded", finished_at_ms: Date.now() };
    const own = bridge.scheduledTasksList.bind(bridge);
    vi.spyOn(bridge, "scheduledTasksList").mockImplementation(async request => [task(finished), ...await own(request)]);
    vi.spyOn(bridge, "scheduledTaskRuns").mockResolvedValue([]);
    return bridge;
  }
  const briefingRow = () => document.querySelector<HTMLElement>("[data-schedule-briefing]");

  it("stays in the list while off, in Active and All, and Turn on there makes it today's row", async () => {
    const user = userEvent.setup();
    const bridge = await scheduleWithCustomTask();
    const enable = vi.spyOn(bridge, "briefingSettingsSet");
    render(<ScheduleView bridge={bridge} loupist={loupist} theme="light" open={vi.fn()} onOpenChat={vi.fn()} />);

    const list = await screen.findByRole("region", { name: "Scheduled work" });
    expect(within(list).getByRole("button", { name: /^Chip news/ })).toBeVisible();
    for (const filter of ["All", "Active"]) {
      const row = await vi.waitFor(() => { const found = briefingRow(); if (!found) throw new Error("no briefing row"); return found; });
      expect(row).toHaveTextContent("Briefing");
      expect(within(row).getByText("Built in")).toBeVisible();
      expect(within(row).getByText("Off")).toBeVisible();
      expect(within(row).getByText("What changed for the assets you follow, on weekday mornings. Built in.")).toBeVisible();
      expect(within(row).getByRole("button", { name: "Turn on" })).toBeVisible();
      // It is not scheduled while it is off, so the header counts only the custom task.
      expect(screen.getByText(/^1 scheduled task/)).toBeVisible();
      await user.click(screen.getByRole("radio", { name: filter }));
    }

    await user.click(within(briefingRow()!).getByRole("button", { name: "Turn on" }));
    expect(enable).toHaveBeenCalledExactlyOnceWith({ loupistId: loupist.id, enabled: true });
    const on = await within(list).findByRole("button", { name: /^Briefing/ });
    expect(on.closest("li")).toHaveAttribute("data-status", "active");
    expect(within(on).getByText("Built in")).toBeVisible();
    expect(briefingRow()).toBeNull();
    const evening = document.querySelector<HTMLElement>("[data-schedule-evening]");
    expect(evening).toHaveTextContent("Evening briefing");
    expect(within(evening!).getByText("Off")).toBeVisible();
  });

  it("offers Turn on for a briefing that was turned off, and its name still opens its runs", async () => {
    const user = userEvent.setup();
    const bridge = await scheduleWithCustomTask();
    await bridge.briefingSettingsSet({ loupistId: loupist.id, enabled: false, schedule: { kind: "weekly", days: [1, 2, 3, 4, 5], hour: 8, minute: 0, timezone: "Asia/Seoul" } });
    render(<ScheduleView bridge={bridge} loupist={loupist} theme="light" open={vi.fn()} onOpenChat={vi.fn()} />);

    const row = await vi.waitFor(() => { const found = briefingRow(); if (!found) throw new Error("no briefing row"); return found; });
    expect(within(row).getByText("Off")).toBeVisible();
    expect(within(row).getByRole("button", { name: "Turn on" })).toBeVisible();
    // Off sits beside Turn on at the row's end, centred with it, not on the name's line inside its button.
    expect(within(row).getByText("Off").parentElement).toContainElement(within(row).getByRole("button", { name: "Turn on" }));
    expect(within(row).getByRole("button", { name: /^Briefing/ })).not.toHaveTextContent("Off");
    await user.click(within(row).getByRole("button", { name: /^Briefing/ }));
    expect(await screen.findByRole("article", { name: "Briefing" })).toBeVisible();
  });
});

it("tries a failed run again by running its task now, then shows that task", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const failed: ScheduledRun = { ...running, status: "failed", finished_at_ms: Date.now(), error: "connect ECONNREFUSED", thread_id: null };
  vi.spyOn(bridge, "scheduledTasksList").mockResolvedValue([task(failed)]);
  vi.spyOn(bridge, "scheduledTaskRuns").mockResolvedValue([failed]);
  vi.spyOn(bridge, "inboxList").mockResolvedValue({ items: [inboxItemOf(failed)], nextCursor: null });
  const runNow = vi.spyOn(bridge, "scheduledTaskRunNow").mockResolvedValue(undefined as never);
  render(<ScheduleView bridge={bridge} loupist={loupist} theme="light" open={vi.fn()} onOpenChat={vi.fn()} initialRunId="run-1" />);

  // Reloading would show the same failure; the failure offers running the task once more.
  const failure = await screen.findByRole("alert");
  expect(within(failure).queryByRole("button", { name: "Try again" })).toBeNull();
  await user.click(within(failure).getByRole("button", { name: "Run now" }));
  expect(runNow).toHaveBeenCalledWith({ loupist_id: loupist.id, task_id: "task-1" });
  expect(await screen.findByRole("article", { name: "Chip news" })).toBeVisible();
});

it("leads from the footer's 'tell Bot' to Bot", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const finished: ScheduledRun = { ...running, status: "succeeded", finished_at_ms: Date.now() };
  vi.spyOn(bridge, "scheduledTasksList").mockResolvedValue([task(finished)]);
  vi.spyOn(bridge, "scheduledTaskRuns").mockResolvedValue([]);
  const onAskHome = vi.fn();
  const { container } = render(<ScheduleView bridge={bridge} loupist={loupist} theme="light" open={vi.fn()} onOpenChat={vi.fn()} onAskHome={onAskHome} />);
  await screen.findByRole("button", { name: /Chip news/ });
  const footer = container.querySelector<HTMLElement>(".schedule-footer")!;
  expect(footer).toHaveTextContent("tell Bot what you want and when");
  await user.click(within(footer).getByRole("button", { name: "Ask in Bot" }));
  expect(onAskHome).toHaveBeenCalledExactlyOnceWith("");
});

it("words a failed run by its task's kind, and an unknown failure by its title and Details alone", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const failed: ScheduledRun = { ...running, status: "failed", finished_at_ms: Date.now(), error: "An unexpected execution error", thread_id: null };
  vi.spyOn(bridge, "scheduledTasksList").mockResolvedValue([task(failed)]);
  vi.spyOn(bridge, "scheduledTaskRuns").mockResolvedValue([failed]);
  render(<ScheduleView bridge={bridge} loupist={loupist} theme="light" open={vi.fn()} onOpenChat={vi.fn()} />);

  await user.click(await screen.findByRole("button", { name: /Chip news/ }));
  const detail = await screen.findByRole("article", { name: "Chip news" });
  expect(within(detail).getByRole("status")).toHaveTextContent(/^The last run didn't finish\.$/);
  const row = await within(detail).findByRole("button", { name: /Couldn't finish this scheduled research/ });
  expect(row).not.toHaveTextContent("briefing");
  await user.click(row);
  const failure = await screen.findByRole("alert");
  expect(within(failure).getByRole("heading", { level: 3 })).toHaveTextContent("Couldn't finish this scheduled research");
  expect(failure.querySelector(":scope > p")).toBeNull();
  expect(within(failure).getByText("Details")).toBeInTheDocument();
});

it("keeps a briefing's own words for its failure and its reason", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const failed: ScheduledRun = { ...running, kind: "briefing", task_title: "Briefing", status: "failed", finished_at_ms: Date.now(), error: "connect ECONNREFUSED", thread_id: null };
  vi.spyOn(bridge, "scheduledTasksList").mockResolvedValue([{ ...task(failed), kind: "briefing", title: "Briefing" } as ScheduledTaskListItem]);
  vi.spyOn(bridge, "scheduledTaskRuns").mockResolvedValue([failed]);
  vi.spyOn(bridge, "inboxList").mockResolvedValue({ items: [inboxItemOf(failed)], nextCursor: null });
  render(<ScheduleView bridge={bridge} loupist={loupist} theme="light" open={vi.fn()} onOpenChat={vi.fn()} initialRunId="run-1" />);

  const failure = await screen.findByRole("alert");
  expect(within(failure).getByRole("heading", { level: 3 })).toHaveTextContent("Couldn't finish this briefing");
  expect(within(failure).getByText("Couldn't reach your AI provider. Check your connection and try again.")).toBeVisible();
});

it("marks a check that found nothing to report as quiet, never as waiting to be read", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const finished = (id: string, at: number, extra: Partial<ScheduledRun>): ScheduledRun =>
    ({ ...running, id: `row-${id}`, run_id: id, status: "succeeded", started_at_ms: at, finished_at_ms: at + 1_000, ...extra });
  const quiet = finished("quiet-run", Date.now() - 86_400_000, { quiet: true, summary: "Neither crossed its line." });
  const reported = finished("reported-run", Date.now(), { summary: "SK hynix crossed its 52-week low." });
  vi.spyOn(bridge, "scheduledTasksList").mockResolvedValue([task(reported)]);
  vi.spyOn(bridge, "scheduledTaskRuns").mockResolvedValue([reported, quiet]);
  render(<ScheduleView bridge={bridge} loupist={loupist} theme="light" open={vi.fn()} onOpenChat={vi.fn()} />);

  await user.click(await screen.findByRole("button", { name: /Chip news/ }));
  const history = await screen.findByRole("list", { name: "Runs of Chip news" });
  const [reportedRow, quietRow] = within(history).getAllByRole("button");
  expect(reportedRow).toHaveAttribute("data-unread", "true");
  expect(within(reportedRow!).queryByText("Nothing to report")).toBeNull();
  expect(quietRow).toHaveAttribute("data-unread", "false");
  expect(quietRow).toHaveAttribute("data-quiet", "true");
  expect(within(quietRow!).getByText("Nothing to report")).toBeVisible();
  expect(within(quietRow!).getByText("Neither crossed its line.")).toBeVisible();
  expect(within(quietRow!).queryByText("Unread")).toBeNull();
});

it("says a run's session was removed when Continue in Chat finds it gone with its run", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const finished: ScheduledRun = { ...running, status: "succeeded", finished_at_ms: Date.now(), summary: "Chip demand held up." };
  vi.spyOn(bridge, "scheduledTasksList").mockResolvedValue([task(finished)]);
  const runs = vi.spyOn(bridge, "scheduledTaskRuns").mockResolvedValue([finished]);
  vi.spyOn(bridge, "inboxList").mockResolvedValue({ items: [inboxItemOf(finished)], nextCursor: null });
  const onOpenChat = vi.fn(async () => false);
  render(<ScheduleView bridge={bridge} loupist={loupist} theme="light" open={vi.fn()} onOpenChat={onOpenChat} initialRunId="run-1" />);

  const resume = await screen.findByRole("button", { name: "Continue in Chat →" });
  await vi.waitFor(() => expect(runs).toHaveBeenCalled());
  const reads = runs.mock.calls.length;
  await user.click(resume);
  expect(onOpenChat).toHaveBeenCalledWith(expect.objectContaining({ runId: "run-1", threadId: "chat-run" }));
  expect(await screen.findByText("This run's session was removed.")).toBeVisible();
  expect(screen.queryByRole("button", { name: "Continue in Chat →" })).toBeNull();
  // The history is read again, so a run that left it leaves the list too.
  await vi.waitFor(() => expect(runs.mock.calls.length).toBeGreaterThan(reads));
});

describe("Schedule's login hint", () => {
  const visit = (bridge: MockLoupistBridge) => render(<ScheduleView bridge={bridge} loupist={loupist} theme="light" open={vi.fn()} onOpenChat={vi.fn()} />);
  const hint = "Scheduled tasks run only while Loupist is open.";
  it.each(["dismiss", "enable"])("remembers %s across visits and only confirms for the enabling visit", async action => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    bridge.resident = { ...bridge.resident, keepRunning: false, loginItem: "off" };
    vi.spyOn(bridge, "scheduledTasksList").mockResolvedValue([task(running)]);
    const view = visit(bridge);
    expect(await screen.findByText(hint)).toBeVisible();
    await userEvent.setup().click(screen.getByRole("button", { name: action === "enable" ? "Open at login" : "Dismiss" }));
    expect(screen.queryByText(hint)).toBeNull();
    if (action === "enable") {
      expect(screen.getByText("Loupist will open at login")).toBeVisible();
      expect(bridge.resident.keepRunning).toBe(true);
      await bridge.loginItemSet({ enabled: false });
    } else expect(bridge.resident.keepRunning).toBe(false);
    view.unmount();
    visit(bridge);
    await screen.findByRole("heading", { name: "Scheduled work" });
    await act(async () => {});
    expect(screen.queryByText(hint)).toBeNull();
    expect(screen.queryByText("Loupist will open at login")).toBeNull();
  });

  it.each([["paused", "off"], ["active", "on"], ["active", "unavailable"], ["active", "requiresApproval"], ["active", "error"]] as const)(
    "has no hint with task %s and registration %s", async (status, loginItem) => {
      const bridge = new MockLoupistBridge({ delay_ms: 0 });
      bridge.resident.loginItem = loginItem;
      vi.spyOn(bridge, "scheduledTasksList").mockResolvedValue([{ ...task(running), status }]);
      visit(bridge);
      await screen.findByRole("heading", { name: "Scheduled work" });
      await act(async () => {});
      expect(screen.queryByText(hint)).toBeNull();
    });
});

it("groups watches in Schedule and changes their notification policy without a run", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  bridge.installScheduledTask({ ...task({ ...running, status: "succeeded" }), report: "when_met",
    delivery: { to: "bot", notify: "now" }, terms: "New filings that bear on your reasons" });
  const change = vi.spyOn(bridge, "scheduledTaskSetDelivery");
  const runNow = vi.spyOn(bridge, "scheduledTaskRunNow");
  render(<ScheduleView bridge={bridge} loupist={loupist} theme="light" open={vi.fn()} onOpenChat={vi.fn()} />);
  expect(await screen.findByRole("heading", { name: "Watches" })).toBeVisible();
  await user.click(screen.getByRole("button", { name: /Chip news/ }));
  expect(screen.getByText("New filings that bear on your reasons")).toBeVisible();
  await user.selectOptions(screen.getByRole("combobox", { name: "Notifies" }), "digest");
  await vi.waitFor(() => expect(screen.getByRole("combobox", { name: "Notifies" })).toHaveValue("digest"));
  expect(change).toHaveBeenCalledWith({ loupistId: loupist.id, taskId: "task-1", expectedRevision: 1, notify: "digest" });
  expect(runNow).not.toHaveBeenCalled();
});
