import { act, renderHook } from "@testing-library/react";
import { MockLoupistBridge } from "../bridge/mockBridge";
import type { ScheduledRun } from "../types";
import { scheduledRunFixture } from "../test/scheduledFixtures";
import { useChatSessions } from "./useChatSessions";
import { useScheduledRuns } from "./useScheduledRuns";

afterEach(() => vi.useRealTimers());

it("discovers a manual run that has already finished when Run now returns", async () => {
  vi.useFakeTimers();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const initial = await bridge.loupistSelect("loupist-quality");
  const activity = vi.spyOn(bridge, "scheduledTasksActivity").mockResolvedValue([]);
  const watch = vi.spyOn(bridge, "scheduledTaskWatch");
  const { result, unmount } = renderHook(() => {
    const chat = useChatSessions(bridge);
    const scheduled = useScheduledRuns(bridge, "loupist-quality", chat);
    return { chat, scheduled };
  });
  act(() => result.current.chat.hydrateThreadsForLoupist("loupist-quality", initial.threads));
  await act(async () => { await vi.advanceTimersByTimeAsync(0); });
  const thread = await bridge.chatCreate({ loupist_id: "loupist-quality", title: "Immediate manual result" });
  const completed = scheduledRunFixture({ thread_id: thread.id, status: "failed", finished_at_ms: 2000 });
  activity.mockResolvedValue([completed]);
  act(() => result.current.scheduled.onRunStarted(completed));
  await act(async () => { await vi.advanceTimersByTimeAsync(2000); });
  expect(result.current.chat.threadsByLoupist["loupist-quality"].some((item) => item.id === thread.id)).toBe(true);
  expect(watch).not.toHaveBeenCalled();
  unmount();
});

it.each([true, false])("discovers completed runs without ever seeing them live (before first poll: %s)", async (beforeFirstPoll) => {
  vi.useFakeTimers();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const initial = await bridge.loupistSelect("loupist-quality");
  const resultThread = await bridge.chatCreate({ loupist_id: "loupist-quality", title: "Automatic result" });
  const completed = scheduledRunFixture({ thread_id: resultThread.id, status: "succeeded", finished_at_ms: 2000 });
  const activity = vi.spyOn(bridge, "scheduledTasksActivity").mockResolvedValue(beforeFirstPoll ? [completed] : []);
  const select = vi.spyOn(bridge, "loupistSelect");
  const watch = vi.spyOn(bridge, "scheduledTaskWatch");
  const { result, unmount } = renderHook(() => {
    const chat = useChatSessions(bridge);
    useScheduledRuns(bridge, "loupist-quality", chat);
    return chat;
  });
  act(() => result.current.hydrateThreadsForLoupist("loupist-quality", initial.threads));
  await act(async () => { await vi.advanceTimersByTimeAsync(0); });
  if (!beforeFirstPoll) {
    expect(result.current.threadsByLoupist["loupist-quality"].some((thread) => thread.id === resultThread.id)).toBe(false);
    activity.mockResolvedValue([completed]);
    await act(async () => { await vi.advanceTimersByTimeAsync(2000); });
  }
  expect(result.current.threadsByLoupist["loupist-quality"].some((thread) => thread.id === resultThread.id)).toBe(true);
  expect(watch).not.toHaveBeenCalled();
  const refreshes = select.mock.calls.length;
  await act(async () => { await vi.advanceTimersByTimeAsync(4000); });
  expect(select).toHaveBeenCalledTimes(refreshes);
  unmount();
});

it("retries failed completion refreshes and updates an already loaded Chat", async () => {
  vi.useFakeTimers();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const workspace = await bridge.loupistSelect("loupist-quality");
  const thread = workspace.threads[0];
  const fresh = { ...thread, messages: [...thread.messages, { id: "scheduled-answer", role: "assistant" as const, content: "Fresh scheduled answer", status: "complete" as const, created_at: new Date().toISOString() }] };
  const activity = vi.spyOn(bridge, "scheduledTasksActivity").mockResolvedValue([]);
  const select = vi.spyOn(bridge, "loupistSelect").mockRejectedValueOnce(new Error("temporary storage failure")).mockResolvedValue({ ...workspace, threads: [fresh, ...workspace.threads.slice(1)] });
  const { result, unmount } = renderHook(() => {
    const chat = useChatSessions(bridge);
    useScheduledRuns(bridge, "loupist-quality", chat);
    return chat;
  });
  act(() => result.current.hydrateThreadsForLoupist("loupist-quality", workspace.threads));
  await act(async () => { await vi.advanceTimersByTimeAsync(0); });
  activity.mockResolvedValue([scheduledRunFixture({ thread_id: thread.id, status: "succeeded", finished_at_ms: 2000 })]);
  await act(async () => { await vi.advanceTimersByTimeAsync(2000); });
  expect(select).toHaveBeenCalledTimes(1);
  expect(result.current.threadsByLoupist["loupist-quality"][0].messages).toEqual(thread.messages);
  await act(async () => { await vi.advanceTimersByTimeAsync(2000); });
  expect(select).toHaveBeenCalledTimes(2);
  expect(result.current.threadsByLoupist["loupist-quality"][0].messages.at(-1)?.content).toBe("Fresh scheduled answer");
  unmount();
});

it("ignores a discovery response from the previous Loupist after switching", async () => {
  vi.useFakeTimers();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  let resolveOld!: (runs: ScheduledRun[]) => void;
  vi.spyOn(bridge, "scheduledTasksActivity").mockImplementationOnce(() => new Promise((resolve) => { resolveOld = resolve; })).mockResolvedValue([]);
  const select = vi.spyOn(bridge, "loupistSelect");
  const watch = vi.spyOn(bridge, "scheduledTaskWatch");
  const { rerender, unmount } = renderHook(({ loupistId }) => {
    const chat = useChatSessions(bridge);
    useScheduledRuns(bridge, loupistId, chat);
  }, { initialProps: { loupistId: "loupist-quality" } });
  rerender({ loupistId: "loupist-value" });
  await act(async () => { resolveOld([scheduledRunFixture()]); await vi.advanceTimersByTimeAsync(0); });
  expect(select).not.toHaveBeenCalled();
  expect(watch).not.toHaveBeenCalled();
  unmount();
});
