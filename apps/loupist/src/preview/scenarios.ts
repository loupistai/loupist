/**
 * The preview capture matrix in capture order (`scripts/capture-preview.mjs`); ids and
 * order are what captures and visual baselines key on. Each feature's scenarios live in
 * `./scenarios/<feature>.ts` and share `./scenarios/kit.ts`.
 */
import { settingsScenarios } from "./settingsScenarios";
import { readerScenarios } from "./readerScenarios";
import { dataScenarios } from "./dataScenarios";
import { assetScenarios } from "./assetScenarios";
import { answerCheckScenarios } from "./answerCheckScenarios";
import { followingScenarios } from "./followingScenarios";
import { gridScenarios } from "./gridScenarios";
import { koreaScenarios } from "./koreaScenarios";
import { browserScenarios } from "./browserScenarios";
import { quoteEvidenceScenarios } from "./quoteEvidenceScenarios";
import { knowledgeScenarios } from "./scenarios/knowledge";
import { connectionScenarios } from "./scenarios/connections";
import { memoryScenarios, principleScenarios, receiptScenarios } from "./scenarios/memory";
import { onboardingScenarios } from "./scenarios/onboarding";
import { chatScenarios } from "./scenarios/chat";
import { botScenarios } from "./scenarios/bot";
import { chatRunScenarios } from "./scenarios/chatRun";
import { activitySheetScenarios } from "./scenarios/activitySheet";
import { chatReadingScenarios } from "./scenarios/chatReading";
import { panelScenarios } from "./scenarios/panel";
import { mapScenarios } from "./scenarios/map";
import { scheduleScenarios } from "./scenarios/schedule";
import { shellScenarios } from "./scenarios/shell";
import { settingsPageScenarios } from "./scenarios/settings";
import { sharingScenarios } from "./scenarios/sharing";
import { marketScenarios } from "./scenarios/market";
import { faceScenarios } from "./scenarios/faces";
import { botCardScenarios } from "./scenarios/botCards";
import { handoffScenarios } from "./scenarios/handoff";
import { base, centerOn, click, clickMapAction, hasLayout, navigate, select, selectPanel, settings, submitPrompt, until, type PreviewScenario } from "./scenarios/kit";

export type { PreviewScenario } from "./scenarios/kit";
export { selectAnswerWords, until, visible } from "./scenarios/kit";
export { liveScenarios } from "./scenarios/live";

export const scenarios: readonly PreviewScenario[] = [
  ...knowledgeScenarios,
  ...connectionScenarios,
  ...principleScenarios,
  ...onboardingScenarios,
  ...chatScenarios,
  ...botScenarios,
  ...chatRunScenarios,
  ...activitySheetScenarios,
  ...chatReadingScenarios,
  ...memoryScenarios,
  ...panelScenarios,
  ...receiptScenarios,
  ...mapScenarios,
  ...scheduleScenarios,
  ...shellScenarios,
  ...settingsScenarios({ base, select, settings, until }),
  ...readerScenarios({ base, select, navigate, selectPanel, click, clickMapAction, until }),
  ...dataScenarios({ base, select, click, settings }),
  ...settingsPageScenarios,
  // Asset pages after the thin host: the replayed price, its states and the thesis.
  ...assetScenarios({ base, select, until, click, hasLayout, centerOn }),
  ...answerCheckScenarios({ base, select, until, click, clickMapAction, hasLayout, submitPrompt, navigate }),
  ...followingScenarios({ base, select, until }),
  ...sharingScenarios(),
  ...gridScenarios({ base, select, until, click, selectPanel, hasLayout, submitPrompt, navigate }),
  // An answer that links its reverse-DCF grid (finance-korea-retail-disagreement.md §8).
  ...koreaScenarios({ base, select, until, hasLayout }),
  ...browserScenarios({ base, select, until, click }),
  ...quoteEvidenceScenarios({ base, select, until, click }),
  // Owner redesign step 1: the shared market primitives (src/components/market/).
  ...marketScenarios,
  // Owner redesign step 4: the Loupist's face (src/components/app/LoupistAvatar.tsx).
  ...faceScenarios,
  // Bot answers as the agent wrote them: a quick answer, a table, and headings with a list and a table.
  ...botCardScenarios,
  // Research hand-offs: a link's Look into it, Bot's hand-off cards and a session's first message.
  ...handoffScenarios,
];
