import { MOCK_LOUPISTS, MOCK_THREADS } from "./fixtures";
import type {
  ChatArtifact,
  ChartDataset,
  ChatArtifactRevision,
  ChatThread,
  LoupistSummary,
  RunActivity,
} from "../../types";

export { DEFAULT_AGENT_SKILL_IDS } from "../../lib/agentSkills";

export type MockBridgeState = {
  delay_ms: number;
  chat_stream_gate?: Promise<void>;
  loupists: LoupistSummary[];
  threads: Record<string, ChatThread[]>;
  /** Preview and test seeds for "What {Loupist} knows" usage counts, by Loupist. */
  memoryUsage?: Record<string, import("../../contracts/knowledge").MemoryUsage[]>;
  chat_runs: Map<string, AbortController>;
  artifacts: Record<
    string,
    Array<{ artifact: ChatArtifact; content: ChatArtifactRevision; dataset?: ChartDataset; history?: Array<import("../../contracts/chat").ChatArtifactView & { restored_from_revision_id?: number }> }>
  >;
  run_activities: Map<string, RunActivity>;
  knowledgeStores: Record<string, unknown>;
  /** Application-wide keep-awake while the World is processing. */
  keepAwake?: boolean;
};

export const clone = <T>(value: T): T => structuredClone(value);

let nextId = 1;

export const makeId = (prefix: string) =>
  `${prefix}-${Date.now()}-${nextId++}`;

export const createMockBridgeState = (delay_ms: number): MockBridgeState => ({
  delay_ms,
  loupists: clone(MOCK_LOUPISTS),
  threads: clone(MOCK_THREADS),
  chat_runs: new Map(),
  artifacts: {},
  run_activities: new Map(),
  knowledgeStores: {},
});

/** Mirrors the host's one-run-per-target rule; worker caps and Memory locks stay in Rust. */
export const registerRunActivity = (state: MockBridgeState, activity: RunActivity) => {
  const domain = (kind: RunActivity["kind"]) => kind === "chat_mutation" ? "chat" : kind;
  if ([...state.run_activities.values()].some((current) => current.run_id === activity.run_id
    || (current.loupist_id === activity.loupist_id && domain(current.kind) === domain(activity.kind) && current.target === activity.target))) {
    throw new Error("This exact Loupist run target is already active.");
  }
  state.run_activities.set(activity.run_id, activity);
};

export const ensureLoupistHasNoActiveRuns = (
  state: MockBridgeState,
  loupistId: string,
) => {
  if (
    [...state.run_activities.values()].some(
      (activity) => activity.loupist_id === loupistId,
    )
  ) {
    throw new Error("This Loupist still has active sessions.");
  }
};

export const finishRunActivity = (state: MockBridgeState, runId: string) => {
  state.run_activities.delete(runId);
};

export const wait = (milliseconds: number, signal: AbortSignal) =>
  new Promise<void>((resolve, reject) => {
    if (signal.aborted) {
      reject(new DOMException("Aborted", "AbortError"));
      return;
    }

    const timer = window.setTimeout(resolve, milliseconds);
    signal.addEventListener(
      "abort",
      () => {
        window.clearTimeout(timer);
        reject(new DOMException("Aborted", "AbortError"));
      },
      { once: true },
    );
  });

export const shortPause = async (state: MockBridgeState) => {
  if (state.delay_ms === 0) return;
  await new Promise<void>((resolve) =>
    window.setTimeout(resolve, state.delay_ms),
  );
};

/** Hold stream progression without holding bridge requests or abort processing. */
export const waitForStreamResume = async (state: MockBridgeState, signal: AbortSignal) => {
  const gate = state.chat_stream_gate;
  if (!gate) return;
  await new Promise<void>((resolve, reject) => {
    const aborted = () => {
      signal.removeEventListener("abort", aborted);
      reject(new DOMException("Aborted", "AbortError"));
    };
    if (signal.aborted) {
      aborted();
      return;
    }
    signal.addEventListener("abort", aborted, { once: true });
    void gate.then(() => {
      signal.removeEventListener("abort", aborted);
      resolve();
    });
  });
};
