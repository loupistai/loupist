import { act, renderHook } from "@testing-library/react";
import { MockLoupistBridge } from "../bridge/mockBridge";
import type { ChatThread } from "../types";
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
});
