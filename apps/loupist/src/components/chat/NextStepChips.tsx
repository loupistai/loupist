import i18n, { useTranslation } from "@/i18n";
import type { GrowthEvent, KnowledgeMap } from "../../contracts/knowledge";
import { isMetricNode } from "../map/kinds";
import { entityLabel } from "../map/labels";
import { claimIdForEdge } from "../map/MapList";

const t = i18n.getFixedT(null, "chat");
const MAX_CHIPS = 3;

/**
 * Up to three follow-up questions from what an answer changed on the Map: a
 * conflict it found, then names it added. Local and deterministic; nothing is
 * sent until the user presses Enter in the composer.
 */
export function nextStepQuestions(map: KnowledgeMap | null | undefined, events: readonly GrowthEvent[], language: string): string[] {
  if (!map) return [];
  const nodes = new Map(map.nodes.filter((node) => node.kind === "entity").map((node) => [node.id, node]));
  const claims = new Map(map.edges.filter((edge) => edge.kind === "claim").map((edge) => [claimIdForEdge(edge), edge]));
  const questions: string[] = [];
  const used = new Set<string>();
  for (const event of events) {
    if (event.kind !== "claim_disputed" || !event.subjectId) continue;
    const edge = claims.get(event.subjectId);
    const subject = edge ? [nodes.get(edge.source), nodes.get(edge.target)].find((node) => node && !isMetricNode(node)) : undefined;
    if (!subject || used.has(subject.id)) continue;
    used.add(subject.id);
    questions.push(t("nextSteps.conflict", { name: entityLabel(subject, language) }));
    break;
  }
  for (const event of events) {
    if (questions.length >= MAX_CHIPS) break;
    if (event.kind !== "entity_added" || !event.subjectId) continue;
    const node = nodes.get(event.subjectId);
    if (!node || isMetricNode(node) || used.has(node.id)) continue;
    used.add(node.id);
    questions.push(t("nextSteps.more", { name: entityLabel(node, language) }));
  }
  return questions.slice(0, MAX_CHIPS);
}

export function NextStepChips({ questions, onAsk }: { questions: readonly string[]; onAsk: (question: string) => void }) {
  useTranslation("chat");
  if (!questions.length) return null;
  return <div className="next-step-chips" role="group" aria-label={t("nextSteps.label")}>
    {questions.map((question) => <button key={question} type="button" className="next-step-chip" onClick={() => onAsk(question)}>{question}</button>)}
  </div>;
}
