import i18n, { useTranslation } from "@/i18n";
import type { GrowthEvent, KnowledgeMap, KnowledgeMapNode } from "../../contracts/knowledge";
import { entityLabel, claimIdForEdge } from "../../knowledge/mapItems";

const t = i18n.getFixedT(null, "chat");
const MAX_CHIPS = 3;

/**
 * A name worth a follow-up: a core World Organization the Map matched by listing to
 * something the user follows or holds. Currencies, places, sectors, metrics,
 * extension and unknown kinds never qualify; today's Map cannot tell a filing or a
 * dated event apart, so those do not either. No chip beats a meaningless one.
 */
function suggestible(node: KnowledgeMapNode | undefined): node is KnowledgeMapNode {
  return !!node && node.kind === "entity" && !node.categoryOverride && !node.kindLabels
    && node.entityKind?.trim().toLowerCase() === "organization" && node.watched;
}

/**
 * Up to three follow-up questions from what an answer changed on the Map: a
 * conflict it found, then names it added, each only when `suggestible`. Local and
 * deterministic; nothing is sent until the user presses Enter in the composer.
 */
export function nextStepQuestions(map: KnowledgeMap | null | undefined, events: readonly GrowthEvent[], language: string): string[] {
  if (!map) return [];
  const nodes = new Map(map.nodes.filter(suggestible).map((node) => [node.id, node]));
  const claims = new Map(map.edges.filter((edge) => edge.kind === "claim").map((edge) => [claimIdForEdge(edge), edge]));
  const questions: string[] = [];
  const used = new Set<string>();
  for (const event of events) {
    if (event.kind !== "claim_disputed" || !event.subjectId) continue;
    const edge = claims.get(event.subjectId);
    const subject = edge ? [nodes.get(edge.source), nodes.get(edge.target)].find(Boolean) : undefined;
    if (!subject || used.has(subject.id)) continue;
    used.add(subject.id);
    questions.push(t("nextSteps.conflict", { name: entityLabel(subject, language) }));
    break;
  }
  for (const event of events) {
    if (questions.length >= MAX_CHIPS) break;
    if (event.kind !== "entity_added" || !event.subjectId) continue;
    const node = nodes.get(event.subjectId);
    if (!node || used.has(node.id)) continue;
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
