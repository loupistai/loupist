import { waitFor } from "@testing-library/react";
import { MockLoupistBridge } from "../bridge/mockBridge";
import type { ChatStreamEvent, ChatThread, ChatWatchRequest, ChatWatchSnapshot, StreamObserver } from "../types";
import { ChatSessionRegistry, type HostedWatchPhase } from "./sessionRegistry";
import { fromLoupistUIMessage } from "./ai-sdk";
import { completedRun } from "../test/streamFixtures";

const RUN = "handoff-1a2b3c4d";
const session = (extra: ChatThread["messages"] = []): ChatThread => ({
  id: "chat-hbm", loupist_id: "loupist-quality", title: "Hynix HBM supply", updated_at: "2026-11-18T09:00:00Z", use_memory: true, update_memory: true,
  messages: [{ id: "request", role: "bot_handoff", content: "Check the 2027 capacity plans.", created_at: "2026-11-18T09:00:00Z", status: "complete",
    handoff_note: { title: "Hynix HBM supply", userMessageInBot: "How tight is HBM4 supply into 2027?" } }, ...extra],
});
const progress = (action: string, sequence: number): ChatStreamEvent => ({ type: "progress", run_id: RUN, sequence, progress: { startedAtMs: 1_000, finishedAtMs: null,
  items: [{ id: `step-${sequence}`, kind: "tool", category: "web", operation: "search", action, status: "running", detailId: `detail-${sequence}`, target: null, href: null, startedAtMs: 1_000, finishedAtMs: null }] } });
const completed = (text: string): ChatStreamEvent => ({ type: "completed", ...completedRun, run_id: RUN, message_id: "answer", final_text: text, created_at: "2026-11-18T09:05:00Z",
  execution_model: { profile_id: "model-test", name: "Test model", provider: "test", model: "test-model", thinking_level: "medium", run_options: {} },
  agent_harness: { schema: "loupist-harness/1", mode: "chat", skill_ids: [], capability_ids: [], digest: "a".repeat(64) } });
const handlers = () => ({ onArtifact: vi.fn(), onMessages: vi.fn(), onStatus: vi.fn(), onTitle: vi.fn(), onAttachedThread: vi.fn() });
const shown = (registry: ChatSessionRegistry) => registry.get("loupist-quality", "chat-hbm")!.messages.map(fromLoupistUIMessage);

/** A host whose watch replays what the run did so far before it answers, as Rust does. */
function hostedRun(bridge: MockLoupistBridge, snapshots: Array<{ replay: ChatStreamEvent[]; thread: ChatThread } | null>) {
  const observers: Array<StreamObserver<ChatStreamEvent>> = [];
  const requests: ChatWatchRequest[] = [];
  const watch = vi.spyOn(bridge, "chatWatch").mockImplementation(async (request, observer): Promise<ChatWatchSnapshot | null> => {
    requests.push(request);
    const next = snapshots.shift();
    if (!next) return null;
    observers.push(observer);
    for (const event of next.replay) observer(event);
    return { run_id: RUN, thread: next.thread };
  });
  const unwatch = vi.spyOn(bridge, "chatUnwatch").mockResolvedValue();
  return { watch, unwatch, requests, live: () => observers.at(-1)! };
}

