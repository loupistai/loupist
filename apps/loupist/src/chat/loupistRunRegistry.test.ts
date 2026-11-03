import { MockLoupistBridge } from "../bridge/mockBridge";
import type { LoupistWorkspace, RunActivity } from "../types";
import { LoupistRunRegistry } from "./loupistRunRegistry";

const chatRun = (run_id: string, loupist_id: string, target: string, started_at_ms = 1): RunActivity => ({ run_id, loupist_id, kind: "chat", target, started_at_ms });
const flush = async () => { await Promise.resolve(); await Promise.resolve(); };

afterEach(() => vi.useRealTimers());

it("keeps in-flight discovery through StrictMode deactivation and reactivation", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  let finish!: (activities: RunActivity[]) => void;
  vi.spyOn(bridge, "runActivityList").mockReturnValue(new Promise((resolve) => { finish = resolve; }));
  const registry = new LoupistRunRegistry(bridge);
  registry.activate();
  const hydration = registry.hydrateActivities(["loupist-quality"]);
  registry.deactivate();
  registry.activate();
  finish([chatRun("live-run", "loupist-quality", "thread-margin")]);
  await hydration;
  expect(registry.getRecoveredChat("loupist-quality", "thread-margin")).toMatchObject({ run_id: "live-run" });
  registry.dispose();
});

it.each(["remove", "dispose"] as const)("ignores late activity discovery after %s", async (action) => {
  vi.useFakeTimers();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  let finish!: (activities: RunActivity[]) => void;
  vi.spyOn(bridge, "runActivityList").mockReturnValue(new Promise((resolve) => { finish = resolve; }));
  const registry = new LoupistRunRegistry(bridge);
  const hydration = registry.hydrateActivities(["loupist-quality", "loupist-value"]);
  if (action === "remove") registry.removeLoupist("loupist-quality"); else registry.dispose();
  finish([chatRun("removed-run", "loupist-quality", "thread-margin"), chatRun("retained-run", "loupist-value", "thread-downside", 2)]);
  await hydration;
  expect(registry.getRecoveredChat("loupist-quality", "thread-margin")).toBeUndefined();
  expect(registry.getSnapshot().active_loupist_ids).toEqual(new Set(action === "remove" ? ["loupist-value"] : []));
  registry.dispose();
});

it("reports hydration failure and allows the same Loupist to retry", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  vi.spyOn(bridge, "runActivityList").mockRejectedValueOnce(new Error("temporary native failure")).mockResolvedValue([]);
  const registry = new LoupistRunRegistry(bridge);
  await registry.hydrateLoupist("loupist-quality");
  expect(registry.getLoupistSnapshot("loupist-quality").hydration).toBe("error");
  await registry.hydrateLoupist("loupist-quality");
  expect(registry.getLoupistSnapshot("loupist-quality").hydration).toBe("ready");
});

it("isolates recovered Chat runs by Loupist and thread and retries exact Stop", async () => {
  vi.useFakeTimers();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  vi.spyOn(bridge, "runActivityList").mockResolvedValue([
    chatRun("chat-quality-margin", "loupist-quality", "thread-margin"),
    chatRun("chat-quality-capital", "loupist-quality", "thread-capital", 2),
    chatRun("chat-value-downside", "loupist-value", "thread-downside", 3),
  ]);
  const chatAbort = vi.spyOn(bridge, "chatAbort").mockRejectedValueOnce(new Error("Active Chat run not found.")).mockResolvedValue();
  const registry = new LoupistRunRegistry(bridge);
  await registry.hydrateActivities(["loupist-quality", "loupist-value"]);
  expect(registry.getRecoveredChat("loupist-quality", "thread-margin")).toMatchObject({ run_id: "chat-quality-margin", status: "running" });
  expect(registry.getRecoveredChat("loupist-value", "thread-downside")).toMatchObject({ run_id: "chat-value-downside" });
  expect(registry.getSnapshot().active_loupist_ids).toEqual(new Set(["loupist-quality", "loupist-value"]));

  expect(registry.abortRecoveredChat("loupist-quality", "thread-margin")).toBe(true);
  await vi.advanceTimersByTimeAsync(100);
  expect(chatAbort.mock.calls).toEqual([["chat-quality-margin"], ["chat-quality-margin"]]);
  expect(registry.getRecoveredChat("loupist-quality", "thread-capital")?.abort_requested).toBe(false);
  registry.dispose();
});

it("does not attach a native Chat already owned by the live AI SDK registry", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  vi.spyOn(bridge, "runActivityList").mockResolvedValue([chatRun("chat-local-owned", "loupist-quality", "thread-margin")]);
  const registry = new LoupistRunRegistry(bridge, { isLocalChatActive: (loupistId, threadId) => loupistId === "loupist-quality" && threadId === "thread-margin" });
  await registry.hydrateActivities(["loupist-quality"]);
  expect(registry.getRecoveredChat("loupist-quality", "thread-margin")).toBeUndefined();
  expect(registry.getSnapshot().active_chat_threads).toEqual([]);
});

it("keeps a completed recovered Chat reconciling until canonical state is applied", async () => {
  vi.useFakeTimers();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const canonical = await bridge.loupistSelect("loupist-quality");
  vi.spyOn(bridge, "runActivityList").mockResolvedValueOnce([chatRun("chat-reconcile", "loupist-quality", "thread-margin")]).mockResolvedValue([]);
  let resolveCanonical!: (workspace: LoupistWorkspace) => void;
  vi.spyOn(bridge, "loupistSelect").mockImplementation(() => new Promise<LoupistWorkspace>((resolve) => { resolveCanonical = resolve; }));
  const reconciled = vi.fn();
  const registry = new LoupistRunRegistry(bridge, { onRecoveredChatReconciled: reconciled });

  await registry.hydrateActivities(["loupist-quality"]);
  expect(registry.getRecoveredChat("loupist-quality", "thread-margin")?.status).toBe("running");
  vi.advanceTimersByTime(0);
  await flush();
  expect(registry.getRecoveredChat("loupist-quality", "thread-margin")?.status).toBe("reconciling");
  resolveCanonical(canonical);
  await flush();
  expect(reconciled).toHaveBeenCalledWith(canonical, "thread-margin");
  expect(registry.getRecoveredChat("loupist-quality", "thread-margin")).toBeUndefined();
  registry.dispose();
});
