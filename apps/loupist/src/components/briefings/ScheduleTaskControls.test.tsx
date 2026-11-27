import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ScheduleTaskControls } from "./ScheduleTaskControls";
import { commandError } from "@/bridge/mock/commandError";
import type { LoupistBridge, ScheduledTask } from "@/types";
import i18n from "@/i18n";

const task = (patch: Partial<ScheduledTask> = {}) => ({
  id: "task-1", loupist_id: "loupist", kind: "custom", title: "Chip news", revision: 3, status: "active", awaiting_confirmation: false,
  next_run_at_ms: null, last_run_at_ms: null, created_at_ms: 0, updated_at_ms: 0, last_error: null, session_thread_id: null,
  ...patch,
}) as unknown as ScheduledTask;

function setup(current: ScheduledTask, bridge: Partial<LoupistBridge> = {}) {
  const calls = {
    scheduledTaskConfirm: vi.fn(async () => current),
    scheduledTaskSetStatus: vi.fn(async () => current), scheduledTaskRunNow: vi.fn(async () => ({})), scheduledTaskDelete: vi.fn(async () => undefined),
  };
  const onChanged = vi.fn(), onDeleted = vi.fn(), onAskHome = vi.fn();
  render(<ScheduleTaskControls bridge={{ ...calls, ...bridge } as unknown as LoupistBridge} task={current} title="Chip news" running={false} onChanged={onChanged} onDeleted={onDeleted} onAskHome={onAskHome} />);
  return { calls, onChanged, onDeleted, onAskHome, user: userEvent.setup() };
}

it("confirms the displayed proposal revision from the task detail", async () => {
  const { calls, user } = setup(task({ status: "paused", awaiting_confirmation: true }));
  await user.click(screen.getByRole("button", { name: "Turn on" }));
  expect(calls.scheduledTaskConfirm).toHaveBeenCalledExactlyOnceWith({
    loupist_id: "loupist", task_id: "task-1", expected_revision: 3,
  });
  expect(calls.scheduledTaskSetStatus).not.toHaveBeenCalled();
});

it("pauses, runs once and deletes a task with the host commands, without a model call", async () => {
  const { calls, onChanged, onDeleted, user } = setup(task());
  await user.click(screen.getByRole("button", { name: "Pause" }));
  expect(calls.scheduledTaskSetStatus).toHaveBeenCalledWith({ loupist_id: "loupist", task_id: "task-1", expected_revision: 3, status: "paused" });
  await user.click(screen.getByRole("button", { name: "Run now" }));
  expect(calls.scheduledTaskRunNow).toHaveBeenCalledWith({ loupist_id: "loupist", task_id: "task-1" });
  expect(onChanged).toHaveBeenCalledTimes(2);
  await user.click(screen.getByRole("button", { name: "More actions for Chip news" }));
  await user.click(await screen.findByRole("menuitem", { name: "Delete…" }));
  const dialog = await screen.findByRole("dialog", { name: "Delete “Chip news”?" });
  expect(dialog).toHaveAccessibleDescription(/Its run history .* are deleted with it.*Summaries already delivered to Bot stay, without a link to the run\./);
  expect(calls.scheduledTaskDelete).not.toHaveBeenCalled();
  await user.click(within(dialog).getByRole("button", { name: "Delete" }));
  expect(calls.scheduledTaskDelete).toHaveBeenCalledWith({ loupist_id: "loupist", task_id: "task-1", expected_revision: 3 });
  expect(onDeleted).toHaveBeenCalledOnce();
});

it("explains what happens to run history and Bot summaries in Korean", async () => {
  await i18n.changeLanguage("ko");
  try {
    const { user } = setup(task());
    await user.click(screen.getByRole("button", { name: "Chip news 더보기" }));
    await user.click(await screen.findByRole("menuitem", { name: "삭제…" }));
    const dialog = await screen.findByRole("dialog", { name: "“Chip news” 작업을 삭제하시겠습니까?" });
    expect(dialog).toHaveAccessibleDescription(/실행 기록과 .* 함께 삭제됩니다.*이미 봇에 전달된 요약은 남지만 실행으로 가는 링크는 없어집니다\./);
  } finally {
    await act(async () => { await i18n.changeLanguage("en"); });
  }
});

