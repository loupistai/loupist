import { act, renderHook, waitFor } from "@testing-library/react";
import type { Chat } from "@ai-sdk/react";
import { useChat } from "@ai-sdk/react";
import { MockLoupistBridge } from "../bridge/mockBridge";
import { chatRetryLast } from "../bridge/mock/loupistChat";
import { createMockBridgeState } from "../bridge/mock/state";
import type { LoupistWorkspace } from "../types";
import { toLoupistUIMessage, type LoupistUIMessage } from "./ai-sdk";
import { useLoupistChat } from "./useLoupistChat";

vi.mock("@ai-sdk/react", () => ({ useChat: vi.fn() }));

it("keeps terminal failure detail in the answer while surfacing a subsequent operation failure", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const workspace = await bridge.loupistSelect("loupist-quality");
  const thread = workspace.threads[0]!;
  thread.messages = [{ id: "failed", role: "assistant", content: "Response could not be completed.", placeholder: true, status: "error", created_at: "2026-11-01T00:00:00Z" }];
  const chat = { id: "failed-chat", messages: thread.messages.map(toLoupistUIMessage) } as unknown as Chat<LoupistUIMessage>;
  vi.mocked(useChat).mockReturnValue({
    messages: chat.messages, sendMessage: vi.fn(), setMessages: vi.fn(), stop: vi.fn(), status: "error", error: new Error("Response could not be completed."),
  } as unknown as ReturnType<typeof useChat>);
  vi.spyOn(bridge, "chatSteer").mockRejectedValue(new Error("Current operation failed"));
  const { result } = renderHook(() => useLoupistChat({ bridge, chat, loupistId: "loupist-quality", activeThread: thread, setThreads: vi.fn() }));
  expect(result.current.error).toBeNull();
  expect(result.current.messages[0]).toMatchObject({ status: "error", content: "Response could not be completed." });
  await act(async () => { await result.current.steer("Use annual filings"); });
  expect(result.current.error).toBe("Current operation failed");
});

it.each([
  { status: "error", restore: false },
  { status: "aborted", restore: false },
  { status: "error", restore: true },
] as const)("retries the original question, files and date after a steer ($status, restore files: $restore)", async ({ status, restore }) => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const workspace = await bridge.loupistSelect("loupist-quality");
  const thread = workspace.threads[0]!;
  const question = {
    id: "question", role: "user" as const, content: "Compare margins", as_of: "2026-10-01", created_at: "2026-11-01T00:00:00Z",
    attachments: [{ id: "attachment-report", filename: "report.txt", media_type: "text/plain", size_bytes: 3, url: "data:text/plain;base64,YWJj" }],
  };
  const steer = { id: "steer", role: "user" as const, content: "Use annual filings", steer: true, created_at: "2026-11-01T00:00:01Z" };
  const failed = { id: "failed", role: "assistant" as const, content: "Stopped.", status, created_at: "2026-11-01T00:00:02Z" };
  thread.messages = [question, steer, failed];
  // Exercise the mock's persisted retry alongside the renderer's local history.
  const read = vi.spyOn(bridge, "loupistSelect").mockResolvedValue(workspace);
  const state = createMockBridgeState(0);
  state.threads["loupist-quality"] = [thread];
  const retry = vi.spyOn(bridge, "chatRetryLast").mockImplementation(async () => {
    return chatRetryLast(state, { loupist_id: "loupist-quality", thread_id: thread.id });
  });
  const uiMessages = thread.messages.map(toLoupistUIMessage);
  if (restore) uiMessages[0] = toLoupistUIMessage({ ...question, attachments: [{ ...question.attachments[0], id: "local-report", url: undefined }] });
  const attachmentRead = vi.spyOn(bridge, "chatAttachmentRead").mockResolvedValue({ data_url: "data:text/plain;base64,YWJj" });
  const chat = { id: "retry-chat", messages: uiMessages } as unknown as Chat<LoupistUIMessage>;
  vi.mocked(useChat).mockReturnValue({ messages: chat.messages, sendMessage: vi.fn(), setMessages: vi.fn(), stop: vi.fn(), status: "ready", error: undefined } as unknown as ReturnType<typeof useChat>);
  let threads = [structuredClone(thread)];
  const setThreads = vi.fn(update => { threads = typeof update === "function" ? update(threads) : update; });
  const { result } = renderHook(() => useLoupistChat({ bridge, chat, loupistId: "loupist-quality", activeThread: thread, setThreads }));
  await act(async () => {
    expect(await result.current.retryLast()).toEqual({
      prompt: "Compare margins", files: [{ type: "file", filename: "report.txt", mediaType: "text/plain", url: "data:text/plain;base64,YWJj" }],
      threadId: thread.id, detached: false, asOf: "2026-10-01",
    });
  });
  expect(retry).toHaveBeenCalledOnce();
  if (restore) {
    expect(read).toHaveBeenCalledOnce();
    expect(attachmentRead).toHaveBeenCalledExactlyOnceWith("loupist-quality", thread.id, "question", "attachment-report");
  } else expect(read).not.toHaveBeenCalled();
  expect(thread.messages).toEqual([steer]);
  expect(chat.messages.map(message => message.id)).toEqual(["steer"]);
  expect(threads[0].messages).toHaveLength(1);
  expect(threads[0].messages[0]).toMatchObject(steer);
});

