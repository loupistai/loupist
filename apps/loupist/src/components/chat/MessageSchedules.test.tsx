import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useLoupistVisit } from "../../app/loupistVisit";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { MOCK_LOUPISTS } from "../../bridge/mock/fixtures";
import type { ScheduleReceipt } from "../../types";
import { BotScheduleTasks, useBotScheduleTasks } from "./BotScheduleTasks";
import { BotNeedsYou } from "./BotNeedsYou";
import { ChatExecutionProvider } from "./ChatExecutionContext";
import { MessageSchedules } from "./MessageSchedules";

const loupistId = MOCK_LOUPISTS[0]!.id;
const weekly = { kind: "weekly" as const, days: [1], hour: 8, minute: 0, timezone: "Asia/Seoul" };

async function proposed() {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const task = await bridge.proposeScheduledTask({
    loupist_id: loupistId, title: "Compare inventory days", prompt: "Compare inventory days across what I follow.", schedule: weekly,
    target: { kind: "new_chat" }, model_profile_id: "model", thinking_level: "medium", run_options: {}, use_memory: true, update_memory: false, status: "active",
  });
  const receipt: ScheduleReceipt = { task_id: task.id, title: task.title, revision: task.revision, action: "created", status: "paused", schedule: weekly,
    target: { kind: "new_chat" }, next_run_at_ms: null, use_memory: true, update_memory: false, awaiting_confirmation: true };
  const view = () => <ChatExecutionProvider value={{ bridge, loupistId, threadId: "chat", surface: "session", theme: "light", active: true }}>
    <MessageSchedules receipts={[receipt]} />
  </ChatExecutionProvider>;
  return { bridge, task, receipt, view };
}

it("keeps a routine set up from a sentence paused until the user turns it on", async () => {
  const user = userEvent.setup();
  const { bridge, task, view } = await proposed();
  render(view());
  const card = await screen.findByRole("article", { name: "Scheduled task: Compare inventory days" });
  expect(await within(card).findByText("Waiting for your OK")).toBeVisible();
  expect(within(card).getByText("Nothing runs until you turn it on.")).toBeVisible();
  await expect(bridge.scheduledTaskRunNow({ loupist_id: loupistId, task_id: task.id })).rejects.toThrow("waiting for approval");
  await user.click(within(card).getByRole("button", { name: "Turn on" }));
  expect(await within(card).findByText("Active")).toBeVisible();
  expect(within(card).getByText(/^Next run /)).toBeVisible();
  expect(within(card).queryByRole("button", { name: "Turn on" })).not.toBeInTheDocument();
  expect(await bridge.scheduledTaskGet({ loupist_id: loupistId, task_id: task.id })).toMatchObject({ status: "active", awaiting_confirmation: false });
});

it("sends the displayed revision and leaves a changed proposal paused", async () => {
  const user = userEvent.setup();
  const { bridge, task, view } = await proposed();
  const confirm = vi.spyOn(bridge, "scheduledTaskConfirm");
  render(view());
  const button = await screen.findByRole("button", { name: "Turn on" });
  bridge.installScheduledTask({ ...task, revision: task.revision + 1, prompt: "A different check" });
  await user.click(button);
  expect(confirm).toHaveBeenCalledExactlyOnceWith({
    loupist_id: loupistId, task_id: task.id, expected_revision: task.revision,
  });
  expect((await bridge.scheduledTaskGet({ loupist_id: loupistId, task_id: task.id }))?.status).toBe("paused");
  await waitFor(() => expect(screen.queryByRole("button", { name: "Turn on" })).not.toBeInTheDocument());
});

