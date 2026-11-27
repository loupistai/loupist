import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { AssetRow } from "../../contracts/assets";
import type { JournalRecord } from "../../contracts/knowledge";
import type { InboxItem } from "../../contracts/scheduled";
import type { LoupistBridge, ScheduledTaskListItem } from "../../types";
import { FollowingAhead } from "./FollowingAhead";

const now = new Date(2026, 10, 25, 12).getTime();
const tomorrow = new Date(2026, 10, 26, 9).getTime();
const assets = [{ assetId: "nvda", name: "NVIDIA", reference: null, followedAtMs: 1, createdAtMs: 1 }] as AssetRow[];

const followUp = {
  kind: "custom", id: "follow-up", loupist_id: "loupist", revision: 1, title: "Results", prompt: "",
  schedule: { kind: "once", at_ms: tomorrow }, target: { kind: "new_chat" }, session_thread_id: null,
  model_profile_id: "", thinking_level: "", run_options: {}, use_memory: true, update_memory: false,
  status: "active", next_run_at_ms: tomorrow, last_run_at_ms: null, created_at_ms: 0, updated_at_ms: 0,
  last_error: null, awaiting_confirmation: false, latest_run: null, preset: "results-follow-up",
  terms: "After the filing", subjects: [{ asset: "nvda" }],
} as ScheduledTaskListItem;

const check = (journalId: string, byDate: string, assetId?: string) => ({
  journalId, loupistId: "loupist", researchId: "", judgment: "Watch the claim", reasoning: "", entities: ["entity-1"],
  archived: false, createdAtMs: 1, updatedAtMs: 1, originChatId: null, inputDigest: "x",
  inputs: { sources: [], worlds: [], lenses: [], earlier: [] },
  check: { claim: journalId === "overdue" ? "Margin falls" : "A claim with no asset", metric: "margin", direction: "below", threshold: 1, byDate },
  thesis: assetId ? { assetId, role: "reason" } : undefined,
}) as JournalRecord;

function bridge(tasks: ScheduledTaskListItem[], journal: JournalRecord[], inbox: InboxItem[][] = [[]]) {
  const inboxList = vi.fn(async ({ cursor }: { cursor?: string | null }) => {
    const page = Number(cursor ?? 0);
    return { items: inbox[page] ?? [], nextCursor: page + 1 < inbox.length ? String(page + 1) : null };
  });
  return { scheduledTasksList: async () => tasks, journalList: async () => journal, inboxList } as unknown as LoupistBridge;
}

it("lists a follow-up, an overdue check, a check tied to no asset, and nothing ahead", async () => {
  const onOpenTask = vi.fn();
  const user = userEvent.setup();
  const view = render(<FollowingAhead bridge={bridge([followUp], [check("overdue", "2026-11-20", "nvda"), check("loose", "2026-11-25")])} loupistId="loupist" assets={assets} now={now} onOpenTask={onOpenTask} />);
  const overdue = await screen.findByRole("region", { name: "Overdue" });
  expect(within(overdue).getByText("Check due: Margin falls")).toBeVisible();
  expect(within(overdue).getByRole("button", { name: "NVIDIA" })).toBeVisible();
  const tomorrowGroup = screen.getByRole("region", { name: "Tomorrow" });
  expect(within(tomorrowGroup).getByText("Results follow-up runs")).toBeVisible();
  expect(within(tomorrowGroup).getByText("After the filing")).toBeVisible();
  await user.click(within(tomorrowGroup).getByRole("button", { name: "Open" }));
  expect(onOpenTask).toHaveBeenCalledWith("follow-up");
  expect(within(screen.getByRole("region", { name: "Today" })).getByText("Not tied to an asset")).toBeVisible();

  view.rerender(<FollowingAhead bridge={bridge([], [])} loupistId="loupist" assets={assets} now={now} />);
  expect(await screen.findByText("Nothing ahead for the assets you follow. Ask Bot for a results follow-up.")).toBeVisible();
});

