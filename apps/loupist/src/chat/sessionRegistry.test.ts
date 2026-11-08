import { MockLoupistBridge } from "../bridge/mockBridge";
import type { ChatThread } from "../types";
import { fromLoupistUIMessage, toLoupistUIMessage } from "./ai-sdk";
import { ChatSessionRegistry } from "./sessionRegistry";

const thread = (id: string): ChatThread => ({ id, loupist_id: "loupist-quality", title: id, use_memory: false, update_memory: false, messages: [], updated_at: "2026-08-12T00:00:00.000Z" });
const handlers = () => ({ onArtifact: vi.fn(), onMessages: vi.fn(), onStatus: vi.fn(), onTitle: vi.fn() });
const body = (threadId: string) => ({ body: { loupist_id: "loupist-quality", thread_id: threadId, privateChat: true, model_profile_id: "model-test", thinking_level: "medium", run_options: {} } });
const terminal = { created_at: "2026-08-12T00:00:01.000Z",
  execution_model: { profile_id: "model-test", name: "Test model", provider: "test", model: "test-model", thinking_level: "medium", run_options: {} },
  agent_harness: { schema: "loupist-harness/1", mode: "chat" as const, skill_ids: [], capability_ids: [], digest: "a".repeat(64) } };
const assistantOf = (chat: { messages: Parameters<typeof fromLoupistUIMessage>[0][] }) => chat.messages.map(fromLoupistUIMessage).find((message) => message.role === "assistant");

it("preserves a completed retry when a background selection returns older messages, then accepts newer native state", () => {
  const registry = new ChatSessionRegistry(new MockLoupistBridge(), handlers());
  const stale: ChatThread = { ...thread("retried"), messages: [
    { id: "question", role: "user", content: "Please retry", created_at: "2026-08-12T00:00:00.000Z" },
    { id: "failed", role: "assistant", status: "error", content: "Old failure", created_at: "2026-08-12T00:00:01.000Z" },
  ] };
  const chat = registry.ensure(stale);
  const completed: ChatThread["messages"][number] = { id: "retried-answer", role: "assistant", status: "complete", content: "Completed retry", created_at: "2026-08-12T00:00:03.000Z" };
  chat.messages = [stale.messages[0]!, completed].map(toLoupistUIMessage);
  const recovered = registry.reconcile({ ...stale, title: "Canonical title", use_memory: true });
  expect(recovered).toMatchObject({ title: "Canonical title", use_memory: true });
  expect(recovered.messages.at(-1)).toMatchObject(completed);
  const newer = { ...completed, id: "newer-native", content: "Newer native response", created_at: "2026-08-12T00:00:05.000Z" };
  registry.reconcile({ ...stale, messages: [stale.messages[0]!, newer] });
  expect(chat.messages.map(fromLoupistUIMessage).at(-1)).toMatchObject(newer);
  registry.dispose();
});

it("runs two Chats concurrently and keeps progress out of each answer", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  vi.spyOn(bridge, "chatSend").mockImplementation(async (request, observer) => {
    const label = request.thread_id;
    observer({ type: "started", run_id: request.run_id });
    observer({ type: "progress", run_id: request.run_id, progress: { startedAtMs: 1, items: [{ id: `commentary-${label}`, kind: "commentary", text: `${label} live commentary` }] } });
    observer({ type: "completed", run_id: request.run_id, message_id: `assistant-${label}`, final_text: `Answer for ${label}`, ...terminal });
    return { run_id: request.run_id };
  });
  const registry = new ChatSessionRegistry(bridge, handlers());
  const first = registry.ensure(thread("thread-one"));
  const second = registry.ensure(thread("thread-two"));
  await Promise.all([first.sendMessage({ text: "first" }, body("thread-one")), second.sendMessage({ text: "second" }, body("thread-two"))]);
  expect(assistantOf(first)?.content).toBe("Answer for thread-one");
  expect(assistantOf(second)?.content).toBe("Answer for thread-two");
  registry.dispose();
});

it("replaces streamed partial text with the canonical native error terminal", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  vi.spyOn(bridge, "chatSend").mockImplementation(async (request, observer) => {
    observer({ type: "started", run_id: request.run_id });
    observer({ type: "delta", run_id: request.run_id, text: "Partial provider answer." });
    observer({ type: "error", run_id: request.run_id, message: "Response could not be completed.", message_id: "assistant-canonical-error", final_text: "Response could not be completed.", ...terminal });
    return { run_id: request.run_id };
  });
  const registry = new ChatSessionRegistry(bridge, handlers());
  const chat = registry.ensure(thread("thread-error"));
  // AI SDK reports the terminal failure after applying the native terminal to the transcript.
  await chat.sendMessage({ text: "cause a terminal error" }, body("thread-error")).catch(() => undefined);
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(assistantOf(chat)).toMatchObject({ id: "assistant-canonical-error", status: "error", content: "Response could not be completed." });
  registry.dispose();
});

it("keeps the key a settled answer was drawn under when the store refreshes the idle Chat", () => {
  const registry = new ChatSessionRegistry(new MockLoupistBridge(), handlers());
  const stored: ChatThread = { ...thread("settled"), messages: [
    { id: "question", role: "user", content: "How did margins move?", created_at: "2026-08-12T00:00:00.000Z" },
    { id: "native-answer", role: "assistant", status: "complete", content: "Margins fell.", created_at: "2026-08-12T00:00:02.000Z" },
  ] };
  const chat = registry.ensure(stored);
  chat.messages = stored.messages.map((message) => {
    const ui = toLoupistUIMessage(message);
    return message.role === "assistant" ? { ...ui, id: "assistant-local", metadata: { ...ui.metadata, answer_key: "commentary-9" } } : ui;
  });
  registry.reconcile({ ...stored, title: "Refreshed" });
  expect(assistantOf(chat)).toMatchObject({ id: "native-answer", content: "Margins fell.", answer_key: "commentary-9" });
  registry.dispose();
});
