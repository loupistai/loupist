import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ProgramUndo } from "@/contracts/scheduled";
import type { LoupistBridge, ScheduledTask } from "@/types";
import { LearningProgramActions } from "./LearningProgramActions";

const task = {
  id: "learn-busan", loupist_id: "loupist-b", kind: "learning", title: "부산항 물동량", revision: 2, status: "active", prompt: "Monthly throughput at Busan.",
  schedule: { kind: "daily", hour: 17, minute: 0, timezone: "Asia/Seoul" }, awaiting_confirmation: false,
} as unknown as ScheduledTask;

function setup(undoAll: (request: { preview?: boolean }) => Promise<ProgramUndo>) {
  const calls = { learningProgramUndoAll: vi.fn(undoAll), learningProgramUpdate: vi.fn() };
  const onChanged = vi.fn();
  render(<LearningProgramActions bridge={calls as unknown as LoupistBridge} task={task} names={new Map()} onChanged={onChanged} />);
  return { calls, onChanged, user: userEvent.setup() };
}

it("states the counts from a dry call before it undoes every run, then says what it did", async () => {
  const { calls, onChanged, user } = setup(async request => request.preview ? { runs: 3, undone: 5, kept: 1 } : { runs: 3, undone: 5, kept: 1 });
  await user.click(screen.getByRole("button", { name: "Undo all runs" }));
  const dialog = await screen.findByRole("dialog", { name: "Undo every run of “부산항 물동량”?" });
  expect(calls.learningProgramUndoAll).toHaveBeenCalledExactlyOnceWith({ loupistId: "loupist-b", taskId: "learn-busan", preview: true });
  expect(await within(dialog).findByText("3 runs are undone.")).toBeVisible();
  expect(within(dialog).getByText("5 sections go back to how they were.")).toBeVisible();
  expect(within(dialog).getByText("1 section changed since and is kept.")).toBeVisible();
  expect(dialog).toHaveAccessibleDescription("Sources stay in the Library, and a thesis move keeps its own Undo on the asset page.");
  await user.click(within(dialog).getByRole("button", { name: "Undo all runs" }));
  expect(calls.learningProgramUndoAll).toHaveBeenLastCalledWith({ loupistId: "loupist-b", taskId: "learn-busan" });
  expect(await screen.findByRole("status")).toHaveTextContent("Undid 3 runs (1 section changed since and was kept)");
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(onChanged).toHaveBeenCalledOnce();
});

it("pluralizes both counts in the line it leaves", async () => {
  const { user } = setup(async () => ({ runs: 1, undone: 2, kept: 4 }));
  await user.click(screen.getByRole("button", { name: "Undo all runs" }));
  await user.click(within(await screen.findByRole("dialog")).getByRole("button", { name: "Undo all runs" }));
  expect(await screen.findByRole("status")).toHaveTextContent("Undid 1 run (4 sections changed since and were kept)");
});

it("offers nothing to confirm when every run is already undone", async () => {
  const { user } = setup(async () => ({ runs: 0, undone: 0, kept: 0 }));
  await user.click(screen.getByRole("button", { name: "Undo all runs" }));
  const dialog = await screen.findByRole("dialog");
  expect(await within(dialog).findByText("There is nothing left to undo.")).toBeVisible();
  expect(within(dialog).getByRole("button", { name: "Undo all runs" })).toBeDisabled();
});

it("shows counting only after a slow moment, and Try again when counting fails", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  try {
    let attempt = 0;
    let release!: (value: ProgramUndo) => void;
    const { user } = setup(() => ++attempt === 1 ? Promise.reject(new Error("offline")) : new Promise(resolve => { release = resolve; }));
    await user.click(screen.getByRole("button", { name: "Undo all runs" }));
    const dialog = await screen.findByRole("dialog");
    expect(await within(dialog).findByRole("alert")).toHaveTextContent("Couldn't count what this undoes.");
    await user.click(within(dialog).getByRole("button", { name: "Try again" }));
    expect(within(dialog).queryByText("Counting what this undoes…")).toBeNull();
    await act(async () => { vi.advanceTimersByTime(300); });
    expect(within(dialog).getByText("Counting what this undoes…")).toBeVisible();
    await act(async () => { release({ runs: 2, undone: 2, kept: 0 }); });
    expect(within(dialog).getByText("2 runs are undone.")).toBeVisible();
    expect(within(dialog).queryByText(/changed since/)).toBeNull();
  } finally { vi.useRealTimers(); }
});
