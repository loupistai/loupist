import { waitFor } from "@testing-library/react";
import { MockLoupistBridge } from "../bridge/mockBridge";
import type { ChatStreamEvent, ChatThread, ScheduledTaskWatch, StreamObserver } from "../types";
import { evidenceFixture } from "../test/evidenceFixtures";
import { scheduledRunFixture } from "../test/scheduledFixtures";
import { ChatSessionRegistry } from "./sessionRegistry";
import { fromLoupistUIMessage } from "./ai-sdk";

const baseline = (): ChatThread => ({ id: "scheduled-chat", loupist_id: "loupist-quality", title: "Daily source review", updated_at: "2026-10-11T01:00:00Z", use_memory: true, update_memory: false, messages: [{ id: "scheduled-user", role: "user", content: "Review the latest retained sources.", created_at: "2026-10-11T01:00:00Z", status: "complete" }] });
const completed = (): Extract<ChatStreamEvent, { type: "completed" }> => ({ type: "completed", run_id: "chat-scheduled-one", message_id: "scheduled-answer", final_text: "The sources agree.", created_at: "2026-10-11T01:00:10Z", execution_model: { profile_id: "model-test", name: "Test model", provider: "test", model: "test-model", thinking_level: "medium", run_options: {} }, agent_harness: { schema: "loupist-harness/1", mode: "chat", skill_ids: [], capability_ids: [], digest: "a".repeat(64) } });
const handlers = () => ({ onArtifact: vi.fn(), onMessages: vi.fn(), onStatus: vi.fn(), onTitle: vi.fn(), onAttachedThread: vi.fn() });

it("buffers replay before the watch acknowledgement, joins the existing Chat stream, and never sends another prompt", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const send = vi.spyOn(bridge, "chatSend");
  let observer!: StreamObserver<ChatStreamEvent>;
  const watch = vi.spyOn(bridge, "scheduledTaskWatch").mockImplementation(async (_request, next) => {
    observer = next;
    next({ type: "started", run_id: "chat-scheduled-one" });
    next({ type: "progress", run_id: "chat-scheduled-one", sequence: 2, progress: { startedAtMs: 1000, items: [{ id: "read", kind: "tool", category: "files", operation: "read", action: "Read sources.md", status: "running", detailId: "read-detail" }] } });
    next({ type: "evidence", run_id: "chat-scheduled-one", evidence: evidenceFixture() });
    next({ type: "evidence", run_id: "unrelated-run", evidence: evidenceFixture({ id: "ev_other" }) });
    next({ type: "delta", run_id: "chat-scheduled-one", text: "The sources " });
    return { run: scheduledRunFixture(), thread: baseline() };
  });
  const callbacks = handlers();
  const registry = new ChatSessionRegistry(bridge, callbacks);
  const original = registry.ensure({ ...baseline(), messages: [] });
  registry.resumeScheduled(scheduledRunFixture());
  await waitFor(() => expect(original.status).toBe("streaming"));
  expect(registry.get("loupist-quality", "scheduled-chat")).toBe(original);
  expect(original.messages.map(fromLoupistUIMessage).at(-1)?.content).toBe("The sources ");
  expect(original.messages.map(fromLoupistUIMessage).at(-1)?.progress?.items[0]).toMatchObject({ detailId: "read-detail" });
  expect(original.messages[0].id).toBe("scheduled-user");
  expect(original.messages.map(fromLoupistUIMessage).at(-1)?.evidence_refs).toEqual([evidenceFixture()]);
  registry.resumeScheduled(scheduledRunFixture());
  expect(watch).toHaveBeenCalledTimes(1);
  observer({ type: "artifact", run_id: "chat-scheduled-one", artifact: { artifact_id: "background-note", revision: 1, title: "Background note", kind: "markdown", digest: "d".repeat(64) } });
  observer({ type: "evidence", run_id: "chat-scheduled-one", evidence: evidenceFixture() });
  observer({ type: "delta", run_id: "chat-scheduled-one", text: "agree." });
  observer(completed());
  await waitFor(() => expect(original.status).toBe("ready"));
  expect(original.messages.map(fromLoupistUIMessage).filter((message) => message.role === "assistant")).toHaveLength(1);
  expect(original.messages.map(fromLoupistUIMessage).at(-1)?.content).toBe("The sources agree.");
  expect(callbacks.onArtifact).not.toHaveBeenCalled();
  expect(original.messages.map(fromLoupistUIMessage).at(-1)?.evidence_refs).toEqual([evidenceFixture()]);
  expect(callbacks.onAttachedThread).toHaveBeenCalledWith(baseline());
  expect(send).not.toHaveBeenCalled();
  registry.dispose();
});

