import { act, renderHook } from "@testing-library/react";
import { MockLoupistBridge } from "../bridge/mockBridge";
import { MOCK_LOUPISTS } from "../bridge/mock/fixtures";
import { chatSessionKey } from "../chat/sessionRegistry";
import { useFinishedWhileHidden } from "./useAttention";
import { EMPTY_CHAT_THREAD_ID } from "./emptyChat";

const loupist = MOCK_LOUPISTS[0]!;
const key = chatSessionKey(loupist.id, "chat-a");
const other = chatSessionKey(loupist.id, "chat-b");

it("marks a run that ended out of sight, notifies once, and clears when the chat is opened", () => {
  vi.spyOn(document, "hasFocus").mockReturnValue(true);
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const { result, rerender } = renderHook((props: { running: ReadonlySet<string>; visible: string | null }) =>
    useFinishedWhileHidden({ bridge, loupists: [loupist], runningThreadKeys: props.running, visibleChatKey: props.visible }),
  { initialProps: { running: new Set([key, other]), visible: other } });
  // chat-a ends while the user reads chat-b; chat-b ends in view.
  rerender({ running: new Set(), visible: other });
  expect([...result.current.finishedKeys]).toEqual([key]);
  expect(bridge.notifications).toEqual([{ loupistId: loupist.id, chatId: "chat-a", title: `${loupist.name} finished` }]);
  act(() => rerender({ running: new Set(), visible: key }));
  expect(result.current.finishedKeys.size).toBe(0);
});

it("does not treat a new chat's move from its draft id as an ending", () => {
  vi.spyOn(document, "hasFocus").mockReturnValue(true);
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const draft = chatSessionKey(loupist.id, EMPTY_CHAT_THREAD_ID);
  const { result, rerender } = renderHook((props: { running: ReadonlySet<string> }) =>
    useFinishedWhileHidden({ bridge, loupists: [loupist], runningThreadKeys: props.running, visibleChatKey: key }),
  { initialProps: { running: new Set([draft]) } });
  rerender({ running: new Set([key]) });
  expect(result.current.finishedKeys.size).toBe(0);
  expect(bridge.notifications).toEqual([]);
});
