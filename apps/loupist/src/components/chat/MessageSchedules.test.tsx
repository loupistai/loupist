import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { MOCK_LOUPISTS } from "../../bridge/mock/fixtures";
import type { ScheduleReceipt } from "../../types";
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
    <MessageSchedules receipts={[receipt]} onOpen={vi.fn()} />
  </ChatExecutionProvider>;
  return { bridge, task, view };
}

it("keeps a routine set up from a sentence paused until the user turns it on", async () => {
  const user = userEvent.setup();
  const { bridge, task, view } = await proposed();
  render(view());
  const card = await screen.findByRole("article", { name: "Scheduled task: Compare inventory days" });
  expect(await within(card).findByText("Waiting for your OK")).toBeVisible();
  expect(within(card).getByText("Nothing runs until you turn it on.")).toBeVisible();
  await user.click(within(card).getByRole("button", { name: "Turn on" }));
  expect(await within(card).findByText("Active")).toBeVisible();
  expect(within(card).getByText(/^Next run /)).toBeVisible();
  expect(within(card).queryByRole("button", { name: "Turn on" })).not.toBeInTheDocument();
  expect(await bridge.scheduledTaskGet({ loupist_id: loupistId, task_id: task.id })).toMatchObject({ status: "active", awaiting_confirmation: false });
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
  await second.bridge.scheduledTaskConfirm({ loupist_id: loupistId, task_id: second.task.id });
  render(second.view());
  expect(await screen.findByText("Active")).toBeVisible();
  expect(screen.queryByRole("button", { name: "Turn on" })).not.toBeInTheDocument();
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
  expect(screen.getByText("Active")).toBeVisible();
  expect(screen.queryByText(/Handed to Bot/)).toBeNull();
});
