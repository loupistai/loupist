/** Preview scenarios on a real clock, outside the capture matrix (`src/preview/scenarios.ts`). */
import { prepareFixture } from "../fixtures";
import { liveResearchRun } from "../liveScenario";
import { longThread, messagesRendered, select, submitPrompt, until, type PreviewScenario } from "./kit";

/**
 * Scenarios on a real clock, outside the capture matrix: their frames depend on
 * timing, so they are driven by `scripts/probe-streaming.mjs`, not screenshotted.
 */
export const liveScenarios: readonly PreviewScenario[] = [
  {
    id: "chat-live-research",
    title: "A research reply streams on a real clock",
    setup: async (bridge) => { await prepareFixture(bridge, "answer"); longThread(bridge); liveResearchRun(bridge); },
    enter: async (root) => { await until(messagesRendered, "Message Markdown did not finish rendering"); await submitPrompt(root, "Why did Hanul Precision’s margin fall while revenue grew?"); },
    landmark: () => select(".message.user:last-of-type") ?? select(".message.user"),
  },
];