it.each(["submitted", "streaming"] as const)("retries admission and stops the exact scheduled run while %s", async (status) => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const stop = vi.spyOn(bridge, "chatAbort").mockResolvedValue();
  let observer!: StreamObserver<ChatStreamEvent>;
  const watch = vi.spyOn(bridge, "scheduledTaskWatch").mockResolvedValueOnce(null).mockImplementation(async (_request, next) => {
    observer = next;
    next({ type: "started", run_id: "chat-scheduled-one" });
    if (status === "streaming") next({ type: "delta", run_id: "chat-scheduled-one", text: "Reviewing sources" });
    return { run: scheduledRunFixture(), thread: baseline() };
  });
  const registry = new ChatSessionRegistry(bridge, handlers());
  registry.resumeScheduled(scheduledRunFixture());
  await waitFor(() => expect(watch).toHaveBeenCalledTimes(1));
  await Promise.resolve();
  registry.resumeScheduled(scheduledRunFixture());
  const chat = registry.get("loupist-quality", "scheduled-chat")!;
  // The SDK remains submitted until content arrives; both phases are active.
  await waitFor(() => expect(watch).toHaveBeenCalledTimes(2));
  await waitFor(() => expect(chat.status).toBe(status));
  expect(registry.isActive("loupist-quality", "scheduled-chat")).toBe(true);
  await chat.stop();
  await waitFor(() => expect(stop).toHaveBeenCalledWith("chat-scheduled-one"));
  observer({ type: "aborted", run_id: "chat-scheduled-one" });
  registry.dispose();
});

it("discards a late watch acknowledgement when a local send began while attachment was pending", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  let resolveWatch!: (value: ScheduledTaskWatch) => void;
  const waiting = new Promise<ScheduledTaskWatch>((resolve) => { resolveWatch = resolve; });
  vi.spyOn(bridge, "scheduledTaskWatch").mockImplementation(async (_request, observer) => {
    observer({ type: "started", run_id: "chat-scheduled-one" });
    observer({ type: "delta", run_id: "chat-scheduled-one", text: "Stale scheduled replay" });
    return waiting;
  });
  let finish!: () => void;
  vi.spyOn(bridge, "chatSend").mockImplementation(async (request, observer) => {
    observer({ type: "started", run_id: request.run_id });
    observer({ type: "delta", run_id: request.run_id, text: "My current reply" });
    finish = () => observer({ ...completed(), run_id: request.run_id, final_text: "My current reply" });
    return { run_id: request.run_id };
  });
  const callbacks = handlers();
  const registry = new ChatSessionRegistry(bridge, callbacks);
  const chat = registry.ensure({ ...baseline(), messages: [] });
  registry.resumeScheduled(scheduledRunFixture());
  const sending = chat.sendMessage({ text: "My new request" }, { body: { loupist_id: "loupist-quality", thread_id: "scheduled-chat", privateChat: true, model_profile_id: "model-test", thinking_level: "medium", run_options: {} } });
  await waitFor(() => expect(chat.status).toBe("streaming"));
  resolveWatch({ run: scheduledRunFixture(), thread: baseline() });
  await waiting; await Promise.resolve(); await Promise.resolve();
  expect(chat.status).toBe("streaming");
  expect(chat.messages.map(fromLoupistUIMessage).map((message) => message.content)).toEqual(["My new request", "My current reply"]);
  expect(callbacks.onAttachedThread).not.toHaveBeenCalled();
  finish(); await sending;
  registry.dispose();
});