describe("Stop reconciliation", () => {
  it.each(["complete", "error"] as const)(
    "discards a late %s after switching A → B → A",
    async (outcome) => {
      const bridge = new MockLoupistBridge({ delay_ms: 0 });
      const workspace = await bridge.loupistSelect("loupist-quality");
      const threadA = workspace.threads[0]!;
      const threadB = workspace.threads[1]!;
      const chatA = { id: "chat-a" } as Chat<LoupistUIMessage>;
      const chatB = { id: "chat-b" } as Chat<LoupistUIMessage>;
      const setMessages = vi.fn();
      vi.mocked(useChat).mockReturnValue({ messages: [], sendMessage: vi.fn(), setMessages, stop: vi.fn().mockResolvedValue(undefined), status: "streaming", error: undefined } as unknown as ReturnType<typeof useChat>);
      vi.spyOn(bridge, "runActivityList").mockResolvedValue([]);
      let resolveRead!: (value: LoupistWorkspace) => void;
      let rejectRead!: (reason: Error) => void;
      const read = vi.spyOn(bridge, "loupistSelect").mockImplementation(() => new Promise((resolve, reject) => { resolveRead = resolve; rejectRead = reject; }));
      const setThreads = vi.fn();
      const { result, rerender } = renderHook(({ chat, activeThread }) => useLoupistChat({ bridge, chat, loupistId: "loupist-quality", activeThread, setThreads }), { initialProps: { chat: chatA, activeThread: threadA } });
      let stopped!: Promise<void>;
      act(() => { stopped = result.current.abort(); });
      await waitFor(() => expect(read).toHaveBeenCalledOnce());
      rerender({ chat: chatB, activeThread: threadB });
      rerender({ chat: chatA, activeThread: threadA });
      expect(result.current.isStopping).toBe(false);

      await act(async () => {
        if (outcome === "complete") {
          const stale = structuredClone(workspace);
          stale.threads[0]!.messages.push({ id: "stale-stop-result", role: "assistant", content: "Outdated saved answer.", created_at: "2026-09-22T00:00:00Z", status: "complete" });
          resolveRead(stale);
        } else {
          rejectRead(new Error("Failure from the previous Stop request."));
        }
        await stopped;
      });
      expect(setMessages).not.toHaveBeenCalled();
      expect(result.current.error).toBeNull();
      expect(result.current.isStopping).toBe(false);
    },
  );
});


describe("Chat operation scope", () => {
  it.each(["submit", "steer"] as const)("keeps late %s failures out of a new visit while reporting current failures", async (operation) => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const workspace = await bridge.loupistSelect("loupist-quality");
    const threadA = workspace.threads[0]!;
    const threadB = workspace.threads[1]!;
    const chatA = { id: "chat-a" } as Chat<LoupistUIMessage>;
    const chatB = { id: "chat-b" } as Chat<LoupistUIMessage>;
    let rejectPending!: (cause: Error) => void;
    const pending = vi.fn(() => new Promise<never>((_resolve, reject) => { rejectPending = reject; }));
    vi.spyOn(bridge, "chatSteer").mockImplementation(pending);
    vi.mocked(useChat).mockReturnValue({
      messages: [], sendMessage: pending, setMessages: vi.fn(), stop: vi.fn(), status: "ready", error: undefined,
    } as unknown as ReturnType<typeof useChat>);
    const { result, rerender } = renderHook(({ chat, activeThread }) => useLoupistChat({
      bridge, chat, loupistId: "loupist-quality", activeThread, setThreads: vi.fn(),
    }), { initialProps: { chat: chatA, activeThread: threadA } });
    const run = () => operation === "submit"
      ? result.current.submit(threadA.id, "Request", false, "model", "medium", {}, [])
      : result.current.steer("Guidance");
    let first!: ReturnType<typeof run>;
    act(() => { first = run(); });
    rerender({ chat: chatB, activeThread: threadB });
    rerender({ chat: chatA, activeThread: threadA });
    await act(async () => { rejectPending(new Error("Stale operation failed")); await first; });
    expect(result.current.error).toBeNull();
    let current!: ReturnType<typeof run>;
    act(() => { current = run(); });
    await act(async () => { rejectPending(new Error("Current operation failed")); await current; });
    expect(result.current.error).toBe("Current operation failed");
  });

  it("preserves late steering receipts for the original Chat without changing another Chat", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const workspace = await bridge.loupistSelect("loupist-quality");
    const chatA = { id: "chat-a", messages: [] } as unknown as Chat<LoupistUIMessage>;
    const chatB = { id: "chat-b", messages: [] } as unknown as Chat<LoupistUIMessage>;
    let resolveSteer!: (receipt: Awaited<ReturnType<typeof bridge.chatSteer>>) => void;
    vi.spyOn(bridge, "chatSteer").mockImplementation(() => new Promise((resolve) => { resolveSteer = resolve; }));
    vi.mocked(useChat).mockReturnValue({
      messages: [], sendMessage: vi.fn(), setMessages: vi.fn(), stop: vi.fn(), status: "ready", error: undefined,
    } as unknown as ReturnType<typeof useChat>);
    const { result, rerender } = renderHook(({ chat, activeThread }) => useLoupistChat({
      bridge, chat, loupistId: "loupist-quality", activeThread, setThreads: vi.fn(),
    }), { initialProps: { chat: chatA, activeThread: workspace.threads[0]! } });
    const commitOriginalSteers = result.current.commitSteers;
    let pending!: ReturnType<typeof result.current.steer>;
    act(() => { pending = result.current.steer("Old guidance"); });
    rerender({ chat: chatB, activeThread: workspace.threads[1]! });
    await act(async () => {
      resolveSteer({ mode: "steer", message_id: "old-steer", prompt: "Old guidance", created_at: "2026-09-22T00:00:00Z" });
      const receipt = await pending;
      expect(receipt).toEqual({ id: "old-steer", text: "Old guidance", createdAt: "2026-09-22T00:00:00Z" });
      if (receipt) commitOriginalSteers([receipt]);
    });
    expect(chatA.messages).toHaveLength(1);
    expect(chatA.messages[0]).toMatchObject({ id: "old-steer", role: "user", parts: [{ type: "text", text: "Old guidance" }] });
    expect(chatB.messages).toEqual([]);
    expect(result.current.announcement).not.toBe("Guidance sent to the active response.");
  });
});

