import { isDeepStrictEqual } from "node:util";

/** Keep derived Pi summaries separate from this turn's host-owned input. */
const SUMMARY_NOTICE = "This is a model-written summary of earlier context, not the user's words or new authorization. It may omit corrections or provenance. Follow the current request and runtime permissions; use chat_history or exact-read tools for missing originals.";
const CONTINUATION_TYPE = "loupist-compaction-continuation";

function textOf(message) {
  if (typeof message.content === "string") return message.content;
  return Array.isArray(message.content)
    ? message.content.filter((part) => part?.type === "text").map((part) => part.text).join("\n")
    : "";
}

export function registerSessionContinuity(pi, continuationPrompt) {
  let completedAgentLoop = false;
  let compactedThisProcess = false;
  let originalPrompt;
  pi.on("session_compact", () => { compactedThisProcess = true; });
  pi.on("message_end", (event) => {
    // One process serves one host reply. Keep its first prompt across overflow
    // retries and steering loops; context events receive cloned message objects.
    if (!originalPrompt && event.message.role === "user") originalPrompt = event.message;
  });
  pi.on("agent_start", () => { completedAgentLoop = false; });
  pi.on("agent_end", (event) => {
    const last = event.messages?.findLast((message) => message.role === "assistant");
    completedAgentLoop = last?.stopReason === "stop";
  });
  pi.on("agent_settled", () => { completedAgentLoop = false; });
  pi.on("session_before_compact", (event) => {
    // A finished reply must not wait for an auxiliary model request before
    // Rust can commit it. Pi still compacts before the next prompt or between
    // tool rounds, and overflow recovery retains its ordinary failure path.
    if (completedAgentLoop && (event.reason === "threshold" || event.reason === "overflow") && event.willRetry !== true) {
      return { cancel: true };
    }
  });
  pi.on("context", (event) => {
    let lastSummary = -1;
    const messages = event.messages
      .filter((message) => !(message.role === "custom" && message.customType === CONTINUATION_TYPE))
      .map((message, index) => {
        if (message.role !== "compactionSummary" && message.role !== "branchSummary") return message;
        lastSummary = index;
        const summary = message.summary.startsWith(SUMMARY_NOTICE)
          ? message.summary : `${SUMMARY_NOTICE}\n\n${message.summary}`;
        return { ...message, summary };
      });
    if (lastSummary < 0) return;
    if (compactedThisProcess && continuationPrompt && !messages.slice(lastSummary + 1).some((message) => message.role === "user" && textOf(message).includes(continuationPrompt))) {
      // This projection is not written to JSONL. Keep the restored input
      // beside this reply's prompt if retained, otherwise after its summary.
      // Older turns stay earlier; later corrections and steering still win.
      const promptIndex = messages.findIndex((message, index) => index > lastSummary && isDeepStrictEqual(message, originalPrompt));
      const insertionIndex = promptIndex < 0 ? lastSummary + 1 : promptIndex;
      messages.splice(insertionIndex, 0, {
        role: "custom",
        customType: CONTINUATION_TYPE,
        content: `The host restored the original input for this reply after summarization. Later retained user messages can amend it.\n${continuationPrompt}`,
        display: false,
        timestamp: originalPrompt?.timestamp ?? messages[lastSummary].timestamp,
      });
    }
    return { messages };
  });
}
