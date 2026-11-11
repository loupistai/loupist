import type { ChatStreamEvent } from "../types";

type Completed = Extract<ChatStreamEvent, { type: "completed" }>;

/** The fields Rust sends on every `completed` event besides the answer itself. */
export const completedRun: Pick<Completed, "lens_receipts" | "tool_refusals" | "host_composed" | "performance"> = {
  lens_receipts: [],
  tool_refusals: [],
  host_composed: false,
  performance: { setupMs: 40, generationMs: 400, totalMs: 460, sessionCache: "warm", inputTokens: 320, outputTokens: 96, cacheReadTokens: 0, cacheWriteTokens: 0 },
};
