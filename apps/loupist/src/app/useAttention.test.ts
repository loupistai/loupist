import { act, renderHook, waitFor } from "@testing-library/react";
import { MockLoupistBridge } from "../bridge/mockBridge";
import { MOCK_LOUPISTS } from "../bridge/mock/fixtures";
import { chatSessionKey } from "../chat/sessionRegistry";
import { useAttention, useFinishedWhileHidden } from "./useAttention";
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

it("counts a visible but unfocused window as watching its chat, and a hidden one as away", () => {
  vi.spyOn(document, "hasFocus").mockReturnValue(false);
  const visibility = vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const { result, rerender } = renderHook((props: { running: ReadonlySet<string> }) =>
    useFinishedWhileHidden({ bridge, loupists: [loupist], runningThreadKeys: props.running, visibleChatKey: key }),
  { initialProps: { running: new Set([key]) } });
  // Another app is in front; the chat stays on screen and its reply finishes.
  rerender({ running: new Set() });
  expect(result.current.finishedKeys.size).toBe(0);
  expect(bridge.notifications).toEqual([]);
  // The window is hidden when the next reply finishes.
  visibility.mockReturnValue("hidden");
  rerender({ running: new Set([key]) });
  rerender({ running: new Set() });
  expect([...result.current.finishedKeys]).toEqual([key]);
  expect(bridge.notifications).toHaveLength(1);
  // Shown again, though still unfocused: the chat is on screen, so the dot clears.
  visibility.mockReturnValue("visible");
  act(() => { document.dispatchEvent(new Event("visibilitychange")); });
  expect(result.current.finishedKeys.size).toBe(0);
  visibility.mockRestore();
});

it("identifies a question in an unloaded briefing chat by its dedicated session ID", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const settings = await bridge.briefingSettingsSet({ loupistId: loupist.id, enabled: true });
  bridge.installScheduledTask({ ...settings.task!, session_thread_id: "briefing-session" });
  const row = { kind: "question" as const, loupistId: loupist.id, loupistName: loupist.name, title: "Briefings", atMs: 1 };
  vi.spyOn(bridge, "attentionList").mockResolvedValue([{ ...row, chatId: "briefing-session" }, { ...row, chatId: "custom-session" }]);
  const settingsRead = vi.spyOn(bridge, "briefingSettingsGet");
  const { result } = renderHook(() => useAttention(bridge, "", {}));
  await waitFor(() => expect(result.current).toHaveLength(2));
  expect(result.current[0]).toMatchObject({ chatId: "briefing-session", taskKind: "briefing" });
  expect(result.current[1]).not.toHaveProperty("taskKind");
  expect(settingsRead).toHaveBeenCalledOnce();
});