it("joins the host's run from its replay, streams its live work into the same Chat, and never sends a prompt", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const send = vi.spyOn(bridge, "chatSend");
  const abort = vi.spyOn(bridge, "chatAbort");
  const host = hostedRun(bridge, [{ thread: session(), replay: [{ type: "started", run_id: RUN }, progress("Searching for HBM4 capacity", 1), { type: "delta", run_id: RUN, text: "SK hynix " }] }]);
  const callbacks = handlers();
  const registry = new ChatSessionRegistry(bridge, callbacks);
  const chat = registry.ensure({ ...session(), messages: [] });
  const phases: HostedWatchPhase[] = [];
  registry.watchHosted("loupist-quality", "chat-hbm", (phase) => phases.push(phase));
  await waitFor(() => expect(chat.status).toBe("streaming"));
  expect(host.requests[0]).toMatchObject({ loupist_id: "loupist-quality", thread_id: "chat-hbm", subscription_id: expect.any(String) });
  expect(callbacks.onAttachedThread).toHaveBeenCalledWith(session());
  expect(shown(registry).map((message) => message.role)).toEqual(["bot_handoff", "assistant"]);
  expect(shown(registry)[0]!.handoff_note).toEqual({ title: "Hynix HBM supply", userMessageInBot: "How tight is HBM4 supply into 2027?" });
  expect(shown(registry).at(-1)).toMatchObject({ content: "SK hynix ", status: "streaming" });

  host.live()(progress("Reading the capacity filing", 2));
  host.live()({ type: "delta", run_id: RUN, text: "sold out 2026." });
  host.live()(completed("SK hynix sold out 2026."));
  await waitFor(() => expect(chat.status).toBe("ready"));
  expect(shown(registry).at(-1)).toMatchObject({ content: "SK hynix sold out 2026.", status: "complete" });
  expect(phases).toEqual(["attaching", "live", "idle"]);
  // The finished stream releases its subscription; nothing stopped the run or asked another reply.
  await waitFor(() => expect(host.unwatch).toHaveBeenCalledExactlyOnceWith(host.requests[0]));
  expect(abort).not.toHaveBeenCalled();
  expect(send).not.toHaveBeenCalled();
  registry.dispose();
});

it("detaches on leave without stopping the run, unsubscribes, and drops the reply it left half drawn", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const abort = vi.spyOn(bridge, "chatAbort");
  const host = hostedRun(bridge, [{ thread: session(), replay: [{ type: "started", run_id: RUN }, { type: "delta", run_id: RUN, text: "Half an answer" }] }]);
  const callbacks = handlers();
  const registry = new ChatSessionRegistry(bridge, callbacks);
  const chat = registry.ensure({ ...session(), messages: [] });
  const phases: HostedWatchPhase[] = [];
  const release = registry.watchHosted("loupist-quality", "chat-hbm", (phase) => phases.push(phase));
  await waitFor(() => expect(chat.status).toBe("streaming"));
  release();
  await waitFor(() => expect(host.unwatch).toHaveBeenCalledExactlyOnceWith(host.requests[0]));
  await waitFor(() => expect(chat.status).toBe("ready"));
  expect(abort).not.toHaveBeenCalled();
  expect(shown(registry).map((message) => message.id)).toEqual(["request"]);
  // The thread list keeps its recency: nothing new was said, the view only left.
  expect(callbacks.onMessages).toHaveBeenLastCalledWith("loupist-quality", "chat-hbm", [expect.objectContaining({ id: "request" })], true);
  // Events after the leave reach nobody.
  host.live()({ type: "delta", run_id: RUN, text: " more" });
  expect(shown(registry).map((message) => message.id)).toEqual(["request"]);
  expect(phases).toEqual(["attaching", "live"]);
  registry.dispose();
});

