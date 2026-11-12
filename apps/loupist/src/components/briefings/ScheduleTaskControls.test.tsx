import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ScheduleTaskControls } from "./ScheduleTaskControls";
import type { LoupistBridge, ScheduledTask } from "@/types";

const task = (patch: Partial<ScheduledTask> = {}) => ({
  id: "task-1", loupist_id: "loupist", kind: "custom", title: "Chip news", revision: 3, status: "active", awaiting_confirmation: false,
  next_run_at_ms: null, last_run_at_ms: null, created_at_ms: 0, updated_at_ms: 0, last_error: null, session_thread_id: null,
  ...patch,
}) as unknown as ScheduledTask;

function setup(current: ScheduledTask, bridge: Partial<LoupistBridge> = {}) {
  const calls = {
    scheduledTaskSetStatus: vi.fn(async () => current), scheduledTaskRunNow: vi.fn(async () => ({})), scheduledTaskDelete: vi.fn(async () => undefined),
  };
  const onChanged = vi.fn(), onDeleted = vi.fn(), onAskHome = vi.fn();
  render(<ScheduleTaskControls bridge={{ ...calls, ...bridge } as unknown as LoupistBridge} task={current} title="Chip news" running={false} onChanged={onChanged} onDeleted={onDeleted} onAskHome={onAskHome} />);
  return { calls, onChanged, onDeleted, onAskHome, user: userEvent.setup() };
}

it("pauses, runs once and deletes a task with the host commands, without a model call", async () => {
  const { calls, onChanged, onDeleted, user } = setup(task());
  await user.click(screen.getByRole("button", { name: "Pause" }));
  expect(calls.scheduledTaskSetStatus).toHaveBeenCalledWith({ loupist_id: "loupist", task_id: "task-1", expected_revision: 3, status: "paused" });
  await user.click(screen.getByRole("button", { name: "Run now" }));
  expect(calls.scheduledTaskRunNow).toHaveBeenCalledWith({ loupist_id: "loupist", task_id: "task-1" });
  expect(onChanged).toHaveBeenCalledTimes(2);
  await user.click(screen.getByRole("button", { name: "Delete" }));
  const dialog = await screen.findByRole("dialog", { name: "Delete “Chip news”?" });
  expect(calls.scheduledTaskDelete).not.toHaveBeenCalled();
  await user.click(within(dialog).getByRole("button", { name: "Delete" }));
  expect(calls.scheduledTaskDelete).toHaveBeenCalledWith({ loupist_id: "loupist", task_id: "task-1", expected_revision: 3 });
  expect(onDeleted).toHaveBeenCalledOnce();
});

it("resumes a paused task, keeps the briefing undeletable, and shows a failure with retry", async () => {
  const { calls, user } = setup(task({ kind: "briefing", status: "paused" }), { scheduledTaskSetStatus: vi.fn().mockRejectedValueOnce(new Error("")) as never });
  expect(screen.queryByRole("button", { name: "Delete" })).toBeNull();
  await user.click(screen.getByRole("button", { name: "Resume" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't change this task. Try again.");
  expect(calls.scheduledTaskRunNow).not.toHaveBeenCalled();
});

it("hands time and prompt changes to Home as a draft", async () => {
  const { onAskHome, user } = setup(task());
  expect(screen.getByText(/To change a task's time or prompt, tell your Loupist in Home/)).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Ask in Home" }));
  expect(onAskHome).toHaveBeenCalledWith("Change my scheduled task “Chip news”: ");
});
