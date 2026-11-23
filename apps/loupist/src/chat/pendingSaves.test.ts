import type { ChatMessage, ChatSaveChangedEvent, ChatSaveState } from "../types";
import { PendingSaves, withPendingSaves } from "./pendingSaves";

const save = (runId: string, messageId: string, waiting = true): ChatSaveState => ({
  run_id: runId,
  thread_id: "bot",
  message_id: messageId,
  waiting_for_other_chat: waiting,
  blocking_chat_id: waiting ? "research" : undefined,
  save_activity: { startedAtMs: 1_000, finishedAtMs: null, waitedMs: 0 },
});

const reply = (id: string, status: ChatMessage["status"] = "complete"): ChatMessage => ({
  id, role: "assistant", content: `Answer ${id}.`, created_at: "2026-11-23T00:00:00.000Z", status,
});

function store(listed: ChatSaveState[] = []) {
  let handler: ((event: ChatSaveChangedEvent) => void) | undefined;
  const settled: Array<[string, string, ChatMessage]> = [];
  let read = (): Promise<ChatSaveState[]> => Promise.resolve(listed);
  const saves = new PendingSaves({
    chatSaves: () => read(),
    onChatSaveChanged: (next) => { handler = next; return () => { handler = undefined; }; },
  }, (loupistId, threadId, message) => settled.push([loupistId, threadId, message]));
  const stop = saves.listen();
  return {
    saves, settled, stop,
    emit: (event: ChatSaveChangedEvent) => handler?.(event),
    reads: (next: () => Promise<ChatSaveState[]>) => { read = next; },
  };
}

it("follows each reply's wait and hands its stored receipt over when it resolves", () => {
  const { saves, settled, emit } = store();
  const seen = vi.fn();
  saves.subscribe(seen);
  emit({ loupist_id: "a", thread_id: "bot", message_id: "r1", run_id: "run-1", pending: save("run-1", "r1") });
  emit({ loupist_id: "a", thread_id: "bot", message_id: "r2", run_id: "run-2", pending: save("run-2", "r2") });
  expect([...saves.get("a").keys()]).toEqual(["r1", "r2"]);
  expect(saves.get("b").size).toBe(0);

  const stored = { ...reply("r1"), memory_update: { status: "no_change" as const, commitId: null, changes: [], skipReason: "user" as const } };
  emit({ loupist_id: "a", thread_id: "bot", message_id: "r1", run_id: "run-1", message: stored });
  expect([...saves.get("a").keys()]).toEqual(["r2"]);
  expect(settled).toEqual([["a", "bot", stored]]);
  // A late wait of the resolved save does not bring it back.
  emit({ loupist_id: "a", thread_id: "bot", message_id: "r1", run_id: "run-1", pending: save("run-1", "r1") });
  expect([...saves.get("a").keys()]).toEqual(["r2"]);
  expect(seen).toHaveBeenCalledTimes(3);
});

it("reads the saves Rust lists after a reload, without undoing a change that arrived meanwhile", async () => {
  const { saves, emit, reads } = store();
  let answer: (saves: ChatSaveState[]) => void = () => {};
  reads(() => new Promise((resolve) => { answer = resolve; }));
  const hydrated = saves.hydrate("a");
  // While the read is out, one save resolves and another reports a newer wait.
  emit({ loupist_id: "a", thread_id: "bot", message_id: "r1", run_id: "run-1", message: reply("r1") });
  const newer = save("run-2", "r2", false);
  emit({ loupist_id: "a", thread_id: "bot", message_id: "r2", run_id: "run-2", pending: newer });
  answer([save("run-1", "r1"), save("run-2", "r2"), save("run-3", "r3")]);
  await hydrated;
  expect([...saves.get("a").keys()].sort()).toEqual(["r2", "r3"]);
  expect(saves.get("a").get("r2")).toBe(newer);
});

it("shows a pending save on its stored reply as the live saving state", () => {
  const pending = new Map([["r1", save("run-1", "r1")], ["r2", save("run-2", "r2")]]);
  const [first, second, live] = withPendingSaves([reply("r1"), { ...reply("r2"), progress: { startedAtMs: 10, finishedAtMs: 20, items: [] } }, reply("r3", "streaming")], pending);
  expect(first!.memorySaving).toEqual({ run_id: "run-1", waiting_for_other_chat: true, blocking_chat_id: "research" });
  expect(first!.progress?.saveActivity).toEqual(save("run-1", "r1").save_activity);
  expect(second!.progress).toMatchObject({ startedAtMs: 10, finishedAtMs: 20, saveActivity: { startedAtMs: 1_000 } });
  expect(live!.memorySaving).toBeUndefined();
  const unchanged = [reply("r9")];
  expect(withPendingSaves(unchanged, new Map())).toBe(unchanged);
});
