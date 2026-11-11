import type { KnowledgeMapEdge, KnowledgeMapNode } from "../contracts/knowledge";

/** Language tag → the script its Map names prefer. Another locale adds a row here. */
const localScripts: Record<string, RegExp> = {
  ko: /[\u1100-\u11FF\u3130-\u318F\uAC00-\uD7A3]/u,
};
/** The name to draw. Korean uses the first Hangul alias the host selected; every other language uses the canonical name. */
export function entityLabel(node: Pick<KnowledgeMapNode, "label" | "localNames">, language: string) {
  const script = Object.entries(localScripts).find(([tag]) => language.toLowerCase() === tag || language.toLowerCase().startsWith(`${tag}-`))?.[1];
  return script ? node.localNames?.find(name => script.test(name)) ?? node.label : node.label;
}
/** Map search matches the canonical name and every host-selected local name. */
export function matchesMapQuery(node: Pick<KnowledgeMapNode, "label" | "localNames">, query: string) {
  const needle = query.toLocaleLowerCase();
  return [node.label, ...(node.localNames ?? [])].some(name => name.toLocaleLowerCase().includes(needle));
}
/** The World claim a drawn edge projects; the Map suffixes a claim's id with its target. */
export function claimIdForEdge(edge: KnowledgeMapEdge) {
  const projectedSuffix = `:${JSON.stringify(edge.target)}`;
  return edge.id.endsWith(projectedSuffix) ? edge.id.slice(0, -projectedSuffix.length) : edge.id;
}
