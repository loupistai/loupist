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
import { holdingsScenarios } from "./holdingsScenarios";
import { gridScenarios } from "./gridScenarios";
import { financeScenarios } from "./financeScenarios";
import { koreaScenarios } from "./koreaScenarios";
import { browserScenarios } from "./browserScenarios";
import { quoteEvidenceScenarios } from "./quoteEvidenceScenarios";
import { knowledgeScenarios } from "./scenarios/knowledge";
import { connectionScenarios } from "./scenarios/connections";
import { memoryScenarios, principleScenarios, receiptScenarios } from "./scenarios/memory";
import { onboardingScenarios } from "./scenarios/onboarding";
import { chatScenarios } from "./scenarios/chat";
import { chatRunScenarios } from "./scenarios/chatRun";
import { chatRailScenarios } from "./scenarios/chatRail";
import { chatReadingScenarios } from "./scenarios/chatReading";
import { panelScenarios } from "./scenarios/panel";
import { mapScenarios } from "./scenarios/map";
import { scheduleScenarios } from "./scenarios/schedule";
import { shellScenarios } from "./scenarios/shell";
import { settingsPageScenarios } from "./scenarios/settings";
import { sharingScenarios } from "./scenarios/sharing";
import { base, centerOn, click, clickMapAction, enterMap, hasLayout, navigate, select, selectPanel, settings, submitPrompt, until, type PreviewScenario } from "./scenarios/kit";

export type { PreviewScenario } from "./scenarios/kit";
export { selectAnswerWords, until, visible } from "./scenarios/kit";
export { liveScenarios } from "./scenarios/live";

export const scenarios: readonly PreviewScenario[] = [
  ...knowledgeScenarios,
  ...connectionScenarios,
  ...principleScenarios,
  ...onboardingScenarios,
  ...chatScenarios,
  ...chatRunScenarios,
  ...chatRailScenarios,
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
  // W16.1–W16.2: asset pages and "What changed" (finance-asset-page.md §8).
  ...assetScenarios({ base, select, until, click, enterMap, hasLayout, centerOn }),
  ...answerCheckScenarios({ base, select, until, click, clickMapAction, hasLayout, submitPrompt, navigate }),
  ...holdingsScenarios({ base, select, until, click }),
  ...sharingScenarios(),
  ...gridScenarios({ base, select, until, click, selectPanel, hasLayout, submitPrompt, navigate }),
  // W16.6–W16.7: results before and after, local watchers (finance-earnings-snapshot.md §8, finance-watchers.md §8).
  ...financeScenarios({ base, select, until, click, hasLayout, navigate, centerOn }),
  // W16.4, W16.8: Korean asset-page depth and the sensitivity card (finance-korea-retail-disagreement.md §8).
  ...koreaScenarios({ base, select, until, click, hasLayout }),
  ...browserScenarios({ base, select, until, click }),
  ...quoteEvidenceScenarios({ base, select, until, click }),
];