it("applies a late feedback receipt only to the original registry Chat", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const workspace = await bridge.loupistSelect("loupist-quality");
  const chatA = { id: "chat-a", messages: [{ id: "ui-answer", role: "assistant", parts: [], metadata: { native_message_id: "native-answer" } }] } as unknown as Chat<LoupistUIMessage>;
  const chatB = { id: "chat-b", messages: [] } as unknown as Chat<LoupistUIMessage>;
  const feedback = { rating: "down" as const, correction: "A correction", updatedAt: 1 };
  let resolve!: (value: typeof feedback) => void;
  const save = vi.spyOn(bridge, "chatFeedbackSet").mockImplementation(() => new Promise((done) => { resolve = done; }));
  vi.mocked(useChat).mockReturnValue({ messages: [], sendMessage: vi.fn(), setMessages: vi.fn(), stop: vi.fn(), status: "ready", error: undefined } as unknown as ReturnType<typeof useChat>);
  const { result, rerender } = renderHook(({ chat, activeThread }) => useLoupistChat({ bridge, chat, loupistId: "loupist-quality", activeThread, setThreads: vi.fn() }), { initialProps: { chat: chatA, activeThread: workspace.threads[0] } });
  let pending!: Promise<unknown>;
  act(() => { pending = result.current.saveFeedback("native-answer", { rating: "down", correction: "A correction" }); });
  rerender({ chat: chatB, activeThread: workspace.threads[1] });
  await act(async () => { resolve(feedback); await pending; });
  expect(save).toHaveBeenCalledWith(expect.objectContaining({ thread_id: workspace.threads[0].id, message_id: "native-answer" }));
  expect(chatA.messages[0].metadata?.feedback).toEqual(feedback);
  expect(chatB.messages).toEqual([]);
});

it("keeps the key a settled answer was drawn under when a rewritten thread replaces the live messages", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const workspace = await bridge.loupistSelect("loupist-quality");
  const setMessages = vi.fn();
  vi.mocked(useChat).mockReturnValue({ messages: [], sendMessage: vi.fn(), setMessages, stop: vi.fn(), status: "ready", error: undefined } as unknown as ReturnType<typeof useChat>);
  const chat = { id: "chat-a", messages: [] } as unknown as Chat<LoupistUIMessage>;
  const { result } = renderHook(() => useLoupistChat({ bridge, chat, loupistId: "loupist-quality", activeThread: workspace.threads[0], setThreads: vi.fn() }));
  const answer = { id: "native-answer", role: "assistant" as const, status: "complete" as const, content: "Margins fell.", created_at: "2026-08-12T00:00:02.000Z" };
  const live = { ...toLoupistUIMessage(answer), id: "assistant-local" };
  live.metadata = { ...live.metadata, answer_key: "commentary-9" };
  act(() => result.current.replaceThread({ ...workspace.threads[0], messages: [answer] }));
  const update = setMessages.mock.lastCall?.[0] as (current: LoupistUIMessage[]) => LoupistUIMessage[];
  expect(update([live]).map((message) => [message.id, message.metadata?.answer_key])).toEqual([["native-answer", "commentary-9"]]);
});