const run = (runId: string, kind: string, startedAtMs: number, ahead?: { journalId: string; title: string; byDate: string | null }[]) => ({
  runId, taskId: kind, kind, title: kind, status: "succeeded", summary: "", startedAtMs, finishedAtMs: startedAtMs + 60_000, readAtMs: null, threadId: null, messageId: null, skippedTools: [], error: null,
  briefingContext: ahead ? { symbols: [], principlesChecked: 0, newFacts: 0, disputedEntityIds: [], changed: { names: 0, relations: 0, conflicts: 0, rows: [] }, ahead } : null,
}) as unknown as InboxItem;

it("adds the newest briefing's Ahead rows, each from that briefing, without repeating or reviving a check (plan 4.7)", async () => {
  const briefedAt = new Date(2026, 10, 24, 7).getTime();
  const undated = { ...check("undated", "2026-11-20", "nvda"), check: { claim: "Inventario por debajo de 50 días", metric: "days", direction: "below", threshold: 50 } } as JournalRecord;
  const resolved = { ...check("resolved", "2026-11-28"), outcome: { kind: "met" } } as unknown as JournalRecord;
  const journal = [check("overdue", "2026-11-20", "nvda"), undated, resolved];
  const inbox = [
    [run("watch-1", "custom", briefedAt + 3_600_000)],
    [run("briefing-new", "briefing", briefedAt, [
      { journalId: "overdue", title: "Margin falls", byDate: "2026-11-20" },
      { journalId: "undated", title: "Inventory", byDate: null },
      { journalId: "resolved", title: "Resolved since", byDate: "2026-11-28" },
      { journalId: "deleted", title: "Deleted since", byDate: "2026-11-27" },
    ]), run("briefing-old", "briefing", briefedAt - 86_400_000, [{ journalId: "undated", title: "Older", byDate: null }])],
  ];
  render(<FollowingAhead bridge={bridge([], journal, inbox)} loupistId="loupist" assets={assets} now={now} />);
  const label = "From the briefing of Nov 24";
  const overdue = await screen.findByRole("region", { name: "Overdue" });
  expect(within(overdue).getAllByRole("listitem")).toHaveLength(1);
  expect(within(overdue).getByText("Check due: Margin falls")).toBeVisible();
  expect(within(overdue).getByText(label)).toBeVisible();
  const noDate = screen.getByRole("region", { name: "No date" });
  expect(within(noDate).getByText("Check due: Inventario por debajo de 50 días")).toBeVisible();
  expect(within(noDate).getByText(label)).toBeVisible();
  expect(screen.queryByText(/Resolved since|Deleted since/)).toBeNull();
  expect(screen.getAllByText(label)).toHaveLength(2);
});

it("keeps the rows as they were when no briefing has run", async () => {
  render(<FollowingAhead bridge={bridge([], [check("loose", "2026-11-25")], [[run("watch-1", "custom", now)]])} loupistId="loupist" assets={assets} now={now} />);
  const today = await screen.findByRole("region", { name: "Today" });
  expect(within(today).getByText("Not tied to an asset")).toBeVisible();
  expect(screen.queryByText(/From the briefing/)).toBeNull();
});

it("shows a follow-up's results on their stored day, announced or expected, and keeps its run row (plan 4.4)", async () => {
  const announced = { ...followUp, id: "announced", event: { date: "2026-12-03", precision: "exact" } } as ScheduledTaskListItem;
  const expected = { ...followUp, id: "expected", next_run_at_ms: new Date(2026, 11, 9, 9).getTime(), schedule: { kind: "once", at_ms: new Date(2026, 11, 9, 9).getTime() }, event: { date: "2026-12-08", precision: "estimated" } } as ScheduledTaskListItem;
  render(<FollowingAhead bridge={bridge([announced, expected], [])} loupistId="loupist" assets={assets} now={now} />);
  const results = await screen.findByText(/^Results Dec 3/);
  const day = results.closest("section")!;
  expect(within(day).getByRole("button", { name: "NVIDIA" })).toBeVisible();
  expect(screen.getByText(/^Results expected around Dec 8/)).toBeVisible();
  // Each follow-up still shows when it runs.
  expect(screen.getAllByText("Results follow-up runs")).toHaveLength(2);
  expect(within(screen.getByRole("region", { name: "Tomorrow" })).getByText("Results follow-up runs")).toBeVisible();
});
