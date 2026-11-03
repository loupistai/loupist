import { act, renderHook } from "@testing-library/react";
import { MockLoupistBridge } from "../bridge/mockBridge";
import type { ChatThread, LoupistWorkspace } from "../types";
import { useChatSessions } from "./useChatSessions";

const thread: ChatThread = { id: "thread-noop", loupist_id: "loupist-noop", title: "No-op state update", updated_at: "2026-08-25T00:00:00.000Z", use_memory: true, update_memory: false, messages: [] };

describe("useChatSessions", () => {
  it("does not publish a new state object when a thread updater returns current", () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const { result } = renderHook(() => useChatSessions(bridge));
    act(() => result.current.setThreadsForLoupist(thread.loupist_id, [thread]));
    const currentState = result.current.threadsByLoupist;
    const currentRef = result.current.threadsByLoupistRef.current;
    act(() => result.current.setThreadsForLoupist(thread.loupist_id, (current) => current));
    expect(result.current.threadsByLoupist).toBe(currentState);
    expect(result.current.threadsByLoupistRef.current).toBe(currentRef);
  });
  it("merges a background listing with live changes without selecting a Chat", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const workspace = await bridge.loupistSelect("loupist-quality");
    workspace.threads = workspace.threads.filter((thread) => !thread.home);
    const [first, second] = workspace.threads;
    let release!: (workspace: LoupistWorkspace) => void;
    const select = vi.spyOn(bridge, "loupistSelect").mockImplementationOnce(() => new Promise((resolve) => { release = resolve; }));
    const { result } = renderHook(() => useChatSessions(bridge));
    act(() => result.current.setThreadsForLoupist(first.loupist_id, [first, second]));
    let pending!: Promise<void>;
    act(() => { pending = result.current.loadThreadsForLoupist(first.loupist_id); });
    expect(result.current.loadThreadsForLoupist(first.loupist_id)).toBe(pending);
    const created = { ...first, id: "created-while-loading", title: "New conversation" };
    act(() => result.current.setThreadsForLoupist(first.loupist_id, [{ ...first, title: "Renamed while loading" }, created]));
    await act(async () => { release(workspace); await pending; });
    expect(result.current.threadsByLoupist[first.loupist_id].map((item) => item.title)).toEqual(["Renamed while loading", "New conversation"]);
    expect(result.current.activeThreadIds).toEqual({});
    expect(result.current.loadedLoupistIds.has(first.loupist_id)).toBe(true);
    await result.current.loadThreadsForLoupist(first.loupist_id);
    expect(select).toHaveBeenCalledOnce();
  });

  it("does not restore a removed Loupist when its sidebar load completes", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const workspace = await bridge.loupistSelect("loupist-quality");
    let release!: (workspace: LoupistWorkspace) => void;
    vi.spyOn(bridge, "loupistSelect").mockImplementationOnce(() => new Promise((resolve) => { release = resolve; }));
    const { result } = renderHook(() => useChatSessions(bridge));
    let pending!: Promise<void>;
    act(() => { pending = result.current.loadThreadsForLoupist(workspace.loupist.id); });
    act(() => result.current.removeLoupist(workspace.loupist.id));
    await act(async () => { release(workspace); await pending; });
    expect(result.current.threadsByLoupist[workspace.loupist.id]).toBeUndefined();
    expect(result.current.loadedLoupistIds.has(workspace.loupist.id)).toBe(false);
  });
});
