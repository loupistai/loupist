/** Preview scenarios on a real clock, outside the capture matrix (`src/preview/scenarios.ts`). */
import { prepareFixture } from "../fixtures";
import { liveResearchRun, livePrompt, type LiveEnding } from "../liveScenario";
import { longThread, messagesRendered, select, submitPrompt, until, type PreviewScenario } from "./kit";

const live = (id: string, title: string, ending: LiveEnding): PreviewScenario => ({
  id,
  title,
  setup: async (bridge) => { await prepareFixture(bridge, "answer"); longThread(bridge); liveResearchRun(bridge, ending); },
  enter: async (root) => { await until(messagesRendered, "Message Markdown did not finish rendering"); await submitPrompt(root, livePrompt(ending)); },
  landmark: () => select(".message.user:last-of-type") ?? select(".message.user"),
});

/**
 * Scenarios on a real clock, outside the capture matrix: their frames depend on
 * timing, so they are driven by `scripts/probe-streaming.mjs` and
 * `scripts/capture-streaming.mjs`, not screenshotted by the matrix.
 */
export const liveScenarios: readonly PreviewScenario[] = [
  live("chat-live-research", "A research reply streams on a real clock: a note, parallel steps, a retried page, a table card, the answer, the commit", "complete"),
  live("chat-live-stopped", "The reader stops a research reply part-way through its answer", "stopped"),
  live("chat-live-failed", "The provider fails part-way through a research reply's answer", "failed"),
  live("chat-live-question", "A research reply asks one question, waits, then finishes once answered", "question"),
  live("chat-live-quick", "A fast answer with no steps holds its table whole", "quick"),
];