it("reloads the snapshot and attaches again when the host resets the stream", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const guided = session([{ id: "guide", role: "user", steer: true, content: "Focus on Samsung too.", created_at: "2026-11-18T09:02:00Z", status: "complete" }]);
  const host = hostedRun(bridge, [
    { thread: session(), replay: [{ type: "started", run_id: RUN }, { type: "delta", run_id: RUN, text: "Before the reset" }] },
    { thread: guided, replay: [{ type: "started", run_id: RUN }, progress("Comparing Samsung's plan", 3), { type: "delta", run_id: RUN, text: "After the reset" }] },
  ]);
  const registry = new ChatSessionRegistry(bridge, handlers());
  const chat = registry.ensure({ ...session(), messages: [] });
  registry.watchHosted("loupist-quality", "chat-hbm", () => {});
  await waitFor(() => expect(shown(registry).at(-1)?.content).toBe("Before the reset"));
  host.live()({ type: "stream_reset", run_id: RUN });
  await waitFor(() => expect(host.watch).toHaveBeenCalledTimes(2));
  await waitFor(() => expect(shown(registry).at(-1)?.content).toBe("After the reset"));
  expect(host.unwatch).toHaveBeenCalledWith(host.requests[0]);
  expect(host.requests[1]!.subscription_id).not.toBe(host.requests[0]!.subscription_id);
  expect(shown(registry).map((message) => message.id).slice(0, 2)).toEqual(["request", "guide"]);
  expect(shown(registry).filter((message) => message.role === "assistant")).toHaveLength(1);
  expect(chat.status).toBe("streaming");
  host.live()(completed("Done."));
  await waitFor(() => expect(chat.status).toBe("ready"));
  registry.dispose();
});

it("shows the stored thread when the run finished before the view joined it", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const host = hostedRun(bridge, [null]);
  const finished = session([{ id: "answer", role: "assistant", content: "SK hynix sold out 2026.", created_at: "2026-11-18T09:05:00Z", status: "complete" }]);
  const workspace = await bridge.loupistSelect("loupist-quality");
  const select = vi.spyOn(bridge, "loupistSelect").mockResolvedValue({ ...workspace, threads: [finished] });
  const callbacks = handlers();
  const registry = new ChatSessionRegistry(bridge, callbacks);
  registry.ensure(session());
  const phases: HostedWatchPhase[] = [];
  registry.watchHosted("loupist-quality", "chat-hbm", (phase) => phases.push(phase));
  await waitFor(() => expect(phases).toEqual(["attaching", "idle"]));
  expect(select).toHaveBeenCalledWith("loupist-quality");
  expect(callbacks.onAttachedThread).toHaveBeenCalledWith(finished);
  expect(shown(registry).map((message) => message.id)).toEqual(["request", "answer"]);
  // No subscription was made, so none is released.
  expect(host.unwatch).not.toHaveBeenCalled();
  registry.dispose();
});

it("stops the host's run by its id from the joined Chat, as Stop does for a reply sent here", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const abort = vi.spyOn(bridge, "chatAbort").mockResolvedValue();
  const host = hostedRun(bridge, [{ thread: session(), replay: [{ type: "started", run_id: RUN }, { type: "delta", run_id: RUN, text: "Working" }] }]);
  const registry = new ChatSessionRegistry(bridge, handlers());
  const chat = registry.ensure({ ...session(), messages: [] });
  registry.watchHosted("loupist-quality", "chat-hbm", () => {});
  await waitFor(() => expect(chat.status).toBe("streaming"));
  await chat.stop();
  await waitFor(() => expect(abort).toHaveBeenCalledWith(RUN));
  await waitFor(() => expect(host.unwatch).toHaveBeenCalledOnce());
  registry.dispose();
});

it("leaves a Chat whose own reply is streaming alone", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const host = hostedRun(bridge, []);
  vi.spyOn(bridge, "chatSend").mockImplementation(async (request, observer) => {
    observer({ type: "started", run_id: request.run_id });
    observer({ type: "delta", run_id: request.run_id, text: "My reply" });
    return { run_id: request.run_id };
  });
  const registry = new ChatSessionRegistry(bridge, handlers());
  const chat = registry.ensure(session());
  void chat.sendMessage({ text: "Another question" }, { body: { loupist_id: "loupist-quality", thread_id: "chat-hbm", privateChat: false, model_profile_id: "model-test", thinking_level: "medium", run_options: {} } });
  await waitFor(() => expect(chat.status).toBe("streaming"));
  const phases: HostedWatchPhase[] = [];
  registry.watchHosted("loupist-quality", "chat-hbm", (phase) => phases.push(phase));
  expect(phases).toEqual(["idle"]);
  expect(host.watch).not.toHaveBeenCalled();
  registry.dispose();
});
