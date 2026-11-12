import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { MOCK_LOUPISTS } from "../../bridge/mock/fixtures";
import type { InboxChangedEvent } from "../../contracts/scheduled";
import type { ChatMessage, LoupistWorkspace, ScheduledRun, ScheduledTaskListItem } from "@/types";
import { ScheduleView } from "./ScheduleView";

const loupist = MOCK_LOUPISTS[0]!;
const running: ScheduledRun = {
  briefing_context: null, kind: "custom", summary: "", read_at_ms: null, message_id: null, skipped_tools: [], id: "run-row", task_id: "task-1",
  loupist_id: loupist.id, task_revision: 1, task_title: "Chip news", thread_id: "chat-run", run_id: "run-1", scheduled_for_ms: 0,
  started_at_ms: Date.now(), finished_at_ms: null, status: "running", error: null,
};
const task = (latest: ScheduledRun) => ({
  id: "task-1", loupist_id: loupist.id, kind: "custom", title: "Chip news", revision: 1, status: "active", awaiting_confirmation: false,
  next_run_at_ms: null, last_run_at_ms: latest.started_at_ms, created_at_ms: 0, updated_at_ms: 0, last_error: null, session_thread_id: null,
  schedule: { kind: "once", at_ms: 0 }, latest_run: latest,
}) as unknown as ScheduledTaskListItem;

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
  expect(await screen.findByText("Preparing your briefing…", { selector: "p" })).toBeVisible();

  run = { ...running, status: "succeeded", message_id: answer.id, finished_at_ms: Date.now(), summary: "Chip demand held up this week." };
  await act(async () => { changed?.({ loupistId: loupist.id }); });
  expect(await screen.findByText("Chip demand held up this week.", { selector: ".message-content p" })).toBeVisible();
  // Read once it has been on screen for a second.
  await vi.waitFor(() => expect(markRead).toHaveBeenCalledWith({ loupistId: loupist.id, runIds: ["run-1"] }), { timeout: 2_000 });
});