it("resumes a paused task, keeps the briefing undeletable, and shows a failure with retry", async () => {
  const { calls, user } = setup(task({ kind: "briefing", status: "paused" }), { scheduledTaskSetStatus: vi.fn().mockRejectedValueOnce(new Error("")) as never });
  expect(screen.queryByRole("button", { name: /More actions/ })).toBeNull();
  expect(screen.getByRole("button", { name: "Change in Bot" })).toBeVisible();
  // Paused: Resume is the one filled button.
  expect(screen.getByRole("button", { name: "Resume" })).toHaveAttribute("data-variant", "default");
  await user.click(screen.getByRole("button", { name: "Resume" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't change this task. Try again.");
  expect(calls.scheduledTaskRunNow).not.toHaveBeenCalled();
});

it("asks before running a held task and deletes a learning program", async () => {
  const allowance = setup(task({ kind: "learning", hold: { reason: "allowance", local_day: "2026-11-25", provider: null } }));
  expect(screen.getByRole("button", { name: "Ask Bot to change" })).toBeVisible();
  expect(screen.queryByRole("combobox", { name: "Posts to" })).toBeNull();
  await allowance.user.click(screen.getByRole("button", { name: "Run now" }));
  const dialog = await screen.findByRole("dialog", { name: "Run now" });
  expect(dialog).toHaveAccessibleDescription("This runs beyond today's allowance.");
  expect(allowance.calls.scheduledTaskRunNow).not.toHaveBeenCalled();
  await allowance.user.click(within(dialog).getByRole("button", { name: "Run now" }));
  expect(allowance.calls.scheduledTaskRunNow).toHaveBeenCalledWith({ loupist_id: "loupist", task_id: "task-1", confirm: true });
  await allowance.user.click(screen.getByRole("button", { name: "More actions for Chip news" }));
  await allowance.user.click(await screen.findByRole("menuitem", { name: "Delete…" }));
  expect(await screen.findByRole("dialog", { name: "Delete “Chip news”?" })).toBeVisible();
});

it("opens the one-run confirmation when the host refuses with ask_first and the task has no hold", async () => {
  const hold = { reason: "allowance" as const, local_day: "2026-11-25", provider: null };
  const runNow = vi.fn(async (request: { confirm?: boolean }) => {
    if (!request.confirm) throw commandError("conflict", "Run now needs confirmation because background work is held.", { action: "ask_first", hold });
    return {};
  });
  const { user, onChanged } = setup(task({ kind: "learning" }), { scheduledTaskRunNow: runNow as never });
  await user.click(screen.getByRole("button", { name: "Run now" }));
  const dialog = await screen.findByRole("dialog", { name: "Run now" });
  expect(dialog).toHaveAccessibleDescription("This runs beyond today's allowance.");
  expect(screen.queryByRole("alert")).toBeNull();
  expect(runNow).toHaveBeenCalledTimes(1);
  expect(runNow).toHaveBeenLastCalledWith({ loupist_id: "loupist", task_id: "task-1" });
  await user.click(within(dialog).getByRole("button", { name: "Run now" }));
  expect(runNow).toHaveBeenLastCalledWith({ loupist_id: "loupist", task_id: "task-1", confirm: true });
  expect(onChanged).toHaveBeenCalledOnce();
});

it("shows a Run now failure that is not a typed refusal", async () => {
  const { user } = setup(task(), { scheduledTaskRunNow: vi.fn().mockRejectedValue(new Error("offline")) as never });
  await user.click(screen.getByRole("button", { name: "Run now" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("offline");
  expect(screen.queryByRole("dialog", { name: "Run now" })).toBeNull();
});

it("asks before running a learning program held by the plan limit", async () => {
  const plan = setup(task({ kind: "learning", hold: { reason: "plan_limit", local_day: "2026-11-25", provider: "openai" } }));
  await plan.user.click(screen.getByRole("button", { name: "Run now" }));
  expect(await screen.findByRole("dialog", { name: "Run now" })).toHaveAccessibleDescription("Your AI plan's limit was reached. Runs resume tomorrow.");
  expect(plan.calls.scheduledTaskRunNow).not.toHaveBeenCalled();
});

it("hands time and prompt changes to Home as a draft", async () => {
  const { onAskHome, user } = setup(task());
  await user.click(screen.getByRole("button", { name: "Change in Bot" }));
  expect(onAskHome).toHaveBeenCalledWith("Change my scheduled task “Chip news”: ");
});