it("shows and confirms the same proposal revision in Needs you", async () => {
  const user = userEvent.setup();
  const { bridge, task } = await proposed();
  const next = { ...task, title: "Review cash flow", revision: task.revision + 1,
    schedule: { kind: "interval" as const, every_minutes: 45 } };
  bridge.installScheduledTask(next);
  const confirm = vi.spyOn(bridge, "scheduledTaskConfirm");
  render(<BotNeedsYou items={[{
    kind: "proposal", recordId: task.id, loupistId, loupistName: "Research", title: task.title, atMs: 0,
  }]} bridge={bridge} loupistId={loupistId} onOpen={vi.fn()} onAsk={vi.fn()} />);
  expect(await screen.findByText(/Review cash flow/)).toBeVisible();
  expect(screen.getByText(/45 minutes/)).toBeVisible();
  expect(screen.queryByText(/Compare inventory days/)).not.toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Turn on" }));
  expect(confirm).toHaveBeenCalledExactlyOnceWith({ loupist_id: loupistId, task_id: next.id, expected_revision: next.revision });
});

it("discards a declined routine and shows the confirmed state after a reload", async () => {
  const user = userEvent.setup();
  const first = await proposed();
  const { unmount } = render(first.view());
  await user.click(await screen.findByRole("button", { name: "Discard" }));
  expect(await screen.findByText("Discarded")).toBeVisible();
  expect(await first.bridge.scheduledTaskGet({ loupist_id: loupistId, task_id: first.task.id })).toBeNull();
  unmount();

  const second = await proposed();
  await second.bridge.scheduledTaskConfirm({ loupist_id: loupistId, task_id: second.task.id, expected_revision: second.task.revision });
  render(second.view());
  expect(await screen.findByText("Active")).toBeVisible();
  expect(screen.queryByRole("button", { name: "Turn on" })).not.toBeInTheDocument();
});

it("restores a declined Bot proposal during the visit and removes it if still declined when the visit ends", async () => {
  const user = userEvent.setup();
  const { bridge, task, receipt } = await proposed();
  bridge.installScheduledTask({ ...task, report: "when_met" });
  const scoping = { bridge, loupistId, threadId: "bot", surface: "bot" as const, theme: "light" as const, active: true };
  const closeVisit = (loupist_id: string, visit: number) => { void bridge.scheduledProposalVisitEnd({ loupist_id, visit }); };
  function Bot({ present }: { present: boolean }) {
    useLoupistVisit(loupistId, present, closeVisit);
    const tasks = useBotScheduleTasks(bridge, loupistId, "bot");
    return <ChatExecutionProvider value={scoping}>
      <BotScheduleTasks.Provider value={tasks}><MessageSchedules receipts={[receipt]} /></BotScheduleTasks.Provider>
    </ChatExecutionProvider>;
  }
  const { rerender } = render(<Bot present />);
  await user.click(await screen.findByRole("button", { name: "Not now" }));
  expect(await screen.findByText("Not turned on")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Undo" }));
  expect(await screen.findByRole("button", { name: "Turn on" })).toBeVisible();
  expect((await bridge.scheduledTaskGet({ loupist_id: loupistId, task_id: task.id }))?.status).toBe("paused");
  await user.click(screen.getByRole("button", { name: "Not now" }));
  await screen.findByRole("button", { name: "Undo" });
  rerender(<Bot present={false} />);
  await waitFor(() => expect(screen.queryByRole("button", { name: "Undo" })).not.toBeInTheDocument());
  expect(await bridge.scheduledTaskGet({ loupist_id: loupistId, task_id: task.id })).toBeNull();
});

it("labels a hand-off receipt by the conversation's surface, not its id", () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const receipt: ScheduleReceipt = { task_id: "bot-handoff-abc", title: "Weekly comparison", revision: 1, action: "created", status: "active", schedule: weekly,
    target: { kind: "new_chat" }, next_run_at_ms: null, use_memory: true, update_memory: false, awaiting_confirmation: false };
  const view = (surface: "bot" | "session") => <ChatExecutionProvider value={{ bridge, loupistId, threadId: "any", surface, theme: "light", active: true }}>
    <MessageSchedules receipts={[receipt]} />
  </ChatExecutionProvider>;
  const { rerender } = render(view("session"));
  expect(screen.getByText("Handed to Bot · Active")).toBeVisible();
  rerender(view("bot"));
  expect(screen.queryByText("Active")).toBeNull();
  expect(screen.queryByText(/Handed to Bot/)).toBeNull();
});
